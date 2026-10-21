//! Exports of the active impeller: points, surface mesh and drawings, and CAD solids,
//! which are built on a background thread because Open CASCADE takes a few seconds.

use std::path::PathBuf;

use eframe::egui;
use tp_design::impeller::ImpellerGeometry;
use tp_design::volute::VoluteGeometry;
use tp_project::{ComponentId, Project};

use crate::derived::Derived;
use crate::i18n::tr;

/// Whether this build can make CAD solids (the `cad` feature).
pub(crate) const CAD_AVAILABLE: bool = cfg!(feature = "cad");

/// Exports written as text by `tp-io`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextExport {
    Points,
    Mesh,
    Meridional,
    Front,
}

impl TextExport {
    pub const ALL: [TextExport; 4] = [
        TextExport::Points,
        TextExport::Mesh,
        TextExport::Meridional,
        TextExport::Front,
    ];

    pub fn menu_label(self) -> &'static str {
        tr(match self {
            TextExport::Points => "Geometry points (TXT)…",
            TextExport::Mesh => "Surface mesh (STL)…",
            TextExport::Meridional => "Meridional drawing (SVG)…",
            TextExport::Front => "Front view drawing (SVG)…",
        })
    }

    fn file(self) -> (&'static str, &'static str, &'static str) {
        // (file-name suffix, extension, filter name)
        match self {
            TextExport::Points => ("points", "txt", tr("Text")),
            TextExport::Mesh => ("mesh", "stl", tr("STL mesh")),
            TextExport::Meridional => ("meridional", "svg", tr("SVG drawing")),
            TextExport::Front => ("front", "svg", tr("SVG drawing")),
        }
    }

    pub fn contents(self, g: &ImpellerGeometry) -> String {
        match self {
            TextExport::Points => tp_io::points::geometry_txt(g),
            TextExport::Mesh => tp_io::stl::ascii("impeller", &tp_io::mesh::impeller_triangles(g)),
            TextExport::Meridional => tp_io::svg::meridional(g),
            TextExport::Front => tp_io::svg::front_view(g),
        }
    }
}

/// Asks for a file name and writes the export. `Ok(None)` when the user cancelled.
pub(crate) fn save_text(
    kind: TextExport,
    g: &ImpellerGeometry,
    project_name: &str,
) -> Result<Option<PathBuf>, String> {
    let (suffix, extension, filter) = kind.file();
    save_file(suffix, extension, filter, || kind.contents(g), project_name)
}

/// Exports of the volute written by `tp-io`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoluteExport {
    Points,
    Mesh,
}

impl VoluteExport {
    pub const ALL: [VoluteExport; 2] = [VoluteExport::Points, VoluteExport::Mesh];

    pub fn menu_label(self) -> &'static str {
        tr(match self {
            VoluteExport::Points => "Volute points (TXT)…",
            VoluteExport::Mesh => "Volute surface mesh (STL)…",
        })
    }

    pub fn contents(self, g: &VoluteGeometry) -> String {
        match self {
            VoluteExport::Points => tp_io::points::volute_txt(g),
            VoluteExport::Mesh => tp_io::stl::ascii("volute", &tp_io::mesh::volute_triangles(g)),
        }
    }
}

/// Asks for a file name and writes a volute export.
pub(crate) fn save_volute(
    kind: VoluteExport,
    g: &VoluteGeometry,
    project_name: &str,
) -> Result<Option<PathBuf>, String> {
    let (suffix, extension, filter) = match kind {
        VoluteExport::Points => ("volute", "txt", tr("Text")),
        VoluteExport::Mesh => ("volute", "stl", tr("STL mesh")),
    };
    save_file(suffix, extension, filter, || kind.contents(g), project_name)
}

fn save_file(
    suffix: &str,
    extension: &str,
    filter: &str,
    contents: impl FnOnce() -> String,
    project_name: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(filter, &[extension])
        .set_file_name(format!("{project_name}-{suffix}.{extension}"))
        .save_file()
    else {
        return Ok(None);
    };
    std::fs::write(&path, contents()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path))
}

/// State of the export dialogs.
#[derive(Debug, Default)]
pub(crate) struct Exports {
    cad_open: bool,
    #[cfg(feature = "cad")]
    cad: cad::CadExport,
}

impl Exports {
    pub fn open_cad(&mut self) {
        self.cad_open = true;
    }

    /// Collects the result of a finished background job.
    pub fn poll(&mut self) {
        #[cfg(feature = "cad")]
        self.cad.poll();
    }

    pub fn cad_running(&self) -> bool {
        #[cfg(feature = "cad")]
        return self.cad.running();
        #[cfg(not(feature = "cad"))]
        false
    }

    pub fn cad_window(
        &mut self,
        ctx: &egui::Context,
        derived: &Derived,
        project: &Project,
        active: Option<ComponentId>,
        stator: Option<ComponentId>,
        volute: Option<ComponentId>,
    ) {
        if !self.cad_open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Export CAD solids"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                #[cfg(feature = "cad")]
                self.cad.ui(ui, derived, project, active, stator, volute);
                #[cfg(not(feature = "cad"))]
                {
                    let _ = (derived, project, active, stator, volute);
                    ui.label(tr("This build has no CAD support."));
                }
            });
        self.cad_open = open;
    }
}

#[cfg(feature = "cad")]
mod cad {
    use crate::i18n::{tf, tr};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, TryRecvError, channel};

    use eframe::egui::{self, Grid, Ui};
    use tp_cad::{BalanceHoleOptions, BalanceHoles, CadOptions, Shape};
    use tp_design::Param;
    use tp_design::diffuser::DiffuserDesign;
    use tp_design::impeller::ImpellerGeometry;
    use tp_design::volute::VoluteGeometry;
    use tp_project::{
        ComponentId, ComponentKind, DesignStep, Project, StepStatus, VANED_STEPS, VANELESS_STEPS,
        VOLUTE_STEPS,
    };
    use tp_units::QuantityKind;

    use crate::derived::Derived;
    use crate::widgets::{param_f64, quantity, value_row};

    /// What a finished export produced.
    #[derive(Debug, Clone)]
    pub(super) struct CadReport {
        folder: PathBuf,
        files: Vec<PathBuf>,
        impeller_volume: f64,
        fluid_volume: f64,
        volute_volume: Option<f64>,
        /// Volume and wall thickness of the volute casing.
        casing: Option<(f64, f64)>,
        diffuser_volume: Option<f64>,
        wall_thickness: f64,
        bore_diameter: f64,
        boss_diameter: f64,
        fillet_radius: f64,
        balance_holes: Option<BalanceHoles>,
        problems: Vec<String>,
    }

    #[derive(Debug)]
    pub(super) struct CadExport {
        step: bool,
        iges: bool,
        stl: bool,
        wall: Param<f64>,
        bore: Param<f64>,
        boss: Param<f64>,
        fillet: Param<f64>,
        balance_holes: bool,
        /// Material density, kg/m³.
        density: f64,
        job: Option<Receiver<Result<CadReport, String>>>,
        last: Option<Result<CadReport, String>>,
    }

    impl Default for CadExport {
        fn default() -> Self {
            Self {
                step: true,
                iges: false,
                stl: false,
                wall: Param::Auto,
                bore: Param::Auto,
                boss: Param::Auto,
                fillet: Param::Auto,
                balance_holes: false,
                density: 7850.0,
                job: None,
                last: None,
            }
        }
    }

    fn write(shape: &Shape, path: &Path, format: &str, deflection: f64) -> Result<(), String> {
        match format {
            "step" => tp_cad::write_step(shape, path),
            "igs" => tp_cad::write_iges(shape, path),
            _ => tp_cad::write_stl(shape, path, deflection),
        }
        .map_err(|e| format!("{}: {e}", path.display()))
    }

    fn run(
        g: &ImpellerGeometry,
        stator: Option<&DiffuserDesign>,
        volute: Option<&VoluteGeometry>,
        options: &CadOptions,
        folder: &Path,
        formats: &[&str],
    ) -> Result<CadReport, String> {
        let cad = tp_cad::build_impeller(g, options).map_err(|e| e.to_string())?;
        let volute = volute
            .map(|g| {
                let cad = tp_cad::build_volute(g)?;
                let casing = tp_cad::build_volute_casing(g, &cad)?;
                Ok::<_, tp_cad::CadError>((cad, casing, g.design.wall_thickness))
            })
            .transpose()
            .map_err(|e| e.to_string())?;
        let diffuser = stator
            .map(tp_cad::build_diffuser)
            .transpose()
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        // STL chordal deflection: 1/2000 of the outlet diameter, in mm.
        let deflection = g.basis.outlet_diameter * 1e3 / 2000.0;
        let mut files = Vec::new();
        for format in formats {
            for (stem, shape) in [("impeller", &cad.impeller), ("fluid", &cad.fluid)] {
                let path = folder.join(format!("{stem}.{format}"));
                write(shape, &path, format, deflection)?;
                files.push(path);
            }
        }
        if let Some((v, casing, _)) = &volute {
            for format in formats {
                for (stem, shape) in [("volute", &v.fluid), ("volute-casing", &casing.solid)] {
                    let path = folder.join(format!("{stem}.{format}"));
                    write(shape, &path, format, deflection)?;
                    files.push(path);
                }
            }
        }
        if let Some(d) = &diffuser {
            for format in formats {
                let mut shapes = vec![("diffuser", &d.fluid)];
                if let Some(vanes) = &d.vanes {
                    shapes.push(("diffuser-vanes", vanes));
                }
                for (stem, shape) in shapes {
                    let path = folder.join(format!("{stem}.{format}"));
                    write(shape, &path, format, deflection)?;
                    files.push(path);
                }
            }
        }
        let mut problems: Vec<String> = cad.warnings.clone();
        let invalid = [
            ("blade", Some(&cad.blade)),
            ("flow domain", Some(&cad.fluid)),
            ("impeller", Some(&cad.impeller)),
            (
                "volute flow domain",
                volute.as_ref().map(|(v, _, _)| &v.fluid),
            ),
            ("volute casing", volute.as_ref().map(|(_, c, _)| &c.solid)),
            ("diffuser flow domain", diffuser.as_ref().map(|d| &d.fluid)),
        ]
        .into_iter()
        .filter_map(|(label, s)| s.map(|s| (label, s)))
        .filter(|(_, s)| !s.is_valid() || s.solid_count() != 1)
        .map(|(label, s)| {
            tf(
                "the {} is not a single valid solid ({} solids)",
                &[&tr(label), &s.solid_count()],
            )
        });
        problems.extend(invalid);
        Ok(CadReport {
            folder: folder.to_owned(),
            files,
            impeller_volume: cad.impeller_volume(),
            fluid_volume: cad.fluid_volume(),
            volute_volume: volute.as_ref().map(|(v, _, _)| v.fluid_volume()),
            casing: volute.as_ref().map(|(_, c, wall)| (c.volume(), *wall)),
            diffuser_volume: diffuser.as_ref().map(|d| d.fluid_volume()),
            wall_thickness: cad.wall_thickness,
            bore_diameter: cad.bore_diameter,
            boss_diameter: cad.boss_diameter,
            fillet_radius: cad.fillet_radius,
            balance_holes: cad.balance_holes,
            problems,
        })
    }

    impl CadExport {
        pub(super) fn running(&self) -> bool {
            self.job.is_some()
        }

        pub(super) fn poll(&mut self) {
            if let Some(rx) = &self.job {
                match rx.try_recv() {
                    Ok(result) => {
                        self.last = Some(result);
                        self.job = None;
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        self.last = Some(Err(tr("the CAD export stopped unexpectedly").into()));
                        self.job = None;
                    }
                }
            }
        }

        fn start(
            &mut self,
            ctx: &egui::Context,
            g: ImpellerGeometry,
            stator: Option<DiffuserDesign>,
            volute: Option<VoluteGeometry>,
            holes: Option<BalanceHoleOptions>,
            folder: PathBuf,
        ) {
            let options = CadOptions {
                wall_thickness: self.wall.manual_value(),
                bore_diameter: self.bore.manual_value(),
                boss_diameter: self.boss.manual_value(),
                fillet_radius: self.fillet.manual_value(),
                balance_holes: holes,
                ..CadOptions::default()
            };
            let formats: Vec<&'static str> =
                [(self.step, "step"), (self.iges, "igs"), (self.stl, "stl")]
                    .into_iter()
                    .filter_map(|(on, f)| on.then_some(f))
                    .collect();
            let (tx, rx) = channel();
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new()
                .name("cad-export".into())
                .spawn(move || {
                    let result = run(
                        &g,
                        stator.as_ref(),
                        volute.as_ref(),
                        &options,
                        &folder,
                        &formats,
                    );
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
            match spawned {
                Ok(_) => {
                    self.job = Some(rx);
                    self.last = None;
                }
                Err(e) => {
                    self.last = Some(Err(tf("could not start the export: {}", &[&e])));
                }
            }
        }

        /// The balance holes of impeller `id` when asked for: one per channel, sized for
        /// the automatic rear seal of the axial thrust.
        fn hole_options(
            &self,
            project: &Project,
            id: ComponentId,
        ) -> Result<Option<BalanceHoleOptions>, String> {
            use tp_design::axial_thrust::{AxialThrust, AxialThrustInput};
            if !self.balance_holes {
                return Ok(None);
            }
            let duty = project.pump_duty(id).map_err(|e| e.to_string())?;
            let design = project
                .compute_main_dimensions(id)
                .map_err(|e| e.to_string())?;
            let input = AxialThrustInput {
                balance_holes: true,
                ..AxialThrustInput::default()
            };
            let axial =
                AxialThrust::new(&duty, &design, &input, false).map_err(|e| e.to_string())?;
            Ok(axial
                .balance_holes(design.blade_count)
                .map(|h| BalanceHoleOptions {
                    count: h.count,
                    diameter: h.diameter,
                    seal_diameter: h.seal_diameter,
                }))
        }

        pub(super) fn ui(
            &mut self,
            ui: &mut Ui,
            derived: &Derived,
            project: &Project,
            active: Option<ComponentId>,
            stator: Option<ComponentId>,
            volute: Option<ComponentId>,
        ) {
            use QuantityKind::*;
            let units = project.display_units.clone();
            let Some(id) = active else {
                ui.label(tr("The project has no impeller."));
                return;
            };
            let ready = project
                .component(id)
                .is_some_and(|c| c.status(DesignStep::BladeEdges) == StepStatus::Done);
            let Some(g) = derived.geometry(id) else {
                ui.label(tr("The impeller geometry has errors; fix them first."));
                return;
            };
            let d2 = g.basis.outlet_diameter;
            // The volute goes along once all of its steps are accepted.
            let volute_ready = volute.filter(|&v| {
                project.component(v).is_some_and(|c| {
                    VOLUTE_STEPS
                        .iter()
                        .all(|&s| c.status(s) == StepStatus::Done)
                })
            });
            let volute_geometry = volute_ready.and_then(|v| derived.volute(v));
            let stator_design = stator
                .filter(|&s| {
                    project.component(s).is_some_and(|c| {
                        let steps: &[DesignStep] = match c.kind() {
                            ComponentKind::Stator { vaned: true } => &VANED_STEPS,
                            _ => &VANELESS_STEPS,
                        };
                        steps.iter().all(|&s| c.status(s) == StepStatus::Done)
                    })
                })
                .and_then(|s| derived.diffuser(s));

            Grid::new("cad_options")
                .num_columns(3)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    param_f64(
                        ui,
                        tr("Wall thickness"),
                        tr("Hub disk and shroud; automatic: max(3 mm, 0.02·d₂)"),
                        &mut self.wall,
                        Length,
                        Some((0.02 * d2).max(0.003)),
                        &units,
                        0.0005..=0.2,
                    );
                    param_f64(
                        ui,
                        tr("Bore diameter"),
                        tr("Automatic: the hub diameter at the eye"),
                        &mut self.bore,
                        Length,
                        Some(g.basis.hub_diameter),
                        &units,
                        0.0..=1.0,
                    );
                    let wall = self.wall.manual_value().unwrap_or((0.02 * d2).max(0.003));
                    let bore = self.bore.manual_value().unwrap_or(g.basis.hub_diameter);
                    param_f64(
                        ui,
                        tr("Boss diameter"),
                        tr("Around the bore; automatic: the bore plus a wall of a quarter of it, no thinner than the wall"),
                        &mut self.boss,
                        Length,
                        Some(bore + 2.0 * (0.25 * bore).max(wall)),
                        &units,
                        0.0..=1.0,
                    );
                    param_f64(
                        ui,
                        tr("Fillet radius"),
                        tr("Where the blades meet hub and shroud; automatic: half the blade thickness; 0 for sharp edges"),
                        &mut self.fillet,
                        Length,
                        Some(0.5 * g.basis.blade_thickness),
                        &units,
                        0.0..=0.05,
                    );
                    ui.label(tr("Material density"));
                    quantity(ui, &mut self.density, Density, &units, 100.0..=30_000.0);
                    ui.end_row();
                });
            ui.checkbox(&mut self.balance_holes, tr("Balance holes"))
                .on_hover_text(tr(
                    "One per channel inside the rear seal, sized by Gülich Chap. 9.2.2",
                ));
            if stator.is_some() {
                ui.label(tr(if stator_design.is_some() {
                    "The diffuser flow domain and vanes are exported too."
                } else {
                    "The diffuser is left out until its steps are accepted."
                }));
            }
            if volute.is_some() {
                ui.label(tr(if volute_geometry.is_some() {
                    "The volute flow domain and its casing are exported too."
                } else {
                    "The volute is left out until its steps are accepted."
                }));
            }
            ui.horizontal(|ui| {
                ui.label(tr("Formats"));
                ui.checkbox(&mut self.step, tr("STEP"));
                ui.checkbox(&mut self.iges, tr("IGES"));
                ui.checkbox(&mut self.stl, tr("STL"));
            });
            ui.add_space(6.0);

            let any_format = self.step || self.iges || self.stl;
            if self.running() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Building the solids with Open CASCADE…"));
                });
            } else {
                let enabled = ready && any_format;
                let response = ui
                    .add_enabled(enabled, egui::Button::new(tr("Choose folder and export…")))
                    .on_disabled_hover_text(tr(if ready {
                        "Choose at least one format."
                    } else {
                        "Accept the geometry steps up to the blade edges first."
                    }));
                if response.clicked() {
                    match self.hole_options(project, id) {
                        Ok(holes) => {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                self.start(
                                    ui.ctx(),
                                    g.clone(),
                                    stator_design.cloned(),
                                    volute_geometry.cloned(),
                                    holes,
                                    folder,
                                );
                            }
                        }
                        Err(e) => self.last = Some(Err(e)),
                    }
                }
                if !ready {
                    ui.weak(tr("Accept the geometry steps up to the blade edges first."));
                }
            }

            match &self.last {
                Some(Ok(report)) => {
                    ui.separator();
                    let mass = |volume: f64| {
                        let kg = volume * self.density;
                        if kg < 1.0 {
                            format!("{:.0} g", kg * 1e3)
                        } else {
                            format!("{kg:.2} kg")
                        }
                    };
                    Grid::new("cad_report").num_columns(2).show(ui, |ui| {
                        value_row(ui, tr("Folder"), report.folder.display().to_string());
                        value_row(
                            ui,
                            tr("Wall thickness"),
                            units.show(Length, report.wall_thickness),
                        );
                        value_row(
                            ui,
                            tr("Impeller"),
                            format!(
                                "{:.1} cm³, {}",
                                report.impeller_volume * 1e6,
                                mass(report.impeller_volume)
                            ),
                        );
                        value_row(
                            ui,
                            tr("Flow domain"),
                            format!("{:.1} cm³", report.fluid_volume * 1e6),
                        );
                        value_row(
                            ui,
                            tr("Hub"),
                            tf(
                                "bore {} · boss {}",
                                &[
                                    &units.show(Length, report.bore_diameter),
                                    &units.show(Length, report.boss_diameter),
                                ],
                            ),
                        );
                        if report.fillet_radius > 0.0 {
                            value_row(
                                ui,
                                tr("Fillet radius"),
                                units.show(Length, report.fillet_radius),
                            );
                        }
                        if let Some(h) = &report.balance_holes {
                            value_row(
                                ui,
                                tr("Balance holes"),
                                tf(
                                    "{} × {} on {}",
                                    &[
                                        &h.count,
                                        &units.show(Length, h.diameter),
                                        &units.show(Length, h.pitch_diameter),
                                    ],
                                ),
                            );
                        }
                        if let Some(v) = report.diffuser_volume {
                            value_row(
                                ui,
                                tr("Diffuser flow domain"),
                                format!("{:.1} cm³", v * 1e6),
                            );
                        }
                        if let Some(v) = report.volute_volume {
                            value_row(ui, tr("Volute flow domain"), format!("{:.1} cm³", v * 1e6));
                        }
                        if let Some((v, wall)) = report.casing {
                            value_row(
                                ui,
                                tr("Volute casing"),
                                tf(
                                    "{} cm³, {}, wall {}",
                                    &[
                                        &format!("{:.1}", v * 1e6),
                                        &mass(v),
                                        &units.show(Length, wall),
                                    ],
                                ),
                            );
                        }
                    });
                    for file in &report.files {
                        ui.weak(file.display().to_string());
                    }
                    if report.problems.is_empty() {
                        ui.label(tr("Every solid is a single valid solid."));
                    }
                    for p in &report.problems {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            format!("{} {p}", crate::theme::icon::WARNING),
                        );
                    }
                }
                Some(Err(e)) => {
                    ui.separator();
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                None => {}
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::default_project;

        #[test]
        fn cad_export_writes_both_solids() {
            let project = default_project();
            let mut derived = Derived::default();
            derived.update(&project);
            let g = derived.geometry(project.components()[0].id()).unwrap();
            let folder = std::env::temp_dir().join(format!("tp-gui-cad-{}", std::process::id()));
            let volute = derived.volute(project.components()[1].id());
            assert!(volute.is_some());
            let report = run(g, None, volute, &CadOptions::default(), &folder, &["step"]).unwrap();
            assert_eq!(report.files.len(), 4);
            assert!(report.volute_volume.is_some_and(|v| v > 0.0));
            assert!(report.casing.is_some_and(|(v, t)| v > 0.0 && t > 0.0));
            for file in &report.files {
                assert!(std::fs::metadata(file).unwrap().len() > 1000);
            }
            assert!(report.problems.is_empty(), "{:?}", report.problems);
            assert!(report.impeller_volume > 0.0 && report.fluid_volume > 0.0);
            std::fs::remove_dir_all(&folder).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_project;

    #[test]
    fn text_exports_have_content() {
        let project = default_project();
        let mut derived = Derived::default();
        derived.update(&project);
        let g = derived.geometry(project.components()[0].id()).unwrap();
        for kind in TextExport::ALL {
            let text = kind.contents(g);
            assert!(text.len() > 1000, "{kind:?}");
        }
        assert!(TextExport::Mesh.contents(g).starts_with("solid"));
        assert!(TextExport::Front.contents(g).contains("<svg"));
    }
}
