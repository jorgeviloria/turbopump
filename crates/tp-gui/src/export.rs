//! Exports of the active impeller: points, surface mesh and drawings, and CAD solids,
//! which are built on a background thread because Open CASCADE takes a few seconds.

use std::path::PathBuf;

use eframe::egui;
use tp_design::impeller::ImpellerGeometry;
use tp_project::{ComponentId, Project};

use crate::derived::Derived;

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
        match self {
            TextExport::Points => "Geometry points (TXT)…",
            TextExport::Mesh => "Surface mesh (STL)…",
            TextExport::Meridional => "Meridional drawing (SVG)…",
            TextExport::Front => "Front view drawing (SVG)…",
        }
    }

    fn file(self) -> (&'static str, &'static str, &'static str) {
        // (file-name suffix, extension, filter name)
        match self {
            TextExport::Points => ("points", "txt", "Text"),
            TextExport::Mesh => ("mesh", "stl", "STL mesh"),
            TextExport::Meridional => ("meridional", "svg", "SVG drawing"),
            TextExport::Front => ("front", "svg", "SVG drawing"),
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
    let Some(path) = rfd::FileDialog::new()
        .add_filter(filter, &[extension])
        .set_file_name(format!("{project_name}-{suffix}.{extension}"))
        .save_file()
    else {
        return Ok(None);
    };
    std::fs::write(&path, kind.contents(g)).map_err(|e| format!("{}: {e}", path.display()))?;
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
    ) {
        if !self.cad_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Export CAD solids")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                #[cfg(feature = "cad")]
                self.cad.ui(ui, derived, project, active);
                #[cfg(not(feature = "cad"))]
                {
                    let _ = (derived, project, active);
                    ui.label("This build has no CAD support.");
                }
            });
        self.cad_open = open;
    }
}

#[cfg(feature = "cad")]
mod cad {
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, TryRecvError, channel};

    use eframe::egui::{self, Grid, Ui};
    use tp_cad::{CadOptions, Shape};
    use tp_design::Param;
    use tp_design::impeller::ImpellerGeometry;
    use tp_project::{ComponentId, DesignStep, Project, StepStatus};
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
        wall_thickness: f64,
        problems: Vec<String>,
    }

    #[derive(Debug)]
    pub(super) struct CadExport {
        step: bool,
        iges: bool,
        stl: bool,
        wall: Param<f64>,
        bore: Param<f64>,
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
        options: &CadOptions,
        folder: &Path,
        formats: &[&str],
    ) -> Result<CadReport, String> {
        let cad = tp_cad::build_impeller(g, options).map_err(|e| e.to_string())?;
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
        let problems = [
            ("blade", &cad.blade),
            ("flow domain", &cad.fluid),
            ("impeller", &cad.impeller),
        ]
        .into_iter()
        .filter(|(_, s)| !s.is_valid() || s.solid_count() != 1)
        .map(|(label, s)| {
            format!(
                "the {label} is not a single valid solid ({} solids)",
                s.solid_count()
            )
        })
        .collect();
        Ok(CadReport {
            folder: folder.to_owned(),
            files,
            impeller_volume: cad.impeller_volume(),
            fluid_volume: cad.fluid_volume(),
            wall_thickness: cad.wall_thickness,
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
                        self.last = Some(Err("the CAD export stopped unexpectedly".into()));
                        self.job = None;
                    }
                }
            }
        }

        fn start(&mut self, ctx: &egui::Context, g: ImpellerGeometry, folder: PathBuf) {
            let options = CadOptions {
                wall_thickness: self.wall.manual_value(),
                bore_diameter: self.bore.manual_value(),
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
                    let result = run(&g, &options, &folder, &formats);
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
            match spawned {
                Ok(_) => {
                    self.job = Some(rx);
                    self.last = None;
                }
                Err(e) => self.last = Some(Err(format!("could not start the export: {e}"))),
            }
        }

        pub(super) fn ui(
            &mut self,
            ui: &mut Ui,
            derived: &Derived,
            project: &Project,
            active: Option<ComponentId>,
        ) {
            use QuantityKind::*;
            let units = project.display_units.clone();
            let Some(id) = active else {
                ui.label("The project has no impeller.");
                return;
            };
            let ready = project
                .component(id)
                .is_some_and(|c| c.status(DesignStep::BladeEdges) == StepStatus::Done);
            let Some(g) = derived.geometry(id) else {
                ui.label("The impeller geometry has errors; fix them first.");
                return;
            };
            let d2 = g.basis.outlet_diameter;

            Grid::new("cad_options")
                .num_columns(3)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    param_f64(
                        ui,
                        "Wall thickness",
                        "Hub disk and shroud; automatic: max(3 mm, 0.02·d₂)",
                        &mut self.wall,
                        Length,
                        Some((0.02 * d2).max(0.003)),
                        &units,
                        0.0005..=0.2,
                    );
                    param_f64(
                        ui,
                        "Bore diameter",
                        "Automatic: the hub diameter at the eye",
                        &mut self.bore,
                        Length,
                        Some(g.basis.hub_diameter),
                        &units,
                        0.0..=1.0,
                    );
                    ui.label("Material density");
                    quantity(ui, &mut self.density, Density, &units, 100.0..=30_000.0);
                    ui.end_row();
                });
            ui.horizontal(|ui| {
                ui.label("Formats");
                ui.checkbox(&mut self.step, "STEP");
                ui.checkbox(&mut self.iges, "IGES");
                ui.checkbox(&mut self.stl, "STL");
            });
            ui.add_space(6.0);

            let any_format = self.step || self.iges || self.stl;
            if self.running() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Building the solids with Open CASCADE…");
                });
            } else {
                let enabled = ready && any_format;
                let response = ui
                    .add_enabled(enabled, egui::Button::new("Choose folder and export…"))
                    .on_disabled_hover_text(if ready {
                        "Choose at least one format."
                    } else {
                        "Accept the geometry steps up to the blade edges first."
                    });
                if response.clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    self.start(ui.ctx(), g.clone(), folder);
                }
                if !ready {
                    ui.weak("Accept the geometry steps up to the blade edges first.");
                }
            }

            match &self.last {
                Some(Ok(report)) => {
                    ui.separator();
                    let mass = report.impeller_volume * self.density;
                    Grid::new("cad_report").num_columns(2).show(ui, |ui| {
                        value_row(ui, "Folder", report.folder.display().to_string());
                        value_row(
                            ui,
                            "Wall thickness",
                            units.show(Length, report.wall_thickness),
                        );
                        value_row(
                            ui,
                            "Impeller",
                            format!(
                                "{:.1} cm³, {}",
                                report.impeller_volume * 1e6,
                                if mass < 1.0 {
                                    format!("{:.0} g", mass * 1e3)
                                } else {
                                    format!("{mass:.2} kg")
                                }
                            ),
                        );
                        value_row(
                            ui,
                            "Flow domain",
                            format!("{:.1} cm³", report.fluid_volume * 1e6),
                        );
                    });
                    for file in &report.files {
                        ui.weak(file.display().to_string());
                    }
                    if report.problems.is_empty() {
                        ui.label("Blade, flow domain and impeller are single valid solids.");
                    }
                    for p in &report.problems {
                        ui.colored_label(ui.visuals().warn_fg_color, format!("⚑ {p}"));
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
            let report = run(g, &CadOptions::default(), &folder, &["step"]).unwrap();
            assert_eq!(report.files.len(), 2);
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
