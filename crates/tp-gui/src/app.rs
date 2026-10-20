//! The application window: menus, panels, dock and dialogs.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Button, CentralPanel, Color32, Key, KeyboardShortcut, Modal, Modifiers, Panel, RichText,
    Ui, ViewportCommand,
};
use eframe::egui_wgpu::RenderState;
use egui_dock::{DockArea, DockState, NodeIndex, Style};
use tp_project::{
    ComponentId, ComponentKind, DesignPoint, DesignStep, Document, EnergyTransfer, FlowRate,
    GlobalSetup, MachineType, Project, ProjectError,
};
use tp_units::{DisplayUnits, QuantityKind};

use crate::derived::Derived;
use crate::export::{self, Exports, TextExport};
use crate::viewer::Viewer;
use crate::views::{blade, front, meridional::MeridionalView, report};
use crate::{default_project, steps, tree, widgets};

/// What the step panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// Design point, fluid and units, shared by every component.
    Setup,
    /// One design step of one component.
    Step(ComponentId, DesignStep),
}

/// A message in the status bar.
#[derive(Debug, Clone)]
pub(crate) struct Status {
    pub text: String,
    pub error: bool,
}

/// What the panels may change while they are drawn.
pub(crate) struct EditCtx<'a> {
    pub doc: &'a mut Document,
    pub derived: &'a Derived,
    pub selection: &'a mut Selection,
    pub status: &'a mut Option<Status>,
    pub exports: &'a mut Exports,
}

impl EditCtx<'_> {
    /// Applies an edit as part of a gesture (see [`Document::edit_gesture`]). Errors go
    /// to the status bar and leave the project unchanged. Returns whether it succeeded.
    pub fn edit(
        &mut self,
        label: &str,
        edit: impl FnOnce(&mut Project) -> Result<(), ProjectError>,
    ) -> bool {
        match self.doc.edit_gesture(label, edit) {
            Ok(()) => true,
            Err(e) => {
                self.error(format!("{label}: {e}"));
                false
            }
        }
    }

    pub fn info(&mut self, text: impl Into<String>) {
        *self.status = Some(Status {
            text: text.into(),
            error: false,
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        *self.status = Some(Status {
            text: text.into(),
            error: true,
        });
    }

    pub fn units(&self) -> DisplayUnits {
        self.doc.project().display_units.clone()
    }
}

/// The dockable views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    Meridional,
    Blade,
    FrontView,
    View3d,
    Report,
}

impl View {
    pub const ALL: [View; 5] = [
        View::Meridional,
        View::Blade,
        View::FrontView,
        View::View3d,
        View::Report,
    ];

    pub fn title(self) -> &'static str {
        match self {
            View::Meridional => "Meridional view",
            View::Blade => "Blade",
            View::FrontView => "Front view",
            View::View3d => "3D view",
            View::Report => "Report",
        }
    }
}

fn default_dock() -> DockState<View> {
    let mut dock = DockState::new(vec![View::Meridional, View::Blade, View::Report]);
    dock.main_surface_mut().split_right(
        NodeIndex::root(),
        0.5,
        vec![View::View3d, View::FrontView],
    );
    dock
}

/// An action that would discard unsaved changes, waiting for the user's answer.
#[derive(Debug)]
enum Pending {
    New(Box<Project>),
    Open(PathBuf),
    Quit,
}

/// Inputs of the "New project" dialog, in SI.
#[derive(Debug, Clone)]
struct NewProjectForm {
    name: String,
    flow: f64,
    head: f64,
    speed: f64,
    fluid: String,
}

impl Default for NewProjectForm {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            flow: 100.0 / 3600.0,
            head: 32.0,
            speed: 2900.0 * std::f64::consts::PI / 30.0,
            fluid: "Water".into(),
        }
    }
}

impl NewProjectForm {
    fn build(&self) -> Result<Project, ProjectError> {
        let mut setup = GlobalSetup::new(
            MachineType::Pump,
            DesignPoint {
                flow: FlowRate::Volume(self.flow),
                energy: EnergyTransfer::Head(self.head),
                speed: self.speed,
            },
        );
        setup.fluid = self.fluid.trim().to_owned();
        let mut project = Project::new(self.name.trim(), MachineType::Pump, setup)?;
        project.add_component(ComponentKind::Impeller, None)?;
        Ok(project)
    }
}

#[derive(Debug)]
enum Dialog {
    NewProject(NewProjectForm),
    ConfirmDiscard(Pending),
    About,
}

/// The Turbopump editor.
pub struct TurbopumpApp {
    doc: Document,
    derived: Derived,
    selection: Selection,
    dock: DockState<View>,
    meridional: MeridionalView,
    viewer: Viewer,
    exports: Exports,
    status: Option<Status>,
    dialog: Option<Dialog>,
    /// Set once the user agreed to close the window.
    allow_close: bool,
    title: String,
}

impl std::fmt::Debug for TurbopumpApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurbopumpApp")
            .field("selection", &self.selection)
            .finish_non_exhaustive()
    }
}

impl TurbopumpApp {
    /// Creates the editor, opening `path` if given.
    pub fn new(_cc: &eframe::CreationContext<'_>, path: Option<&Path>) -> Self {
        let mut app = Self::with_document(Document::new(default_project()));
        if let Some(path) = path {
            app.open_path(path);
        }
        app
    }

    /// Creates the editor for `doc`.
    pub fn with_document(doc: Document) -> Self {
        let mut app = Self {
            doc,
            derived: Derived::default(),
            selection: Selection::Setup,
            dock: default_dock(),
            meridional: MeridionalView::default(),
            viewer: Viewer::default(),
            exports: Exports::default(),
            status: None,
            dialog: None,
            allow_close: false,
            title: String::new(),
        };
        app.select_first_step();
        app
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    /// Brings `view` to the front of its dock panel.
    pub fn focus_view(&mut self, view: View) {
        if let Some(path) = self.dock.find_tab(&view) {
            // The path was just found, so activating it cannot fail.
            let _ = self.dock.set_active_tab(path);
        }
    }

    /// Draws the whole window. `render_state` enables the 3D view.
    pub fn show(&mut self, ui: &mut Ui, render_state: Option<&RenderState>) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        self.handle_close_request(&ctx);
        self.derived.update(self.doc.project());
        self.fix_selection();
        self.update_title(&ctx);
        self.exports.poll();

        Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| self.menu_bar(ui));
        });
        Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));
        Panel::left("project_tree")
            .default_size(240.0)
            .size_range(180.0..=420.0)
            .show(ui, |ui| {
                let mut cx = self.edit_ctx();
                tree::show(ui, &mut cx);
            });
        Panel::right("step_panel")
            .default_size(380.0)
            .size_range(300.0..=600.0)
            .show(ui, |ui| {
                let mut cx = EditCtx {
                    doc: &mut self.doc,
                    derived: &self.derived,
                    selection: &mut self.selection,
                    status: &mut self.status,
                    exports: &mut self.exports,
                };
                steps::show(ui, &mut cx);
            });
        CentralPanel::no_frame().show(ui, |ui| {
            let active = self.active_impeller();
            let mut views = Views {
                cx: EditCtx {
                    doc: &mut self.doc,
                    derived: &self.derived,
                    selection: &mut self.selection,
                    status: &mut self.status,
                    exports: &mut self.exports,
                },
                active,
                meridional: &mut self.meridional,
                viewer: &mut self.viewer,
                render_state,
            };
            DockArea::new(&mut self.dock)
                .style(Style::from_egui(ui.style().as_ref()))
                .show_close_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_leaf_collapse_buttons(false)
                .show_inside(ui, &mut views);
        });

        self.dialogs(&ctx);
        self.exports.cad_window(
            &ctx,
            &self.derived,
            self.doc.project(),
            self.active_impeller(),
        );

        // A gesture ends when the pointer is released and no field is being typed in.
        let idle = !ctx.input(|i| i.pointer.any_down()) && !ctx.text_edit_focused();
        if idle {
            self.doc.end_gesture();
        }
    }

    fn edit_ctx(&mut self) -> EditCtx<'_> {
        EditCtx {
            doc: &mut self.doc,
            derived: &self.derived,
            selection: &mut self.selection,
            status: &mut self.status,
            exports: &mut self.exports,
        }
    }

    /// The impeller the views show: the selected one, or the first.
    fn active_impeller(&self) -> Option<ComponentId> {
        let project = self.doc.project();
        let is_impeller = |id| {
            project
                .component(id)
                .is_some_and(|c| c.kind() == ComponentKind::Impeller)
        };
        match self.selection {
            Selection::Step(id, _) if is_impeller(id) => Some(id),
            _ => project
                .components()
                .iter()
                .find(|c| c.kind() == ComponentKind::Impeller)
                .map(|c| c.id()),
        }
    }

    fn select_first_step(&mut self) {
        self.selection = self
            .doc
            .project()
            .components()
            .iter()
            .find(|c| c.kind() == ComponentKind::Impeller)
            .map_or(Selection::Setup, |c| {
                Selection::Step(c.id(), c.next_step().unwrap_or(DesignStep::MainDimensions))
            });
    }

    /// Falls back to the setup when the selected component disappeared (undo, removal).
    fn fix_selection(&mut self) {
        if let Selection::Step(id, step) = self.selection {
            let valid = self
                .doc
                .project()
                .component(id)
                .is_some_and(|c| c.kind().steps().contains(&step));
            if !valid {
                self.selection = Selection::Setup;
            }
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let file = self.doc.path().and_then(|p| p.file_name()).map_or_else(
            || "not saved".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let dirty = if self.doc.is_dirty() { " •" } else { "" };
        let title = format!(
            "{} ({file}){dirty} — Turbopump",
            self.doc.project().info.name
        );
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let shortcut = |modifiers, key| KeyboardShortcut::new(modifiers, key);
        let command_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        // Text fields handle their own undo while the user types in them.
        let editing = ctx.text_edit_focused();
        ctx.input_mut(|i| {
            if i.consume_shortcut(&shortcut(command_shift, Key::S)) {
                self.save_as();
            } else if i.consume_shortcut(&shortcut(Modifiers::COMMAND, Key::S)) {
                self.save();
            }
            if i.consume_shortcut(&shortcut(Modifiers::COMMAND, Key::O)) {
                self.open_dialog();
            }
            if i.consume_shortcut(&shortcut(Modifiers::COMMAND, Key::N)) {
                self.dialog = Some(Dialog::NewProject(NewProjectForm::default()));
            }
            if !editing {
                if i.consume_shortcut(&shortcut(command_shift, Key::Z))
                    || i.consume_shortcut(&shortcut(Modifiers::COMMAND, Key::Y))
                {
                    self.redo();
                } else if i.consume_shortcut(&shortcut(Modifiers::COMMAND, Key::Z)) {
                    self.undo();
                }
            }
        });
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.doc.is_dirty()
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::ConfirmDiscard(Pending::Quit));
        }
    }

    fn undo(&mut self) {
        match self.doc.undo() {
            Some(label) => self.info(format!("Undid: {label}")),
            None => self.info("Nothing to undo"),
        }
    }

    fn redo(&mut self) {
        match self.doc.redo() {
            Some(label) => self.info(format!("Redid: {label}")),
            None => self.info("Nothing to redo"),
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            error: false,
        });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            error: true,
        });
    }

    /// Runs `pending` now, or asks first if there are unsaved changes.
    fn request(&mut self, pending: Pending) {
        if self.doc.is_dirty() {
            self.dialog = Some(Dialog::ConfirmDiscard(pending));
        } else {
            self.proceed(pending);
        }
    }

    fn proceed(&mut self, pending: Pending) {
        match pending {
            Pending::New(project) => {
                self.doc = Document::new(*project);
                self.select_first_step();
                self.info("New project");
            }
            Pending::Open(path) => self.open_path(&path),
            Pending::Quit => self.allow_close = true,
        }
    }

    fn open_path(&mut self, path: &Path) {
        match Document::open(path) {
            Ok(doc) => {
                self.doc = doc;
                self.select_first_step();
                self.info(format!("Opened {}", path.display()));
            }
            Err(e) => self.error(format!("Could not open {}: {e}", path.display())),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Turbopump project", &[tp_project::FILE_EXTENSION])
            .pick_file()
        {
            self.request(Pending::Open(path));
        }
    }

    /// Saves to the current file, asking for one if there is none. Returns whether the
    /// project was saved.
    fn save(&mut self) -> bool {
        if self.doc.path().is_none() {
            return self.save_as();
        }
        match self.doc.save() {
            Ok(()) => {
                let path = self.doc.path().map(|p| p.display().to_string());
                self.info(format!("Saved {}", path.unwrap_or_default()));
                true
            }
            Err(e) => {
                self.error(format!("Could not save: {e}"));
                false
            }
        }
    }

    fn save_as(&mut self) -> bool {
        let name = format!(
            "{}.{}",
            self.doc.project().info.name,
            tp_project::FILE_EXTENSION
        );
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Turbopump project", &[tp_project::FILE_EXTENSION])
            .set_file_name(name)
            .save_file()
        else {
            return false;
        };
        match self.doc.save_as(&path) {
            Ok(()) => {
                self.info(format!("Saved {}", path.display()));
                true
            }
            Err(e) => {
                self.error(format!("Could not save: {e}"));
                false
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let text = |m, key| ctx.format_shortcut(&KeyboardShortcut::new(m, key));
        ui.menu_button("File", |ui| {
            if ui
                .add(Button::new("New project…").shortcut_text(text(Modifiers::COMMAND, Key::N)))
                .clicked()
            {
                self.dialog = Some(Dialog::NewProject(NewProjectForm::default()));
            }
            if ui
                .add(Button::new("Open…").shortcut_text(text(Modifiers::COMMAND, Key::O)))
                .clicked()
            {
                self.open_dialog();
            }
            if ui
                .add(Button::new("Save").shortcut_text(text(Modifiers::COMMAND, Key::S)))
                .clicked()
            {
                self.save();
            }
            if ui
                .add(
                    Button::new("Save as…")
                        .shortcut_text(text(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)),
                )
                .clicked()
            {
                self.save_as();
            }
            ui.separator();
            let active = self.active_impeller();
            let has_geometry = active.is_some_and(|id| self.derived.geometry(id).is_some());
            ui.menu_button("Export", |ui| {
                for kind in TextExport::ALL {
                    if ui
                        .add_enabled(has_geometry, Button::new(kind.menu_label()))
                        .clicked()
                        && let Some(g) = active.and_then(|id| self.derived.geometry(id))
                    {
                        let result = export::save_text(kind, g, &self.doc.project().info.name);
                        match result {
                            Ok(Some(path)) => self.info(format!("Wrote {}", path.display())),
                            Ok(None) => {}
                            Err(e) => self.error(e),
                        }
                    }
                }
                ui.separator();
                if ui
                    .add_enabled(
                        export::CAD_AVAILABLE,
                        Button::new("CAD solids (STEP, IGES, STL)…"),
                    )
                    .on_disabled_hover_text("This build has no CAD support.")
                    .clicked()
                {
                    self.exports.open_cad();
                }
            });
            ui.separator();
            if ui.button("Quit").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }
        });

        ui.menu_button("Edit", |ui| {
            let undo = self.doc.history().undo_label().map(str::to_owned);
            let redo = self.doc.history().redo_label().map(str::to_owned);
            let undo_text = undo.map_or("Undo".to_owned(), |l| format!("Undo {l}"));
            let redo_text = redo.map_or("Redo".to_owned(), |l| format!("Redo {l}"));
            if ui
                .add_enabled(
                    self.doc.history().can_undo(),
                    Button::new(undo_text).shortcut_text(text(Modifiers::COMMAND, Key::Z)),
                )
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(
                    self.doc.history().can_redo(),
                    Button::new(redo_text)
                        .shortcut_text(text(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
                )
                .clicked()
            {
                self.redo();
            }
        });

        ui.menu_button("Project", |ui| {
            if ui.button("Global setup").clicked() {
                self.selection = Selection::Setup;
            }
            ui.separator();
            let mut cx = self.edit_ctx();
            tree::add_component_menu(ui, &mut cx);
        });

        ui.menu_button("View", |ui| {
            ui.label(RichText::new("Units").strong());
            let units = self.doc.project().display_units.clone();
            for (label, preset) in [
                ("SI (mm, m³/h, bar, °C)", DisplayUnits::si()),
                ("US customary (in, gpm, psi, °F)", DisplayUnits::us()),
            ] {
                if ui.radio(units == preset, label).clicked() {
                    let mut cx = self.edit_ctx();
                    cx.edit("change units", |p| {
                        p.display_units = preset;
                        Ok(())
                    });
                }
            }
            ui.separator();
            ui.label(RichText::new("Show").strong());
            for view in View::ALL {
                if ui.button(view.title()).clicked() {
                    self.focus_view(view);
                }
            }
            ui.separator();
            if ui.button("Reset layout").clicked() {
                self.dock = default_dock();
            }
        });

        ui.menu_button("Help", |ui| {
            if ui.button("About Turbopump").clicked() {
                self.dialog = Some(Dialog::About);
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            match &self.status {
                Some(Status { text, error: true }) => {
                    ui.colored_label(ui.visuals().error_fg_color, text);
                }
                Some(Status { text, .. }) => {
                    ui.label(text);
                }
                None => {
                    ui.weak("Ready");
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let source = match &self.derived.liquid {
                    Some(Ok(l)) => match l.source {
                        tp_fluids::PropertySource::CoolProp => "fluid properties: CoolProp",
                        tp_fluids::PropertySource::BuiltinWater => {
                            "fluid properties: built-in water model"
                        }
                    },
                    Some(Err(_)) => "fluid properties: unavailable",
                    None => "",
                };
                ui.weak(source);
                if self.exports.cad_running() {
                    ui.spinner();
                    ui.label("Building CAD solids…");
                }
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let units = self.doc.project().display_units.clone();
        let keep = match dialog {
            Dialog::NewProject(mut form) => {
                let mut create = false;
                let mut cancel = false;
                let response = Modal::new(egui::Id::new("new_project")).show(ctx, |ui| {
                    ui.heading("New pump project");
                    ui.add_space(6.0);
                    egui::Grid::new("new_project_grid")
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Name");
                            ui.text_edit_singleline(&mut form.name);
                            ui.end_row();
                            ui.label("Flow rate Q");
                            widgets::quantity(
                                ui,
                                &mut form.flow,
                                QuantityKind::VolumeFlow,
                                &units,
                                1e-5..=100.0,
                            );
                            ui.end_row();
                            ui.label("Head H");
                            widgets::quantity(
                                ui,
                                &mut form.head,
                                QuantityKind::Head,
                                &units,
                                0.1..=5000.0,
                            );
                            ui.end_row();
                            ui.label("Speed n");
                            widgets::quantity(
                                ui,
                                &mut form.speed,
                                QuantityKind::RotationalSpeed,
                                &units,
                                1.0..=10_000.0,
                            );
                            ui.end_row();
                            ui.label("Fluid");
                            ui.text_edit_singleline(&mut form.fluid);
                            ui.end_row();
                        });
                    ui.add_space(4.0);
                    ui.weak("Other machine types arrive in phase 6.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        create = ui.button("Create").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
                if create {
                    match form.build() {
                        Ok(project) => {
                            self.request(Pending::New(Box::new(project)));
                            None
                        }
                        Err(e) => {
                            self.error(format!("Invalid project: {e}"));
                            Some(Dialog::NewProject(form))
                        }
                    }
                } else if cancel || response.should_close() {
                    None
                } else {
                    Some(Dialog::NewProject(form))
                }
            }
            Dialog::ConfirmDiscard(pending) => {
                let mut choice = None;
                let name = self.doc.project().info.name.clone();
                let response = Modal::new(egui::Id::new("confirm_discard")).show(ctx, |ui| {
                    ui.heading("Unsaved changes");
                    ui.label(format!("Save the changes to \"{name}\" first?"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            choice = Some(Choice::Save);
                        }
                        if ui.button("Don't save").clicked() {
                            choice = Some(Choice::Discard);
                        }
                        if ui.button("Cancel").clicked() {
                            choice = Some(Choice::Cancel);
                        }
                    });
                });
                match choice {
                    Some(Choice::Save) => {
                        if self.save() {
                            self.finish(pending, ctx);
                        }
                        None
                    }
                    Some(Choice::Discard) => {
                        self.finish(pending, ctx);
                        None
                    }
                    Some(Choice::Cancel) => None,
                    None if response.should_close() => None,
                    None => Some(Dialog::ConfirmDiscard(pending)),
                }
            }
            Dialog::About => {
                let response = Modal::new(egui::Id::new("about")).show(ctx, |ui| {
                    ui.heading("Turbopump");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Open-source preliminary design of turbomachinery.");
                    ui.label("License: MIT OR Apache-2.0");
                    ui.add_space(6.0);
                    ui.button("Close").clicked()
                });
                (!response.inner && !response.should_close()).then_some(Dialog::About)
            }
        };
        if self.dialog.is_none() {
            self.dialog = keep;
        }
    }

    fn finish(&mut self, pending: Pending, ctx: &egui::Context) {
        let quit = matches!(pending, Pending::Quit);
        self.proceed(pending);
        if quit {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

enum Choice {
    Save,
    Discard,
    Cancel,
}

impl eframe::App for TurbopumpApp {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let render_state = frame.wgpu_render_state().cloned();
        self.show(ui, render_state.as_ref());
    }
}

/// Draws the dock tabs.
struct Views<'a> {
    cx: EditCtx<'a>,
    active: Option<ComponentId>,
    meridional: &'a mut MeridionalView,
    viewer: &'a mut Viewer,
    render_state: Option<&'a RenderState>,
}

impl egui_dock::TabViewer for Views<'_> {
    type Tab = View;

    fn id(&mut self, tab: &mut View) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut View) -> egui::WidgetText {
        tab.title().into()
    }

    fn is_closeable(&self, _tab: &View) -> bool {
        false
    }

    /// Plots and the 3D view fill the tab; the report scrolls on its own.
    fn scroll_bars(&self, _tab: &View) -> [bool; 2] {
        [false, false]
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut View) {
        let Some(id) = self.active else {
            ui.centered_and_justified(|ui| {
                ui.weak("Add an impeller to see its geometry (Project > Add component).");
            });
            return;
        };
        let units = self.cx.units();
        match tab {
            View::Meridional => self.meridional.show(ui, &mut self.cx, id),
            View::Blade => blade::show(ui, self.cx.derived, id, &units),
            View::FrontView => front::show(ui, self.cx.derived, id, &units),
            View::View3d => self.viewer.show(ui, self.render_state, self.cx.derived, id),
            View::Report => report::show(ui, self.cx.doc.project(), self.cx.derived, id, &units),
        }
    }
}

/// Colors for step states, readable on light and dark themes.
pub(crate) fn status_color(ui: &Ui, status: tp_project::StepStatus) -> Color32 {
    use tp_project::StepStatus;
    let dark = ui.visuals().dark_mode;
    match status {
        StepStatus::Done if dark => Color32::from_rgb(110, 200, 120),
        StepStatus::Done => Color32::from_rgb(30, 130, 50),
        StepStatus::Stale if dark => Color32::from_rgb(230, 170, 70),
        StepStatus::Stale => Color32::from_rgb(180, 110, 0),
        StepStatus::Pending => ui.visuals().weak_text_color(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::meridional::{Handle, handle_position};
    use egui_kittest::Harness;
    use egui_plot::PlotPoint;
    use tp_design::impeller::meridian::DEFAULT_TANGENT;

    /// Screen position of a meridional handle in the last frame.
    fn handle_on_screen(app: &TurbopumpApp, handle: Handle) -> egui::Pos2 {
        let id = app.active_impeller().unwrap();
        let g = app.derived.geometry(id).unwrap();
        let p = handle_position(handle, &g.contour);
        let scale = app
            .doc
            .project()
            .display_units
            .to_display(QuantityKind::Length, 1.0);
        app.meridional
            .transform()
            .unwrap()
            .position_from_point(&PlotPoint::new(p.x * scale, p.y * scale))
    }

    #[test]
    fn dragging_a_meridional_handle_is_one_undo_step() {
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, None));
        harness.run();
        let id = harness.state().active_impeller().unwrap();
        let tangent = |app: &TurbopumpApp| {
            app.doc
                .project()
                .impeller_geometry_input(id)
                .unwrap()
                .meridional
                .hub_inlet_tangent
        };

        let start = handle_on_screen(harness.state(), Handle::HubInlet);
        harness.hover_at(start);
        harness.run();
        harness.drag_at(start);
        harness.run();
        for step in 1..=6 {
            harness.hover_at(start + egui::vec2(8.0 * step as f32, 0.0));
            harness.run();
        }
        harness.drop_at(start + egui::vec2(48.0, 0.0));
        harness.run();

        let dragged = tangent(harness.state()).manual_value();
        assert!(
            dragged.is_some_and(|t| t > DEFAULT_TANGENT),
            "the drag should lengthen the hub inlet tangent: {dragged:?}"
        );
        harness.state_mut().undo();
        assert!(tangent(harness.state()).is_auto());
        assert!(!harness.state().doc.history().can_undo());
    }
}
