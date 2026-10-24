//! The application window: toolbar, panels, the main view and dialogs.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Button, CentralPanel, Frame, Key, KeyboardShortcut, Margin, Modifiers, Panel, RichText,
    ThemePreference, Ui, ViewportCommand,
};
use eframe::egui_wgpu::RenderState;
use tp_project::{ComponentId, ComponentKind, DesignStep, Document, Project, ProjectError};
use tp_units::DisplayUnits;

use crate::derived::Derived;
use crate::dialogs::{self, Choice};
use crate::export::{self, Exports};
use crate::i18n::{self, Language, message, project_name, tf, tr};
use crate::messages::{self, Message};
use crate::new_project::{NewProjectForm, Outcome};
use crate::objective;
use crate::ribbon::{Command, Ribbon, RibbonCx};
use crate::settings::{self, Section, SettingsCx};
use crate::steps::AcceptState;
use crate::theme::{self, Palette, icon};
use crate::tree::StageParts;
use crate::viewer::Viewer;
use crate::views::cross_section::CrossSectionView;
use crate::views::performance::PerformanceView;
use crate::views::{blade, front, meridional::MeridionalView, report};
use crate::{default_project, steps, tree};

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
                self.error(format!("{}: {}", message(label), message(&e.to_string())));
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
    MeridianEditor,
    CrossSection,
    Blade,
    FrontView,
    View3d,
    Performance,
    Report,
}

impl View {
    pub const ALL: [View; 8] = [
        View::Meridional,
        View::MeridianEditor,
        View::CrossSection,
        View::Blade,
        View::FrontView,
        View::View3d,
        View::Performance,
        View::Report,
    ];

    pub fn title(self) -> &'static str {
        tr(match self {
            View::Meridional => "Meridional view",
            View::MeridianEditor => "Meridian editor",
            View::CrossSection => "Volute cross sections",
            View::Blade => "Blade",
            View::FrontView => "Front view",
            View::View3d => "3D view",
            View::Performance => "Performance curves",
            View::Report => "Report",
        })
    }

    /// Name under a toolbar button.
    pub fn short_title(self) -> &'static str {
        tr(match self {
            View::Meridional => "Meridional",
            View::MeridianEditor => "Editor",
            View::CrossSection => "Section",
            View::Blade => "Blade",
            View::FrontView => "Front",
            View::View3d => "3D",
            View::Performance => "Curves",
            View::Report => "Report",
        })
    }

    pub fn icon(self) -> &'static str {
        match self {
            View::Meridional => icon::BEZIER_CURVE,
            View::MeridianEditor => icon::PENCIL_RULER,
            View::CrossSection => icon::CIRCLE_HALF,
            View::Blade => icon::WAVE_SINE,
            View::FrontView => icon::FAN,
            View::View3d => icon::CUBE,
            View::Performance => icon::CHART_LINE,
            View::Report => icon::TABLE,
        }
    }
}

/// Key of the language in the stored preferences.
const LANGUAGE_KEY: &str = "language";
/// Keys of the theme, the interface scale and "view follows step".
const THEME_KEY: &str = "theme";
const ZOOM_KEY: &str = "zoom";
const FOLLOW_STEP_KEY: &str = "follow_step";

/// Code of a theme in the stored preferences.
fn theme_code(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
        ThemePreference::System => "system",
    }
}

fn theme_from_code(code: &str) -> Option<ThemePreference> {
    [
        ThemePreference::Light,
        ThemePreference::Dark,
        ThemePreference::System,
    ]
    .into_iter()
    .find(|&t| theme_code(t) == code)
}

/// Spanish if the system speaks it, else English.
fn system_language() -> Language {
    let spanish = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .any(|v| v.starts_with("es"));
    if spanish {
        Language::Spanish
    } else {
        Language::English
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
        .on_hover_text(tr("Show the properties"));
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
        format!("{}  {}", icon::SLIDERS_HORIZONTAL, tr("Properties")),
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
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("Show the properties"))
    });
    response.clicked()
}

/// The view that suits a step best, if any.
fn view_for(kind: ComponentKind, step: DesignStep) -> Option<View> {
    use DesignStep::*;
    match (kind, step) {
        (_, ModelSettings) => Some(View::View3d),
        (ComponentKind::Impeller, MeridionalContour) => Some(View::MeridianEditor),
        (
            ComponentKind::Impeller,
            BladeProperties | BladeMeanLines | BladeProfiles | BladeEdges,
        ) => Some(View::Blade),
        (ComponentKind::Volute, CrossSection) => Some(View::CrossSection),
        (ComponentKind::Volute, WallThickness) => Some(View::Meridional),
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

#[derive(Debug)]
enum Dialog {
    NewProject(NewProjectForm),
    ConfirmDiscard(Pending),
    About,
    Settings(Section),
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
    /// The view shown last frame.
    shown: Option<View>,
    /// The main view changes to suit the step that opens.
    follow_step: bool,
    /// Whether the tree panel is open.
    tree_open: bool,
    /// Language of the interface, kept between sessions.
    language: Language,
    /// Theme and interface scale last frame, kept between sessions.
    theme: ThemePreference,
    zoom: f32,
    /// Width of the open properties panel, set by dragging its edge.
    sidebar_width: f32,
    meridional: MeridionalView,
    cross_section: CrossSectionView,
    performance: PerformanceView,
    blade: blade::BladeView,
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
        // The stored choices, else the system's language and the light theme. Without
        // storage (tests), English and the theme the harness sets.
        if let Some(storage) = cc.storage {
            app.language = storage
                .get_string(LANGUAGE_KEY)
                .and_then(|code| Language::from_code(&code))
                .unwrap_or_else(system_language);
            let theme = storage
                .get_string(THEME_KEY)
                .and_then(|code| theme_from_code(&code))
                .unwrap_or(ThemePreference::Light);
            cc.egui_ctx.set_theme(theme);
            if let Some(zoom) = storage.get_string(ZOOM_KEY).and_then(|z| z.parse().ok()) {
                cc.egui_ctx.set_zoom_factor(zoom);
            }
            if let Some(follow) = storage
                .get_string(FOLLOW_STEP_KEY)
                .and_then(|f| f.parse().ok())
            {
                app.follow_step = follow;
            }
        }
        i18n::set(app.language);
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
            shown: None,
            follow_step: true,
            tree_open: true,
            language: Language::English,
            theme: ThemePreference::Light,
            zoom: 1.0,
            sidebar_width: SIDEBAR_WIDTH,
            meridional: MeridionalView::default(),
            cross_section: CrossSectionView::default(),
            performance: PerformanceView::default(),
            blade: blade::BladeView::default(),
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
        i18n::set(self.language);
        self.theme = ctx.options(|o| o.theme_preference);
        self.zoom = ctx.zoom_factor();
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
                    tree_open: self.tree_open,
                    sidebar_open: self.sidebar_open,
                    messages_open: self.show_messages,
                    follow_step: self.follow_step,
                };
                command = self.ribbon.show(ui, &mut cx);
            });
        let mut objective = None;
        Panel::top("objective")
            .frame(
                Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(Margin::symmetric(8, 2)),
            )
            .show(ui, |ui| {
                let units = self.doc.project().display_units.clone();
                objective = objective::show(
                    ui,
                    self.doc.project(),
                    &self.derived,
                    self.active_impeller(),
                    &units,
                );
            });
        match objective {
            Some(objective::Action::Setup) => self.edit_ctx().open(Selection::Setup),
            Some(objective::Action::Curves) => self.view = View::Performance,
            Some(objective::Action::Step(id, step)) => {
                self.edit_ctx().open(Selection::Step(id, step));
            }
            None => {}
        }
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
                        ui.label(theme::bold(tr("Messages")));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(Button::new(icon::X).frame(false))
                                .on_hover_text(tr("Close"))
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
                        (
                            LeftTab::Design,
                            format!("{}  {}", icon::TREE_STRUCTURE, tr("Design")),
                        ),
                        (
                            LeftTab::Model,
                            format!("{}  {}", icon::CUBE, tr("3D model")),
                        ),
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
            // The editor opens on the meridian, framed, with its step in the panel.
            if self.view == View::MeridianEditor && self.shown != Some(View::MeridianEditor) {
                self.meridional.framed = false;
                if let Some(id) = active {
                    self.selection = Selection::Step(id, DesignStep::MeridionalContour);
                }
            }
            self.shown = Some(self.view);
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
                    ui.weak(tr(
                        "Add an impeller to see its geometry (Home > Components).",
                    ));
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
                    View::MeridianEditor => {
                        self.meridional
                            .show_editor(ui, &mut cx, id, stator, volute, cover);
                    }
                    View::CrossSection => self.cross_section.show(ui, &mut cx, volute, cover),
                    View::Blade => self.blade.show(ui, &mut cx, id, &units, cover),
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
                            .show(ui, cx.doc.project(), cx.derived, id, &units, cover);
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
                        tr("Hide the tree")
                    } else {
                        tr("Show the tree")
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
                        let text = format!("{}  {}", icon::SLIDERS_HORIZONTAL, tr("Properties"));
                        if ui
                            .add(Button::selectable(self.sidebar_open, text))
                            .on_hover_text(tr("Show or hide the dialog of the selected step"))
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
                                            .on_hover_text(tr("Fold the properties"))
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
                if self.doc.project().display_units != preset {
                    self.edit_ctx().edit(tr("change units"), |p| {
                        p.display_units = preset;
                        Ok(())
                    });
                }
            }
            Command::Unit(kind, symbol) => {
                let mut units = self.doc.project().display_units.clone();
                if units.get(kind).symbol != symbol && units.set(kind, symbol).is_ok() {
                    self.edit_ctx().edit(tr("change units"), |p| {
                        p.display_units = units;
                        Ok(())
                    });
                }
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
            Command::ExportDrawing(kind) => {
                if let Some(g) = self
                    .active_impeller()
                    .and_then(|id| self.derived.geometry(id))
                {
                    let result = export::save_drawing(kind, g, &self.doc.project().info.name);
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
            Command::Zoom(zoom) => ctx.set_zoom_factor(zoom),
            Command::Language(language) => {
                self.language = language;
                i18n::set(language);
            }
            Command::About => self.dialog = Some(Dialog::About),
            Command::Settings(section) => self.dialog = Some(Dialog::Settings(section)),
            Command::Documentation => {
                ctx.open_url(egui::OpenUrl::new_tab(dialogs::PROJECT_PAGE));
            }
        }
    }

    fn report_export(&mut self, result: Result<Option<PathBuf>, String>) {
        match result {
            Ok(Some(path)) => self.info(tf("Wrote {}", &[&path.display()])),
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
            || tr("not saved").to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let dirty = if self.doc.is_dirty() { " •" } else { "" };
        let title = format!(
            "{} ({file}){dirty} — Turbopump",
            i18n::project_name(&self.doc.project().info.name)
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
            if i.consume_shortcut(&shortcut(Modifiers::COMMAND, Key::Comma)) {
                self.dialog = Some(Dialog::Settings(Section::default()));
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
            Some(label) => self.info(tf("Undid: {}", &[&message(&label)])),
            None => self.info(tr("Nothing to undo")),
        }
    }

    fn redo(&mut self) {
        match self.doc.redo() {
            Some(label) => self.info(tf("Redid: {}", &[&message(&label)])),
            None => self.info(tr("Nothing to redo")),
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
                self.info(tr("New project"));
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
                self.info(tf("Opened {}", &[&path.display()]));
            }
            Err(e) => self.error(tf(
                "Could not open {}: {}",
                &[&path.display(), &message(&e.to_string())],
            )),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(tr("Turbopump project"), &[tp_project::FILE_EXTENSION])
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
                self.info(tf("Saved {}", &[&path.unwrap_or_default()]));
                true
            }
            Err(e) => {
                self.error(tf("Could not save: {}", &[&message(&e.to_string())]));
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
            .add_filter(tr("Turbopump project"), &[tp_project::FILE_EXTENSION])
            .set_file_name(name)
            .save_file()
        else {
            return false;
        };
        match self.doc.save_as(&path) {
            Ok(()) => {
                self.info(tf("Saved {}", &[&path.display()]));
                true
            }
            Err(e) => {
                self.error(tf("Could not save: {}", &[&message(&e.to_string())]));
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
                        format!("{}  {}", icon::X_CIRCLE, message(text)),
                    );
                }
                Some(Status { text, .. }) => {
                    ui.label(format!("{}  {}", icon::INFO, message(text)));
                }
                None => {
                    ui.label(RichText::new(tr("Ready")).color(p.weak));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (errors, warnings) = messages::count(all_messages);
                let text = if errors + warnings == 0 {
                    RichText::new(format!("{}  {}", icon::CHECK_CIRCLE, tr("No problems")))
                        .color(p.done)
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
                    .on_hover_text(tr("Show every error and warning of the project"))
                    .clicked()
                {
                    self.show_messages = !self.show_messages;
                }
                ui.separator();
                let current = &self.doc.project().display_units;
                let units = if *current == DisplayUnits::us() {
                    tr("US units")
                } else if *current == DisplayUnits::si() {
                    tr("SI units")
                } else {
                    tr("Custom units")
                };
                if ui
                    .add(Button::new(RichText::new(units).color(p.weak)).frame(false))
                    .on_hover_text(tr("Change the units"))
                    .clicked()
                {
                    self.dialog = Some(Dialog::Settings(Section::Units));
                }
                if self.exports.cad_running() {
                    ui.separator();
                    ui.spinner();
                    ui.label(tr("Building CAD solids…"));
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
                let mut failure = None;
                let outcome = form.show(ctx, &units, &mut |e| failure = Some(e));
                if let Some(e) = failure {
                    self.error(e);
                }
                match outcome {
                    Outcome::Open => Some(Dialog::NewProject(form)),
                    Outcome::Cancel => None,
                    Outcome::Create(project) => {
                        self.request(Pending::New(project));
                        None
                    }
                }
            }
            Dialog::ConfirmDiscard(pending) => {
                let name = project_name(&self.doc.project().info.name);
                let quitting = matches!(pending, Pending::Quit);
                match dialogs::confirm_discard(ctx, name, quitting) {
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
                    None => Some(Dialog::ConfirmDiscard(pending)),
                }
            }
            Dialog::About => dialogs::about(ctx).then_some(Dialog::About),
            Dialog::Settings(mut section) => {
                let cx = SettingsCx {
                    theme: ctx.options(|o| o.theme_preference),
                    zoom: ctx.zoom_factor(),
                    language: self.language,
                    units: &units,
                    follow_step: self.follow_step,
                };
                let outcome = settings::show(ctx, &mut section, &cx);
                for command in outcome.commands {
                    self.run(command, ctx);
                }
                (!outcome.close).then_some(Dialog::Settings(section))
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

impl eframe::App for TurbopumpApp {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let render_state = frame.wgpu_render_state().cloned();
        self.show(ui, render_state.as_ref());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(LANGUAGE_KEY, self.language.code().to_owned());
        storage.set_string(THEME_KEY, theme_code(self.theme).to_owned());
        storage.set_string(ZOOM_KEY, self.zoom.to_string());
        storage.set_string(FOLLOW_STEP_KEY, self.follow_step.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::meridional::{Handle, handle_position};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use egui_plot::PlotPoint;
    use tp_design::impeller::meridian::DEFAULT_TANGENT;
    use tp_units::QuantityKind;

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
        // A drag repaints while the geometry follows the pointer; under load egui may
        // start it a frame later, so step a fixed number of frames instead of waiting
        // for the UI to settle.
        for step in 1..=6 {
            harness.hover_at(start + egui::vec2(8.0 * step as f32, 0.0));
            harness.run_steps(3);
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

    /// Clicks `button` at `at`.
    fn click_with(
        harness: &mut Harness<'_, TurbopumpApp>,
        at: egui::Pos2,
        button: egui::PointerButton,
    ) {
        harness.hover_at(at);
        harness.run();
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run();
        }
    }

    /// Clicks the secondary button at `at`.
    fn right_click(harness: &mut Harness<'_, TurbopumpApp>, at: egui::Pos2) {
        click_with(harness, at, egui::PointerButton::Secondary);
    }

    #[test]
    fn the_context_menu_splits_a_wall_and_undo_restores_it() {
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, None));
        harness.run();
        harness.state_mut().focus_view(View::Meridional);
        harness.run();
        let id = harness.state().active_impeller().unwrap();
        let segments = |app: &TurbopumpApp| {
            app.doc
                .project()
                .impeller_geometry_input(id)
                .unwrap()
                .meridional
                .hub_segments
                .map(|s| s.len())
        };
        // A point of the hub away from its handles.
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let p = g.contour.hub_at(0.7);
        let scale = app
            .doc
            .project()
            .display_units
            .to_display(QuantityKind::Length, 1.0);
        let at = app
            .meridional
            .transform()
            .unwrap()
            .position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));

        right_click(&mut harness, at);
        harness.get_by_label("Split here").click();
        harness.run();
        assert_eq!(segments(harness.state()), Some(2));

        // The joint now has its own menu.
        let joint = handle_on_screen(
            harness.state(),
            Handle::Point(
                crate::views::meridional::Side::Hub,
                tp_design::impeller::wall::WallPoint::Joint { segment: 0 },
            ),
        );
        right_click(&mut harness, joint);
        harness.get_by_label("Tangent joint");
        harness.get_by_label("Join the two segments").click();
        harness.run();
        assert_eq!(segments(harness.state()), Some(1));

        harness.state_mut().undo();
        harness.state_mut().undo();
        assert_eq!(segments(harness.state()), None);
    }

    #[test]
    fn a_dimension_of_the_meridian_editor_is_typed_in_place() {
        use crate::views::meridional::{Dimension, dimension_lines};
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, None));
        harness.run();
        harness.state_mut().focus_view(View::MeridianEditor);
        harness.run();
        let id = harness.state().active_impeller().unwrap();
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let lines = dimension_lines(&g.contour, &g.basis, true, None);
        let length = lines
            .iter()
            .find(|l| l.dimension == Dimension::AxialLength)
            .unwrap();
        let middle = length.from.lerp(length.to, 0.5);
        let scale = app
            .doc
            .project()
            .display_units
            .to_display(QuantityKind::Length, 1.0);
        let at = app
            .meridional
            .transform()
            .unwrap()
            .position_from_point(&PlotPoint::new(middle.x * scale, middle.y * scale));
        let length = |app: &TurbopumpApp| {
            app.doc
                .project()
                .impeller_geometry_input(id)
                .unwrap()
                .meridional
                .axial_length
        };
        let type_text = |harness: &mut Harness<'_, TurbopumpApp>, text: &str| {
            harness
                .input_mut()
                .events
                .push(egui::Event::Text(text.into()));
            harness.run();
            harness.key_press(egui::Key::Enter);
            harness.run();
        };
        click_with(&mut harness, at, egui::PointerButton::Primary);
        // Not a number: the box stays open with the text selected, and nothing changes
        // until a number replaces it.
        type_text(&mut harness, "abc");
        assert!(harness.query_by_label("OK").is_some());
        assert!(length(harness.state()).is_auto());
        type_text(&mut harness, "48");
        assert!(harness.query_by_label("OK").is_none());
        assert_eq!(length(harness.state()).manual_value(), Some(0.048));
        harness.state_mut().undo();
        assert!(length(harness.state()).is_auto());

        // Escape closes the box without applying, and leaves the properties panel.
        click_with(&mut harness, at, egui::PointerButton::Primary);
        type_text(&mut harness, "abc");
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(harness.query_by_label("OK").is_none());
        assert!(harness.state().sidebar_open);

        // The box opens with the value selected: typing replaces it, with a decimal
        // comma too, and Enter applies and closes.
        click_with(&mut harness, at, egui::PointerButton::Primary);
        type_text(&mut harness, "52,5");
        let l = length(harness.state());
        assert!(
            l.manual_value().is_some_and(|v| (v - 0.0525).abs() < 1e-12),
            "{l:?}"
        );
        assert!(harness.query_by_label("OK").is_none());
        // One undo step brings L back.
        harness.state_mut().undo();
        assert!(length(harness.state()).is_auto());
    }

    #[test]
    fn the_meridian_editor_offers_a_way_back_from_a_broken_wall() {
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, None));
        harness.run();
        harness.state_mut().focus_view(View::MeridianEditor);
        harness.run();
        let id = harness.state().active_impeller().unwrap();
        // A hub control point below the axis leaves no geometry.
        let app = harness.state_mut();
        let g = app.derived.geometry(id).unwrap();
        let mut segments = g.contour.hub.to_input();
        segments[0].controls[0].y = -0.01;
        let mut input = app.doc.project().impeller_geometry_input(id).unwrap();
        input.meridional.hub_segments = Some(segments);
        app.doc
            .edit_gesture("break", |p| p.set_impeller_geometry(id, input))
            .unwrap();
        harness.run();
        assert!(harness.state().derived.geometry(id).is_none());
        harness
            .get_by_label("Back to the automatic contour")
            .click();
        harness.run();
        assert!(harness.state().derived.geometry(id).is_some());
    }

    #[test]
    fn the_wall_menu_adds_points_keeps_copies_and_alt_measures() {
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, None));
        harness.run();
        harness.state_mut().focus_view(View::MeridianEditor);
        harness.run();
        let id = harness.state().active_impeller().unwrap();
        let on_screen = |app: &TurbopumpApp, p: tp_math::DVec2| {
            let scale = app
                .doc
                .project()
                .display_units
                .to_display(QuantityKind::Length, 1.0);
            app.meridional
                .transform()
                .unwrap()
                .position_from_point(&PlotPoint::new(p.x * scale, p.y * scale))
        };
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let at = on_screen(app, g.contour.shroud_at(0.6));

        right_click(&mut harness, at);
        harness.get_by_label("Keep a copy to compare").click();
        harness.run();
        assert_eq!(harness.state().meridional.references.len(), 1);

        right_click(&mut harness, at);
        harness.get_by_label("Add control point").click();
        harness.run();
        let segments = harness
            .state()
            .doc
            .project()
            .impeller_geometry_input(id)
            .unwrap()
            .meridional
            .shroud_segments
            .unwrap();
        assert_eq!(segments[0].controls.len(), 3);

        // Alt and a drag on the background measure instead of panning.
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let a = g.contour.hub.start() + tp_math::DVec2::new(0.01, -0.02);
        let (from, to) = (
            on_screen(app, a),
            on_screen(app, a + tp_math::DVec2::new(0.02, 0.0)),
        );
        let bounds = *app.meridional.transform().unwrap().bounds();
        harness
            .input_mut()
            .events
            .push(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
        harness.hover_at(from);
        harness.run();
        harness.drag_at(from);
        harness.run();
        for step in 1..=5 {
            harness.hover_at(from + (to - from) * (step as f32 / 5.0));
            harness.run_steps(2);
        }
        let measured = harness.state().meridional.measured();
        harness.drop_at(to);
        harness.run();
        harness
            .input_mut()
            .events
            .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        let (m0, m1) = measured.expect("a measurement");
        assert!((m1.distance(m0) - 0.02).abs() < 0.002, "{m0} {m1}");
        // No panning: the view keeps its centre.
        let after = *harness.state().meridional.transform().unwrap().bounds();
        let centre = |b: &egui_plot::PlotBounds| {
            [
                (b.min()[0] + b.max()[0]) / 2.0,
                (b.min()[1] + b.max()[1]) / 2.0,
            ]
        };
        let (c0, c1) = (centre(&bounds), centre(&after));
        assert!((c0[0] - c1[0]).abs() < 1e-6 && (c0[1] - c1[1]).abs() < 1e-6);
    }

    #[test]
    fn the_leading_edge_turns_into_a_bezier_from_its_menu() {
        use tp_design::impeller::meridian::LeadingEdgeShape;
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, None));
        harness.run();
        harness.state_mut().focus_view(View::Meridional);
        harness.run();
        let id = harness.state().active_impeller().unwrap();
        let shape = |app: &TurbopumpApp| {
            app.doc
                .project()
                .impeller_geometry_input(id)
                .unwrap()
                .meridional
                .leading_edge_shape
        };
        // The middle of the leading edge, away from its end handles.
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let middle = g.blade.sections[g.blade.sections.len() / 2].meridional[0];
        let scale = app
            .doc
            .project()
            .display_units
            .to_display(QuantityKind::Length, 1.0);
        let at = app
            .meridional
            .transform()
            .unwrap()
            .position_from_point(&PlotPoint::new(middle.x * scale, middle.y * scale));

        right_click(&mut harness, at);
        harness.get_by_label("Bézier").click();
        harness.run();
        assert!(matches!(
            shape(harness.state()),
            LeadingEdgeShape::Bezier { .. }
        ));
        // The edge keeps its place: the middle span still starts where it did.
        let g = harness.state().derived.geometry(id).unwrap();
        let moved = g.blade.sections[g.blade.sections.len() / 2].meridional[0];
        assert!(moved.distance(middle) < 2e-4, "{moved} vs {middle}");
        let handles = crate::views::meridional::handles(
            &g.contour,
            &harness
                .state()
                .doc
                .project()
                .impeller_geometry_input(id)
                .unwrap()
                .meridional,
        );
        assert!(handles.contains(&Handle::LeadingEdgeControl(1)));

        harness.state_mut().undo();
        assert_eq!(shape(harness.state()), LeadingEdgeShape::Channel);
    }

    /// The editor with the default pump, framed and settled.
    #[test]
    fn the_blade_edge_step_opens_its_edges_to_drag() {
        use crate::views::blade::BladePage;
        use crate::views::blade_edges::{Edge, Span, grips, to_plot};
        use tp_design::impeller::blade::EdgeShape;
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, None));
        harness.run();
        let id = harness.state().active_impeller().unwrap();
        harness.state_mut().selection = Selection::Step(id, DesignStep::BladeEdges);
        harness.run();
        harness.run();
        assert_eq!(harness.state().view, View::Blade);
        assert_eq!(harness.state().blade.page, BladePage::Edges);
        let leading = |app: &TurbopumpApp| {
            app.doc
                .project()
                .impeller_geometry_input(id)
                .unwrap()
                .edges
                .leading
        };
        assert_eq!(leading(harness.state()), EdgeShape::Ellipse { ratio: 4.0 });
        // The end of the nose, dragged along the face, makes it longer.
        let start = {
            let app = harness.state();
            let g = app.derived.geometry(id).unwrap();
            let e = g.basis.blade_thickness;
            let scale = app
                .doc
                .project()
                .display_units
                .to_display(QuantityKind::Length, 1.0);
            let (_, end) = grips(leading(app), e)[0];
            app.blade
                .edges
                .transform(Edge::Leading, Span::Hub)
                .unwrap()
                .position_from_point(&to_plot(Edge::Leading, end, e, scale))
        };
        drag_by(&mut harness, start, egui::vec2(60.0, 0.0));
        let EdgeShape::Ellipse { ratio } = leading(harness.state()) else {
            panic!("still an ellipse");
        };
        assert!(ratio > 4.5, "{ratio}");
        // One drag, one undo step.
        harness.state_mut().doc.undo();
        harness.run();
        assert_eq!(leading(harness.state()), EdgeShape::Ellipse { ratio: 4.0 });
    }

    fn editor() -> Harness<'static, TurbopumpApp> {
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, None));
        harness.run();
        harness.state_mut().focus_view(View::MeridianEditor);
        harness.run();
        harness
    }

    /// Drags from `start` by `delta` in a few steps, as a hand would.
    fn drag_by(harness: &mut Harness<'_, TurbopumpApp>, start: egui::Pos2, delta: egui::Vec2) {
        harness.hover_at(start);
        harness.run();
        harness.drag_at(start);
        harness.run();
        // A fixed number of frames per step: the geometry repaints while it follows.
        for step in 1..=6 {
            harness.hover_at(start + delta * (step as f32 / 6.0));
            harness.run_steps(3);
        }
        harness.drop_at(start + delta);
        harness.run();
    }

    /// Two clicks at `at` in one frame: each kittest frame lasts a quarter of a second,
    /// longer than a double click.
    fn double_click(harness: &mut Harness<'_, TurbopumpApp>, at: egui::Pos2) {
        harness.hover_at(at);
        harness.run();
        for pressed in [true, false, true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run();
    }

    fn meridional(app: &TurbopumpApp) -> tp_design::impeller::meridian::MeridionalInput {
        let id = app.active_impeller().unwrap();
        app.doc
            .project()
            .impeller_geometry_input(id)
            .unwrap()
            .meridional
    }

    fn main_dimensions(app: &TurbopumpApp) -> tp_design::pump::CentrifugalPumpInput {
        let id = app.active_impeller().unwrap();
        app.doc.project().pump_impeller_input(id).unwrap()
    }

    #[test]
    fn the_editor_opens_with_the_meridional_contour_in_the_panel() {
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        assert_eq!(
            harness.state().selection(),
            Selection::Step(id, DesignStep::MeridionalContour)
        );
        assert_eq!(harness.state().view(), View::MeridianEditor);
        assert!(harness.state().sidebar_open());
        harness.get_by_label("Check against Gülich");
        // Another step keeps the editor; from another view the meridional step opens it.
        harness
            .get_all_by_label("Main dimensions")
            .last()
            .unwrap()
            .click();
        harness.run();
        assert_eq!(harness.state().view(), View::MeridianEditor);
        harness.state_mut().focus_view(View::Blade);
        harness.run();
        harness
            .get_all_by_label("Meridional contour")
            .last()
            .unwrap()
            .click();
        harness.run();
        assert_eq!(harness.state().view(), View::MeridianEditor);
    }

    #[test]
    fn a_dragged_tangent_turns_manual_and_a_double_click_makes_it_automatic() {
        let mut harness = editor();
        let start = handle_on_screen(harness.state(), Handle::ShroudInlet);
        drag_by(&mut harness, start, egui::vec2(30.0, 0.0));
        let dragged = meridional(harness.state()).shroud_inlet_tangent;
        assert!(
            dragged.manual_value().is_some_and(|t| t > DEFAULT_TANGENT),
            "{dragged:?}"
        );
        let at = handle_on_screen(harness.state(), Handle::ShroudInlet);
        double_click(&mut harness, at);
        assert!(meridional(harness.state()).shroud_inlet_tangent.is_auto());
        // Undo brings the hand-set value back.
        harness.state_mut().undo();
        assert_eq!(meridional(harness.state()).shroud_inlet_tangent, dragged);
    }

    #[test]
    fn the_leading_edge_goes_back_to_automatic_from_its_menu() {
        let mut harness = editor();
        let start = handle_on_screen(harness.state(), Handle::LeadingEdgeHub);
        drag_by(&mut harness, start, egui::vec2(20.0, -10.0));
        assert!(!meridional(harness.state()).leading_edge_hub.is_auto());
        let at = handle_on_screen(harness.state(), Handle::LeadingEdgeHub);
        right_click(&mut harness, at);
        harness.get_by_label("Back to automatic").click();
        harness.run();
        assert!(meridional(harness.state()).leading_edge_hub.is_auto());
    }

    #[test]
    fn the_trailing_edge_drags_d2_and_resets_it() {
        let mut harness = editor();
        let d2 = |app: &TurbopumpApp| main_dimensions(app).outlet_diameter;
        let before = harness
            .state()
            .derived
            .geometry(harness.state().active_impeller().unwrap())
            .unwrap()
            .basis
            .outlet_diameter;
        let start = handle_on_screen(harness.state(), Handle::TrailingEdge);
        // Upwards on screen: a larger radius.
        drag_by(&mut harness, start, egui::vec2(0.0, -30.0));
        let dragged = d2(harness.state()).manual_value();
        assert!(
            dragged.is_some_and(|d| d > before),
            "{dragged:?} vs {before}"
        );
        let at = handle_on_screen(harness.state(), Handle::TrailingEdge);
        double_click(&mut harness, at);
        assert!(d2(harness.state()).is_auto());
    }

    #[test]
    fn the_outlet_diameter_is_typed_on_its_dimension() {
        use crate::views::meridional::{Dimension, dimension_lines};
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let lines = dimension_lines(&g.contour, &g.basis, true, None);
        let d2 = lines
            .iter()
            .find(|l| l.dimension == Dimension::OutletDiameter)
            .unwrap();
        let middle = d2.from.lerp(d2.to, 0.5);
        let scale = app
            .doc
            .project()
            .display_units
            .to_display(QuantityKind::Length, 1.0);
        let at = app
            .meridional
            .transform()
            .unwrap()
            .position_from_point(&PlotPoint::new(middle.x * scale, middle.y * scale));
        click_with(&mut harness, at, egui::PointerButton::Primary);
        harness
            .input_mut()
            .events
            .push(egui::Event::Text("180".into()));
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        let typed = main_dimensions(harness.state()).outlet_diameter;
        assert!(
            typed
                .manual_value()
                .is_some_and(|d| (d - 0.18).abs() < 1e-12),
            "{typed:?}"
        );
        let g = harness.state().derived.geometry(id).unwrap();
        assert!((g.basis.outlet_diameter - 0.18).abs() < 1e-9);
    }

    /// Screen position of `p` ((z, r) in m) in the last frame of the diagram.
    fn screen_of(app: &TurbopumpApp, p: tp_math::DVec2) -> egui::Pos2 {
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
    fn the_segment_menu_offers_arcs_b_splines_and_coupled_beziers() {
        use crate::views::meridional::Side;
        use tp_design::impeller::wall::SegmentKind;
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        let shroud = |app: &TurbopumpApp| meridional(app).shroud_segments.unwrap_or_default();
        // A point of the shroud near `t`, clear of every handle.
        let wall_point = |harness: &Harness<'_, TurbopumpApp>, t: f64| {
            let app = harness.state();
            let g = app.derived.geometry(id).unwrap();
            let input = meridional(app);
            let handles: Vec<egui::Pos2> = crate::views::meridional::handles(&g.contour, &input)
                .into_iter()
                .map(|h| handle_on_screen(app, h))
                .collect();
            (0..=40)
                .map(|k| t - 0.2 + 0.4 * k as f64 / 40.0)
                .map(|u| screen_of(app, g.contour.shroud_at(u)))
                .max_by(|a, b| {
                    let clear = |p: &egui::Pos2| {
                        handles
                            .iter()
                            .map(|h| h.distance(*p))
                            .fold(f32::INFINITY, f32::min)
                    };
                    clear(a).total_cmp(&clear(b))
                })
                .unwrap()
        };

        let at = wall_point(&harness, 0.6);
        right_click(&mut harness, at);
        harness.get_by_label("Lines and arc").click();
        harness.run();
        assert_eq!(shroud(harness.state())[0].kind, SegmentKind::Arc);
        let radius = |app: &TurbopumpApp| {
            app.derived.geometry(id).unwrap().contour.shroud.segments()[0]
                .fillet()
                .unwrap()
                .radius
        };
        let before = radius(harness.state());
        // The centre moves away from the corner: a larger radius.
        let center = handle_on_screen(harness.state(), Handle::ArcCenter(Side::Shroud, 0));
        let app = harness.state();
        let s = &app.derived.geometry(id).unwrap().contour.shroud.segments()[0];
        let corner = screen_of(app, s.control_points()[1]);
        drag_by(&mut harness, center, (center - corner).normalized() * 20.0);
        let after = radius(harness.state());
        assert!(after > before * 1.05, "{after} vs {before}");

        let at = wall_point(&harness, 0.5);
        right_click(&mut harness, at);
        harness.get_by_label("B-spline").click();
        harness.run();
        let spline = shroud(harness.state());
        assert_eq!(spline[0].kind, SegmentKind::BSpline);
        let at = wall_point(&harness, 0.5);
        right_click(&mut harness, at);
        harness.get_by_label("Insert knot here").click();
        harness.run();
        assert_eq!(
            shroud(harness.state())[0].controls.len(),
            spline[0].controls.len() + 1
        );

        let at = wall_point(&harness, 0.5);
        right_click(&mut harness, at);
        harness.get_by_label("Bézier").click();
        harness.run();
        let at = wall_point(&harness, 0.5);
        right_click(&mut harness, at);
        harness.get_by_label("Coupled control points").click();
        harness.run();
        let coupled = &shroud(harness.state())[0];
        assert!(coupled.coupled && coupled.controls.len() >= 3);
    }

    #[test]
    fn the_hub_point_of_the_leading_edge_follows_the_shroud_point() {
        use tp_design::impeller::meridian::EdgeExtent;
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        let points = |app: &TurbopumpApp| {
            let c = &app.derived.geometry(id).unwrap().contour;
            (c.hub_at(c.leading_edge.0), c.shroud_at(c.leading_edge.1))
        };
        // The middle of the edge, away from its end handles.
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let middle = g.blade.sections[g.blade.sections.len() / 2].meridional[0];
        let at = screen_of(app, middle);
        right_click(&mut harness, at);
        harness.get_by_label("same radial distance").click();
        harness.run();
        let EdgeExtent::Radial(dr) = meridional(harness.state()).leading_edge_extent else {
            panic!("a radial extent");
        };
        let (hub, shroud) = points(harness.state());
        assert!((hub.y - shroud.y - dr).abs() < 1e-6);

        // The shroud point moves; the hub point keeps its radial distance.
        let start = handle_on_screen(harness.state(), Handle::LeadingEdgeShroud);
        drag_by(&mut harness, start, egui::vec2(25.0, -25.0));
        let (moved_hub, moved_shroud) = points(harness.state());
        assert!(
            moved_shroud.distance(shroud) > 1e-3,
            "the shroud point moved"
        );
        assert!((moved_hub.y - moved_shroud.y - dr).abs() < 1e-5);
        assert!(is_manual_hub(harness.state()));

        // A double click on the hub point frees it again.
        let at = handle_on_screen(harness.state(), Handle::LeadingEdgeHub);
        double_click(&mut harness, at);
        assert_eq!(
            meridional(harness.state()).leading_edge_extent,
            EdgeExtent::Independent
        );
    }

    fn is_manual_hub(app: &TurbopumpApp) -> bool {
        let id = app.active_impeller().unwrap();
        let geometry = app.doc.project().impeller_geometry_input(id).unwrap();
        let main = app.doc.project().pump_impeller_input(id).ok();
        crate::views::meridional::is_manual(
            Handle::LeadingEdgeHub,
            crate::views::meridional::Inputs {
                geometry: &geometry,
                main: main.as_ref(),
                semi_axial: false,
            },
        )
    }

    #[test]
    fn the_hub_end_of_a_semi_axial_outlet_drags_l_and_d2i() {
        use std::f64::consts::PI;
        use tp_project::{DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType};
        let setup = GlobalSetup::new(
            MachineType::Pump,
            DesignPoint {
                flow: FlowRate::Volume(800.0 / 3600.0),
                energy: EnergyTransfer::Head(12.0),
                speed: 1450.0 * PI / 30.0,
            },
        );
        let mut project = Project::new("Mixed flow", MachineType::Pump, setup).unwrap();
        project
            .add_component(ComponentKind::Impeller, None)
            .unwrap();
        let path = std::env::temp_dir().join("turbopump-semi-axial-drag.tpj");
        project.save(&path).unwrap();
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
        harness.run();
        harness.state_mut().focus_view(View::MeridianEditor);
        harness.run();
        let id = harness.state().active_impeller().unwrap();
        let basis = |app: &TurbopumpApp| app.derived.geometry(id).unwrap().basis;
        assert!(
            basis(harness.state()).outlet_inclination > 0.0,
            "semi-axial"
        );
        let before = basis(harness.state()).outlet_hub_diameter;

        // Down on screen: a smaller hub diameter at the outlet, set by hand with L.
        let start = handle_on_screen(harness.state(), Handle::AxialLength);
        drag_by(&mut harness, start, egui::vec2(0.0, 30.0));
        let d2i = main_dimensions(harness.state()).outlet_hub_diameter;
        assert!(
            d2i.manual_value().is_some_and(|d| d < before),
            "{d2i:?} vs {before}"
        );
        assert!(!meridional(harness.state()).axial_length.is_auto());
        // One undo step takes both back.
        harness.state_mut().undo();
        assert!(
            main_dimensions(harness.state())
                .outlet_hub_diameter
                .is_auto()
        );
        assert!(meridional(harness.state()).axial_length.is_auto());
    }

    #[test]
    fn a_dragged_point_snaps_to_the_z_of_another_point_unless_ctrl_is_held() {
        use crate::views::meridional::Side;
        use tp_design::impeller::wall::WallPoint;
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        // The shroud as lines and an arc: its corner is a free point.
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let mut input = app.doc.project().impeller_geometry_input(id).unwrap();
        let shroud = &g.contour.shroud;
        let segments = tp_design::impeller::wall::set_kind(
            shroud,
            0,
            tp_design::impeller::wall::SegmentKind::Arc,
        );
        input.meridional.shroud_segments = Some(segments);
        harness
            .state_mut()
            .doc
            .edit_gesture("arc", |p| p.set_impeller_geometry(id, input))
            .unwrap();
        harness.run();
        let corner = Handle::Point(
            Side::Shroud,
            WallPoint::Control {
                segment: 0,
                index: 0,
            },
        );
        let corner_x =
            |app: &TurbopumpApp| meridional(app).shroud_segments.unwrap()[0].controls[0].x;
        // The z of the leading edge on the hub: the corner passes 3 px from it.
        let c = &harness.state().derived.geometry(id).unwrap().contour;
        let goal = c.hub_at(c.leading_edge.0).x;
        let target = |harness: &Harness<'_, TurbopumpApp>| {
            let app = harness.state();
            let edge = handle_on_screen(app, Handle::LeadingEdgeHub);
            let start = handle_on_screen(app, corner);
            (start, egui::pos2(edge.x + 3.0, start.y + 40.0))
        };
        let (start, to) = target(&harness);
        drag_by(&mut harness, start, to - start);
        assert!((corner_x(harness.state()) - goal).abs() < 1e-12);

        harness.state_mut().undo();
        harness.run();
        let (start, to) = target(&harness);
        harness
            .input_mut()
            .events
            .push(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        drag_by(&mut harness, start, to - start);
        harness
            .input_mut()
            .events
            .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        assert!((corner_x(harness.state()) - goal).abs() > 1e-5);
    }

    #[test]
    fn the_materials_page_edits_the_closed_hub_disk_and_shroud_cover() {
        use crate::views::meridional::Side;
        use tp_design::impeller::wall::WallPoint;
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        harness.get_by_label("Materials").click();
        harness.run();
        harness.run();
        // The corner of the boss: dragging it draws the hub disk by hand.
        let app = harness.state();
        let wire = &app.derived.geometry(id).unwrap().contour.hub_material;
        let (corner, _) = wire
            .points()
            .into_iter()
            .find(|(at, _)| matches!(at, WallPoint::Joint { .. }))
            .unwrap();
        let handle = Handle::Point(Side::HubMaterial, corner);
        let before = wire.point(corner).unwrap();
        let start = handle_on_screen(app, handle);
        drag_by(&mut harness, start, egui::vec2(15.0, 0.0));
        let drawn = meridional(harness.state()).hub_material;
        assert!(drawn.is_some(), "the hub disk is drawn by hand");
        let after = harness
            .state()
            .derived
            .geometry(id)
            .unwrap()
            .contour
            .hub_material
            .point(corner)
            .unwrap();
        assert!(after.x > before.x + 1e-4, "{after} vs {before}");
        assert!(
            meridional(harness.state()).hub_segments.is_none(),
            "the hub stays"
        );
        // A double click puts the automatic disk back.
        let at = handle_on_screen(harness.state(), handle);
        double_click(&mut harness, at);
        assert!(meridional(harness.state()).hub_material.is_none());

        // The shroud cover, offset outwards by a wall thickness: twice as thick there.
        let app = harness.state();
        let c = &app.derived.geometry(id).unwrap().contour;
        let thickness = c.material.wall_thickness;
        let cover = c.shroud_material.segments()[1].eval(0.5);
        let at = screen_of(app, cover);
        right_click(&mut harness, at);
        harness.get_by_label("Apply").click();
        harness.run();
        let c = &harness.state().derived.geometry(id).unwrap().contour;
        assert!(meridional(harness.state()).shroud_material.is_some());
        let moved = c.shroud_material.segments()[1].eval(0.5);
        let wall = c.shroud.sample(2000);
        let gap = |p: tp_math::DVec2| {
            wall.iter()
                .map(|q| q.distance(p))
                .fold(f64::INFINITY, f64::min)
        };
        assert!(
            (gap(moved) - 2.0 * thickness).abs() < 0.1 * thickness,
            "{}",
            gap(moved)
        );
    }

    #[test]
    fn the_side_chambers_page_designs_and_shapes_the_casing_walls() {
        use crate::views::meridional::{Page, Side};
        use tp_design::impeller::wall::WallPoint;
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        harness.state_mut().meridional.editor.page = Page::Chambers;
        harness.run();
        harness.run();
        harness.get_by_label("design the side chambers").click();
        harness.run();
        assert!(meridional(harness.state()).chambers.enabled);

        // A control point of the front wall: dragged away from the shroud, the wall is
        // drawn by hand and the chamber widens there.
        let app = harness.state();
        let c = &app.derived.geometry(id).unwrap().contour;
        let at = WallPoint::Control {
            segment: 0,
            index: 3,
        };
        let before = c.chambers.front_casing.point(at).unwrap();
        let handle = Handle::Point(Side::FrontCasing, at);
        let start = handle_on_screen(app, handle);
        drag_by(&mut harness, start, egui::vec2(-20.0, 0.0));
        assert!(meridional(harness.state()).chambers.front.is_some());
        let c = &harness.state().derived.geometry(id).unwrap().contour;
        let after = c.chambers.front_casing.point(at).unwrap();
        assert!(after.x < before.x - 1e-4, "{after} vs {before}");
        // The rear wall stays automatic.
        assert!(meridional(harness.state()).chambers.rear.is_none());
        let at = handle_on_screen(harness.state(), handle);
        double_click(&mut harness, at);
        assert!(meridional(harness.state()).chambers.front.is_none());
    }

    #[test]
    fn in_the_center_line_mode_the_walls_follow_the_dragged_center_line() {
        use crate::views::meridional::Side;
        use tp_design::impeller::meridian::DesignMode;
        use tp_design::impeller::wall::WallPoint;
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        let mut input = harness
            .state()
            .doc
            .project()
            .impeller_geometry_input(id)
            .unwrap();
        input.meridional.design_mode = DesignMode::CenterLine;
        harness
            .state_mut()
            .doc
            .edit_gesture("mode", |p| p.set_impeller_geometry(id, input))
            .unwrap();
        harness.run();
        harness.run();
        // The handles are those of the center line, not of hub and shroud.
        let app = harness.state();
        let g = app.derived.geometry(id).unwrap();
        let shown = crate::views::meridional::handles(&g.contour, &meridional(app));
        assert!(!shown.contains(&Handle::HubInlet));
        let at = WallPoint::Control {
            segment: 0,
            index: 0,
        };
        let handle = Handle::Point(Side::Center, at);
        assert!(shown.contains(&handle));
        let hub_before = g.contour.hub.sample(50);
        let start = handle_on_screen(app, handle);
        drag_by(&mut harness, start, egui::vec2(20.0, 0.0));
        assert!(meridional(harness.state()).center_line.is_some());
        let hub_after = harness
            .state()
            .derived
            .geometry(id)
            .unwrap()
            .contour
            .hub
            .sample(50);
        let moved = hub_before
            .iter()
            .zip(&hub_after)
            .map(|(a, b)| a.distance(*b))
            .fold(0.0, f64::max);
        assert!(moved > 1e-4, "the hub follows the center line: {moved}");
        let at = handle_on_screen(harness.state(), handle);
        double_click(&mut harness, at);
        assert!(meridional(harness.state()).center_line.is_none());
    }

    #[test]
    fn the_reference_point_moves_the_meridian_and_the_inlet_point_leans_it() {
        use tp_design::impeller::meridian::InletTilt;
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        // A drawn hub, to see it move along.
        let mut input = harness
            .state()
            .doc
            .project()
            .impeller_geometry_input(id)
            .unwrap();
        let g = harness.state().derived.geometry(id).unwrap();
        input.meridional.hub_segments =
            Some(tp_design::impeller::wall::split(&g.contour.hub, 0, 0.5));
        harness
            .state_mut()
            .doc
            .edit_gesture("drawn", |p| p.set_impeller_geometry(id, input))
            .unwrap();
        harness.run();
        let joint =
            |app: &TurbopumpApp| app.derived.geometry(id).unwrap().contour.hub.segments()[0].end();
        let before = joint(harness.state());
        let start = handle_on_screen(harness.state(), Handle::AxialReference);
        drag_by(&mut harness, start, egui::vec2(-30.0, 0.0));
        let m = meridional(harness.state());
        assert!(m.axial_position < -1e-3, "{}", m.axial_position);
        let after = joint(harness.state());
        assert!(
            (after.x - before.x - m.axial_position).abs() < 1e-9,
            "the drawn hub moves along"
        );
        assert!((after.y - before.y).abs() < 1e-12);

        // The shroud point of the inlet, dragged downstream: the line leans.
        let start = handle_on_screen(harness.state(), Handle::InletShroud);
        drag_by(&mut harness, start, egui::vec2(12.0, 0.0));
        let InletTilt::Axial(dz) = meridional(harness.state()).inlet_tilt else {
            panic!("an axial tilt");
        };
        assert!(dz > 1e-4, "{dz}");
        let at = handle_on_screen(harness.state(), Handle::InletShroud);
        double_click(&mut harness, at);
        assert_eq!(meridional(harness.state()).inlet_tilt, InletTilt::Normal);
        let at = handle_on_screen(harness.state(), Handle::AxialReference);
        double_click(&mut harness, at);
        let m = meridional(harness.state());
        assert!(m.axial_position.abs() < 1e-15);
        assert!((joint(harness.state()).x - before.x).abs() < 1e-9);
    }

    #[test]
    fn the_hub_end_of_a_radial_outlet_slants_the_trailing_edge() {
        let mut harness = editor();
        let id = harness.state().active_impeller().unwrap();
        // Along the axis alone: L by hand, d₂i still automatic.
        let start = handle_on_screen(harness.state(), Handle::AxialLength);
        drag_by(&mut harness, start, egui::vec2(15.0, 0.0));
        assert!(!meridional(harness.state()).axial_length.is_auto());
        assert!(
            main_dimensions(harness.state())
                .outlet_hub_diameter
                .is_auto()
        );
        harness.state_mut().undo();
        harness.run();
        // Down: a smaller d₂i, the edge slants and the impeller stays radial.
        let start = handle_on_screen(harness.state(), Handle::AxialLength);
        drag_by(&mut harness, start, egui::vec2(0.0, 30.0));
        let d2i = main_dimensions(harness.state()).outlet_hub_diameter;
        let g = harness.state().derived.geometry(id).unwrap();
        assert!(
            d2i.manual_value()
                .is_some_and(|d| d < g.basis.outlet_diameter),
            "{d2i:?}"
        );
        assert!(g.basis.outlet_inclination.abs() < 1e-12, "radial");
        assert!(g.contour.hub.end().y < g.contour.shroud.end().y - 1e-4);
    }
}
