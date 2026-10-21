//! Settings window: appearance, language, units and behavior of the editor in one
//! place. Like the toolbar, it only reports [`Command`]s; the application applies them.

use eframe::egui::{
    self, Align, Button, ComboBox, Layout, Modal, RichText, ScrollArea, ThemePreference, Ui, vec2,
};
use tp_units::{DisplayUnits, QuantityKind};

use crate::i18n::{Language, tr};
use crate::ribbon::Command;
use crate::theme::{Palette, bold, icon};

/// A page of the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Section {
    #[default]
    Appearance,
    Language,
    Units,
    Behavior,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::Appearance,
        Section::Language,
        Section::Units,
        Section::Behavior,
    ];

    fn glyph(self) -> &'static str {
        match self {
            Section::Appearance => icon::PALETTE,
            Section::Language => icon::TRANSLATE,
            Section::Units => icon::RULER,
            Section::Behavior => icon::CURSOR_CLICK,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Section::Appearance => tr("Appearance"),
            Section::Language => tr("Language"),
            Section::Units => tr("Units"),
            Section::Behavior => tr("Behavior"),
        }
    }
}

/// Widths of the list of pages, the gap after it and a page.
const NAV_WIDTH: f32 = 160.0;
const GAP: f32 = 17.0;
const PAGE_WIDTH: f32 = 500.0;
const PAGE_HEIGHT: f32 = 430.0;

/// Interface scales offered; the keyboard zoom reaches the ones in between.
const ZOOM_STEPS: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];

/// What the settings window reads.
pub(crate) struct SettingsCx<'a> {
    pub theme: ThemePreference,
    pub zoom: f32,
    pub language: Language,
    pub units: &'a DisplayUnits,
    pub follow_step: bool,
}

/// What the settings window asks for in one frame.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    pub commands: Vec<Command>,
    pub close: bool,
}

/// Draws the settings window open on `section`.
pub(crate) fn show(ctx: &egui::Context, section: &mut Section, cx: &SettingsCx<'_>) -> Outcome {
    let mut out = Outcome::default();
    let response = Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
        // A fixed size: the window neither grows into the screen nor moves between pages.
        ui.set_width(NAV_WIDTH + GAP + PAGE_WIDTH);
        ui.horizontal(|ui| {
            ui.heading(format!("{}  {}", icon::GEAR_SIX, tr("Settings")));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(Button::new(icon::X).frame(false))
                    .on_hover_text(tr("Close"))
                    .clicked()
                {
                    out.close = true;
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let size = vec2(NAV_WIDTH, PAGE_HEIGHT);
            let nav = ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                ui.set_min_size(size);
                ui.spacing_mut().item_spacing.y = 2.0;
                for s in Section::ALL {
                    let label = format!("{}  {}", s.glyph(), s.title());
                    let button = Button::selectable(*section == s, label)
                        .right_text("")
                        .min_size(vec2(NAV_WIDTH, 30.0));
                    if ui.add(button).clicked() {
                        *section = s;
                    }
                }
            });
            let rect = nav.response.rect;
            let x = rect.right() + GAP / 2.0;
            ui.painter().vline(
                x,
                rect.y_range(),
                ui.visuals().widgets.noninteractive.bg_stroke,
            );
            ui.add_space(GAP);
            let size = vec2(PAGE_WIDTH, PAGE_HEIGHT);
            ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                ui.set_min_size(size);
                ScrollArea::vertical()
                    .id_salt(("settings_page", *section))
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.label(bold(section.title()).size(16.0));
                        ui.add_space(8.0);
                        let commands = &mut out.commands;
                        match section {
                            Section::Appearance => appearance(ui, cx, commands),
                            Section::Language => language(ui, cx, commands),
                            Section::Units => units(ui, cx, commands),
                            Section::Behavior => behavior(ui, cx, commands),
                        }
                    });
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(tr("Close")).clicked() {
                    out.close = true;
                }
            });
        });
    });
    if response.should_close() {
        out.close = true;
    }
    out
}

/// A labelled block of a page, with a weak line under the label.
fn block(ui: &mut Ui, title: &str, hint: &str, content: impl FnOnce(&mut Ui)) {
    let p = Palette::of(ui);
    ui.label(bold(title));
    if !hint.is_empty() {
        ui.label(RichText::new(hint).color(p.weak).small());
    }
    ui.add_space(4.0);
    content(ui);
    ui.add_space(14.0);
}

fn appearance(ui: &mut Ui, cx: &SettingsCx<'_>, out: &mut Vec<Command>) {
    block(ui, tr("Theme"), tr("Colors of the whole editor"), |ui| {
        ui.horizontal(|ui| {
            for (glyph, label, theme) in [
                (icon::SUN, tr("Light"), ThemePreference::Light),
                (icon::MOON, tr("Dark"), ThemePreference::Dark),
                (icon::CIRCLE_HALF, tr("System"), ThemePreference::System),
            ] {
                let text = RichText::new(format!("{glyph}\n{label}")).size(14.0);
                let button =
                    Button::selectable(cx.theme == theme, text).min_size(vec2(110.0, 60.0));
                if ui.add(button).clicked() && cx.theme != theme {
                    out.push(Command::Theme(theme));
                }
            }
        });
    });
    let hint = if cfg!(target_os = "macos") {
        tr("Size of text and controls. ⌘+ and ⌘− also change it")
    } else {
        tr("Size of text and controls. Ctrl+ and Ctrl− also change it")
    };
    block(ui, tr("Interface size"), hint, |ui| {
        ui.horizontal(|ui| {
            for zoom in ZOOM_STEPS {
                let selected = (cx.zoom - zoom).abs() < 0.01;
                let label = format!("{:.0} %", zoom * 100.0);
                if ui
                    .add(Button::selectable(selected, label).min_size(vec2(56.0, 26.0)))
                    .clicked()
                    && !selected
                {
                    out.push(Command::Zoom(zoom));
                }
            }
        });
    });
}

fn language(ui: &mut Ui, cx: &SettingsCx<'_>, out: &mut Vec<Command>) {
    block(
        ui,
        tr("Interface language"),
        tr("Menus, steps, messages and reports. The choice is kept between sessions"),
        |ui| {
            for language in Language::ALL {
                let selected = cx.language == language;
                let label = format!("{}  {}", icon::TRANSLATE, language.name());
                let button = Button::selectable(selected, label)
                    .right_text("")
                    .min_size(vec2(220.0, 28.0));
                if ui.add(button).clicked() && !selected {
                    out.push(Command::Language(language));
                }
            }
        },
    );
}

fn units(ui: &mut Ui, cx: &SettingsCx<'_>, out: &mut Vec<Command>) {
    let si = *cx.units == DisplayUnits::si();
    let us = *cx.units == DisplayUnits::us();
    block(
        ui,
        tr("Unit system"),
        tr("Units are saved with the project, and changing them can be undone"),
        |ui| {
            ui.horizontal(|ui| {
                for (label, selected, use_us) in [
                    (tr("SI (metric)"), si, false),
                    (tr("US customary"), us, true),
                ] {
                    let button = Button::selectable(selected, label).min_size(vec2(120.0, 26.0));
                    if ui.add(button).clicked() && !selected {
                        out.push(Command::UsUnits(use_us));
                    }
                }
                if !si && !us {
                    ui.add_enabled(
                        false,
                        Button::selectable(true, tr("Custom")).min_size(vec2(90.0, 26.0)),
                    );
                }
            });
        },
    );
    block(ui, tr("Unit of each quantity"), "", |ui| {
        egui::Grid::new("settings_units")
            .num_columns(2)
            .spacing([24.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for kind in QuantityKind::ALL {
                    if kind.units().len() < 2 {
                        continue;
                    }
                    let current = cx.units.get(kind).symbol;
                    ui.label(kind_label(kind));
                    ComboBox::from_id_salt(("settings_unit", kind))
                        .selected_text(current)
                        .width(110.0)
                        .height(400.0)
                        .show_ui(ui, |ui| {
                            for unit in kind.units() {
                                let selected = unit.symbol == current;
                                if ui.selectable_label(selected, unit.symbol).clicked() && !selected
                                {
                                    out.push(Command::Unit(kind, unit.symbol));
                                }
                            }
                        });
                    ui.end_row();
                }
            });
    });
}

fn behavior(ui: &mut Ui, cx: &SettingsCx<'_>, out: &mut Vec<Command>) {
    block(
        ui,
        tr("Views"),
        tr("Opening a step shows the view that suits it"),
        |ui| {
            let mut follow = cx.follow_step;
            if ui.checkbox(&mut follow, tr("View follows step")).changed() {
                out.push(Command::FollowStep);
            }
        },
    );
}

/// Name of a quantity in the settings window.
fn kind_label(kind: QuantityKind) -> &'static str {
    match kind {
        QuantityKind::Dimensionless => tr("Dimensionless"),
        QuantityKind::Length => tr("Length"),
        QuantityKind::Head => tr("Head"),
        QuantityKind::Area => tr("Area"),
        QuantityKind::Angle => tr("Angle"),
        QuantityKind::Velocity => tr("Velocity"),
        QuantityKind::RotationalSpeed => tr("Rotational speed"),
        QuantityKind::VolumeFlow => tr("Volume flow"),
        QuantityKind::MassFlow => tr("Mass flow"),
        QuantityKind::Pressure => tr("Pressure"),
        QuantityKind::Temperature => tr("Temperature"),
        QuantityKind::Density => tr("Density"),
        QuantityKind::DynamicViscosity => tr("Dynamic viscosity"),
        QuantityKind::KinematicViscosity => tr("Kinematic viscosity"),
        QuantityKind::Power => tr("Power"),
        QuantityKind::Torque => tr("Torque"),
        QuantityKind::Force => tr("Force"),
        QuantityKind::SpecificEnergy => tr("Specific energy"),
    }
}
