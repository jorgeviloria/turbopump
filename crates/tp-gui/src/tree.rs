//! Left side of the window: the component bar, the tree of design steps and the tree
//! of what the 3D view shows.

use eframe::egui::{
    self, Align2, Color32, FontId, Id, Popup, Rect, Response, RichText, Sense, Stroke, StrokeKind,
    Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use tp_project::{ComponentId, ComponentKind, DesignStep, StepStatus};

use crate::app::{EditCtx, Selection};
use crate::i18n::{component_name, project_name, tf, tr};
use crate::icons;
use crate::messages::{self, Severity};
use crate::steps;
use crate::theme::{Palette, bold, bold_family, component_color, icon};
use crate::viewer::{Part, ViewOptions};

/// Step name with a capital first letter.
pub(crate) fn step_title(step: DesignStep) -> String {
    use DesignStep::*;
    tr(match step {
        MainDimensions => "Main dimensions",
        MeridionalContour => "Meridional contour",
        BladeProperties => "Blade properties",
        BladeMeanLines => "Blade mean lines",
        BladeProfiles => "Blade profiles",
        BladeEdges => "Blade edges",
        VoluteSetup => "Volute setup",
        CrossSection => "Cross section",
        SpiralDevelopment => "Spiral development",
        Diffuser => "Diffuser",
        CutWater => "Cut-water",
        WallThickness => "Wall thickness",
        CfdSetup => "CFD setup",
        ModelSettings => "Model settings",
        ModelFinishing => "Model finishing",
    })
    .to_owned()
}

pub(crate) fn status_text(status: StepStatus) -> &'static str {
    tr(match status {
        StepStatus::Done => "done",
        StepStatus::Stale => "outdated: an input changed since",
        StepStatus::Pending => "pending",
    })
}

/// Icon and color of a step status.
pub(crate) fn status_icon(p: &Palette, status: StepStatus) -> (&'static str, Color32) {
    match status {
        StepStatus::Done => (icon::CHECK_CIRCLE, p.done),
        StepStatus::Stale => (icon::ARROWS_CLOCKWISE, p.stale),
        StepStatus::Pending => (icon::CIRCLE, p.weak),
    }
}

/// Name of a machine type, as `MachineType` writes it.
fn machine_name(machine: tp_project::MachineType) -> &'static str {
    match machine.to_string().as_str() {
        "pump" => "pump",
        "fan" => "fan",
        "compressor" => "compressor",
        "gas turbine" => "gas turbine",
        "hydro turbine" => "hydro turbine",
        _ => "machine",
    }
}

/// A component the user can add.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AddEntry {
    pub label: &'static str,
    pub kind: ComponentKind,
    /// Goes before every other component.
    pub at_inlet: bool,
}

pub(crate) const ADD_ENTRIES: [AddEntry; 5] = [
    AddEntry {
        label: "Impeller",
        kind: ComponentKind::Impeller,
        at_inlet: false,
    },
    AddEntry {
        label: "Stator (vaned diffuser)",
        kind: ComponentKind::Stator { vaned: true },
        at_inlet: false,
    },
    AddEntry {
        label: "Vaneless stator",
        kind: ComponentKind::Stator { vaned: false },
        at_inlet: false,
    },
    AddEntry {
        label: "Volute at the outlet",
        kind: ComponentKind::Volute,
        at_inlet: false,
    },
    AddEntry {
        label: "Volute at the inlet",
        kind: ComponentKind::Volute,
        at_inlet: true,
    },
];

/// Adds the component of `ADD_ENTRIES[index]` and selects its first step.
pub(crate) fn add_component(cx: &mut EditCtx<'_>, index: usize) {
    let Some(entry) = ADD_ENTRIES.get(index) else {
        return;
    };
    let components = cx.doc.project().components();
    let len = components.len();
    // An outlet volute stays last: new components go before it.
    let end = match components.last() {
        Some(c) if c.kind() == ComponentKind::Volute && len > 1 => len - 1,
        _ => len,
    };
    let at = match (entry.kind, entry.at_inlet) {
        (_, true) => 0,
        (ComponentKind::Volute, false) => len,
        _ => end,
    };
    let kind = entry.kind;
    let label = tf("add {}", &[&tr(kind.label()).to_lowercase()]);
    let mut added = None;
    let ok = cx.edit(&label, |p| {
        added = Some(p.add_component(kind, Some(at))?);
        Ok(())
    });
    if ok && let Some(id) = added {
        *cx.selection = Selection::Step(id, kind.steps()[0]);
        cx.info(tf("Added {}", &[&tr(kind.label()).to_lowercase()]));
    }
}

/// Entries that add a component to the project.
pub(crate) fn add_component_menu(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    for (index, entry) in ADD_ENTRIES.iter().enumerate() {
        let text = format!("{}  {}", icon::PLUS, tr(entry.label));
        if ui.add(egui::Button::new(text)).clicked() {
            add_component(cx, index);
        }
    }
}

pub(crate) fn remove_component(cx: &mut EditCtx<'_>, id: ComponentId) {
    let name = cx
        .doc
        .project()
        .component(id)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let name = component_name(&name);
    if cx.edit(&tf("remove {}", &[&name]), |p| {
        p.remove_component(id).map(|_| ())
    }) {
        cx.info(tf("Removed {}", &[&name]));
    }
}

/// The step to open when a component is picked: the next one to do, else the first.
pub(crate) fn entry_step(cx: &EditCtx<'_>, id: ComponentId) -> Option<DesignStep> {
    let c = cx.doc.project().component(id)?;
    Some(c.next_step().unwrap_or(c.kind().steps()[0]))
}

/// Summary status of a component: outdated if any step is, done if all are.
fn component_status(cx: &EditCtx<'_>, id: ComponentId) -> Option<StepStatus> {
    let c = cx.doc.project().component(id)?;
    let kind = c.kind();
    let available: Vec<StepStatus> = kind
        .steps()
        .iter()
        .filter(|&&s| steps::is_available(kind, s) && s != DesignStep::ModelSettings)
        .map(|&s| c.status(s))
        .collect();
    Some(if available.contains(&StepStatus::Stale) {
        StepStatus::Stale
    } else if available.iter().all(|&s| s == StepStatus::Done) {
        StepStatus::Done
    } else {
        StepStatus::Pending
    })
}

// ---------------------------------------------------------------------------------------
// Component bar

const TILE: f32 = 46.0;

/// One square tile of the component bar.
fn tile(
    ui: &mut Ui,
    label: &str,
    selected: bool,
    draw: impl FnOnce(&egui::Painter, Rect, Color32),
) -> Response {
    let p = Palette::of(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, label));
    let fill = if selected {
        p.selected
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if selected {
        Stroke::new(1.5, p.accent)
    } else {
        Stroke::NONE
    };
    ui.painter().rect(rect, 7, fill, stroke, StrokeKind::Inside);
    draw(ui.painter(), rect.shrink(8.0), p.text);
    response.on_hover_text(label)
}

/// The column of component pictograms at the far left.
pub(crate) fn component_bar(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    let p = Palette::of(ui);
    ui.add_space(6.0);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let setup = *cx.selection == Selection::Setup;
        if tile(ui, tr("Global setup"), setup, |painter, rect, color| {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                icon::GEAR,
                FontId::proportional(24.0),
                color,
            );
        })
        .clicked()
        {
            cx.open(Selection::Setup);
        }
        let r = ui.cursor();
        ui.painter().hline(
            (r.left() + 10.0)..=(r.right() - 10.0),
            r.top(),
            Stroke::new(1.0, p.border),
        );
        ui.add_space(4.0);

        let components: Vec<(ComponentId, ComponentKind, String)> = cx
            .doc
            .project()
            .components()
            .iter()
            .map(|c| (c.id(), c.kind(), c.name.clone()))
            .collect();
        let mut remove = None;
        for (id, kind, name) in components {
            let selected = matches!(*cx.selection, Selection::Step(s, _) if s == id);
            let status = component_status(cx, id);
            let response = tile(ui, &component_name(&name), selected, |painter, rect, _| {
                icons::component(painter, rect, kind, component_color(kind));
            });
            let dot = match status {
                Some(StepStatus::Done) => Some(p.done),
                Some(StepStatus::Stale) => Some(p.stale),
                _ => None,
            };
            if let Some(color) = dot {
                ui.painter().circle(
                    response.rect.right_bottom() - vec2(7.0, 7.0),
                    4.0,
                    color,
                    Stroke::new(1.5, p.strip),
                );
            }
            if response.clicked()
                && let Some(step) = entry_step(cx, id)
            {
                cx.open(Selection::Step(id, step));
            }
            response.context_menu(|ui| {
                if ui
                    .button(format!(
                        "{}  {}",
                        icon::TRASH,
                        tf("Remove {}", &[&component_name(&name)])
                    ))
                    .clicked()
                {
                    remove = Some(id);
                }
            });
        }
        if let Some(id) = remove {
            remove_component(cx, id);
        }

        let add = tile(ui, tr("Add component"), false, |painter, rect, color| {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                icon::PLUS,
                FontId::proportional(22.0),
                color,
            );
        });
        Popup::menu(&add).show(|ui| {
            ui.set_min_width(200.0);
            add_component_menu(ui, cx);
        });
    });
}

// ---------------------------------------------------------------------------------------
// Trees

const ROW: f32 = 22.0;

/// A full-width tree row with an icon and a text, `indent` points in.
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut Ui,
    indent: f32,
    glyph: &str,
    glyph_color: Color32,
    text: &str,
    selected: bool,
    enabled: bool,
) -> Response {
    let p = Palette::of(ui);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, text));
    let fill = if selected {
        p.selected
    } else if response.hovered() && enabled {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4, fill);
    let y = rect.center().y;
    ui.painter().text(
        pos2(rect.left() + indent + 9.0, y),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(15.0),
        glyph_color,
    );
    ui.painter().text(
        pos2(rect.left() + indent + 22.0, y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.0),
        if enabled { p.text } else { p.weak },
    );
    response
}

/// Caption above a tree.
fn caption(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui);
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(11.0).color(p.weak).strong());
    ui.add_space(2.0);
}

/// Header of a component in a tree, which folds its rows. Returns whether it is open.
fn component_header(
    ui: &mut Ui,
    id: Id,
    kind: ComponentKind,
    name: &str,
    badge: Option<(usize, usize)>,
) -> (bool, Response) {
    let p = Palette::of(ui);
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW + 2.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::CollapsingHeader, true, name));
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    let y = rect.center().y;
    let caret = if open {
        icon::CARET_DOWN
    } else {
        icon::CARET_RIGHT
    };
    ui.painter().text(
        pos2(rect.left() + 7.0, y),
        Align2::CENTER_CENTER,
        caret,
        FontId::proportional(12.0),
        p.weak,
    );
    icons::component(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 24.0, y), Vec2::splat(16.0)),
        kind,
        component_color(kind),
    );
    let galley =
        ui.painter()
            .layout_no_wrap(name.to_owned(), FontId::new(13.0, bold_family()), p.text);
    ui.painter().galley(
        pos2(rect.left() + 38.0, y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    if let Some((errors, warnings)) = badge {
        let mut x = rect.right() - 6.0;
        for (n, glyph, color) in [
            (warnings, icon::WARNING, ui.visuals().warn_fg_color),
            (errors, icon::X_CIRCLE, ui.visuals().error_fg_color),
        ] {
            if n > 0 {
                let text = format!("{glyph} {n}");
                let r = ui.painter().text(
                    pos2(x, y),
                    Align2::RIGHT_CENTER,
                    text,
                    FontId::proportional(12.0),
                    color,
                );
                x = r.left() - 6.0;
            }
        }
    }
    (open, response)
}

/// The tree of design steps.
pub(crate) fn design_tree(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    let p = Palette::of(ui);
    let project = cx.doc.project().clone();
    ui.add_space(6.0);
    ui.label(bold(project_name(&project.info.name)).size(15.0));
    ui.label(
        RichText::new(tf("{} project", &[&tr(machine_name(project.machine()))]))
            .size(11.5)
            .color(p.weak),
    );
    caption(ui, tr("DESIGN"));
    let setup = *cx.selection == Selection::Setup;
    if row(ui, 0.0, icon::GEAR, p.text, tr("Global setup"), setup, true)
        .on_hover_text(tr("Design point, fluid, inlet conditions and units"))
        .clicked()
    {
        cx.open(Selection::Setup);
    }

    let mut remove = None;
    for component in project.components() {
        let id = component.id();
        let kind = component.kind();
        let all = messages::of_component(cx.derived, id);
        let errors = all.iter().filter(|m| m.severity == Severity::Error).count();
        let (open, header) = component_header(
            ui,
            Id::new(("design_open", id.0)),
            kind,
            &component_name(&component.name),
            Some((errors, all.len() - errors)),
        );
        header.context_menu(|ui| {
            if ui
                .button(format!("{}  {}", icon::TRASH, tr("Remove component")))
                .clicked()
            {
                remove = Some(id);
            }
        });
        if !open {
            continue;
        }
        for (step, status) in component.steps() {
            let available = steps::is_available(kind, step);
            let selected = *cx.selection == Selection::Step(id, step);
            let (glyph, color) = status_icon(&p, status);
            let title = step_title(step);
            let response = row(ui, 14.0, glyph, color, &title, selected, available);
            let response = if available {
                response.on_hover_text(status_text(status))
            } else {
                response.on_hover_text(steps::planned_phase(kind, step))
            };
            if response.clicked() {
                cx.open(Selection::Step(id, step));
            }
        }
    }
    if let Some(id) = remove {
        remove_component(cx, id);
    }
    ui.add_space(4.0);
    let add =
        ui.add(egui::Button::new(format!("{}  {}", icon::PLUS, tr("Add component"))).frame(false));
    Popup::menu(&add).show(|ui| {
        ui.set_min_width(200.0);
        add_component_menu(ui, cx);
    });
}

/// Which parts the active stage has, for the model tree.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StageParts {
    /// `Some(vaned)` when a stator follows the impeller.
    pub stator: Option<bool>,
    pub volute: bool,
}

/// One part: an eye that shows or hides it and, if allowed, a see-through toggle.
fn part_row(ui: &mut Ui, label: &str, part: &mut Part, glass: bool) {
    let p = Palette::of(ui);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let (glyph, color) = if part.shown {
            (icon::EYE, p.text)
        } else {
            (icon::EYE_SLASH, p.weak)
        };
        if ui
            .add(egui::Button::new(RichText::new(glyph).color(color)).frame(false))
            .on_hover_text(if part.shown { tr("Hide") } else { tr("Show") })
            .clicked()
        {
            part.shown = !part.shown;
        }
        let text = RichText::new(label).color(if part.shown { p.text } else { p.weak });
        ui.label(text);
        if glass {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button = egui::Button::selectable(part.glass, icon::CUBE_TRANSPARENT);
                if ui
                    .add_enabled(part.shown, button)
                    .on_hover_text(tr("See-through"))
                    .clicked()
                {
                    part.glass = !part.glass;
                }
            });
        }
    });
}

fn switch_row(ui: &mut Ui, label: &str, value: &mut bool) {
    let mut part = Part {
        shown: *value,
        glass: false,
    };
    part_row(ui, label, &mut part, false);
    *value = part.shown;
}

/// The tree of what the 3D view shows.
pub(crate) fn model_tree(ui: &mut Ui, options: &mut ViewOptions, stage: StageParts) {
    caption(ui, tr("3D MODEL"));
    let header = |ui: &mut Ui, kind: ComponentKind, name: &str| {
        component_header(ui, Id::new(("model_open", name)), kind, name, None).0
    };
    if header(ui, ComponentKind::Impeller, tr("Impeller")) {
        part_row(ui, tr("Blades"), &mut options.blades, true);
        part_row(ui, tr("Hub"), &mut options.hub, true);
        part_row(ui, tr("Shroud"), &mut options.shroud, true);
    }
    if let Some(vaned) = stage.stator
        && header(ui, ComponentKind::Stator { vaned }, tr("Diffuser"))
    {
        if vaned {
            part_row(ui, tr("Vanes"), &mut options.vanes, true);
        }
        part_row(ui, tr("Side walls"), &mut options.walls, true);
    }
    if stage.volute && header(ui, ComponentKind::Volute, tr("Volute")) {
        part_row(ui, tr("Casing"), &mut options.volute, true);
        switch_row(ui, tr("Section curves"), &mut options.sections);
    }
    caption(ui, tr("DISPLAY"));
    switch_row(ui, tr("Feature edges"), &mut options.edges);
    switch_row(ui, tr("All blades"), &mut options.all_blades);
    switch_row(ui, tr("Wireframe"), &mut options.wireframe);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_start_with_a_capital() {
        assert_eq!(step_title(DesignStep::MainDimensions), "Main dimensions");
        assert_eq!(step_title(DesignStep::CfdSetup), "CFD setup");
    }
}
