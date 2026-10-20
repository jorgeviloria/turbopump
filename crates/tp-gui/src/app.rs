//! The application window: toolbar, panels, the main view and dialogs.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Button, CentralPanel, Frame, Key, KeyboardShortcut, Margin, Modal, Modifiers, Panel,
    RichText, Ui, ViewportCommand,
};
use eframe::egui_wgpu::RenderState;
use tp_project::{
    ComponentId, ComponentKind, DesignPoint, DesignStep, Document, EnergyTransfer, FlowRate,
    GlobalSetup, MachineType, Project, ProjectError,
};
use tp_units::{DisplayUnits, QuantityKind};

use crate::derived::Derived;
use crate::export::{self, Exports};
use crate::messages::{self, Message};
use crate::ribbon::{Command, Ribbon, RibbonCx};
use crate::steps::AcceptState;
use crate::theme::{self, Palette, icon};
use crate::tree::StageParts;
use crate::viewer::Viewer;
use crate::views::performance::PerformanceView;
use crate::views::{blade, front, meridional::MeridionalView, report};
use crate::{default_project, steps, tree, widgets};

/// Where the documentation lives.
const PROJECT_PAGE: &str = "https://github.com/jorgeviloria/turbopump";

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
    /// Whether the properties panel of the selected step is open.
    pub properties: &'a mut bool,
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

    /// Selects `selection` and opens its properties.
    pub fn open(&mut self, selection: Selection) {
        *self.selection = selection;
        *self.properties = true;
    }
}

/// The views of the main area, one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    Meridional,
    Blade,
    FrontView,
    View3d,
    Performance,
    Report,
}

impl View {
    pub const ALL: [View; 6] = [
        View::Meridional,
        View::Blade,
        View::FrontView,
        View::View3d,
        View::Performance,
        View::Report,
    ];

    pub fn title(self) -> &'static str {
        match self {
            View::Meridional => "Meridional view",
            View::Blade => "Blade",
            View::FrontView => "Front view",
            View::View3d => "3D view",
            View::Performance => "Performance curves",
            View::Report => "Report",
        }
    }

    /// Name under a toolbar button.
    pub fn short_title(self) -> &'static str {
        match self {
            View::Meridional => "Meridional",
            View::Blade => "Blade",
            View::FrontView => "Front",
            View::View3d => "3D",
            View::Performance => "Curves",
            View::Report => "Report",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            View::Meridional => icon::BEZIER_CURVE,
            View::Blade => icon::WAVE_SINE,
            View::FrontView => icon::FAN,
            View::View3d => icon::CUBE,
            View::Performance => icon::CHART_LINE,
            View::Report => icon::TABLE,
        }
    }
}

/// Default width of the properties panel.
const SIDEBAR_WIDTH: f32 = 420.0;
/// Width of the folded properties panel.
const SIDEBAR_STRIP: f32 = 30.0;
/// Gap between the properties panel and the edges of the view.
const SIDEBAR_MARGIN: f32 = 8.0;

/// The folded properties panel: a strip with its name written upward. Returns whether
/// it was clicked.
fn folded_strip(ui: &mut Ui, rect: egui::Rect, p: &Palette) -> bool {
    let response = ui
        .interact(
            rect,
            egui::Id::new("properties_strip"),
            egui::Sense::click(),
        )
        .on_hover_text("Show the properties");
    if response.hovered() {
        ui.painter().rect_filled(rect, 8, p.hover);
    }
    let painter = ui.painter();
    painter.text(
        rect.center_top() + egui::vec2(0.0, 18.0),
        egui::Align2::CENTER_CENTER,
        icon::CARET_LEFT,
        egui::FontId::proportional(16.0),
        p.text,
    );
    let galley = painter.layout_no_wrap(
        format!("{}  Properties", icon::SLIDERS_HORIZONTAL),
        egui::FontId::proportional(13.0),
        p.text,
    );
    let size = galley.size();
    let anchor = egui::pos2(rect.center().x - size.y / 2.0, rect.top() + 40.0 + size.x);
    painter.add(
        egui::epaint::TextShape::new(anchor, galley, p.text)
            .with_angle(-std::f32::consts::FRAC_PI_2),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Show the properties")
    });
    response.clicked()
}

/// The view that suits a step best, if any.
fn view_for(kind: ComponentKind, step: DesignStep) -> Option<View> {
    use DesignStep::*;
    match (kind, step) {
        (_, ModelSettings) => Some(View::View3d),
        (ComponentKind::Impeller, MeridionalContour) => Some(View::Meridional),
        (
            ComponentKind::Impeller,
            BladeProperties | BladeMeanLines | BladeProfiles | BladeEdges,
        ) => Some(View::Blade),
        (ComponentKind::Volute, CrossSection | WallThickness) => Some(View::Meridional),
        (ComponentKind::Volute, VoluteSetup | SpiralDevelopment | Diffuser | CutWater) => {
            Some(View::FrontView)
        }
        (ComponentKind::Stator { .. }, MainDimensions | MeridionalContour) => {
            Some(View::Meridional)
        }
        (ComponentKind::Stator { .. }, _) => Some(View::FrontView),
        _ => None,
    }
}

/// The trees of the left panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    Design,
    Model,
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
    volute: bool,
}

impl Default for NewProjectForm {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            flow: 100.0 / 3600.0,
            head: 32.0,
            speed: 2900.0 * std::f64::consts::PI / 30.0,
            fluid: "Water".into(),
            volute: true,
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
        if self.volute {
            project.add_component(ComponentKind::Volute, None)?;
        }
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
    /// The view in the main area.
    view: View,
    /// Whether the dialog of the selected step is open.
    sidebar_open: bool,
    /// The selection the dialog and the view last followed.
    followed: Option<Selection>,
    /// The main view changes to suit the step that opens.
    follow_step: bool,
    /// Whether the tree panel is open.
    tree_open: bool,
    /// Width of the open properties panel, set by dragging its edge.
    sidebar_width: f32,
    meridional: MeridionalView,
    performance: PerformanceView,
    viewer: Viewer,
    exports: Exports,
    status: Option<Status>,
    dialog: Option<Dialog>,
    ribbon: Ribbon,
    /// Which tree the left panel shows.
    left: LeftTab,
    /// Whether the messages panel is open.
    show_messages: bool,
    /// Fonts and styles are installed on the first frame.
    styled: bool,
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
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<&Path>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut app = Self::with_document(Document::new(default_project()));
        app.styled = true;
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
            view: View::View3d,
            sidebar_open: true,
            followed: None,
            follow_step: true,
            tree_open: true,
            sidebar_width: SIDEBAR_WIDTH,
            meridional: MeridionalView::default(),
            performance: PerformanceView::default(),
            viewer: Viewer::default(),
            exports: Exports::default(),
            status: None,
            dialog: None,
            ribbon: Ribbon::default(),
            left: LeftTab::Design,
            show_messages: false,
            styled: false,
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

    /// Shows `view` in the main area.
    pub fn focus_view(&mut self, view: View) {
        self.view = view;
    }

    /// The view in the main area.
    pub fn view(&self) -> View {
        self.view
    }

    /// Whether the dialog of the selected step is open.
    pub fn sidebar_open(&self) -> bool {
        self.sidebar_open
    }

    /// Draws the whole window. `render_state` enables the 3D view.
    pub fn show(&mut self, ui: &mut Ui, render_state: Option<&RenderState>) {
        let ctx = ui.ctx().clone();
        if !self.styled {
            // Fonts load at the start of the next pass: draw nothing until then.
            theme::install(&ctx);
            self.styled = true;
            ctx.request_discard("installing the fonts");
            return;
        }
        self.handle_shortcuts(&ctx);
        self.handle_close_request(&ctx);
        self.derived.update(self.doc.project());
        self.fix_selection();
        self.update_title(&ctx);
        self.exports.poll();
        let p = Palette::of(ui);
        let all_messages = messages::all(self.doc.project(), &self.derived);

        let mut command = None;
        Panel::top("ribbon")
            .frame(Frame::new().fill(p.ribbon))
            .show_separator_line(false)
            .show(ui, |ui| {
                let accept = match self.selection {
                    Selection::Step(id, step) => {
                        steps::accept_state(self.doc.project(), &self.derived, id, step)
                    }
                    Selection::Setup => AcceptState::NotApplicable,
                };
                let impeller = self.active_impeller();
                let volute = self.active_volute();
                let history = self.doc.history();
                let mut cx = RibbonCx {
                    project: self.doc.project(),
                    selection: self.selection,
                    dirty: self.doc.is_dirty(),
                    can_undo: history.can_undo(),
                    can_redo: history.can_redo(),
                    undo_label: history.undo_label(),
                    redo_label: history.redo_label(),
                    accept,
                    has_geometry: impeller.is_some_and(|id| self.derived.geometry(id).is_some()),
                    has_volute: volute.is_some_and(|id| self.derived.volute(id).is_some()),
                    cad_running: self.exports.cad_running(),
                    options: &mut self.viewer.options,
                    theme: ctx.options(|o| o.theme_preference),
                    tree_open: self.tree_open,
                    sidebar_open: self.sidebar_open,
                    messages_open: self.show_messages,
                    follow_step: self.follow_step,
                };
                command = self.ribbon.show(ui, &mut cx);
            });
        Panel::bottom("status_bar")
            .frame(
                Frame::new()
                    .fill(p.strip)
                    .inner_margin(Margin::symmetric(8, 3)),
            )
            .show(ui, |ui| self.status_bar(ui, &all_messages));
        if self.show_messages {
            Panel::bottom("messages")
                .resizable(true)
                .default_size(140.0)
                .size_range(70.0..=420.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::bold("Messages"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(Button::new(icon::X).frame(false))
                                .on_hover_text("Close")
                                .clicked()
                            {
                                self.show_messages = false;
                            }
                        });
                    });
                    if let Some(id) = messages::panel(ui, self.doc.project(), &all_messages) {
                        let mut cx = self.edit_ctx();
                        if let Some(step) = tree::entry_step(&cx, id) {
                            cx.open(Selection::Step(id, step));
                        }
                    }
                });
        }
        Panel::left("component_bar")
            .exact_size(62.0)
            .resizable(false)
            .frame(Frame::new().fill(p.strip))
            .show(ui, |ui| {
                let mut cx = self.edit_ctx();
                tree::component_bar(ui, &mut cx);
            });
        let stage = StageParts {
            stator: self
                .active_stator()
                .and_then(|id| self.doc.project().component(id))
                .map(|c| matches!(c.kind(), ComponentKind::Stator { vaned: true })),
            volute: self.active_volute().is_some(),
        };
        Panel::left("project_tree")
            .default_size(250.0)
            .size_range(200.0..=420.0)
            .show_collapsible(ui, &mut self.tree_open, |ui| {
                ui.add_space(6.0);
                ui.columns(2, |columns| {
                    let tabs = [
                        (LeftTab::Design, format!("{}  Design", icon::TREE_STRUCTURE)),
                        (LeftTab::Model, format!("{}  3D model", icon::CUBE)),
                    ];
                    for (ui, (tab, label)) in columns.iter_mut().zip(tabs) {
                        let button = Button::selectable(self.left == tab, label)
                            .min_size(egui::vec2(ui.available_width(), 24.0));
                        if ui.add(button).clicked() {
                            self.left = tab;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| match self.left {
                    LeftTab::Design => {
                        let mut cx = EditCtx {
                            doc: &mut self.doc,
                            derived: &self.derived,
                            selection: &mut self.selection,
                            status: &mut self.status,
                            exports: &mut self.exports,
                            properties: &mut self.sidebar_open,
                        };
                        tree::design_tree(ui, &mut cx);
                    }
                    LeftTab::Model => tree::model_tree(ui, &mut self.viewer.options, stage),
                });
            });
        let mut main_area = ui.available_rect_before_wrap();
        CentralPanel::no_frame().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.view_bar(ui);
            main_area = ui.available_rect_before_wrap();
            // The width the floating properties panel hides on the right of the view.
            let cover = self.sidebar_extent(ui.ctx(), main_area) + SIDEBAR_MARGIN;
            let active = self.active_impeller();
            let stator = self.active_stator();
            let volute = self.active_volute();
            let mut cx = EditCtx {
                doc: &mut self.doc,
                derived: &self.derived,
                selection: &mut self.selection,
                status: &mut self.status,
                exports: &mut self.exports,
                properties: &mut self.sidebar_open,
            };
            let Some(id) = active else {
                ui.centered_and_justified(|ui| {
                    ui.weak("Add an impeller to see its geometry (Home > Components).");
                });
                return;
            };
            let units = cx.units();
            let area = ui.available_rect_before_wrap();
            ui.painter()
                .rect_filled(area, 0, ui.visuals().extreme_bg_color);
            // Every view fills the whole area; the properties panel floats over it. The
            // 3D view keeps its model and controls clear of the panel.
            let target = if self.view == View::View3d {
                area
            } else {
                area.shrink2(egui::vec2(6.0, 4.0))
            };
            ui.scope_builder(egui::UiBuilder::new().max_rect(target), |ui| {
                ui.set_min_size(target.size());
                match self.view {
                    View::Meridional => {
                        self.meridional.show(ui, &mut cx, id, stator, volute);
                    }
                    View::Blade => blade::show(ui, cx.derived, id, &units),
                    View::FrontView => {
                        front::show(ui, cx.derived, id, stator, volute, &units);
                    }
                    View::View3d => self.viewer.show(
                        ui,
                        render_state,
                        cx.derived,
                        id,
                        stator,
                        volute,
                        &units,
                        cover,
                    ),
                    View::Performance => {
                        self.performance
                            .show(ui, cx.doc.project(), cx.derived, id, &units);
                    }
                    View::Report => {
                        report::show(ui, cx.doc.project(), cx.derived, id, stator, volute, &units)
                    }
                }
            });
        });
        self.properties_panel(&ctx, main_area);

        if let Some(command) = command {
            self.run(command, &ctx);
        }
        self.dialogs(&ctx);
        self.exports.cad_window(
            &ctx,
            &self.derived,
            self.doc.project(),
            self.active_impeller(),
            self.active_stator(),
            self.active_volute(),
        );

        // A new selection opens its dialog and, if asked, the view that suits it.
        if self.followed != Some(self.selection) {
            self.sidebar_open = true;
            if self.follow_step
                && let Selection::Step(id, step) = self.selection
                && let Some(kind) = self.doc.project().component(id).map(|c| c.kind())
                && let Some(view) = view_for(kind, step)
            {
                self.view = view;
            }
            self.followed = Some(self.selection);
        }

        // A gesture ends when the pointer is released and no field is being typed in.
        let idle = !ctx.input(|i| i.pointer.any_down()) && !ctx.text_edit_focused();
        if idle {
            self.doc.end_gesture();
        }
    }

    /// The strip above the main view: the tree toggle, the views and the dialog toggle.
    fn view_bar(&mut self, ui: &mut Ui) {
        let p = Palette::of(ui);
        Frame::new()
            .fill(p.strip)
            .inner_margin(Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let tip = if self.tree_open {
                        "Hide the tree"
                    } else {
                        "Show the tree"
                    };
                    if ui
                        .add(Button::selectable(
                            self.tree_open,
                            RichText::new(icon::SIDEBAR_SIMPLE).size(16.0),
                        ))
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.tree_open = !self.tree_open;
                    }
                    const TAB: f32 = 104.0;
                    let total = TAB * View::ALL.len() as f32;
                    let free = ui.available_width() - total - 150.0;
                    ui.add_space((free / 2.0).max(8.0));
                    for view in View::ALL {
                        let text = format!("{}  {}", view.icon(), view.short_title());
                        let button = Button::selectable(self.view == view, text)
                            .min_size(egui::vec2(TAB, 26.0));
                        if ui.add(button).on_hover_text(view.title()).clicked() {
                            self.view = view;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let text = format!("{}  Properties", icon::SLIDERS_HORIZONTAL);
                        if ui
                            .add(Button::selectable(self.sidebar_open, text))
                            .on_hover_text("Show or hide the dialog of the selected step")
                            .clicked()
                        {
                            self.sidebar_open = !self.sidebar_open;
                        }
                    });
                });
            });
    }

    /// Width the properties panel takes this frame, animated between folded and open.
    fn sidebar_extent(&self, ctx: &egui::Context, area: egui::Rect) -> f32 {
        let open =
            ctx.animate_bool_with_time(egui::Id::new("properties_open"), self.sidebar_open, 0.15);
        let full = self
            .sidebar_width
            .clamp(300.0, (area.width() * 0.7).max(300.0));
        SIDEBAR_STRIP + (full - SIDEBAR_STRIP) * open
    }

    /// The properties of the selected step, in a panel that floats over the right of
    /// the view. It never shrinks the view; folded, it leaves a narrow strip.
    fn properties_panel(&mut self, ctx: &egui::Context, area: egui::Rect) {
        let width = self.sidebar_extent(ctx, area);
        let rect = egui::Rect::from_min_max(
            egui::pos2(
                area.right() - width - SIDEBAR_MARGIN,
                area.top() + SIDEBAR_MARGIN,
            ),
            egui::pos2(
                area.right() - SIDEBAR_MARGIN,
                area.bottom() - SIDEBAR_MARGIN,
            ),
        );
        let style = ctx.style_of(ctx.theme());
        let p = Palette::for_dark(style.visuals.dark_mode);
        let shadow = style.visuals.popup_shadow;
        let mut toggle = false;
        egui::Area::new(egui::Id::new("properties_panel"))
            .order(egui::Order::Middle)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let frame = Frame::new()
                    .fill(ui.visuals().window_fill)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .corner_radius(8)
                    .shadow(shadow);
                frame.show(ui, |ui| {
                    ui.set_min_size(rect.size());
                    ui.set_max_size(rect.size());
                    if width < SIDEBAR_STRIP + 40.0 {
                        toggle = folded_strip(ui, rect, &p);
                        return;
                    }
                    // The left edge resizes the panel.
                    let edge = egui::Rect::from_min_max(
                        rect.left_top(),
                        egui::pos2(rect.left() + 5.0, rect.bottom()),
                    );
                    let resize = ui
                        .interact(
                            edge,
                            egui::Id::new("properties_resize"),
                            egui::Sense::drag(),
                        )
                        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                    if resize.dragged() {
                        self.sidebar_width -= resize.drag_delta().x;
                    }
                    Frame::new()
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_min_size(ui.available_size());
                            ui.horizontal(|ui| {
                                let title = steps::title(self.doc.project(), self.selection);
                                ui.add(egui::Label::new(theme::bold(title).size(15.0)).truncate());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add(Button::new(icon::CARET_RIGHT).frame(false))
                                            .on_hover_text("Fold the properties")
                                            .clicked()
                                        {
                                            toggle = true;
                                        }
                                    },
                                );
                            });
                            ui.separator();
                            let mut cx = self.edit_ctx();
                            Panel::bottom("properties_footer")
                                .frame(Frame::new().inner_margin(Margin::symmetric(0, 6)))
                                .show(ui, |ui| steps::footer(ui, &mut cx));
                            steps::show(ui, &mut cx);
                        });
                });
            });
        if toggle {
            self.sidebar_open = !self.sidebar_open;
        }
        if self.sidebar_open
            && ctx.input(|i| i.key_pressed(Key::Escape))
            && !ctx.text_edit_focused()
        {
            self.sidebar_open = false;
        }
    }

    /// Carries out a toolbar command.
    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::NewProject => {
                self.dialog = Some(Dialog::NewProject(NewProjectForm::default()));
            }
            Command::Open => self.open_dialog(),
            Command::Save => {
                self.save();
            }
            Command::SaveAs => {
                self.save_as();
            }
            Command::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::GlobalSetup => self.edit_ctx().open(Selection::Setup),
            Command::UsUnits(us) => {
                let preset = if us {
                    DisplayUnits::us()
                } else {
                    DisplayUnits::si()
                };
                self.edit_ctx().edit("change units", |p| {
                    p.display_units = preset;
                    Ok(())
                });
            }
            Command::Add(index) => tree::add_component(&mut self.edit_ctx(), index),
            Command::Remove(id) => tree::remove_component(&mut self.edit_ctx(), id),
            Command::Select(id, step) => self.edit_ctx().open(Selection::Step(id, step)),
            Command::Accept(id, step) => steps::accept(&mut self.edit_ctx(), id, step),
            Command::Reset(id, step) => {
                let project = self.doc.project().clone();
                steps::reset(&mut self.edit_ctx(), &project, id, step);
            }
            Command::ExportText(kind) => {
                if let Some(g) = self
                    .active_impeller()
                    .and_then(|id| self.derived.geometry(id))
                {
                    let result = export::save_text(kind, g, &self.doc.project().info.name);
                    self.report_export(result);
                }
            }
            Command::ExportVolute(kind) => {
                if let Some(g) = self.active_volute().and_then(|id| self.derived.volute(id)) {
                    let result = export::save_volute(kind, g, &self.doc.project().info.name);
                    self.report_export(result);
                }
            }
            Command::ExportCad => self.exports.open_cad(),
            Command::Focus(view) => self.focus_view(view),
            Command::Camera(view) => {
                self.focus_view(View::View3d);
                self.viewer.apply(view);
            }
            Command::ToggleTree => self.tree_open = !self.tree_open,
            Command::ToggleDialog => self.sidebar_open = !self.sidebar_open,
            Command::ToggleMessages => self.show_messages = !self.show_messages,
            Command::FollowStep => self.follow_step = !self.follow_step,
            Command::Theme(preference) => ctx.set_theme(preference),
            Command::About => self.dialog = Some(Dialog::About),
            Command::Documentation => ctx.open_url(egui::OpenUrl::new_tab(PROJECT_PAGE)),
        }
    }

    fn report_export(&mut self, result: Result<Option<PathBuf>, String>) {
        match result {
            Ok(Some(path)) => self.info(format!("Wrote {}", path.display())),
            Ok(None) => {}
            Err(e) => self.error(e),
        }
    }

    fn edit_ctx(&mut self) -> EditCtx<'_> {
        EditCtx {
            doc: &mut self.doc,
            derived: &self.derived,
            selection: &mut self.selection,
            status: &mut self.status,
            exports: &mut self.exports,
            properties: &mut self.sidebar_open,
        }
    }

    /// The impeller the views show: the selected one, the one feeding the selected
    /// component, or the first.
    fn active_impeller(&self) -> Option<ComponentId> {
        let components = self.doc.project().components();
        let selected = match self.selection {
            Selection::Step(id, _) => components.iter().position(|c| c.id() == id),
            Selection::Setup => None,
        };
        if let Some(index) = selected
            && let Some(c) = components[..=index]
                .iter()
                .rev()
                .find(|c| c.kind() == ComponentKind::Impeller)
        {
            return Some(c.id());
        }
        components
            .iter()
            .find(|c| c.kind() == ComponentKind::Impeller)
            .map(|c| c.id())
    }

    /// The stator right after the active impeller, if any.
    fn active_stator(&self) -> Option<ComponentId> {
        let components = self.doc.project().components();
        let impeller = self.active_impeller()?;
        let index = components.iter().position(|c| c.id() == impeller)?;
        components
            .get(index + 1)
            .filter(|c| matches!(c.kind(), ComponentKind::Stator { .. }))
            .map(|c| c.id())
    }

    /// The volute of the active stage: right after the impeller, or after its stator.
    fn active_volute(&self) -> Option<ComponentId> {
        let components = self.doc.project().components();
        let impeller = self.active_impeller()?;
        let mut index = components.iter().position(|c| c.id() == impeller)? + 1;
        if self.active_stator().is_some() {
            index += 1;
        }
        components
            .get(index)
            .filter(|c| c.kind() == ComponentKind::Volute)
            .map(|c| c.id())
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

    fn status_bar(&mut self, ui: &mut Ui, all_messages: &[Message]) {
        let p = Palette::of(ui);
        ui.horizontal(|ui| {
            match &self.status {
                Some(Status { text, error: true }) => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{}  {text}", icon::X_CIRCLE),
                    );
                }
                Some(Status { text, .. }) => {
                    ui.label(format!("{}  {text}", icon::INFO));
                }
                None => {
                    ui.label(RichText::new("Ready").color(p.weak));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (errors, warnings) = messages::count(all_messages);
                let text = if errors + warnings == 0 {
                    RichText::new(format!("{}  No problems", icon::CHECK_CIRCLE)).color(p.done)
                } else {
                    let color = if errors > 0 {
                        ui.visuals().error_fg_color
                    } else {
                        ui.visuals().warn_fg_color
                    };
                    RichText::new(format!(
                        "{} {errors}   {} {warnings}",
                        icon::X_CIRCLE,
                        icon::WARNING
                    ))
                    .color(color)
                };
                if ui
                    .add(Button::selectable(self.show_messages, text))
                    .on_hover_text("Show every error and warning of the project")
                    .clicked()
                {
                    self.show_messages = !self.show_messages;
                }
                ui.separator();
                let units = if self.doc.project().display_units == DisplayUnits::us() {
                    "US units"
                } else {
                    "SI units"
                };
                ui.label(RichText::new(units).color(p.weak));
                ui.separator();
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
                ui.label(RichText::new(source).color(p.weak));
                if self.exports.cad_running() {
                    ui.separator();
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
                            ui.label("Casing");
                            ui.checkbox(&mut form.volute, "volute after the impeller");
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
        // The dialog stays open: it does not block the views.
        harness.state_mut().focus_view(View::Meridional);
        harness.run();
        assert!(harness.state().sidebar_open);
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
