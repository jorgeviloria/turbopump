//! Toolbar at the top of the window. Its tabs are the views of the main area: a tab
//! shows its view and, under the tabs, the tools of that view only. Left of the tabs
//! are the File menu and quick access to the tree, saving, undo and redo; right of them
//! the project name, the settings, the help and the button that folds the tools away.
//! Buttons only report a [`Command`]; the application carries it out.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Response, RichText, Sense, Stroke,
    StrokeKind, ThemePreference, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use tp_project::Project;
use tp_units::QuantityKind;

use crate::app::View;
use crate::export::{CAD_AVAILABLE, DrawingExport, TextExport, VoluteExport};
use crate::i18n::{Language, message, project_name, tf, tr};
use crate::settings::Section;
use crate::theme::{Palette, bold, icon};
use crate::viewer::CameraView;

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
    UsUnits(bool),
    /// Display unit of a quantity, by symbol.
    Unit(QuantityKind, &'static str),
    ExportText(TextExport),
    ExportDrawing(DrawingExport),
    ExportVolute(VoluteExport),
    ExportCad,
    /// A PNG picture of the 3D view.
    Picture3d,
    /// While the curves are shown, the settings of the curves in the properties panel
    /// (true) or the step (false).
    CurveSettings(bool),
    Focus(View),
    Camera(CameraView),
    ToggleTree,
    FollowStep,
    Theme(ThemePreference),
    /// Scale of the whole interface.
    Zoom(f32),
    Language(Language),
    Settings(Section),
    About,
    Documentation,
}

/// Toolbar state between frames.
#[derive(Debug, Default)]
pub(crate) struct Ribbon {
    collapsed: bool,
}

/// What the toolbar reads.
pub(crate) struct RibbonCx<'a> {
    pub project: &'a Project,
    /// The view in the main area, whose tab is selected.
    pub view: View,
    /// The project has unsaved changes.
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<&'a str>,
    pub redo_label: Option<&'a str>,
    pub has_geometry: bool,
    pub has_volute: bool,
    pub tree_open: bool,
}

/// Height of a large button.
const BIG_HEIGHT: f32 = 70.0;
const LABEL_SIZE: f32 = 11.5;

impl Ribbon {
    /// The toolbar. `tools` draws the groups of the selected view under the tabs.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &RibbonCx<'_>,
        tools: impl FnOnce(&mut Ui) -> Option<Command>,
    ) -> Option<Command> {
        let p = Palette::of(ui);
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
                    command = command.or(file_menu(ui, cx, &p));
                    ui.add_space(4.0);
                    command = command.or(quick_access(ui, cx));
                    ui.add_space(6.0);
                    for view in View::ALL {
                        if matches!(view, View::View3d | View::Performance) {
                            let r = ui.cursor();
                            ui.add_space(4.0);
                            ui.painter().vline(
                                r.left() + 2.0,
                                (r.top() + 6.0)..=(r.top() + 20.0),
                                Stroke::new(1.0, p.border),
                            );
                            ui.add_space(4.0);
                        }
                        if tab_button(ui, view, cx.view == view) {
                            command = Some(Command::Focus(view));
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let (glyph, tip) = if self.collapsed {
                            (icon::CARET_DOWN, tr("Show the tools"))
                        } else {
                            (
                                icon::CARET_UP,
                                tr("Hide the tools; the tabs still pick the view"),
                            )
                        };
                        let fold = ui.add(egui::Button::new(glyph).frame(false));
                        fold.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tip));
                        if fold.on_hover_text(tip).clicked() {
                            self.collapsed = !self.collapsed;
                        }
                        command = command.or(help_menu(ui));
                        let tip = tr("Settings: theme, language, units…");
                        let gear = ui.add(egui::Button::new(icon::GEAR_SIX).frame(false));
                        gear.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tip));
                        if gear.on_hover_text(tip).clicked() {
                            command = Some(Command::Settings(Section::default()));
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
        // The tools of the view.
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
                    command = command.or(tools(ui));
                });
            });
        let rect = ui.min_rect();
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.border));
        command
    }
}

/// A tab of the strip: a view, with its icon. Returns whether it was clicked.
fn tab_button(ui: &mut Ui, view: View, selected: bool) -> bool {
    let p = Palette::of(ui);
    let font = FontId::proportional(13.0);
    let text_color = if selected { p.text } else { p.weak };
    let label = view.short_title();
    let galley = ui
        .painter()
        .layout_no_wrap(format!("{}  {label}", view.icon()), font, text_color);
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
    if selected {
        ui.painter().hline(
            rect.shrink2(vec2(6.0, 0.0)).x_range(),
            rect.bottom() - 1.0,
            Stroke::new(2.0, p.accent),
        );
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, text_color);
    response.on_hover_text(view.title()).clicked()
}

/// The tree toggle, saving, undo and redo, always at hand.
fn quick_access(ui: &mut Ui, cx: &RibbonCx<'_>) -> Option<Command> {
    let mut command = None;
    let undo = cx.undo_label.map_or(tr("Nothing to undo").to_owned(), |l| {
        tf("Undo {}", &[&message(l)])
    });
    let redo = cx.redo_label.map_or(tr("Nothing to redo").to_owned(), |l| {
        tf("Redo {}", &[&message(l)])
    });
    let tree = if cx.tree_open {
        tr("Hide the tree")
    } else {
        tr("Show the tree")
    };
    let items = [
        (
            icon::SIDEBAR_SIMPLE,
            tree,
            true,
            cx.tree_open,
            Command::ToggleTree,
        ),
        (icon::FLOPPY_DISK, tr("Save"), true, false, Command::Save),
        (
            icon::ARROW_U_UP_LEFT,
            &undo[..],
            cx.can_undo,
            false,
            Command::Undo,
        ),
        (
            icon::ARROW_U_UP_RIGHT,
            &redo[..],
            cx.can_redo,
            false,
            Command::Redo,
        ),
    ];
    for (glyph, tip, enabled, selected, c) in items {
        let button = egui::Button::selectable(selected, RichText::new(glyph).size(16.0));
        let response = ui.add_enabled(enabled, button);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, tip));
        if response
            .on_hover_text(tip)
            .on_disabled_hover_text(tip)
            .clicked()
        {
            command = Some(c);
        }
    }
    command
}

/// The help: documentation and the about box.
fn help_menu(ui: &mut Ui) -> Option<Command> {
    let mut command = None;
    let button = egui::Button::new(icon::QUESTION).frame(false);
    let response = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(200.0);
        if ui
            .button(format!("{}  {}", icon::BOOK_OPEN, tr("Documentation")))
            .on_hover_text(tr("Theory notes and examples on the project page"))
            .clicked()
        {
            command = Some(Command::Documentation);
        }
        if ui
            .button(format!("{}  {}", icon::INFO, tr("About")))
            .clicked()
        {
            command = Some(Command::About);
        }
    });
    response
        .0
        .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tr("Help")));
    response.0.on_hover_text(tr("Help"));
    command
}

fn file_menu(ui: &mut Ui, cx: &RibbonCx<'_>, p: &Palette) -> Option<Command> {
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
        item(
            ui,
            icon::GEAR_SIX,
            tr("Settings…"),
            &format!("{cmd},"),
            Command::Settings(Section::default()),
        );
        ui.separator();
        if let Some(c) = export_items(ui, cx.has_geometry, cx.has_volute) {
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

/// Every export: points, meshes, drawings and CAD solids.
pub(crate) fn export_items(ui: &mut Ui, has_geometry: bool, has_volute: bool) -> Option<Command> {
    let mut command = None;
    for kind in TextExport::ALL {
        if ui
            .add_enabled(has_geometry, egui::Button::new(kind.menu_label()))
            .clicked()
        {
            command = Some(Command::ExportText(kind));
        }
    }
    ui.separator();
    for kind in DrawingExport::ALL {
        if ui
            .add_enabled(has_geometry, egui::Button::new(kind.menu_label()))
            .clicked()
        {
            command = Some(Command::ExportDrawing(kind));
        }
    }
    if has_volute {
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
pub(crate) enum Icon {
    Glyph(&'static str),
}

/// A large button: icon over a label of up to two lines, with an optional status dot.
pub(crate) fn big_button(
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
    let Icon::Glyph(glyph) = icon;
    painter.text(
        icon_center,
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(24.0),
        if enabled { p.text } else { color },
    );
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
pub(crate) fn small_button(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    enabled: bool,
    selected: bool,
) -> Response {
    let text = format!("{glyph}  {label}");
    let button = egui::Button::selectable(selected, text).min_size(vec2(0.0, 19.0));
    ui.add_enabled(enabled, button)
}

/// A group of buttons with its caption underneath and a divider after it.
pub(crate) fn group<R>(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
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
pub(crate) fn stack<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        add(ui)
    })
    .inner
}
