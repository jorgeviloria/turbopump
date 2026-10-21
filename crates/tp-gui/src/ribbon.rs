//! Toolbar at the top of the window: a strip of tabs and, under it, groups of large
//! buttons. The "component" tab follows the selected component and holds its design
//! steps. Buttons only report a [`Command`]; the application carries it out.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Popup, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, ThemePreference, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use tp_project::{ComponentId, ComponentKind, DesignStep, Project, StepStatus};

use crate::app::{Selection, View};
use crate::export::{CAD_AVAILABLE, DrawingExport, TextExport, VoluteExport};
use crate::i18n::{Language, component_name, message, project_name, tf, tr};
use crate::icons;
use crate::steps::{self, AcceptState};
use crate::theme::{Palette, bold, component_color, icon};
use crate::tree::{ADD_ENTRIES, step_title};
use crate::viewer::{CameraView, ViewOptions};

/// What a toolbar button asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Command {
    NewProject,
    Open,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
    GlobalSetup,
    UsUnits(bool),
    /// Index into [`ADD_ENTRIES`].
    Add(usize),
    Remove(ComponentId),
    Select(ComponentId, DesignStep),
    Accept(ComponentId, DesignStep),
    Reset(ComponentId, DesignStep),
    ExportText(TextExport),
    ExportDrawing(DrawingExport),
    ExportVolute(VoluteExport),
    ExportCad,
    Focus(View),
    Camera(CameraView),
    ToggleTree,
    ToggleDialog,
    ToggleMessages,
    FollowStep,
    Theme(ThemePreference),
    Language(Language),
    About,
    Documentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Home,
    Component,
    View,
    Help,
}

/// Toolbar state between frames.
#[derive(Debug, Default)]
pub(crate) struct Ribbon {
    tab: Tab,
    collapsed: bool,
    /// Component the "component" tab showed last frame.
    last_component: Option<ComponentId>,
}

/// What the toolbar reads, and the display options it edits in place.
pub(crate) struct RibbonCx<'a> {
    pub project: &'a Project,
    pub selection: Selection,
    /// The project has unsaved changes.
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<&'a str>,
    pub redo_label: Option<&'a str>,
    pub accept: AcceptState,
    pub has_geometry: bool,
    pub has_volute: bool,
    pub cad_running: bool,
    pub options: &'a mut ViewOptions,
    pub theme: ThemePreference,
    pub tree_open: bool,
    pub sidebar_open: bool,
    pub messages_open: bool,
    pub follow_step: bool,
    pub language: Language,
}

/// Height of a large button.
const BIG_HEIGHT: f32 = 70.0;
const LABEL_SIZE: f32 = 11.5;

impl Ribbon {
    pub(crate) fn show(&mut self, ui: &mut Ui, cx: &mut RibbonCx<'_>) -> Option<Command> {
        let p = Palette::of(ui);
        let component = match cx.selection {
            Selection::Step(id, _) => cx.project.component(id).map(|c| (id, c.kind())),
            Selection::Setup => None,
        };
        // Selecting another component brings its tab forward.
        let current = component.map(|c| c.0);
        if current != self.last_component {
            if current.is_some() {
                self.tab = Tab::Component;
            } else if self.tab == Tab::Component {
                self.tab = Tab::Home;
            }
            self.last_component = current;
        }

        let mut command = None;
        ui.spacing_mut().item_spacing.y = 0.0;
        // Tab strip.
        egui::Frame::new()
            .fill(p.strip)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    command = command.or(self.file_menu(ui, cx, &p));
                    ui.add_space(6.0);
                    self.tab_button(ui, Tab::Home, tr("Home"), None);
                    if let Some((id, kind)) = component {
                        let name = cx
                            .project
                            .component(id)
                            .map_or_else(String::new, |c| component_name(&c.name));
                        self.tab_button(ui, Tab::Component, &name, Some(component_color(kind)));
                    }
                    self.tab_button(ui, Tab::View, tr("View"), None);
                    self.tab_button(ui, Tab::Help, tr("Help"), None);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (glyph, tip) = if self.collapsed {
                            (icon::CARET_DOWN, tr("Show the toolbar"))
                        } else {
                            (icon::CARET_UP, tr("Hide the toolbar"))
                        };
                        if ui
                            .add(egui::Button::new(glyph).frame(false))
                            .on_hover_text(tip)
                            .clicked()
                        {
                            self.collapsed = !self.collapsed;
                        }
                        let dirty = if cx.dirty { " •" } else { "" };
                        ui.label(
                            RichText::new(format!(
                                "{}{dirty}",
                                project_name(&cx.project.info.name)
                            ))
                            .color(p.weak),
                        );
                    });
                });
            });
        if self.collapsed {
            return command;
        }
        // Body.
        egui::Frame::new()
            .fill(p.ribbon)
            .inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 6,
                bottom: 2,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(BIG_HEIGHT + 18.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let found = match self.tab {
                        Tab::Home => home(ui, cx),
                        Tab::Component => match component {
                            Some((id, kind)) => component_tab(ui, cx, id, kind),
                            None => home(ui, cx),
                        },
                        Tab::View => view_tab(ui, cx),
                        Tab::Help => help_tab(ui),
                    };
                    command = command.or(found);
                });
            });
        let rect = ui.min_rect();
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.border));
        command
    }

    fn tab_button(&mut self, ui: &mut Ui, tab: Tab, label: &str, color: Option<Color32>) {
        let p = Palette::of(ui);
        let selected = self.tab == tab;
        let font = FontId::proportional(13.0);
        let text_color = match (color, selected) {
            (Some(c), _) => c,
            (None, true) => p.text,
            (None, false) => p.weak,
        };
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_owned(), font, text_color);
        let size = vec2(galley.size().x + 22.0, 24.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Button, true, selected, format!("{label} tab"))
        });
        let fill = if selected {
            p.ribbon
        } else if response.hovered() {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(
            rect,
            CornerRadius {
                nw: 5,
                ne: 5,
                sw: 0,
                se: 0,
            },
            fill,
        );
        if let Some(c) = color {
            ui.painter().hline(
                rect.shrink2(vec2(4.0, 0.0)).x_range(),
                rect.top() + 1.0,
                Stroke::new(2.0, c),
            );
        } else if selected {
            ui.painter().hline(
                rect.shrink2(vec2(6.0, 0.0)).x_range(),
                rect.bottom() - 1.0,
                Stroke::new(2.0, p.accent),
            );
        }
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, text_color);
        if response.clicked() {
            self.tab = tab;
            self.collapsed = false;
        }
    }

    fn file_menu(&mut self, ui: &mut Ui, cx: &RibbonCx<'_>, p: &Palette) -> Option<Command> {
        let button = egui::Button::new(bold(tr("File")).color(p.on_accent))
            .fill(p.accent)
            .corner_radius(4);
        let mut command = None;
        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            ui.set_min_width(220.0);
            let mut item = |ui: &mut Ui, glyph: &str, label: &str, shortcut: &str, c: Command| {
                if ui
                    .add(egui::Button::new(format!("{glyph}  {label}")).shortcut_text(shortcut))
                    .clicked()
                {
                    command = Some(c);
                }
            };
            let cmd = if cfg!(target_os = "macos") {
                "⌘"
            } else {
                "Ctrl+"
            };
            item(
                ui,
                icon::FILE_PLUS,
                tr("New project…"),
                &format!("{cmd}N"),
                Command::NewProject,
            );
            item(
                ui,
                icon::FOLDER_OPEN,
                tr("Open…"),
                &format!("{cmd}O"),
                Command::Open,
            );
            item(
                ui,
                icon::FLOPPY_DISK,
                tr("Save"),
                &format!("{cmd}S"),
                Command::Save,
            );
            item(ui, icon::FLOPPY_DISK, tr("Save as…"), "", Command::SaveAs);
            ui.separator();
            if let Some(c) = export_items(ui, cx) {
                command = Some(c);
            }
            ui.separator();
            if ui
                .button(format!("{}  {}", icon::X_CIRCLE, tr("Quit")))
                .clicked()
            {
                command = Some(Command::Quit);
            }
        });
        command
    }
}

/// The export entries, shared by the File menu and the Export button.
fn export_items(ui: &mut Ui, cx: &RibbonCx<'_>) -> Option<Command> {
    let mut command = None;
    for kind in TextExport::ALL {
        if ui
            .add_enabled(cx.has_geometry, egui::Button::new(kind.menu_label()))
            .clicked()
        {
            command = Some(Command::ExportText(kind));
        }
    }
    ui.separator();
    for kind in DrawingExport::ALL {
        if ui
            .add_enabled(cx.has_geometry, egui::Button::new(kind.menu_label()))
            .clicked()
        {
            command = Some(Command::ExportDrawing(kind));
        }
    }
    if cx.has_volute {
        ui.separator();
        for kind in VoluteExport::ALL {
            if ui.button(kind.menu_label()).clicked() {
                command = Some(Command::ExportVolute(kind));
            }
        }
    }
    ui.separator();
    if ui
        .add_enabled(
            CAD_AVAILABLE,
            egui::Button::new(tr("CAD solids (STEP, IGES, STL)…")),
        )
        .on_disabled_hover_text(tr("This build has no CAD support."))
        .clicked()
    {
        command = Some(Command::ExportCad);
    }
    command
}

/// What a large button shows above its label.
#[derive(Debug, Clone, Copy)]
enum Icon {
    Glyph(&'static str),
    Component(ComponentKind),
}

/// A large button: icon over a label of up to two lines, with an optional status dot.
fn big_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    selected: bool,
    dot: Option<Color32>,
) -> Response {
    let p = Palette::of(ui);
    let color = if enabled {
        p.text
    } else {
        p.weak.gamma_multiply(0.7)
    };
    let mut job = egui::text::LayoutJob::simple(
        label.to_owned(),
        FontId::proportional(LABEL_SIZE),
        color,
        86.0,
    );
    job.halign = Align::Center;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let width = (galley.size().x + 14.0).clamp(52.0, 100.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, BIG_HEIGHT), sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, label));
    let fill = if selected {
        p.selected
    } else if response.hovered() && enabled {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        5,
        fill,
        if selected {
            Stroke::new(1.0, p.accent.gamma_multiply(0.6))
        } else {
            Stroke::NONE
        },
        StrokeKind::Inside,
    );
    let icon_center = pos2(rect.center().x, rect.top() + 19.0);
    match icon {
        Icon::Glyph(glyph) => {
            painter.text(
                icon_center,
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(24.0),
                if enabled { p.text } else { color },
            );
        }
        Icon::Component(kind) => {
            let c = if enabled {
                component_color(kind)
            } else {
                color
            };
            icons::component(
                painter,
                Rect::from_center_size(icon_center, Vec2::splat(26.0)),
                kind,
                c,
            );
        }
    }
    if let Some(dot) = dot {
        painter.circle(
            icon_center + vec2(13.0, -9.0),
            4.0,
            dot,
            Stroke::new(1.5, p.ribbon),
        );
    }
    painter.galley(pos2(rect.center().x, rect.top() + 38.0), galley, color);
    response
}

/// A small button with an icon and a label on one line, for stacks of three.
fn small_button(ui: &mut Ui, glyph: &str, label: &str, enabled: bool, selected: bool) -> Response {
    let text = format!("{glyph}  {label}");
    let button = egui::Button::selectable(selected, text).min_size(vec2(0.0, 19.0));
    ui.add_enabled(enabled, button)
}

/// A group of buttons with its caption underneath and a divider after it.
fn group<R>(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = Palette::of(ui);
    let inner = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let inner = ui
                .horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.set_min_height(BIG_HEIGHT);
                    add(ui)
                })
                .inner;
            let width = ui.min_rect().width();
            ui.allocate_ui_with_layout(vec2(width, 16.0), Layout::top_down(Align::Center), |ui| {
                ui.label(RichText::new(caption).size(11.0).color(p.weak));
            });
            inner
        })
        .inner;
    let rect = ui.min_rect();
    let x = ui.cursor().min.x + 5.0;
    ui.painter().vline(
        x,
        (rect.top() + 4.0)..=(rect.bottom() - 4.0),
        Stroke::new(1.0, p.border),
    );
    ui.add_space(11.0);
    inner
}

/// A stack of up to three small buttons.
fn stack<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        add(ui)
    })
    .inner
}

fn home(ui: &mut Ui, cx: &mut RibbonCx<'_>) -> Option<Command> {
    let mut command = None;
    let mut set = |c| command = Some(c);
    group(ui, tr("Project"), |ui| {
        if big_button(
            ui,
            Icon::Glyph(icon::FILE_PLUS),
            tr("New"),
            true,
            false,
            None,
        )
        .on_hover_text(tr("New pump project"))
        .clicked()
        {
            set(Command::NewProject);
        }
        if big_button(
            ui,
            Icon::Glyph(icon::FOLDER_OPEN),
            tr("Open"),
            true,
            false,
            None,
        )
        .clicked()
        {
            set(Command::Open);
        }
        if big_button(
            ui,
            Icon::Glyph(icon::FLOPPY_DISK),
            tr("Save"),
            true,
            false,
            None,
        )
        .clicked()
        {
            set(Command::Save);
        }
    });
    group(ui, tr("Edit"), |ui| {
        let undo = cx.undo_label.map_or(tr("Nothing to undo").to_owned(), |l| {
            tf("Undo {}", &[&message(l)])
        });
        let redo = cx.redo_label.map_or(tr("Nothing to redo").to_owned(), |l| {
            tf("Redo {}", &[&message(l)])
        });
        if big_button(
            ui,
            Icon::Glyph(icon::ARROW_U_UP_LEFT),
            tr("Undo"),
            cx.can_undo,
            false,
            None,
        )
        .on_hover_text(undo)
        .clicked()
        {
            set(Command::Undo);
        }
        if big_button(
            ui,
            Icon::Glyph(icon::ARROW_U_UP_RIGHT),
            tr("Redo"),
            cx.can_redo,
            false,
            None,
        )
        .on_hover_text(redo)
        .clicked()
        {
            set(Command::Redo);
        }
    });
    group(ui, tr("Setup"), |ui| {
        let selected = cx.selection == Selection::Setup;
        if big_button(
            ui,
            Icon::Glyph(icon::GEAR),
            tr("Global setup"),
            true,
            selected,
            None,
        )
        .on_hover_text(tr("Design point, fluid, inlet conditions and units"))
        .clicked()
        {
            set(Command::GlobalSetup);
        }
        let us = cx.project.display_units == tp_units::DisplayUnits::us();
        stack(ui, |ui| {
            if small_button(ui, icon::RULER, tr("SI units"), true, !us).clicked() && us {
                set(Command::UsUnits(false));
            }
            if small_button(ui, icon::RULER, tr("US units"), true, us).clicked() && !us {
                set(Command::UsUnits(true));
            }
        });
    });
    group(ui, tr("Components"), |ui| {
        for (index, entry) in ADD_ENTRIES.iter().enumerate().take(4) {
            let label = tr(entry.short);
            if big_button(ui, Icon::Component(entry.kind), label, true, false, None)
                .on_hover_text(tf("Add: {}", &[&tr(entry.label)]))
                .clicked()
            {
                set(Command::Add(index));
            }
        }
    });
    group(ui, tr("Export"), |ui| {
        let response = big_button(
            ui,
            Icon::Glyph(icon::EXPORT),
            tr("Export"),
            true,
            false,
            None,
        );
        Popup::menu(&response).show(|ui| {
            ui.set_min_width(240.0);
            if let Some(c) = export_items(ui, cx) {
                set(c);
            }
        });
        let label = if cx.cad_running {
            tr("CAD running…")
        } else {
            tr("CAD solids")
        };
        if big_button(
            ui,
            Icon::Glyph(icon::CUBE),
            label,
            CAD_AVAILABLE,
            false,
            None,
        )
        .on_hover_text(tr("STEP, IGES or STL solids with Open CASCADE"))
        .clicked()
        {
            set(Command::ExportCad);
        }
    });
    command
}

/// How a step's status shows as a dot.
fn status_dot(p: &Palette, status: StepStatus) -> Option<Color32> {
    match status {
        StepStatus::Done => Some(p.done),
        StepStatus::Stale => Some(p.stale),
        StepStatus::Pending => None,
    }
}

/// Captions of the groups of design steps.
fn step_group(step: DesignStep) -> &'static str {
    use DesignStep::*;
    tr(match step {
        MainDimensions | VoluteSetup => "Dimensions",
        MeridionalContour => "Meridian",
        BladeProperties | BladeMeanLines | BladeProfiles | BladeEdges => "Blading",
        CrossSection | SpiralDevelopment | Diffuser | CutWater | WallThickness => "Casing",
        CfdSetup | ModelSettings | ModelFinishing => "Model",
    })
}

fn component_tab(
    ui: &mut Ui,
    cx: &mut RibbonCx<'_>,
    id: ComponentId,
    kind: ComponentKind,
) -> Option<Command> {
    let p = Palette::of(ui);
    let mut command = None;
    let component = cx.project.component(id)?;
    let selected_step = match cx.selection {
        Selection::Step(s, step) if s == id => Some(step),
        _ => None,
    };
    // The design steps, in groups.
    let steps = kind.steps();
    let mut start = 0;
    while start < steps.len() {
        let caption = step_group(steps[start]);
        let end = steps[start..]
            .iter()
            .position(|&s| step_group(s) != caption)
            .map_or(steps.len(), |n| start + n);
        group(ui, caption, |ui| {
            for &step in &steps[start..end] {
                let available = steps::is_available(kind, step);
                let title = step_title(step);
                let response = big_button(
                    ui,
                    Icon::Glyph(icons::step(step)),
                    &title,
                    available,
                    selected_step == Some(step),
                    status_dot(&p, component.status(step)),
                );
                let response = if available {
                    response.on_hover_text(crate::tree::status_text(component.status(step)))
                } else {
                    response.on_hover_text(steps::planned_phase(kind, step))
                };
                if response.clicked() {
                    command = Some(Command::Select(id, step));
                }
            }
        });
        start = end;
    }
    group(ui, tr("Step"), |ui| {
        if let Some(step) = selected_step {
            let (enabled, tip) = match cx.accept {
                AcceptState::Ready => (true, tr("Mark this step as done with the current values")),
                AcceptState::Blocked(reason) => (false, tr(reason)),
                AcceptState::NotApplicable => (false, tr("This step has nothing to accept.")),
            };
            if big_button(
                ui,
                Icon::Glyph(icon::CHECK_CIRCLE),
                tr("Accept"),
                enabled,
                false,
                None,
            )
            .on_hover_text(tip)
            .clicked()
            {
                command = Some(Command::Accept(id, step));
            }
            let resettable = cx.accept != AcceptState::NotApplicable;
            if big_button(
                ui,
                Icon::Glyph(icon::ARROW_COUNTER_CLOCKWISE),
                tr("Reset"),
                resettable,
                false,
                None,
            )
            .on_hover_text(tr(
                "Return every value of this step to its automatic proposal",
            ))
            .clicked()
            {
                command = Some(Command::Reset(id, step));
            }
        }
        if big_button(
            ui,
            Icon::Glyph(icon::TRASH),
            tr("Remove"),
            true,
            false,
            None,
        )
        .on_hover_text(tf(
            "Remove {} from the project",
            &[&component_name(&component.name)],
        ))
        .clicked()
        {
            command = Some(Command::Remove(id));
        }
    });
    command
}

fn view_tab(ui: &mut Ui, cx: &mut RibbonCx<'_>) -> Option<Command> {
    let mut command = None;
    let mut set = |c| command = Some(c);
    group(ui, tr("Views"), |ui| {
        for view in View::ALL {
            if big_button(
                ui,
                Icon::Glyph(view.icon()),
                view.short_title(),
                true,
                false,
                None,
            )
            .on_hover_text(view.title())
            .clicked()
            {
                set(Command::Focus(view));
            }
        }
    });
    group(ui, tr("Camera"), |ui| {
        for (glyph, label, view, tip) in [
            (
                icon::CORNERS_OUT,
                "Fit",
                CameraView::Fit,
                "Fit the model in the view",
            ),
            (
                icon::CUBE,
                "Iso",
                CameraView::Iso,
                "Oblique view from the inlet side",
            ),
            (
                icon::ARROW_LINE_DOWN,
                "Inlet",
                CameraView::Inlet,
                "Along the axis, from the inlet",
            ),
            (icon::COLUMNS, "Side", CameraView::Side, "Across the axis"),
        ] {
            if big_button(ui, Icon::Glyph(glyph), tr(label), true, false, None)
                .on_hover_text(tr(tip))
                .clicked()
            {
                set(Command::Camera(view));
            }
        }
    });
    group(ui, tr("Display"), |ui| {
        let o = &mut *cx.options;
        stack(ui, |ui| {
            if small_button(ui, icon::POLYGON, tr("Edges"), true, o.edges).clicked() {
                o.edges = !o.edges;
            }
            if small_button(
                ui,
                icon::CUBE_TRANSPARENT,
                tr("Wireframe"),
                true,
                o.wireframe,
            )
            .clicked()
            {
                o.wireframe = !o.wireframe;
            }
            if small_button(ui, icon::FAN, tr("All blades"), true, o.all_blades).clicked() {
                o.all_blades = !o.all_blades;
            }
        });
    });
    group(ui, tr("Panels"), |ui| {
        if big_button(
            ui,
            Icon::Glyph(icon::SIDEBAR_SIMPLE),
            tr("Tree"),
            true,
            cx.tree_open,
            None,
        )
        .on_hover_text(tr("The tree of design steps and of the 3D model"))
        .clicked()
        {
            set(Command::ToggleTree);
        }
        if big_button(
            ui,
            Icon::Glyph(icon::SLIDERS_HORIZONTAL),
            tr("Properties"),
            true,
            cx.sidebar_open,
            None,
        )
        .on_hover_text(tr("The properties of the selected step"))
        .clicked()
        {
            set(Command::ToggleDialog);
        }
        if big_button(
            ui,
            Icon::Glyph(icon::WARNING),
            tr("Messages"),
            true,
            cx.messages_open,
            None,
        )
        .on_hover_text(tr("Every error and warning of the project"))
        .clicked()
        {
            set(Command::ToggleMessages);
        }
        stack(ui, |ui| {
            if small_button(
                ui,
                icon::ARROWS_SPLIT,
                tr("View follows step"),
                true,
                cx.follow_step,
            )
            .on_hover_text(tr("Opening a step shows the view that suits it"))
            .clicked()
            {
                set(Command::FollowStep);
            }
        });
    });
    group(ui, tr("Theme"), |ui| {
        stack(ui, |ui| {
            for (glyph, label, theme) in [
                (icon::SUN, "Light", ThemePreference::Light),
                (icon::MOON, "Dark", ThemePreference::Dark),
                (icon::CIRCLE_HALF, "System", ThemePreference::System),
            ] {
                if small_button(ui, glyph, tr(label), true, cx.theme == theme).clicked() {
                    set(Command::Theme(theme));
                }
            }
        });
    });
    group(ui, tr("Language"), |ui| {
        stack(ui, |ui| {
            for language in Language::ALL {
                if small_button(
                    ui,
                    icon::TRANSLATE,
                    language.name(),
                    true,
                    cx.language == language,
                )
                .clicked()
                {
                    set(Command::Language(language));
                }
            }
        });
    });
    command
}

fn help_tab(ui: &mut Ui) -> Option<Command> {
    let mut command = None;
    group(ui, tr("Help"), |ui| {
        if big_button(
            ui,
            Icon::Glyph(icon::BOOK_OPEN),
            tr("Documentation"),
            true,
            false,
            None,
        )
        .on_hover_text(tr("Theory notes and examples on the project page"))
        .clicked()
        {
            command = Some(Command::Documentation);
        }
        if big_button(ui, Icon::Glyph(icon::INFO), tr("About"), true, false, None).clicked() {
            command = Some(Command::About);
        }
    });
    command
}
