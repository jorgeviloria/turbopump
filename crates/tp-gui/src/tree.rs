//! What the design flow and the panels share: the step names and statuses, and adding
//! and removing components.

use eframe::egui::{self, Color32, Ui};
use tp_project::{ComponentId, ComponentKind, DesignStep, StepStatus};

use crate::app::{EditCtx, Selection};
use crate::i18n::{component_name, tf, tr};
use crate::theme::{Palette, icon};

/// Step name with a capital first letter.
pub(crate) fn step_title(step: DesignStep) -> String {
    use DesignStep::*;
    tr(match step {
        DuctDimensions => "Duct dimensions",
        MainDimensions => "Main dimensions",
        MeridionalContour => "Meridional contour",
        BladeProperties => "Blade properties",
        BladeMeanLines => "Blade mean lines",
        BladeProfiles => "Blade profiles",
        BladeEdges => "Blade edges",
        VoluteSetup => "Volute setup",
        CrossSection => "Cross section",
        SpiralDevelopment => "Spiral development",
        Diffuser => "Discharge diffuser",
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
pub(crate) fn machine_name(machine: tp_project::MachineType) -> &'static str {
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

pub(crate) const ADD_ENTRIES: [AddEntry; 6] = [
    AddEntry {
        label: "Inlet duct",
        kind: ComponentKind::InletDuct,
        at_inlet: true,
    },
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

/// Where the component of `entry` usually goes in `project`.
fn usual_place(project: &tp_project::Project, entry: &AddEntry) -> usize {
    let components = project.components();
    let len = components.len();
    // An outlet volute stays last: new components go before it.
    let end = match components.last() {
        Some(c) if c.kind() == ComponentKind::Volute && len > 1 => len - 1,
        _ => len,
    };
    match (entry.kind, entry.at_inlet) {
        (_, true) => 0,
        (ComponentKind::Volute, false) => len,
        _ => end,
    }
}

/// Adds the component of `ADD_ENTRIES[index]` where it usually goes and selects its
/// first step.
pub(crate) fn add_component(cx: &mut EditCtx<'_>, index: usize) {
    let Some(entry) = ADD_ENTRIES.get(index) else {
        return;
    };
    let at = usual_place(cx.doc.project(), entry);
    add_component_at(cx, index, at);
}

/// Adds the component of `ADD_ENTRIES[index]` at position `at` and selects its first
/// step.
pub(crate) fn add_component_at(cx: &mut EditCtx<'_>, index: usize, at: usize) {
    let Some(entry) = ADD_ENTRIES.get(index) else {
        return;
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

/// Entries that add a component to the project, those the composition rules allow
/// where each usually goes.
pub(crate) fn add_component_menu(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    let project = cx.doc.project().clone();
    let allowed = ADD_ENTRIES.iter().enumerate().filter(|(_, e)| {
        project
            .clone()
            .add_component(e.kind, Some(usual_place(&project, e)))
            .is_ok()
    });
    for (index, entry) in allowed {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_start_with_a_capital() {
        assert_eq!(step_title(DesignStep::MainDimensions), "Main dimensions");
        assert_eq!(step_title(DesignStep::CfdSetup), "CFD setup");
    }
}
