//! The Scene: a panel that floats over the 3D view with the parts of the model, in the
//! order the flow crosses them. Each part has an eye that shows or hides it (Alt+click
//! isolates it), a see-through toggle and a button that isolates it. The panel docks in
//! a corner of the view, moves to another by dragging its header and folds to a pill.

use eframe::egui::{self, Id, Pos2, Rect, RichText, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};
use tp_project::ComponentKind;

use crate::floating;
use crate::i18n::{tf, tr};
use crate::icons;
use crate::theme::{Palette, component_color, icon};
use crate::viewer::{Part, ViewOptions};

/// Width of the open panel.
const WIDTH: f32 = 250.0;
/// Space between the panel and the edges of the view.
const MARGIN: f32 = 12.0;

/// A corner of the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    pub(crate) fn code(self) -> &'static str {
        match self {
            Corner::TopLeft => "top-left",
            Corner::TopRight => "top-right",
            Corner::BottomLeft => "bottom-left",
            Corner::BottomRight => "bottom-right",
        }
    }

    pub(crate) fn from_code(code: &str) -> Option<Self> {
        Corner::ALL.into_iter().find(|c| c.code() == code)
    }

    fn left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }

    fn top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }

    fn glyph(self) -> &'static str {
        match self {
            Corner::TopLeft => icon::ARROW_UP_LEFT,
            Corner::TopRight => icon::ARROW_UP_RIGHT,
            Corner::BottomLeft => icon::ARROW_DOWN_LEFT,
            Corner::BottomRight => icon::ARROW_DOWN_RIGHT,
        }
    }

    fn name(self) -> &'static str {
        tr(match self {
            Corner::TopLeft => "Top left",
            Corner::TopRight => "Top right",
            Corner::BottomLeft => "Bottom left",
            Corner::BottomRight => "Bottom right",
        })
    }
}

/// A part of the model in the scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Blades,
    Hub,
    Shroud,
    Vanes,
    Walls,
    Volute,
    Sections,
}

impl Key {
    const ALL: [Key; 7] = [
        Key::Blades,
        Key::Hub,
        Key::Shroud,
        Key::Vanes,
        Key::Walls,
        Key::Volute,
        Key::Sections,
    ];

    fn shown(self, o: &ViewOptions) -> bool {
        match self.part(o) {
            Some(p) => p.shown,
            None => o.sections,
        }
    }

    fn set_shown(self, o: &mut ViewOptions, shown: bool) {
        match self.part_mut(o) {
            Some(p) => p.shown = shown,
            None => o.sections = shown,
        }
    }

    fn part(self, o: &ViewOptions) -> Option<&Part> {
        match self {
            Key::Blades => Some(&o.blades),
            Key::Hub => Some(&o.hub),
            Key::Shroud => Some(&o.shroud),
            Key::Vanes => Some(&o.vanes),
            Key::Walls => Some(&o.walls),
            Key::Volute => Some(&o.volute),
            Key::Sections => None,
        }
    }

    fn part_mut(self, o: &mut ViewOptions) -> Option<&mut Part> {
        match self {
            Key::Blades => Some(&mut o.blades),
            Key::Hub => Some(&mut o.hub),
            Key::Shroud => Some(&mut o.shroud),
            Key::Vanes => Some(&mut o.vanes),
            Key::Walls => Some(&mut o.walls),
            Key::Volute => Some(&mut o.volute),
            Key::Sections => None,
        }
    }
}

/// The components of the active stage, by name, for the scene.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stage {
    pub impeller: String,
    /// Blades of the impeller, when its geometry is built.
    pub blades: Option<u32>,
    /// The stator after the impeller: its name and whether it has vanes.
    pub stator: Option<(String, bool)>,
    pub volute: Option<String>,
}

impl Stage {
    /// The parts it has, by component.
    fn groups(&self) -> Vec<(ComponentKind, String, Vec<Key>)> {
        let mut groups = vec![(
            ComponentKind::Impeller,
            self.impeller.clone(),
            vec![Key::Blades, Key::Hub, Key::Shroud],
        )];
        if let Some((name, vaned)) = &self.stator {
            let keys = if *vaned {
                vec![Key::Vanes, Key::Walls]
            } else {
                vec![Key::Walls]
            };
            groups.push((ComponentKind::Stator { vaned: *vaned }, name.clone(), keys));
        }
        if let Some(name) = &self.volute {
            groups.push((
                ComponentKind::Volute,
                name.clone(),
                vec![Key::Volute, Key::Sections],
            ));
        }
        groups
    }
}

fn name(key: Key) -> &'static str {
    tr(match key {
        Key::Blades => "Blades",
        Key::Hub => "Hub (disk)",
        Key::Shroud => "Shroud (cover)",
        Key::Vanes => "Vanes",
        Key::Walls => "Side walls",
        Key::Volute => "Casing",
        Key::Sections => "Section curves",
    })
}

/// The Scene between frames.
#[derive(Debug, Default)]
pub(crate) struct ScenePanel {
    pub(crate) corner: Corner,
    pub(crate) folded: bool,
    /// The top-left corner while the header is dragged.
    dragging: Option<Pos2>,
    /// The part isolated, and every part as it was before.
    isolated: Option<(Key, ViewOptions)>,
    /// Size of the panel last frame.
    size: Vec2,
}

impl ScenePanel {
    /// The width the panel takes on the left of the view, for the model to center in
    /// the rest.
    pub(crate) fn left_cover(&self) -> f32 {
        if self.corner.left() && !self.folded {
            WIDTH + MARGIN
        } else {
            0.0
        }
    }

    /// The panel over `area`, clear of the `right` points the properties hide.
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        area: Rect,
        right: f32,
        options: &mut ViewOptions,
        stage: &Stage,
    ) {
        let free = Rect::from_min_max(area.min, pos2(area.right() - right, area.bottom()));
        let size = if self.size == Vec2::ZERO {
            vec2(WIDTH, 300.0)
        } else {
            self.size
        };
        let docked = |corner: Corner| {
            let x = if corner.left() {
                free.left() + MARGIN
            } else {
                free.right() - MARGIN - size.x
            };
            let y = if corner.top() {
                free.top() + MARGIN
            } else {
                free.bottom() - MARGIN - size.y
            };
            pos2(x, y)
        };
        let at = self.dragging.unwrap_or_else(|| docked(self.corner));
        let style = ctx.style_of(ctx.theme());
        let response = egui::Area::new(Id::new("scene_panel"))
            .order(egui::Order::Middle)
            .fixed_pos(at)
            .show(ctx, |ui| {
                floating::frame(&style)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        if self.folded {
                            self.pill(ui, options, stage);
                        } else {
                            ui.set_width(WIDTH - 16.0);
                            self.panel(ui, options, stage, free);
                        }
                    });
            });
        self.size = response.response.rect.size();
        // A drag ends in the nearest corner.
        if let Some(pos) = self.dragging
            && !ctx.input(|i| i.pointer.any_down())
        {
            let center = pos + self.size / 2.0;
            self.corner = match (center.x < free.center().x, center.y < free.center().y) {
                (true, true) => Corner::TopLeft,
                (false, true) => Corner::TopRight,
                (true, false) => Corner::BottomLeft,
                (false, false) => Corner::BottomRight,
            };
            self.dragging = None;
        }
    }

    /// The folded panel: a pill that opens it again.
    fn pill(&mut self, ui: &mut Ui, options: &ViewOptions, stage: &Stage) {
        let keys: Vec<Key> = stage.groups().into_iter().flat_map(|g| g.2).collect();
        let shown = keys.iter().filter(|k| k.shown(options)).count();
        let text = format!(
            "{}  {} · {}",
            icon::STACK,
            tr("Scene"),
            tf("{}/{} shown", &[&shown, &keys.len()])
        );
        let response = ui.add(egui::Button::new(text).frame(false));
        response
            .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tr("Show the scene")));
        if response.on_hover_text(tr("Show the scene")).clicked() {
            self.folded = false;
        }
    }

    fn panel(&mut self, ui: &mut Ui, options: &mut ViewOptions, stage: &Stage, free: Rect) {
        let p = Palette::of(ui);
        let mut fold = false;
        let mut menu = None;
        let header = floating::header(ui, icon::STACK, tr("Scene"), true, |ui| {
            let tip = tr("Fold the scene");
            let button = ui.add(egui::Button::new(icon::CARET_DOUBLE_LEFT).frame(false));
            button.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tip));
            if button.on_hover_text(tip).clicked() {
                fold = true;
            }
            let more = ui.add(egui::Button::new(icon::DOTS_THREE).frame(false));
            more.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tr("Scene menu")));
            egui::Popup::menu(&more).show(|ui| {
                ui.set_min_width(200.0);
                if ui.button(tr("Show all")).clicked() {
                    menu = Some(Menu::ShowAll);
                }
                if ui.button(tr("Hide all")).clicked() {
                    menu = Some(Menu::HideAll);
                }
                if ui.button(tr("Reset the scene")).clicked() {
                    menu = Some(Menu::Reset);
                }
                ui.separator();
                ui.label(
                    RichText::new(tr("Dock in a corner (or drag the header)"))
                        .size(11.5)
                        .color(p.weak),
                );
                ui.horizontal(|ui| {
                    for corner in Corner::ALL {
                        let button =
                            egui::Button::selectable(self.corner == corner, corner.glyph());
                        if ui.add(button).on_hover_text(corner.name()).clicked() {
                            menu = Some(Menu::Dock(corner));
                        }
                    }
                });
            });
        });
        if header.drag_started() {
            self.dragging = Some(header.rect.min - vec2(8.0, 4.0));
        }
        if header.dragged()
            && let Some(pos) = self.dragging.as_mut()
        {
            *pos += header.drag_delta();
            *pos = pos.clamp(free.min, free.max - self.size);
        }
        if fold {
            self.folded = true;
        }
        let keys: Vec<Key> = stage.groups().into_iter().flat_map(|g| g.2).collect();
        match menu {
            Some(Menu::ShowAll) | Some(Menu::HideAll) => {
                let shown = menu == Some(Menu::ShowAll);
                for &k in &keys {
                    k.set_shown(options, shown);
                }
                self.isolated = None;
            }
            Some(Menu::Reset) => {
                let style = (options.edges, options.wireframe);
                *options = ViewOptions::default();
                (options.edges, options.wireframe) = style;
                self.isolated = None;
            }
            Some(Menu::Dock(corner)) => self.corner = corner,
            None => {}
        }

        // Three components at most: the list is short enough to show whole.
        for (kind, component, parts) in stage.groups() {
            self.component_row(ui, kind, &component, &parts, options, &p);
            for key in parts {
                self.part_row(ui, kind, key, options, stage, &p);
            }
        }
    }

    /// A component: its pictogram, name and an eye for all its parts.
    fn component_row(
        &mut self,
        ui: &mut Ui,
        kind: ComponentKind,
        name: &str,
        parts: &[Key],
        options: &mut ViewOptions,
        p: &Palette,
    ) {
        let shown = parts.iter().filter(|k| k.shown(options)).count();
        ui.horizontal(|ui| {
            let (glyph, color) = match shown {
                0 => (icon::EYE_SLASH, p.weak),
                n if n == parts.len() => (icon::EYE, p.text),
                _ => (icon::EYE, p.weak),
            };
            let eye = ui.add(egui::Button::new(RichText::new(glyph).color(color)).frame(false));
            let label = tf("All parts of {}", &[&name]);
            eye.widget_info(|| {
                WidgetInfo::selected(WidgetType::Checkbox, true, shown == parts.len(), &label)
            });
            if eye
                .on_hover_text(tr("Show or hide the whole component"))
                .clicked()
            {
                let show = shown < parts.len();
                for &k in parts {
                    k.set_shown(options, show);
                }
                self.isolated = None;
            }
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
            icons::component(ui.painter(), rect, kind, component_color(kind));
            painted(ui, name, crate::theme::bold_family(), p.text);
        });
    }

    /// A part: eye, color, name, the blade count, see-through and isolate.
    fn part_row(
        &mut self,
        ui: &mut Ui,
        kind: ComponentKind,
        key: Key,
        options: &mut ViewOptions,
        stage: &Stage,
        p: &Palette,
    ) {
        let label = name(key);
        let shown = key.shown(options);
        let isolated = self.isolated.is_some_and(|(k, _)| k == key);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let glyph = if shown { icon::EYE } else { icon::EYE_SLASH };
            let color = if shown { p.text } else { p.weak };
            let eye = ui.add(egui::Button::new(RichText::new(glyph).color(color)).frame(false));
            eye.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, shown, label));
            let eye = eye.on_hover_text(tr("Show or hide; Alt+click isolates it"));
            if eye.clicked() {
                if ui.input(|i| i.modifiers.alt) {
                    self.isolate(key, options);
                } else {
                    key.set_shown(options, !shown);
                    self.isolated = None;
                }
            }
            let (swatch, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(
                swatch,
                2,
                component_color(kind).gamma_multiply(if shown { 1.0 } else { 0.4 }),
            );
            painted(ui, label, egui::FontFamily::Proportional, color);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let tip = tf("Isolate {}", &[&label]);
                let iso = ui.add(egui::Button::selectable(isolated, icon::CROSSHAIR_SIMPLE));
                iso.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, isolated, &tip));
                if iso
                    .on_hover_text(if isolated {
                        tr("Show the other parts again")
                    } else {
                        tr("Show only this part")
                    })
                    .clicked()
                {
                    self.isolate(key, options);
                }
                if let Some(part) = key.part_mut(options) {
                    let tip = tf("See-through {}", &[&label]);
                    let glass = ui.add_enabled(
                        part.shown,
                        egui::Button::selectable(part.glass, icon::CUBE_TRANSPARENT),
                    );
                    glass.widget_info(|| {
                        WidgetInfo::selected(WidgetType::Button, part.shown, part.glass, &tip)
                    });
                    if glass.on_hover_text(tr("See-through")).clicked() {
                        part.glass = !part.glass;
                    }
                }
                if key == Key::Blades
                    && let Some(count) = stage.blades
                {
                    let text = if options.all_blades {
                        format!("{count}/{count}")
                    } else {
                        format!("1/{count}")
                    };
                    let chip = ui.add(egui::Button::selectable(false, text).small());
                    chip.widget_info(|| {
                        WidgetInfo::selected(
                            WidgetType::Button,
                            true,
                            options.all_blades,
                            tr("All blades"),
                        )
                    });
                    if chip.on_hover_text(tr("All blades, or only one")).clicked() {
                        options.all_blades = !options.all_blades;
                    }
                }
            });
        });
    }

    /// Shows only `key`, or, when it is isolated already, every part as before.
    fn isolate(&mut self, key: Key, options: &mut ViewOptions) {
        match self.isolated.take() {
            Some((k, before)) if k == key => {
                for k in Key::ALL {
                    k.set_shown(options, k.shown(&before));
                }
            }
            previous => {
                let before = previous.map_or(*options, |(_, b)| b);
                for k in Key::ALL {
                    k.set_shown(options, k == key);
                }
                self.isolated = Some((key, before));
            }
        }
    }
}

/// A name drawn without a widget: the eye before it already carries it for screen
/// readers.
fn painted(ui: &mut Ui, text: &str, family: egui::FontFamily, color: egui::Color32) {
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::new(13.0, family), color);
    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    ui.painter().galley(rect.min, galley, color);
}

/// What the menu of the scene asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Menu {
    ShowAll,
    HideAll,
    Reset,
    Dock(Corner),
}
