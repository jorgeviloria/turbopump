//! Project tree: global setup, components and their design steps.

use eframe::egui::{self, RichText, Ui};
use tp_project::{ComponentId, ComponentKind, DesignStep, StepStatus};

use crate::app::{EditCtx, Selection, status_color};
use crate::steps;

/// Step name with a capital first letter.
pub(crate) fn step_title(step: DesignStep) -> String {
    let text = step.to_string();
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

/// Marker drawn before a step name.
pub(crate) fn status_marker(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Done => "⏺",
        StepStatus::Stale => "⟲",
        StepStatus::Pending => "○",
    }
}

pub(crate) fn status_text(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Done => "done",
        StepStatus::Stale => "outdated: an input changed since",
        StepStatus::Pending => "pending",
    }
}

pub(crate) fn show(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    let project = cx.doc.project().clone();
    ui.add_space(4.0);
    ui.heading(&project.info.name);
    ui.weak(format!("{} project", project.machine()));
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        let selected = *cx.selection == Selection::Setup;
        if ui
            .selectable_label(selected, "⚙ Global setup")
            .on_hover_text("Design point, fluid, inlet conditions and units")
            .clicked()
        {
            *cx.selection = Selection::Setup;
        }
        ui.add_space(4.0);

        let mut remove = None;
        for component in project.components() {
            let id = component.id();
            let warnings = cx
                .derived
                .impeller(id)
                .map(|r| {
                    let main = r.main.as_ref().map_or(0, |d| d.warnings.len());
                    let geometry = r.geometry.as_ref().map_or(0, |g| g.warnings.len());
                    main + geometry
                })
                .unwrap_or(0);
            let mut title = RichText::new(&component.name).strong();
            if warnings > 0 {
                title = RichText::new(format!("{}  ⚑", component.name)).strong();
            }
            let header = egui::CollapsingHeader::new(title)
                .id_salt(("component", id.0))
                .default_open(true)
                .show(ui, |ui| {
                    for (step, status) in component.steps() {
                        step_row(ui, cx, id, component.kind(), step, status);
                    }
                });
            header.header_response.context_menu(|ui| {
                if ui.button("Remove component").clicked() {
                    remove = Some(id);
                }
            });
        }
        if let Some(id) = remove {
            remove_component(cx, id);
        }

        ui.add_space(8.0);
        ui.menu_button("✚ Add component", |ui| add_component_menu(ui, cx));
    });
}

fn step_row(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    id: ComponentId,
    kind: ComponentKind,
    step: DesignStep,
    status: StepStatus,
) {
    let available = steps::is_available(kind, step);
    let selected = *cx.selection == Selection::Step(id, step);
    let color = status_color(ui, status);
    let mut text = RichText::new(format!("{}  {}", status_marker(status), step_title(step)));
    if available {
        text = text.color(color);
    }
    let response = ui
        .add_enabled(available, egui::Button::selectable(selected, text))
        .on_hover_text(status_text(status))
        .on_disabled_hover_text(steps::planned_phase(kind, step));
    if response.clicked() {
        *cx.selection = Selection::Step(id, step);
    }
}

fn remove_component(cx: &mut EditCtx<'_>, id: ComponentId) {
    let name = cx
        .doc
        .project()
        .component(id)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    if cx.edit(&format!("remove {name}"), |p| {
        p.remove_component(id).map(|_| ())
    }) {
        cx.info(format!("Removed {name}"));
    }
}

/// Entries that add a component to the project.
pub(crate) fn add_component_menu(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    let components = cx.doc.project().components();
    let len = components.len();
    // An outlet volute stays last: new components go before it.
    let end = match components.last() {
        Some(c) if c.kind() == ComponentKind::Volute && len > 1 => len - 1,
        _ => len,
    };
    let entries = [
        ("Impeller", ComponentKind::Impeller, None),
        (
            "Stator (vaned diffuser)",
            ComponentKind::Stator { vaned: true },
            None,
        ),
        (
            "Vaneless stator",
            ComponentKind::Stator { vaned: false },
            None,
        ),
        ("Volute at the outlet", ComponentKind::Volute, None),
        ("Volute at the inlet", ComponentKind::Volute, Some(0)),
    ];
    for (label, kind, position) in entries {
        if ui.button(label).clicked() {
            let label = format!("add {}", kind.label().to_lowercase());
            let mut added = None;
            let at = match (kind, position) {
                (_, Some(p)) => p,
                (ComponentKind::Volute, None) => len,
                (_, None) => end,
            };
            let ok = cx.edit(&label, |p| {
                added = Some(p.add_component(kind, Some(at))?);
                Ok(())
            });
            if ok && let Some(id) = added {
                let step = kind.steps()[0];
                *cx.selection = Selection::Step(id, step);
                cx.info(format!("Added {}", kind.label().to_lowercase()));
            }
        }
    }
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
