//! Exports of the active impeller: points, surface mesh and drawings, and CAD solids,
//! which are built on a background thread because Open CASCADE takes a few seconds.

use std::path::PathBuf;

use eframe::egui;
use tp_design::impeller::ImpellerGeometry;
use tp_design::volute::VoluteGeometry;
use tp_project::{ComponentId, Project};

use crate::derived::Derived;
use crate::i18n::tr;
use crate::viewer::Capture;

/// Whether this build can make CAD solids (the `cad` feature).
pub(crate) const CAD_AVAILABLE: bool = cfg!(feature = "cad");

/// Exports written as text by `tp-io`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextExport {
    Points,
    Mesh,
}

impl TextExport {
    pub const ALL: [TextExport; 2] = [TextExport::Points, TextExport::Mesh];

    pub fn menu_label(self) -> &'static str {
        tr(match self {
            TextExport::Points => "Geometry points (TXT)…",
            TextExport::Mesh => "Surface mesh (STL)…",
        })
    }

    fn file(self) -> (&'static str, &'static str, &'static str) {
        // (file-name suffix, extension, filter name)
        match self {
            TextExport::Points => ("points", "txt", tr("Text")),
            TextExport::Mesh => ("mesh", "stl", tr("STL mesh")),
        }
    }

    pub fn contents(self, g: &ImpellerGeometry) -> String {
        match self {
            TextExport::Points => tp_io::points::geometry_txt(g),
            TextExport::Mesh => tp_io::stl::ascii("impeller", &tp_io::mesh::impeller_triangles(g)),
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
    save_file(
        suffix,
        extension,
        filter,
        || Ok(kind.contents(g).into_bytes()),
        project_name,
    )
}

/// The views of the impeller drawn by `tp-io`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawingView {
    Meridional,
    Front,
}

/// File formats of the drawings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawingFormat {
    Svg,
    Dxf,
    /// AutoCAD 2018 format.
    Dwg,
}

/// A drawing export: one view in one format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawingExport {
    pub view: DrawingView,
    pub format: DrawingFormat,
}

impl DrawingExport {
    pub const ALL: [DrawingExport; 6] = {
        use DrawingFormat::{Dwg, Dxf, Svg};
        use DrawingView::{Front, Meridional};
        [
            DrawingExport::new(Meridional, Svg),
            DrawingExport::new(Meridional, Dxf),
            DrawingExport::new(Meridional, Dwg),
            DrawingExport::new(Front, Svg),
            DrawingExport::new(Front, Dxf),
            DrawingExport::new(Front, Dwg),
        ]
    };

    const fn new(view: DrawingView, format: DrawingFormat) -> Self {
        Self { view, format }
    }

    pub fn menu_label(self) -> &'static str {
        use DrawingFormat::{Dwg, Dxf, Svg};
        use DrawingView::{Front, Meridional};
        tr(match (self.view, self.format) {
            (Meridional, Svg) => "Meridional drawing (SVG)…",
            (Meridional, Dxf) => "Meridional drawing (DXF)…",
            (Meridional, Dwg) => "Meridional drawing (DWG)…",
            (Front, Svg) => "Front view drawing (SVG)…",
            (Front, Dxf) => "Front view drawing (DXF)…",
            (Front, Dwg) => "Front view drawing (DWG)…",
        })
    }

    pub fn drawing(self, g: &ImpellerGeometry) -> tp_io::drawing::Drawing {
        match self.view {
            DrawingView::Meridional => tp_io::drawing::meridional(g),
            DrawingView::Front => tp_io::drawing::front_view(g),
        }
    }

    /// The file's content.
    pub fn contents(self, g: &ImpellerGeometry) -> Result<Vec<u8>, String> {
        let drawing = self.drawing(g);
        Ok(match self.format {
            DrawingFormat::Svg => tp_io::svg::write(&drawing).into_bytes(),
            DrawingFormat::Dxf => tp_io::dxf::write(&drawing).into_bytes(),
            DrawingFormat::Dwg => tp_io::dwg::write(&drawing, tp_io::dwg::DwgVersion::default())?,
        })
    }

    fn file(self) -> (&'static str, &'static str, &'static str) {
        // (file-name suffix, extension, filter name)
        let suffix = match self.view {
            DrawingView::Meridional => "meridional",
            DrawingView::Front => "front",
        };
        match self.format {
            DrawingFormat::Svg => (suffix, "svg", tr("SVG drawing")),
            DrawingFormat::Dxf => (suffix, "dxf", tr("DXF drawing")),
            DrawingFormat::Dwg => (suffix, "dwg", tr("DWG drawing")),
        }
    }
}

/// Asks for a file name and writes the drawing. `Ok(None)` when the user cancelled.
pub(crate) fn save_drawing(
    kind: DrawingExport,
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
    save_file(
        suffix,
        extension,
        filter,
        || Ok(kind.contents(g).into_bytes()),
        project_name,
    )
}

/// `capture` as a PNG file.
pub(crate) fn png(capture: &Capture) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, capture.width, capture.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&capture.rgba)
            .map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}

/// Asks where to save a PNG picture of the 3D view, and saves it.
pub(crate) fn save_picture(png: Vec<u8>, project_name: &str) -> Result<Option<PathBuf>, String> {
    save_file("3d", "png", tr("PNG image"), || Ok(png), project_name)
}

fn save_file(
    suffix: &str,
    extension: &str,
    filter: &str,
    contents: impl FnOnce() -> Result<Vec<u8>, String>,
    project_name: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(filter, &[extension])
        .set_file_name(format!("{project_name}-{suffix}.{extension}"))
        .save_file()
    else {
        return Ok(None);
    };
    std::fs::write(&path, contents()?).map_err(|e| format!("{}: {e}", path.display()))?;
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

    use eframe::egui::{self, Ui};
    use tp_cad::{BalanceHoleOptions, BalanceHoles, CadOptions, Shape};
    use tp_design::Param;
    use tp_design::diffuser::DiffuserDesign;
    use tp_design::impeller::ImpellerGeometry;
    use tp_design::impeller::duct::InletDuct;
    use tp_design::volute::VoluteGeometry;
    use tp_project::{
        ComponentId, ComponentKind, DesignStep, Project, StepStatus, VANED_STEPS, VANELESS_STEPS,
        VOLUTE_STEPS,
    };
    use tp_units::QuantityKind;

    use crate::derived::Derived;
    use crate::widgets::{label, param_f64, quantity, value_info, value_row};

    /// What a finished export produced.
    #[derive(Debug, Clone)]
    pub(super) struct CadReport {
        folder: PathBuf,
        files: Vec<PathBuf>,
        impeller_volume: f64,
        fluid_volume: f64,
        /// Each wear ring built, with its volume, m³.
        rings: Vec<(tp_cad::RingKind, f64)>,
        /// Volume of the back vanes, m³ (part of the impeller); `None` without them.
        back_vanes: Option<f64>,
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
                fillet: Param::Auto,
                balance_holes: false,
                density: 7850.0,
                job: None,
                last: None,
            }
        }
    }

    /// The name of a wear ring in the current language (literals, so that the
    /// translation check sees them).
    fn ring_label(kind: tp_cad::RingKind) -> &'static str {
        use tp_cad::RingKind;
        match kind {
            RingKind::FrontImpeller => tr("Front impeller ring"),
            RingKind::FrontCasing => tr("Front casing ring"),
            RingKind::RearImpeller => tr("Rear impeller ring"),
            RingKind::RearCasing => tr("Rear casing ring"),
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

    #[allow(clippy::too_many_arguments)]
    fn run(
        g: &ImpellerGeometry,
        duct: Option<&InletDuct>,
        stator: Option<&DiffuserDesign>,
        volute: Option<&VoluteGeometry>,
        options: &CadOptions,
        folder: &Path,
        formats: &[&str],
    ) -> Result<CadReport, String> {
        let cad = tp_cad::build_impeller(g, options).map_err(|e| e.to_string())?;
        let mut problems_of_duct = Vec::new();
        let duct = duct.and_then(|d| match tp_cad::build_inlet_duct(d) {
            Ok(shape) => Some(shape),
            Err(e) => {
                problems_of_duct.push(e.to_string());
                None
            }
        });
        let volute_geometry = volute;
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
            let chambers = cad.side_chambers.as_ref().map(|s| ("side-chambers", s));
            let duct = duct.as_ref().map(|s| ("inlet-duct", s));
            // Every wear ring is a file of its own, the chambers designed or not.
            let rings = cad.rings.iter().map(|r| (r.kind.file_stem(), &r.solid));
            for (stem, shape) in [("impeller", &cad.impeller), ("fluid", &cad.fluid)]
                .into_iter()
                .chain(chambers)
                .chain(duct)
                .chain(rings)
            {
                let path = folder.join(format!("{stem}.{format}"));
                write(shape, &path, format, deflection)?;
                files.push(path);
            }
        }
        // One flow domain for CFD when the side chambers are designed.
        let mut joined_warnings = Vec::new();
        if let (Some((v, _, _)), Some(vg), Some(_)) = (&volute, volute_geometry, &cad.side_chambers)
        {
            match tp_cad::join_fluid(&cad, g, v, vg, duct.as_ref()) {
                Ok((joined, warnings)) => {
                    joined_warnings = warnings;
                    for format in formats {
                        let path = folder.join(format!("fluid-joined.{format}"));
                        write(&joined, &path, format, deflection)?;
                        files.push(path);
                    }
                }
                Err(e) => joined_warnings.push(format!("the joined fluid was not built: {e}")),
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
        problems.extend(problems_of_duct);
        problems.extend(joined_warnings);
        if let Some((_, casing, _)) = &volute {
            problems.extend(casing.warnings.iter().cloned());
        }
        let invalid = [
            (tr("blade"), Some(&cad.blade)),
            (tr("flow domain"), Some(&cad.fluid)),
            (tr("impeller"), Some(&cad.impeller)),
            (
                tr("volute flow domain"),
                volute.as_ref().map(|(v, _, _)| &v.fluid),
            ),
            (
                tr("volute casing"),
                volute.as_ref().map(|(_, c, _)| &c.solid),
            ),
            (
                tr("diffuser flow domain"),
                diffuser.as_ref().map(|d| &d.fluid),
            ),
        ]
        .into_iter()
        .filter_map(|(label, s)| s.map(|s| (label, s)))
        .chain(cad.rings.iter().map(|r| (ring_label(r.kind), &r.solid)))
        .filter(|(_, s)| !s.is_valid() || s.solid_count() != 1)
        .map(|(label, s)| {
            tf(
                "the {} is not a single valid solid ({} solids)",
                &[&label.to_lowercase(), &s.solid_count()],
            )
        });
        problems.extend(invalid);
        Ok(CadReport {
            folder: folder.to_owned(),
            files,
            impeller_volume: cad.impeller_volume(),
            fluid_volume: cad.fluid_volume(),
            rings: cad
                .rings
                .iter()
                .filter_map(|r| Some((r.kind, cad.ring_volume(r.kind)?)))
                .collect(),
            back_vanes: cad.back_vanes.as_ref().map(|v| v.volume() * 1e-9),
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

        #[allow(clippy::too_many_arguments)]
        fn start(
            &mut self,
            ctx: &egui::Context,
            g: ImpellerGeometry,
            duct: Option<InletDuct>,
            stator: Option<DiffuserDesign>,
            volute: Option<VoluteGeometry>,
            holes: Option<BalanceHoleOptions>,
            note: Option<String>,
            folder: PathBuf,
        ) {
            let options = CadOptions {
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
                        duct.as_ref(),
                        stator.as_ref(),
                        volute.as_ref(),
                        &options,
                        &folder,
                        &formats,
                    );
                    let result = result.map(|mut report| {
                        report.problems.extend(note);
                        report
                    });
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
        /// the rear seal drawn in `g`. Without a rear seal they are left out, and the
        /// second part says so for the report.
        fn hole_options(
            &self,
            project: &Project,
            id: ComponentId,
            g: &ImpellerGeometry,
        ) -> Result<(Option<BalanceHoleOptions>, Option<String>), String> {
            if !self.balance_holes {
                return Ok((None, None));
            }
            let size = project.balance_hole_size(id, g)?;
            let note = size
                .is_none()
                .then(|| "balance holes need the rear seal; they are left out".to_string());
            Ok((
                size.map(|h| BalanceHoleOptions {
                    count: h.count,
                    diameter: h.diameter,
                    seal_diameter: h.seal_diameter,
                }),
                note,
            ))
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

            crate::widgets::props(ui, "cad_options", |ui| {
                // The material comes from the meridional design.
                let m = g.contour.material;
                let hint = tr("Set in the meridional editor, Materials page");
                for (name, value) in [
                    (tr("Wall thickness"), m.wall_thickness),
                    (tr("Bore diameter"), m.bore_diameter),
                    (tr("Boss diameter"), m.boss_diameter),
                ] {
                    value_info(ui, name, hint, units.show(Length, value));
                }
                param_f64(
                    ui,
                    tr("Fillet radius"),
                    tr(
                        "Where the blades meet hub and shroud; automatic: half the blade thickness; 0 for sharp edges",
                    ),
                    &mut self.fillet,
                    Length,
                    Some(0.5 * g.basis.blade_thickness),
                    &units,
                    0.0..=0.05,
                );
                label(ui, tr("Material density"), "");
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
                    match self.hole_options(project, id, g) {
                        Ok((holes, note)) => {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                self.start(
                                    ui.ctx(),
                                    g.clone(),
                                    derived.duct_before(id).cloned(),
                                    stator_design.cloned(),
                                    volute_geometry.cloned(),
                                    holes,
                                    note,
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
                    crate::widgets::table(ui, "cad_report", |ui| {
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
                        for &(kind, volume) in &report.rings {
                            value_row(
                                ui,
                                ring_label(kind),
                                tf(
                                    "{} cm³, {} (at the impeller density)",
                                    &[&format!("{:.1}", volume * 1e6), &mass(volume)],
                                ),
                            );
                        }
                        if let Some(volume) = report.back_vanes {
                            value_row(
                                ui,
                                tr("Back vanes"),
                                tf(
                                    "{} cm³, {} (part of the impeller)",
                                    &[&format!("{:.1}", volume * 1e6), &mass(volume)],
                                ),
                            );
                        }
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
                            format!(
                                "{} {}",
                                crate::theme::icon::WARNING,
                                crate::i18n::message(p)
                            ),
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
            let report = run(
                g,
                None,
                None,
                volute,
                &CadOptions::default(),
                &folder,
                &["step"],
            )
            .unwrap();
            // Impeller, flow domain, volute and its casing, then the four wear rings.
            assert_eq!(report.files.len(), 8);
            assert_eq!(report.rings.len(), 4);
            assert!(report.rings.iter().all(|&(_, v)| v > 0.0));
            for stem in tp_cad::RingKind::ALL.map(tp_cad::RingKind::file_stem) {
                assert!(
                    report
                        .files
                        .iter()
                        .any(|f| f.file_stem().is_some_and(|s| s == stem)),
                    "{stem}"
                );
            }
            assert!(report.volute_volume.is_some_and(|v| v > 0.0));
            assert!(report.casing.is_some_and(|(v, t)| v > 0.0 && t > 0.0));
            for file in &report.files {
                assert!(std::fs::metadata(file).unwrap().len() > 1000);
            }
            // Only a fillet radius reduced to fit the outlet corner may be reported.
            assert!(
                report.problems.iter().all(|p| p.contains("did not fit")),
                "{:?}",
                report.problems
            );
            assert!(report.impeller_volume > 0.0 && report.fluid_volume > 0.0);
            // The vanes are off by default and so is their line.
            assert!(report.back_vanes.is_none());
            std::fs::remove_dir_all(&folder).unwrap();
        }

        #[test]
        fn balance_holes_follow_the_rear_seal_of_the_contour() {
            let mut project = default_project();
            let id = project.components()[0].id();
            let export = CadExport {
                balance_holes: true,
                ..CadExport::default()
            };
            let mut input = project.impeller_geometry_input(id).unwrap();
            input.meridional.seals.rear.diameter = Param::Manual(0.09);
            project.set_impeller_geometry(id, input.clone()).unwrap();
            let g = project.compute_impeller_geometry(id).unwrap();
            let rear = g.contour.seals.rear.unwrap();
            let (holes, note) = export.hole_options(&project, id, &g).unwrap();
            assert!(note.is_none());
            assert!((holes.unwrap().seal_diameter - 2.0 * rear.radius).abs() < 1e-12);
            // Without the rear seal the holes are left out, and the report says so.
            input.meridional.seals.rear.enabled = false;
            project.set_impeller_geometry(id, input).unwrap();
            let g = project.compute_impeller_geometry(id).unwrap();
            let (holes, note) = export.hole_options(&project, id, &g).unwrap();
            assert!(holes.is_none());
            assert!(note.is_some_and(|n| n.contains("rear seal")));
        }

        #[test]
        fn cad_export_reports_the_back_vanes() {
            let project = default_project();
            let mut derived = Derived::default();
            derived.update(&project);
            let g = derived.geometry(project.components()[0].id()).unwrap();
            let mut input = tp_design::impeller::ImpellerGeometryInput::default();
            input.meridional.back_vanes.enabled = true;
            let g = tp_design::impeller::build(&g.basis, &input, 1.0).unwrap();
            let folder = std::env::temp_dir().join(format!("tp-gui-vanes-{}", std::process::id()));
            let report = run(
                &g,
                None,
                None,
                None,
                &CadOptions::default(),
                &folder,
                &["step"],
            )
            .unwrap();
            assert!(report.back_vanes.is_some_and(|v| v > 0.0));
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
    }

    #[test]
    fn every_drawing_has_a_label_and_a_file_type() {
        let project = default_project();
        let mut derived = Derived::default();
        derived.update(&project);
        let g = derived.geometry(project.components()[0].id()).unwrap();
        for kind in DrawingExport::ALL {
            let (_, extension, _) = kind.file();
            assert!(
                kind.menu_label().contains(&extension.to_uppercase()),
                "{kind:?}"
            );
            assert!(!kind.drawing(g).entities.is_empty(), "{kind:?}");
            let bytes = kind.contents(g).expect("written");
            if kind.format == DrawingFormat::Dwg {
                assert!(bytes.starts_with(b"AC1032"), "{kind:?}");
            }
        }
    }
}
