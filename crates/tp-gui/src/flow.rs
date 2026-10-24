//! The design flow on the left of the window. From top to bottom: the design point,
//! the components in the order the flow crosses them, each with its steps grouped in
//! phases, their status and a key value, and the results. Only the active component
//! stays open, besides those pinned open. Folded, the panel leaves a rail of icons with
//! a progress ring per component.

use std::collections::{BTreeSet, HashMap};

use eframe::egui::{
    self, Align2, Color32, FontId, Popup, Rect, Response, RichText, Sense, Shape, Stroke, Ui, Vec2,
    WidgetInfo, WidgetType, pos2, text::LayoutJob, vec2,
};
use tp_project::{
    Component, ComponentId, ComponentKind, DesignStep, EnergyTransfer, FlowRate, Project,
    StepStatus,
};
use tp_units::{DisplayUnits, QuantityKind};

use crate::app::{EditCtx, Selection, View};
use crate::i18n::{component_name, project_name, tf, tr};
use crate::icons;
use crate::messages::{self, Severity};
use crate::ribbon::{Command, export_items};
use crate::steps;
use crate::theme::{Palette, bold, bold_family, component_color, icon};
use crate::tree::{ADD_ENTRIES, add_component_at, remove_component, status_icon, step_title};

/// Height of a step row.
const ROW: f32 = 24.0;
/// Where the steps start, right of the flow line.
const INDENT: f32 = 22.0;
/// x of the flow line from the left of the panel.
const LINE_X: f32 = 9.0;

/// The panel between frames: what is open and what is pinned.
#[derive(Debug, Default)]
pub(crate) struct FlowPanel {
    /// Components pinned open, by project file (`""` for one not saved yet).
    pins: HashMap<String, BTreeSet<u32>>,
    /// Components opened by hand; they close again when the active one changes.
    opened: BTreeSet<ComponentId>,
    /// The active component was folded by hand; it opens again with the next step.
    active_folded: bool,
    /// The component and step selected last.
    last: Option<(ComponentId, DesignStep)>,
    /// The selection changed: scroll its row into view.
    scroll: bool,
}

impl FlowPanel {
    /// Reads the pins kept between sessions: one line per project file, the path, a
    /// tab and the component ids separated by commas.
    pub(crate) fn load_pins(&mut self, text: &str) {
        for line in text.lines() {
            if let Some((path, ids)) = line.rsplit_once('\t') {
                let ids = ids.split(',').filter_map(|i| i.parse().ok()).collect();
                self.pins.insert(path.to_owned(), ids);
            }
        }
    }

    /// The pins of saved projects, as [`Self::load_pins`] reads them.
    pub(crate) fn pins_text(&self) -> String {
        let mut lines: Vec<String> = self
            .pins
            .iter()
            .filter(|(path, ids)| !path.is_empty() && !ids.is_empty())
            .map(|(path, ids)| {
                let ids: Vec<String> = ids.iter().map(u32::to_string).collect();
                format!("{path}\t{}", ids.join(","))
            })
            .collect();
        lines.sort();
        lines.join("\n")
    }

    fn pinned(&self, file: &str, id: ComponentId) -> bool {
        self.pins.get(file).is_some_and(|ids| ids.contains(&id.0))
    }

    fn toggle_pin(&mut self, file: &str, id: ComponentId) {
        let ids = self.pins.entry(file.to_owned()).or_default();
        if !ids.remove(&id.0) {
            ids.insert(id.0);
        }
    }

    /// Follows the selection: a new active component closes the ones opened by hand,
    /// and a new step opens the active one again.
    pub(crate) fn follow(&mut self, selection: Selection) {
        let Selection::Step(id, step) = selection else {
            return;
        };
        match self.last {
            Some((last, _)) if last != id => {
                self.opened.clear();
                self.active_folded = false;
            }
            Some((_, last)) if last != step => self.active_folded = false,
            _ => {}
        }
        if self.last != Some((id, step)) {
            self.scroll = true;
        }
        self.last = Some((id, step));
    }

    /// The component the panel keeps open: the selected one, or the last one while
    /// the design point is selected.
    fn active(&self, selection: Selection) -> Option<ComponentId> {
        match selection {
            Selection::Step(id, _) => Some(id),
            Selection::Setup => self.last.map(|(id, _)| id),
        }
    }

    fn is_open(&self, file: &str, selection: Selection, id: ComponentId) -> bool {
        let active = self.active(selection) == Some(id);
        (active && !self.active_folded) || self.pinned(file, id) || self.opened.contains(&id)
    }

    /// The open panel. `file` names the project for its pins.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        file: &str,
        has_geometry: bool,
        has_volute: bool,
    ) -> Option<Command> {
        let p = Palette::of(ui);
        let project = cx.doc.project().clone();
        let units = project.display_units.clone();
        let mut command = None;

        // Results and adding components stay at the bottom.
        egui::Panel::bottom("flow_results")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)))
            .show(ui, |ui| {
                command = results(ui, &project, has_geometry, has_volute);
                ui.add_space(4.0);
                let add = ui.add(
                    egui::Button::new(format!("{}  {}", icon::PLUS, tr("Add component")))
                        .frame(false),
                );
                add.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, tr("Add component"))
                });
                Popup::menu(&add).show(|ui| {
                    ui.set_min_width(220.0);
                    crate::tree::add_component_menu(ui, cx);
                });
            });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                header(ui, &project, &p);
                ui.add_space(6.0);
                setup_node(ui, cx, &project, &units, &p);
                ui.add_space(4.0);
                end_marker(ui, tr("INLET"), &p);
                let components: Vec<Component> = project.components().to_vec();
                for (index, component) in components.iter().enumerate() {
                    gap(ui, cx, &project, index, &p);
                    self.component(ui, cx, file, component, &units, &p);
                }
                gap(ui, cx, &project, components.len(), &p);
                end_marker(ui, tr("OUTLET"), &p);
            });
        command
    }

    /// One component: its header and, open, its steps by phase.
    fn component(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        file: &str,
        component: &Component,
        units: &DisplayUnits,
        p: &Palette,
    ) {
        let id = component.id();
        let kind = component.kind();
        let color = component_color(kind);
        let top = ui.cursor().top();
        let open = self.is_open(file, *cx.selection, id);
        let active = self.active(*cx.selection) == Some(id);
        let pinned = self.pinned(file, id);
        let all = messages::of_component(cx.derived, id);
        let errors = all.iter().filter(|m| m.severity == Severity::Error).count();
        let progress = Progress::of(component);

        // Header: caret, pictogram and name, counters, progress and pin.
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
        let name = component_name(&component.name);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &name));
        let caret_rect = Rect::from_min_size(
            pos2(rect.left() + INDENT - 4.0, rect.top()),
            vec2(18.0, 28.0),
        );
        let pin_rect = Rect::from_min_size(pos2(rect.right() - 22.0, rect.top()), vec2(22.0, 28.0));
        let caret = ui.interact(
            caret_rect,
            ui.id().with(("flow_caret", id.0)),
            Sense::click(),
        );
        caret
            .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Fold {}", &[&name])));
        let pin = ui.interact(pin_rect, ui.id().with(("flow_pin", id.0)), Sense::click());
        let pin_tip = if pinned {
            tr("Pinned open: click to let it fold with the others")
        } else {
            tr("Pin open: it stays open when another component is active")
        };
        pin.widget_info(|| {
            WidgetInfo::selected(WidgetType::Checkbox, true, pinned, tr("Pin open"))
        });
        let pin = pin.on_hover_text(pin_tip);
        if response.hovered() || caret.hovered() || pin.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        let painter = ui.painter();
        let y = rect.center().y;
        painter.text(
            caret_rect.center(),
            Align2::CENTER_CENTER,
            if open {
                icon::CARET_DOWN
            } else {
                icon::CARET_RIGHT
            },
            FontId::proportional(12.0),
            p.weak,
        );
        icons::component(
            painter,
            Rect::from_center_size(pos2(caret_rect.right() + 10.0, y), Vec2::splat(16.0)),
            kind,
            color,
        );
        // Right to left: pin, progress, counters.
        let mut x = pin_rect.left();
        if pinned || response.hovered() || pin.hovered() || caret.hovered() {
            painter.text(
                pin_rect.center(),
                Align2::CENTER_CENTER,
                icon::PUSH_PIN,
                FontId::proportional(13.0),
                if pinned { p.accent } else { p.weak },
            );
        }
        let bar = Rect::from_min_size(pos2(x - 34.0, y - 2.0), vec2(30.0, 4.0));
        painter.rect_filled(bar, 2, p.border);
        if progress.total > 0 {
            let done = bar.width() * progress.done as f32 / progress.total as f32;
            painter.rect_filled(Rect::from_min_size(bar.min, vec2(done, 4.0)), 2, color);
        }
        x = bar.left() - 4.0;
        let text = painter.text(
            pos2(x, y),
            Align2::RIGHT_CENTER,
            format!("{}/{}", progress.done, progress.total),
            FontId::proportional(11.5),
            p.weak,
        );
        x = text.left() - 6.0;
        let warn = ui.visuals().warn_fg_color;
        let err = ui.visuals().error_fg_color;
        for (n, glyph, c) in [
            (all.len() - errors, icon::WARNING, warn),
            (errors, icon::X_CIRCLE, err),
            (progress.stale, icon::ARROWS_CLOCKWISE, p.stale),
        ] {
            if n > 0 {
                let r = painter.text(
                    pos2(x, y),
                    Align2::RIGHT_CENTER,
                    format!("{glyph}{n}"),
                    FontId::proportional(11.5),
                    c,
                );
                x = r.left() - 5.0;
            }
        }
        let name_left = caret_rect.right() + 22.0;
        let galley = painter.layout_job(truncated(
            &name,
            FontId::new(13.0, bold_family()),
            p.text,
            (x - name_left).max(20.0),
        ));
        painter.galley(pos2(name_left, y - galley.size().y / 2.0), galley, p.text);

        if caret.clicked() {
            if active {
                self.active_folded = !self.active_folded;
            } else if !self.opened.remove(&id) {
                self.opened.insert(id);
            }
        } else if pin.clicked() {
            self.toggle_pin(file, id);
        } else if response.clicked()
            && let Some(step) = component.next_step().or(kind.steps().first().copied())
        {
            cx.open(Selection::Step(id, step));
        }
        let first_stale = component
            .steps()
            .find(|(_, s)| *s == StepStatus::Stale)
            .map(|(step, _)| step);
        let mut remove = false;
        response.context_menu(|ui| {
            if let Some(step) = first_stale
                && ui
                    .button(format!(
                        "{}  {}",
                        icon::ARROWS_CLOCKWISE,
                        tr("Go to the first outdated step")
                    ))
                    .clicked()
            {
                cx.open(Selection::Step(id, step));
            }
            if ui
                .button(format!("{}  {}", icon::TRASH, tr("Remove component")))
                .clicked()
            {
                remove = true;
            }
        });

        if open {
            let next = component.next_step();
            let mut planned = Vec::new();
            let mut caption = None;
            for (step, status) in component.steps() {
                if !steps::is_available(kind, step) {
                    planned.push(step);
                    continue;
                }
                let phase = phase(kind, step);
                if caption != Some(phase) {
                    caption = Some(phase);
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(INDENT + 6.0);
                        ui.label(RichText::new(tr(phase)).size(10.5).color(p.weak).strong());
                    });
                }
                let selected = *cx.selection == Selection::Step(id, step);
                let (glyph, glyph_color) = if selected {
                    (icon::CIRCLE, p.accent)
                } else {
                    status_icon(p, status)
                };
                let value = if status == StepStatus::Done {
                    key_value(cx, id, kind, step, units)
                } else {
                    None
                };
                let is_next = next == Some(step) && !selected;
                let row = step_row(
                    ui,
                    &step_title(step),
                    glyph,
                    glyph_color,
                    value.as_deref(),
                    is_next,
                    selected,
                    p,
                );
                if selected && self.scroll {
                    row.scroll_to_me(Some(egui::Align::Center));
                    self.scroll = false;
                }
                if row
                    .on_hover_text(crate::tree::status_text(status))
                    .clicked()
                {
                    cx.open(Selection::Step(id, step));
                }
            }
            if !planned.is_empty() {
                let names: Vec<String> = planned.iter().map(|&s| step_title(s)).collect();
                let reasons: Vec<String> = planned
                    .iter()
                    .map(|&s| steps::planned_phase(kind, s))
                    .filter(|r| !r.is_empty())
                    .collect();
                step_row(
                    ui,
                    &names.join(" · "),
                    icon::DOTS_THREE,
                    p.weak,
                    Some(tr("planned")),
                    false,
                    false,
                    p,
                )
                .on_hover_text(reasons.join("\n"));
            }
            ui.add_space(4.0);
        }
        // The flow line through the component, in its color.
        let bottom = ui.cursor().top();
        let left = ui.max_rect().left();
        ui.painter().vline(
            left + LINE_X,
            (top + 4.0)..=(bottom - 2.0),
            Stroke::new(2.0, color.gamma_multiply(0.8)),
        );
        if remove {
            remove_component(cx, id);
        }
    }

    /// The folded panel: a rail of icons, each component with its progress ring.
    pub(crate) fn rail(&mut self, ui: &mut Ui, cx: &mut EditCtx<'_>) -> Option<Command> {
        let p = Palette::of(ui);
        let project = cx.doc.project().clone();
        let mut command = None;
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            if tile(
                ui,
                tr("Show the design flow"),
                false,
                |painter, rect, color| {
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        icon::CARET_DOUBLE_RIGHT,
                        FontId::proportional(18.0),
                        color,
                    );
                },
            )
            .clicked()
            {
                command = Some(Command::ToggleTree);
            }
            let setup = *cx.selection == Selection::Setup;
            if tile(
                ui,
                tr("Design point and fluid"),
                setup,
                |painter, rect, _| {
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        icon::TARGET,
                        FontId::proportional(22.0),
                        p.accent,
                    );
                },
            )
            .clicked()
            {
                cx.open(Selection::Setup);
            }
            separator(ui, &p);
            for component in project.components() {
                let id = component.id();
                let kind = component.kind();
                let progress = Progress::of(component);
                let selected = matches!(*cx.selection, Selection::Step(s, _) if s == id);
                let name = component_name(&component.name);
                let ring = if progress.stale > 0 {
                    p.stale
                } else {
                    component_color(kind)
                };
                let response = tile(ui, &name, selected, |painter, rect, _| {
                    let center = rect.center();
                    let radius = rect.width() / 2.0 + 3.0;
                    painter.circle_stroke(center, radius, Stroke::new(2.5, p.border));
                    if progress.total > 0 && progress.done > 0 {
                        let share = progress.done as f32 / progress.total as f32;
                        let n = 48;
                        let points: Vec<_> = (0..=n)
                            .map(|i| {
                                let a = -std::f32::consts::FRAC_PI_2
                                    + std::f32::consts::TAU * share * i as f32 / n as f32;
                                center + radius * vec2(a.cos(), a.sin())
                            })
                            .collect();
                        painter.add(Shape::line(points, Stroke::new(2.5, ring)));
                    }
                    icons::component(painter, rect.shrink(4.0), kind, component_color(kind));
                });
                if response.clicked()
                    && let Some(step) = component.next_step().or(kind.steps().first().copied())
                {
                    cx.open(Selection::Step(id, step));
                }
                // The steps on hover, to jump to any of them.
                response.on_hover_ui(|ui| {
                    ui.set_min_width(220.0);
                    ui.label(bold(format!(
                        "{name} · {}/{}",
                        progress.done, progress.total
                    )));
                    for (step, status) in component.steps() {
                        if !steps::is_available(kind, step) {
                            continue;
                        }
                        let (glyph, color) = status_icon(&p, status);
                        let text = RichText::new(format!("{glyph}  {}", step_title(step)));
                        let selected = *cx.selection == Selection::Step(id, step);
                        if ui
                            .add(
                                egui::Button::selectable(selected, text).frame_when_inactive(false),
                            )
                            .clicked()
                        {
                            cx.open(Selection::Step(id, step));
                        }
                        let _ = color;
                    }
                });
            }
            separator(ui, &p);
            if tile(ui, tr("Curves"), false, |painter, rect, color| {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icon::CHART_LINE,
                    FontId::proportional(20.0),
                    color,
                );
            })
            .clicked()
            {
                command = Some(Command::Focus(View::Performance));
            }
            let add = tile(ui, tr("Add component"), false, |painter, rect, color| {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icon::PLUS,
                    FontId::proportional(20.0),
                    color,
                );
            });
            Popup::menu(&add).show(|ui| {
                ui.set_min_width(220.0);
                crate::tree::add_component_menu(ui, cx);
            });
        });
        command
    }
}

/// Done, outdated and total of the steps that count: those available, but the model
/// settings, which have nothing to accept.
#[derive(Debug, Clone, Copy)]
struct Progress {
    done: usize,
    stale: usize,
    total: usize,
}

impl Progress {
    fn of(component: &Component) -> Self {
        let kind = component.kind();
        let mut progress = Progress {
            done: 0,
            stale: 0,
            total: 0,
        };
        for (step, status) in component.steps() {
            if !steps::is_available(kind, step) || step == DesignStep::ModelSettings {
                continue;
            }
            progress.total += 1;
            match status {
                StepStatus::Done => progress.done += 1,
                StepStatus::Stale => progress.stale += 1,
                StepStatus::Pending => {}
            }
        }
        progress
    }
}

/// The caption of the phase a step belongs to.
fn phase(kind: ComponentKind, step: DesignStep) -> &'static str {
    use DesignStep::*;
    match (kind, step) {
        (ComponentKind::Volute, VoluteSetup) => "SETUP",
        (ComponentKind::Volute, WallThickness) => "WALL",
        (ComponentKind::Volute, CrossSection | SpiralDevelopment | Diffuser | CutWater) => {
            "GEOMETRY"
        }
        (_, MainDimensions) => "SIZING",
        (_, MeridionalContour) => "MERIDIAN",
        (_, BladeProperties | BladeMeanLines | BladeProfiles | BladeEdges) => "BLADING",
        _ => "MODEL",
    }
}

/// `text` on one line, cut with "…" to `width`.
fn truncated(text: &str, font: FontId, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    job
}

/// A step: status, title and, on the right, its key value or the "next" mark.
#[allow(clippy::too_many_arguments)]
fn step_row(
    ui: &mut Ui,
    title: &str,
    glyph: &str,
    glyph_color: Color32,
    value: Option<&str>,
    next: bool,
    selected: bool,
    p: &Palette,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, title));
    let row = Rect::from_min_max(pos2(rect.left() + INDENT, rect.top()), rect.max);
    let fill = if selected {
        p.selected
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(row, 4, fill);
    if selected {
        painter.rect_filled(
            Rect::from_min_size(pos2(row.left(), row.top() + 4.0), vec2(3.0, ROW - 8.0)),
            2,
            p.accent,
        );
    }
    let y = rect.center().y;
    painter.text(
        pos2(row.left() + 14.0, y),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(14.0),
        glyph_color,
    );
    let mut right = row.right() - 6.0;
    if next {
        let r = painter.text(
            pos2(right, y),
            Align2::RIGHT_CENTER,
            icon::ARROW_RIGHT,
            FontId::proportional(12.0),
            p.accent,
        );
        right = r.left() - 6.0;
    }
    let left = row.left() + 26.0;
    if !next && let Some(value) = value {
        let galley = painter.layout_no_wrap(value.to_owned(), FontId::proportional(11.5), p.weak);
        let width = galley.size().x;
        let title_width = painter
            .layout_no_wrap(title.to_owned(), FontId::proportional(13.0), p.text)
            .size()
            .x;
        // The title has the room first; the value shows when it fits beside it.
        if left + title_width + 10.0 + width <= right {
            painter.galley(
                pos2(right - width, y - galley.size().y / 2.0),
                galley,
                p.weak,
            );
            right -= width + 8.0;
        }
    }
    let galley = painter.layout_job(truncated(
        title,
        FontId::proportional(13.0),
        p.text,
        (right - left).max(20.0),
    ));
    painter.galley(pos2(left, y - galley.size().y / 2.0), galley, p.text);
    response
}

/// A number in the units of `kind`, short: three or four significant digits.
fn short(units: &DisplayUnits, kind: QuantityKind, si: f64) -> String {
    let v = units.to_display(kind, si);
    let digits = match v.abs() {
        a if a >= 100.0 => 0,
        a if a >= 10.0 => 1,
        _ => 2,
    };
    format!("{v:.digits$}")
}

/// The value that sums up a done step, if it has one.
fn key_value(
    cx: &EditCtx<'_>,
    id: ComponentId,
    kind: ComponentKind,
    step: DesignStep,
    units: &DisplayUnits,
) -> Option<String> {
    use DesignStep::*;
    let length = units.get(QuantityKind::Length).symbol;
    match (kind, step) {
        (ComponentKind::Impeller, MainDimensions) => {
            let m = cx.derived.main(id)?;
            Some(format!(
                "d₂ {} · b₂ {} {length}",
                short(units, QuantityKind::Length, m.outlet_diameter),
                short(units, QuantityKind::Length, m.outlet_width)
            ))
        }
        (ComponentKind::Impeller, MeridionalContour) => {
            let g = cx.derived.geometry(id)?;
            Some(format!(
                "L {} {length}",
                short(units, QuantityKind::Length, g.contour.axial_length)
            ))
        }
        (ComponentKind::Impeller, BladeProperties) => {
            let g = cx.derived.geometry(id)?;
            Some(format!(
                "z {} · β₂B {}",
                g.blade.count,
                units.show(QuantityKind::Angle, g.blade.outlet_blade_angle)
            ))
        }
        (ComponentKind::Volute, VoluteSetup) => {
            let v = cx.derived.volute(id)?;
            Some(format!(
                "b₃ {} {length}",
                short(units, QuantityKind::Length, v.design.inlet_width)
            ))
        }
        (ComponentKind::Volute, SpiralDevelopment) => {
            let v = cx.derived.volute(id)?;
            Some(format!(
                "A₈ {} {}",
                short(units, QuantityKind::Area, v.design.throat.area),
                units.get(QuantityKind::Area).symbol
            ))
        }
        (ComponentKind::Volute, CutWater) => {
            let v = cx.derived.volute(id)?;
            Some(format!(
                "d_z {} {length}",
                short(units, QuantityKind::Length, v.design.cutwater_diameter)
            ))
        }
        _ => None,
    }
}

/// Name of the project, its machine and how far its design is.
fn header(ui: &mut Ui, project: &Project, p: &Palette) {
    let (done, total) = project
        .components()
        .iter()
        .map(Progress::of)
        .fold((0, 0), |(d, t), x| (d + x.done, t + x.total));
    ui.add(egui::Label::new(bold(project_name(&project.info.name)).size(15.0)).truncate());
    ui.label(
        RichText::new(tf(
            "{} project · {}/{} steps",
            &[
                &tr(crate::tree::machine_name(project.machine())),
                &done,
                &total,
            ],
        ))
        .size(11.5)
        .color(p.weak),
    );
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let bar = Rect::from_min_size(rect.min + vec2(0.0, 2.0), vec2(rect.width(), 3.0));
    ui.painter().rect_filled(bar, 2, p.border);
    if total > 0 {
        let width = bar.width() * done as f32 / total as f32;
        ui.painter()
            .rect_filled(Rect::from_min_size(bar.min, vec2(width, 3.0)), 2, p.accent);
    }
}

/// The design point and fluid, with the target in a line under it.
fn setup_node(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    project: &Project,
    units: &DisplayUnits,
    p: &Palette,
) {
    let selected = *cx.selection == Selection::Setup;
    let setup = project.setup();
    let flow = match setup.design_point.flow {
        FlowRate::Volume(q) => units.show(QuantityKind::VolumeFlow, q),
        FlowRate::Mass(m) => units.show(QuantityKind::MassFlow, m),
    };
    let energy = match setup.design_point.energy {
        EnergyTransfer::Head(h) => units.show(QuantityKind::Head, h),
        EnergyTransfer::TotalPressureDifference(dp) => units.show(QuantityKind::Pressure, dp),
        other => format!("{:.3}", other.value()),
    };
    let summary = format!(
        "{flow} · {energy} · {}",
        units.show(QuantityKind::RotationalSpeed, setup.design_point.speed)
    );
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let title = tr("Design point and fluid");
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, title));
    let fill = if selected {
        p.selected
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 5, fill);
    painter.text(
        pos2(rect.left() + LINE_X + 1.0, rect.top() + 12.0),
        Align2::CENTER_CENTER,
        icon::TARGET,
        FontId::proportional(16.0),
        p.accent,
    );
    painter.text(
        pos2(rect.left() + INDENT + 2.0, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        title,
        FontId::new(13.0, bold_family()),
        p.text,
    );
    painter.text(
        pos2(rect.left() + INDENT + 2.0, rect.top() + 29.0),
        Align2::LEFT_CENTER,
        summary,
        FontId::proportional(11.5),
        p.weak,
    );
    let (glyph, color) = match &cx.derived.liquid {
        Some(Err(_)) => (icon::X_CIRCLE, ui.visuals().error_fg_color),
        _ => (icon::CHECK_CIRCLE, p.done),
    };
    painter.text(
        pos2(rect.right() - 12.0, rect.top() + 12.0),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(14.0),
        color,
    );
    if response.clicked() {
        cx.open(Selection::Setup);
    }
}

/// Where the flow enters or leaves the machine.
fn end_marker(ui: &mut Ui, text: &str, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let painter = ui.painter();
    let center = pos2(rect.left() + LINE_X, rect.center().y);
    painter.circle_filled(center, 4.0, p.weak);
    painter.text(
        pos2(rect.left() + INDENT, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::new(10.5, bold_family()),
        p.weak,
    );
}

/// The flow line between two components, where a component can go in.
fn gap(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, at: usize, p: &Palette) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tr("Insert here")));
    ui.painter().vline(
        rect.left() + LINE_X,
        rect.y_range(),
        Stroke::new(2.0, p.border),
    );
    // The kinds the composition rules allow there.
    let allowed: Vec<usize> = ADD_ENTRIES
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.at_inlet || at == 0)
        .filter(|(_, e)| project.clone().add_component(e.kind, Some(at)).is_ok())
        .map(|(i, _)| i)
        .collect();
    if allowed.is_empty() {
        return;
    }
    if response.hovered() {
        ui.painter().text(
            pos2(rect.left() + INDENT, rect.center().y),
            Align2::LEFT_CENTER,
            format!("{}  {}", icon::PLUS, tr("Insert here")),
            FontId::proportional(11.5),
            p.accent,
        );
    }
    Popup::menu(&response).show(|ui| {
        ui.set_min_width(220.0);
        for index in allowed {
            let entry = &ADD_ENTRIES[index];
            if ui
                .button(format!("{}  {}", icon::PLUS, tr(entry.label)))
                .clicked()
            {
                add_component_at(cx, index, at);
            }
        }
    });
}

/// Curves, report, CAD solids and the other exports, for the whole project.
fn results(
    ui: &mut Ui,
    project: &Project,
    has_geometry: bool,
    has_volute: bool,
) -> Option<Command> {
    let p = Palette::of(ui);
    let mut command = None;
    ui.label(
        RichText::new(tr("RESULTS"))
            .size(10.5)
            .color(p.weak)
            .strong(),
    );
    let item = |ui: &mut Ui, glyph: &str, label: &str, enabled: bool| {
        ui.add_enabled(
            enabled,
            egui::Button::new(format!("{glyph}  {label}")).frame_when_inactive(false),
        )
    };
    if item(ui, icon::CHART_LINE, tr("Curves"), true).clicked() {
        command = Some(Command::Focus(View::Performance));
    }
    if item(ui, icon::TABLE, tr("Report"), true).clicked() {
        command = Some(Command::Focus(View::Report));
    }
    let missing: Vec<String> = project
        .components()
        .iter()
        .filter(|c| !steps::ready_for_cad(c))
        .map(|c| component_name(&c.name))
        .collect();
    let ready = crate::export::CAD_AVAILABLE && missing.is_empty();
    let reason = if crate::export::CAD_AVAILABLE {
        tf("Accept the design steps first: {}", &[&missing.join(", ")])
    } else {
        tr("This build has no CAD support.").to_owned()
    };
    let label = if ready {
        tr("CAD solids").to_owned()
    } else {
        format!("{}  {}", tr("CAD solids"), icon::LOCK_SIMPLE)
    };
    if item(ui, icon::CUBE, &label, ready)
        .on_disabled_hover_text(reason)
        .clicked()
    {
        command = Some(Command::ExportCad);
    }
    let exports = item(
        ui,
        icon::EXPORT,
        &format!("{}  {}", tr("Drawings and meshes"), icon::CARET_RIGHT),
        true,
    );
    Popup::menu(&exports).show(|ui| {
        ui.set_min_width(240.0);
        if let Some(c) = export_items(ui, has_geometry, has_volute) {
            command = Some(c);
        }
    });
    command
}

fn separator(ui: &mut Ui, p: &Palette) {
    let r = ui.cursor();
    ui.painter().hline(
        (r.left() + 10.0)..=(r.right() - 10.0),
        r.top(),
        Stroke::new(1.0, p.border),
    );
    ui.add_space(4.0);
}

/// A square tile of the rail.
fn tile(
    ui: &mut Ui,
    label: &str,
    selected: bool,
    draw: impl FnOnce(&egui::Painter, Rect, Color32),
) -> Response {
    let p = Palette::of(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, label));
    let fill = if selected {
        p.selected
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect.expand(3.0), 7, fill);
    draw(ui.painter(), rect.shrink(8.0), p.text);
    response.on_hover_text(label)
}
