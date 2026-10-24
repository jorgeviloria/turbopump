//! Step panel: the inputs of the selected design step, its results and its problems.

use eframe::egui::{self, ComboBox, DragValue, Grid, RichText, Ui};
use tp_design::MainDimensionsInput;
use tp_design::impeller::blade::{
    BetaDistribution, BladeShape, DEFAULT_POINTS, DEFAULT_SPANS, EdgeShape, InletVelocity,
    MeridionalFlow, WrapMode,
};
use tp_design::impeller::meridian::{
    DEFAULT_LEADING_EDGE_HUB, DEFAULT_LEADING_EDGE_SHROUD, DEFAULT_TANGENT,
};
use tp_design::impeller::{ImpellerGeometryInput, meridian::MeridionalInput};
use tp_design::pump::correlations as corr;
use tp_design::pump::{CentrifugalPumpInput, ImpellerType, InletDesign, Suction};
use tp_project::{
    ComponentId, ComponentKind, DesignStep, EnergyTransfer, FlowRate, Param, Project, Rotation,
    StepStatus, VANED_STEPS, VANELESS_STEPS, VOLUTE_STEPS,
};
use tp_units::QuantityKind;

use crate::app::{EditCtx, Selection};
use crate::derived::Derived;
use crate::i18n::{component_name, tf, tr};
use crate::icons;
use crate::messages::{self, Severity};
use crate::theme::{Palette, bold, icon};
use crate::tree::{status_icon, status_text, step_title};
use crate::widgets::{self, label, param_f64, param_u32, props, value_info, value_row};
use tp_design::acceptance::{ClassCriteria, QualityClass};

/// Steps the editor can work on today.
pub(crate) fn is_available(kind: ComponentKind, step: DesignStep) -> bool {
    use DesignStep::*;
    match kind {
        ComponentKind::Impeller => matches!(
            step,
            MainDimensions
                | MeridionalContour
                | BladeProperties
                | BladeMeanLines
                | BladeProfiles
                | BladeEdges
                | ModelSettings
        ),
        ComponentKind::Volute => VOLUTE_STEPS.contains(&step) || step == ModelSettings,
        ComponentKind::Stator { vaned: true } => {
            VANED_STEPS.contains(&step) || step == ModelSettings
        }
        ComponentKind::Stator { vaned: false } => {
            VANELESS_STEPS.contains(&step) || step == ModelSettings
        }
    }
}

/// Why a step is not available yet.
pub(crate) fn planned_phase(kind: ComponentKind, step: DesignStep) -> String {
    match (kind, step) {
        (ComponentKind::Impeller, DesignStep::CfdSetup) => {
            tr("CFD setup is planned for phase 8 (meshing and CFD).").into()
        }
        (ComponentKind::Impeller, DesignStep::ModelFinishing) => {
            tr("Model finishing (fillets, trimming, balance holes) is planned for phase 9.").into()
        }
        (_, DesignStep::CfdSetup) => {
            tr("CFD setup is planned for phase 8 (meshing and CFD).").into()
        }
        (_, DesignStep::ModelFinishing) => tr("Model finishing is planned for phase 9.").into(),
        _ => String::new(),
    }
}

/// What a step does and where its method comes from, for the ⓘ beside its status.
fn intro(kind: ComponentKind, step: DesignStep) -> Option<&'static str> {
    use ComponentKind::{Impeller, Stator, Volute};
    use DesignStep::*;
    let text = match (kind, step) {
        (_, ModelSettings) => match kind {
            Volute => {
                "CAD solid of the volute flow domain, built with Open CASCADE. CFD setup and \
                 model finishing arrive in phases 8 and 9."
            }
            Stator { .. } => {
                "CAD solids of the diffuser flow domain and vanes, built with Open CASCADE. \
                 CFD setup and model finishing arrive in phases 8 and 9."
            }
            Impeller => {
                "CAD solids of the impeller and of its flow domain, built with Open CASCADE. \
                 CFD setup and model finishing (fillets, trimming, balance holes) arrive in \
                 phases 8 and 9."
            }
        },
        (Impeller, MainDimensions) => {
            "One-dimensional design with Gülich's correlations. Values in automatic mode follow the design point."
        }
        (Impeller, MeridionalContour) => {
            "Hub and shroud are cubic Bézier curves. Drag their control points in the meridional view, or type the values here."
        }
        (Impeller, BladeProperties) => {
            "Blade sections are built on stream surfaces that split the flow into equal parts."
        }
        (Impeller, BladeMeanLines) => {
            "Conformal mapping dθ = dm/(r·tan β), stacked at the trailing edge."
        }
        (Impeller, BladeProfiles) => {
            "Normal thickness, varying linearly from leading to trailing edge before the edges are shaped."
        }
        (Impeller, BladeEdges) => {
            "A blunt trailing edge leaves material to under-file after casting (Gülich pp. 347–349)."
        }
        (Volute, VoluteSetup) => {
            "The volute collects the flow of the impeller before it: Q, d₂, b₂ and c₂u come from \
             there. Gülich 2nd ed., Chap. 7.8."
        }
        (Volute, CrossSection) => {
            "Each definition gives the shape of the sections at one wrap angle φ; the spiral \
             development scales it to the height its flow needs, and the shape blends smoothly \
             between definitions. Flat sections with B/H = 2 to 3 lose least (Gülich p. 414)."
        }
        (Volute, SpiralDevelopment) => {
            "Each section carries the flow the impeller delivers up to its angle. The velocity rules size it for a velocity in the spiral, the geometric rules prescribe its size directly."
        }
        (Volute, Diffuser) => {
            "Conical diffuser from the throat to the discharge nozzle, straight or with a bend, checked against the allowable opening angle (Gülich eqs. 1.44, 1.45)."
        }
        (Volute, CutWater) => {
            "The cutwater meets the flow leaving the impeller; its camber angle follows the flow angle plus a small incidence (Gülich eqs. T7.7.3 to T7.7.6)."
        }
        (Volute, WallThickness) => "Casing wall around the flow domain.",
        (Stator { vaned: true }, MainDimensions) => {
            "Vaned diffuser after the impeller (Gülich 2nd ed., Chap. 7.9 and Table 7.7)."
        }
        (Stator { vaned: false }, MainDimensions) => {
            "Vaneless ring after the impeller: the flow keeps c_u·r constant and slows down."
        }
        (Stator { .. }, MeridionalContour) => {
            "The side walls run straight from b₃ at d₃ to b₄ at d₄."
        }
        (Stator { .. }, BladeProperties) => {
            "The throat width sets the best efficiency point (Gülich eq. T7.7.10)."
        }
        (Stator { .. }, BladeMeanLines) => {
            "Conformal mapping dθ = dr/(r·tan α). The vane angle grows from α₃B to α₄B; its law \
             is shaped to give the throat of the previous step."
        }
        (Stator { .. }, BladeProfiles) => "The vane thickness varies linearly from e₃ to e₄.",
        (Stator { .. }, BladeEdges) => {
            "An elliptic leading edge makes the vane less sensitive to incidence (Gülich Chap. 7.9.2)."
        }
        _ => return None,
    };
    Some(tr(text))
}

/// Steps that can be accepted (marked done) from the editor.
fn can_accept(kind: ComponentKind, step: DesignStep) -> bool {
    tp_project::can_design(kind, step)
}

/// Title of the dialog for `selection`: icon, component and step.
/// Icon, path (component › phase) and title of the selected step, for the heading of
/// the panel.
pub(crate) fn heading(project: &Project, selection: Selection) -> (&'static str, String, String) {
    match selection {
        Selection::Setup => (
            icon::TARGET,
            tr("Project").to_owned(),
            tr("Design point and fluid").to_owned(),
        ),
        Selection::Step(id, step) => {
            let component = project.component(id);
            let name = component.map_or("", |c| c.name.as_str());
            let phase = component.map_or("", |c| crate::flow::phase(c.kind(), step));
            (
                icons::step(step),
                format!(
                    "{} › {}",
                    component_name(name),
                    crate::flow::phase_name(phase)
                ),
                step_title(step),
            )
        }
    }
}

/// The inputs and results of the selected step, or of the global setup.
pub(crate) fn show(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    // Rows above the first section belong to none; the design point has no status.
    ui.data_mut(|d| {
        d.remove::<egui::Id>(current_section_id());
        d.remove::<StepStatus>(status_id());
    });
    match *cx.selection {
        Selection::Setup => setup(ui, cx),
        Selection::Step(id, step) => step_panel(ui, cx, id, step),
    }
}

/// Every place of the design in order: the design point, then the steps of each
/// component that can be worked on.
pub(crate) fn order(project: &Project) -> Vec<Selection> {
    let mut order = vec![Selection::Setup];
    for c in project.components() {
        for &step in c.kind().steps() {
            if is_available(c.kind(), step) {
                order.push(Selection::Step(c.id(), step));
            }
        }
    }
    order
}

/// The place `by` steps after `selection` (before, if negative), across components.
pub(crate) fn step_from(project: &Project, selection: Selection, by: i32) -> Option<Selection> {
    let order = order(project);
    let at = order.iter().position(|&s| s == selection)? as i32 + by;
    usize::try_from(at).ok().and_then(|i| order.get(i).copied())
}

/// The shortcut that accepts the step and goes on; with Shift, it completes every step
/// of the component.
pub(crate) const ACCEPT_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌘↩"
} else {
    "Ctrl+Enter"
};

/// What the main button under the step does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Primary {
    /// Accept the step and go on to the next one.
    Accept,
    /// Only go on: nothing to accept, or done already.
    Next,
    /// Accepting waits for this.
    Blocked(&'static str),
}

/// The main action of the selected step.
pub(crate) fn primary(cx: &EditCtx<'_>) -> Primary {
    let Selection::Step(id, step) = *cx.selection else {
        return Primary::Next;
    };
    match accept_state(cx.doc.project(), cx.derived, id, step) {
        AcceptState::Ready => Primary::Accept,
        AcceptState::NotApplicable | AcceptState::Blocked("This step is done.") => Primary::Next,
        AcceptState::Blocked(reason) => Primary::Blocked(reason),
    }
}

/// Accepts the selected step when it can be accepted, and opens the next place of the
/// design, in the next component when this one has no more steps.
pub(crate) fn accept_and_continue(cx: &mut EditCtx<'_>) {
    let before = *cx.selection;
    let next = step_from(cx.doc.project(), before, 1);
    match primary(cx) {
        Primary::Accept => {
            let Selection::Step(id, step) = before else {
                return;
            };
            accept(cx, id, step);
            // Accepting the last step of a component stays on it: go on to the next one.
            if *cx.selection == before
                && cx.doc.project().component(id).map(|c| c.status(step)) == Some(StepStatus::Done)
                && let Some(next) = next
            {
                *cx.selection = next;
            }
        }
        Primary::Next => {
            if let Some(next) = next {
                *cx.selection = next;
            }
        }
        Primary::Blocked(_) => {}
    }
}

/// Under the step: the key value it gives, back, and the main button, which accepts the
/// step and goes on (Ctrl+Enter).
pub(crate) fn footer(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    let p = Palette::of(ui);
    let project = cx.doc.project().clone();
    match *cx.selection {
        Selection::Setup => {
            ui.label(
                RichText::new(tr(
                    "Shared by every component. Changing the design point or the fluid makes every step outdated; the acceptance does not.",
                ))
                .color(p.weak),
            );
        }
        Selection::Step(id, step) => {
            let units = cx.units();
            let value = project
                .component(id)
                .and_then(|c| crate::flow::key_value(cx, id, c.kind(), step, &units));
            if let Some(value) = value {
                ui.label(RichText::new(value).size(12.0).color(p.weak));
            }
        }
    }
    let back = step_from(&project, *cx.selection, -1);
    let next = step_from(&project, *cx.selection, 1);
    let primary = primary(cx);
    let shortcut = ACCEPT_SHORTCUT;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (text, enabled, tip) = match primary {
                Primary::Accept => (
                    format!(
                        "{}  {}  {}",
                        icon::CHECK,
                        tr("Accept step and continue"),
                        icon::CARET_RIGHT
                    ),
                    true,
                    tr("Mark this step as done with the current values and open the next one"),
                ),
                Primary::Next => (
                    format!("{}  {}", tr("Next"), icon::CARET_RIGHT),
                    next.is_some(),
                    "",
                ),
                Primary::Blocked(reason) => (
                    format!(
                        "{}  {}  {}",
                        icon::CHECK,
                        tr("Accept step and continue"),
                        icon::CARET_RIGHT
                    ),
                    false,
                    tr(reason),
                ),
            };
            let mut button = egui::Button::new(if primary == Primary::Accept {
                RichText::new(text).color(p.on_accent)
            } else {
                RichText::new(text)
            })
            .shortcut_text(if enabled { shortcut } else { "" });
            if primary == Primary::Accept {
                button = button.fill(p.accent);
            }
            let response = ui.add_enabled(enabled, button);
            let response = if tip.is_empty() {
                response
            } else {
                response.on_hover_text(tip).on_disabled_hover_text(tip)
            };
            if response.clicked() {
                accept_and_continue(cx);
            }
            if ui
                .add_enabled(
                    back.is_some(),
                    egui::Button::new(format!("{} {}", icon::CARET_LEFT, tr("Back"))),
                )
                .clicked()
                && let Some(back) = back
            {
                *cx.selection = back;
            }
        });
    });
}

/// What a section of the panel holds; its heading says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Inputs,
    /// Computed values, read only.
    Results,
    /// Values compared with their recommendations.
    Checks,
}

/// Heading of a group of inputs, with a rule after it, that folds the group. Returns
/// whether the group is open.
pub(crate) fn section(ui: &mut Ui, title: &str) -> bool {
    section_kind(ui, title, "", Kind::Inputs)
}

/// [`section`] with an ⓘ that shows `hint`.
pub(crate) fn section_info(ui: &mut Ui, title: &str, hint: &str) -> bool {
    section_kind(ui, title, hint, Kind::Inputs)
}

/// Where the sections of the shown step keep whether they are open.
fn sections_id(ui: &Ui) -> egui::Id {
    ui.data(|d| d.get_temp::<egui::Id>(egui::Id::new("properties_step")))
        .unwrap_or(egui::Id::NULL)
}

/// Folds or opens every section of the shown step.
pub(crate) fn fold_sections(ctx: &egui::Context, fold: bool) {
    let step = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("properties_step")))
        .unwrap_or(egui::Id::NULL);
    ctx.data_mut(|d| {
        let generation = d.get_temp::<u32>(step.with("generation")).unwrap_or(0);
        d.insert_temp(step.with("generation"), generation + 1);
        d.insert_temp(step.with("folded"), fold);
    });
}

/// Whether every section of the shown step was folded from the menu.
pub(crate) fn sections_folded(ctx: &egui::Context) -> bool {
    let step = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("properties_step")))
        .unwrap_or(egui::Id::NULL);
    ctx.data(|d| d.get_temp::<bool>(step.with("folded")))
        .unwrap_or(false)
}

fn status_id() -> egui::Id {
    egui::Id::new("properties_status")
}

/// Keeps what a row of the shown step showed when the step was last accepted. While
/// the step is done, `value` is what was accepted; once outdated, returns what was
/// accepted when the row shows something else now.
pub(crate) fn changed_since_accepted(ui: &Ui, label: &str, value: &str) -> Option<String> {
    let step = sections_id(ui);
    let status = ui.data(|d| d.get_temp::<StepStatus>(status_id()))?;
    let row = current_section(ui).unwrap_or(step).with(label);
    ui.data_mut(|d| {
        let accepted = d.get_temp_mut_or_default::<std::collections::HashMap<egui::Id, String>>(
            step.with("accepted"),
        );
        match status {
            StepStatus::Done => {
                accepted.insert(row, value.to_owned());
                None
            }
            StepStatus::Stale => accepted.get(&row).filter(|was| *was != value).cloned(),
            StepStatus::Pending => None,
        }
    })
}

fn current_section_id() -> egui::Id {
    egui::Id::new("current_section")
}

/// The open section whose rows are being drawn.
pub(crate) fn current_section(ui: &Ui) -> Option<egui::Id> {
    ui.data(|d| d.get_temp::<egui::Id>(current_section_id()))
}

/// Counts one more value set by hand in `section`.
pub(crate) fn count_manual(ui: &Ui, section: egui::Id) {
    ui.data_mut(|d| {
        let n = d.get_temp::<usize>(section.with("manual")).unwrap_or(0);
        d.insert_temp(section.with("manual"), n + 1);
    });
}

/// Whether the heading of `section` asked, this frame, for its values back to
/// automatic.
pub(crate) fn section_to_automatic(ui: &Ui, section: egui::Id) -> bool {
    ui.data(|d| d.get_temp::<bool>(section.with("to_automatic")))
        .unwrap_or(false)
}

/// A section heading of any [`Kind`]. Returns whether its group is open. An open
/// heading shows how many of its values are set by hand (counted last frame) and puts
/// them all back to automatic with a click.
pub(crate) fn section_kind(ui: &mut Ui, title: &str, hint: &str, kind: Kind) -> bool {
    let p = Palette::of(ui);
    let step = sections_id(ui);
    let folded = ui
        .data(|d| d.get_temp::<bool>(step.with("folded")))
        .unwrap_or(false);
    let generation = ui
        .data(|d| d.get_temp::<u32>(step.with("generation")))
        .unwrap_or(0);
    let key = step.with(("section", title));
    let mut open = ui
        .data(|d| d.get_temp::<(u32, bool)>(key))
        .filter(|(g, _)| *g == generation)
        .map_or(!folded, |(_, open)| open);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let caret = if open {
            icon::CARET_DOWN
        } else {
            icon::CARET_RIGHT
        };
        let mut clicked = ui
            .add(
                egui::Label::new(RichText::new(caret).size(11.0).color(p.weak))
                    .sense(egui::Sense::click()),
            )
            .clicked();
        let heading = ui.add(egui::Button::new(bold(title).size(13.0)).frame(false));
        heading.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, title)
        });
        clicked |= heading.clicked();
        if !hint.is_empty() {
            widgets::info(ui, hint);
        }
        let tag = match kind {
            Kind::Inputs => None,
            Kind::Results => Some(tr("results")),
            Kind::Checks => Some(tr("checks")),
        };
        if let Some(tag) = tag {
            egui::Frame::new()
                .fill(p.strip)
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(5, 0))
                .show(ui, |ui| {
                    ui.label(RichText::new(tag).size(10.5).color(p.weak));
                });
        }
        let manual = ui
            .data(|d| d.get_temp::<usize>(key.with("manual")))
            .unwrap_or(0);
        let mut to_automatic = false;
        let rest = ui.available_rect_before_wrap();
        let rule_end = if manual > 0 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = tf("All {} to automatic", &[&manual]);
                let chip = ui.add(
                    egui::Button::new(
                        RichText::new(format!("● {}", tf("{} by hand", &[&manual])))
                            .size(11.0)
                            .color(p.accent),
                    )
                    .frame(false),
                );
                chip.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label)
                });
                to_automatic = chip
                    .on_hover_text(tr(
                        "Values of this section set by hand; click to make them all automatic again",
                    ))
                    .clicked();
                ui.min_rect().left() - 6.0
            })
            .inner
        } else {
            rest.right()
        };
        ui.painter().hline(
            (rest.left() + 4.0)..=rule_end.max(rest.left() + 4.0),
            rest.center().y,
            egui::Stroke::new(1.0, p.border),
        );
        // A click on the count of a folded section opens it, to apply there.
        if clicked || (to_automatic && !open) {
            open = !open;
            ui.data_mut(|d| d.insert_temp(key, (generation, open)));
        }
        if open {
            // The rows that follow count afresh, and hear the click.
            ui.data_mut(|d| {
                d.insert_temp(key.with("manual"), 0usize);
                d.insert_temp(key.with("to_automatic"), to_automatic);
                d.insert_temp(current_section_id(), key);
            });
        } else {
            ui.data_mut(|d| d.remove::<egui::Id>(current_section_id()));
        }
    });
    ui.add_space(2.0);
    open
}

/// Whether a step can be accepted now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcceptState {
    /// The step has nothing to accept.
    NotApplicable,
    Ready,
    /// Why it cannot be accepted.
    Blocked(&'static str),
}

/// Whether `step` of component `id` can be accepted now.
pub(crate) fn accept_state(
    project: &Project,
    derived: &Derived,
    id: ComponentId,
    step: DesignStep,
) -> AcceptState {
    let Some(component) = project.component(id) else {
        return AcceptState::NotApplicable;
    };
    let kind = component.kind();
    if !can_accept(kind, step) {
        return AcceptState::NotApplicable;
    }
    let previous_done = kind
        .steps()
        .iter()
        .take_while(|&&s| s != step)
        .all(|&s| component.status(s) == StepStatus::Done);
    let computed = match (kind, step) {
        (ComponentKind::Volute, _) => derived.volute(id).is_some(),
        (ComponentKind::Stator { .. }, _) => derived.diffuser(id).is_some(),
        (_, DesignStep::MainDimensions) => derived.main(id).is_some(),
        _ => derived.geometry(id).is_some(),
    };
    if component.status(step) == StepStatus::Done {
        AcceptState::Blocked("This step is done.")
    } else if !previous_done {
        AcceptState::Blocked("Complete the earlier steps first.")
    } else if !computed {
        AcceptState::Blocked("Fix the errors first.")
    } else {
        AcceptState::Ready
    }
}

/// The available steps before and after `step`.
pub(crate) fn neighbours(
    kind: ComponentKind,
    step: DesignStep,
) -> (Option<DesignStep>, Option<DesignStep>) {
    let steps: Vec<DesignStep> = kind
        .steps()
        .iter()
        .copied()
        .filter(|&s| is_available(kind, s))
        .collect();
    let Some(index) = steps.iter().position(|&s| s == step) else {
        return (None, steps.first().copied());
    };
    (
        index.checked_sub(1).map(|i| steps[i]),
        steps.get(index + 1).copied(),
    )
}

/// How many steps of component `id` are left to accept.
pub(crate) fn remaining(project: &Project, id: ComponentId) -> usize {
    project.component(id).map_or(0, |c| {
        c.kind()
            .steps()
            .iter()
            .filter(|&&s| can_accept(c.kind(), s) && c.status(s) != StepStatus::Done)
            .count()
    })
}

/// Accepts every step of component `id` not done yet with the values in use, the
/// suggested ones where nothing was set by hand, as one edit that undoes at once.
/// Opens the last step done, or the one that could not be done.
pub(crate) fn complete_all(cx: &mut EditCtx<'_>, id: ComponentId) {
    cx.doc.end_gesture();
    let Some(name) = cx
        .doc
        .project()
        .component(id)
        .map(|c| component_name(&c.name))
    else {
        return;
    };
    let label = tf("complete every step of {}", &[&name]);
    match cx.doc.edit(&label, |p| p.design_remaining(id)) {
        Ok(tp_project::Designed { done, failed: None }) => match done.last() {
            Some(&last) => {
                *cx.selection = Selection::Step(id, last);
                cx.info(tf(
                    "{}: {} steps done with the suggested values",
                    &[&name, &done.len()],
                ));
            }
            None => cx.info(tf("Every step of {} is done already", &[&name])),
        },
        Ok(tp_project::Designed {
            done,
            failed: Some((step, e)),
        }) => {
            *cx.selection = Selection::Step(id, step);
            cx.error(tf(
                "{} steps done; {} could not be done: {}",
                &[
                    &done.len(),
                    &step_title(step),
                    &crate::i18n::message(&e.to_string()),
                ],
            ));
        }
        Err(e) => cx.error(e.to_string()),
    }
}

/// Marks `step` as done and moves on to the next step.
pub(crate) fn accept(cx: &mut EditCtx<'_>, id: ComponentId, step: DesignStep) {
    cx.doc.end_gesture();
    let Some(kind) = cx.doc.project().component(id).map(|c| c.kind()) else {
        return;
    };
    match cx
        .doc
        .edit(&format!("accept {step}"), |p| p.design_step(id, step))
    {
        Ok(()) => {
            cx.info(tf("{} accepted", &[&step_title(step)]));
            if let (_, Some(next)) = neighbours(kind, step) {
                *cx.selection = Selection::Step(id, next);
            }
        }
        Err(e) => cx.error(e.to_string()),
    }
}

// ---------------------------------------------------------------------------------------
// Global setup

/// Fluids offered in the lists; any other CoolProp name can be typed.
pub(crate) const COMMON_FLUIDS: &[&str] = &[
    "Water",
    "HeavyWater",
    "Ethanol",
    "Methanol",
    "Acetone",
    "Toluene",
    "n-Hexane",
    "n-Heptane",
    "n-Decane",
    "Ammonia",
    "R134a",
    "Propane",
];

fn setup(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    ui.data_mut(|d| {
        d.insert_temp(
            egui::Id::new("properties_step"),
            egui::Id::new(("properties", Selection::Setup)),
        );
    });
    let project = cx.doc.project().clone();
    let units = cx.units();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if section(ui, tr("Project")) {
                let mut info = project.info.clone();
                let mut renamed = false;
                props(ui, "project_info", |ui| {
                    label(ui, tr("Name"), "");
                    renamed |= widgets::text_field(ui, &mut info.name).changed();
                    ui.end_row();
                    label(ui, tr("Author"), "");
                    renamed |= widgets::text_field(ui, &mut info.author).changed();
                    ui.end_row();
                    value_row(ui, tr("Machine"), project.machine().to_string());
                });
                if renamed {
                    cx.edit("edit project information", |p| {
                        p.info = info;
                        Ok(())
                    });
                }
            }
            let mut setup = project.setup().clone();
            let mut changed = false;

            if section(ui, tr("Design point")) {
                props(ui, "design_point", |ui| {
                    label(ui, tr("Flow rate"), "");
                    ui.horizontal(|ui| {
                        widgets::halves(ui);
                        let (mut value, mass) = match setup.design_point.flow {
                            FlowRate::Volume(v) => (v, false),
                            FlowRate::Mass(m) => (m, true),
                        };
                        let kind = if mass {
                            QuantityKind::MassFlow
                        } else {
                            QuantityKind::VolumeFlow
                        };
                        changed |= widgets::quantity(ui, &mut value, kind, &units, 1e-6..=1e5);
                        let mut is_mass = mass;
                        ComboBox::from_id_salt("flow_kind")
                            .selected_text(tr(if mass { "mass" } else { "volume" }))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut is_mass, false, tr("volume"));
                                ui.selectable_value(&mut is_mass, true, tr("mass"));
                            });
                        let density = cx
                            .derived
                            .liquid
                            .as_ref()
                            .and_then(|l| l.as_ref().ok())
                            .map(|l| l.density);
                        if is_mass != mass {
                            // Keep the same flow when switching how it is given.
                            let rho = density.unwrap_or(1000.0);
                            value = if is_mass { value * rho } else { value / rho };
                            changed = true;
                        }
                        setup.design_point.flow = if is_mass {
                            FlowRate::Mass(value)
                        } else {
                            FlowRate::Volume(value)
                        };
                    });
                    ui.end_row();

                    label(ui, tr("Energy"), "");
                    ui.horizontal(|ui| {
                        widgets::halves(ui);
                        let (mut value, pressure) = match setup.design_point.energy {
                            EnergyTransfer::TotalPressureDifference(p) => (p, true),
                            other => (other.value(), false),
                        };
                        let kind = if pressure {
                            QuantityKind::Pressure
                        } else {
                            QuantityKind::Head
                        };
                        changed |= widgets::quantity(ui, &mut value, kind, &units, 1e-3..=1e9);
                        let mut is_pressure = pressure;
                        ComboBox::from_id_salt("energy_kind")
                            .selected_text(tr(if pressure { "pressure rise" } else { "head" }))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut is_pressure, false, tr("head"));
                                ui.selectable_value(&mut is_pressure, true, tr("pressure rise"));
                            });
                        if is_pressure != pressure {
                            let rho_g = cx
                                .derived
                                .liquid
                                .as_ref()
                                .and_then(|l| l.as_ref().ok())
                                .map_or(1000.0, |l| l.density)
                                * tp_units::STANDARD_GRAVITY;
                            value = if is_pressure {
                                value * rho_g
                            } else {
                                value / rho_g
                            };
                            changed = true;
                        }
                        setup.design_point.energy = if is_pressure {
                            EnergyTransfer::TotalPressureDifference(value)
                        } else {
                            EnergyTransfer::Head(value)
                        };
                    });
                    ui.end_row();

                    label(ui, tr("Rotational speed"), "");
                    changed |= widgets::quantity(
                        ui,
                        &mut setup.design_point.speed,
                        QuantityKind::RotationalSpeed,
                        &units,
                        0.1..=1e5,
                    );
                    ui.end_row();

                    label(
                        ui,
                        tr("Rotation"),
                        tr("Seen from the inlet, looking downstream"),
                    );
                    let text = |r: Rotation| {
                        tr(match r {
                            Rotation::Clockwise => "clockwise",
                            Rotation::CounterClockwise => "counter-clockwise",
                        })
                    };
                    ComboBox::from_id_salt("rotation")
                        .selected_text(text(setup.rotation))
                        .show_ui(ui, |ui| {
                            for r in [Rotation::Clockwise, Rotation::CounterClockwise] {
                                changed |= ui
                                    .selectable_value(&mut setup.rotation, r, text(r))
                                    .changed();
                            }
                        });
                    ui.end_row();
                });
            }

            if section(ui, tr("Fluid and inlet conditions")) {
                props(ui, "fluid", |ui| {
                    label(ui, tr("Fluid"), "");
                    ui.horizontal(|ui| {
                        widgets::halves(ui);
                        changed |= widgets::text_field(ui, &mut setup.fluid).changed();
                        ComboBox::from_id_salt("common_fluids")
                            .selected_text(tr("common…"))
                            .show_ui(ui, |ui| {
                                for name in COMMON_FLUIDS {
                                    if ui.selectable_label(setup.fluid == *name, *name).clicked() {
                                        setup.fluid = (*name).to_owned();
                                        changed = true;
                                    }
                                }
                            });
                    });
                    ui.end_row();
                    label(ui, tr("Inlet total pressure"), "");
                    changed |= widgets::quantity(
                        ui,
                        &mut setup.inlet.total_pressure,
                        QuantityKind::Pressure,
                        &units,
                        1.0..=1e9,
                    );
                    ui.end_row();
                    label(ui, tr("Inlet total temperature"), "");
                    changed |= widgets::quantity(
                        ui,
                        &mut setup.inlet.total_temperature,
                        QuantityKind::Temperature,
                        &units,
                        1.0..=2000.0,
                    );
                    ui.end_row();
                });

                match &cx.derived.liquid {
                    Some(Ok(l)) => {
                        props(ui, "liquid", |ui| {
                            value_row(
                                ui,
                                tr("Density ρ"),
                                units.show(QuantityKind::Density, l.density),
                            );
                            value_row(
                                ui,
                                tr("Kinematic viscosity ν"),
                                units.show(
                                    QuantityKind::KinematicViscosity,
                                    l.kinematic_viscosity(),
                                ),
                            );
                            value_row(
                                ui,
                                tr("Vapour pressure p_v"),
                                units.show(QuantityKind::Pressure, l.vapor_pressure),
                            );
                            let source = match l.source {
                                tp_fluids::PropertySource::CoolProp => "CoolProp",
                                tp_fluids::PropertySource::BuiltinWater => {
                                    tr("built-in water model (IAPWS)")
                                }
                            };
                            value_row(ui, tr("Source"), source);
                        });
                    }
                    Some(Err(e)) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    None => {}
                }
            }
            if changed {
                cx.edit("edit global setup", |p| p.set_setup(setup));
            }

            acceptance(ui, cx, &project);
        });
}

/// How the objective strip judges the predicted head. Not a design input, so it is
/// edited apart from the global setup and makes no step stale.
fn acceptance(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project) {
    let units = cx.units();
    if section_info(
        ui,
        tr("Acceptance"),
        tr(
            "How the objective strip judges the predicted head against the design point. It is not a design input: changing it leaves every step as it is.",
        ),
    ) {
        let criteria = project
            .components()
            .iter()
            .find(|c| c.kind() == ComponentKind::Impeller)
            .and_then(|c| cx.derived.impeller(c.id()))
            .and_then(|r| {
                let main = r.main.as_ref().ok()?;
                Some(ClassCriteria::of(main, r.performance.as_ref().ok()))
            });
        let mut input = project.acceptance;
        let resolved = input.resolve(criteria.as_ref());
        let mut changed = false;
        props(ui, "acceptance", |ui| {
            changed |= widgets::param_choice(
                ui,
                tr("Quality class"),
                tr(
                    "Gülich, Centrifugal Pumps, Table 15.2, after ISO 9906. Automatic: the most stringent of the outlet and inlet tip speeds, the head per stage at the best efficiency point and the power decides.",
                ),
                &mut input.class,
                resolved.map(|a| a.class),
                &QualityClass::ALL,
                |c| c.name().to_owned(),
            );
            changed |= widgets::param_percent(
                ui,
                tr("Head tolerance t_H"),
                tr(
                    "G1 ±3 %, G2 ±5 %, G3 ±7 %. ISO 9906 sets them for acceptance tests, a measured pump against its guarantee; the objective strip uses them by analogy for the predicted head.",
                ),
                &mut input.head_tolerance,
                resolved.map(|a| a.head_tolerance),
                0.001..=0.5,
            );
            if let Some(c) = criteria {
                let [u2, u1, head, power] = c.classes().map(QualityClass::name);
                let hint = tf(
                    "u₂ {} ({}), u₁ {} ({}), head per stage {} ({}), power {} ({}). Table 15.2: u₂ above 90 m/s G1, 40 to 90 G2; u₁ above 50 m/s G1, 15 to 50 G2; head above 400 m G1, 80 to 400 G2; power above 3000 kW G1, 300 to 3000 G2.",
                    &[
                        &units.show(QuantityKind::Velocity, c.outlet_tip_speed),
                        &u2,
                        &units.show(QuantityKind::Velocity, c.inlet_tip_speed),
                        &u1,
                        &units.show(QuantityKind::Head, c.head_per_stage),
                        &head,
                        &units.show(QuantityKind::Power, c.power),
                        &power,
                    ],
                );
                value_info(
                    ui,
                    tr("Criteria"),
                    &hint,
                    format!(
                        "u₂ {} · H {} → {}",
                        units.show(QuantityKind::Velocity, c.outlet_tip_speed),
                        units.show(QuantityKind::Head, c.head_per_stage),
                        c.class().name()
                    ),
                );
            }
        });
        if changed {
            cx.edit("edit acceptance", |p| {
                p.acceptance = input;
                Ok(())
            });
        }
    }
}

// ---------------------------------------------------------------------------------------
// Design steps

fn step_panel(ui: &mut Ui, cx: &mut EditCtx<'_>, id: ComponentId, step: DesignStep) {
    let project = cx.doc.project().clone();
    let Some(component) = project.component(id) else {
        return;
    };
    let p = Palette::of(ui);
    let status = component.status(step);
    let (glyph, color) = status_icon(&p, status);
    let step_id = egui::Id::new(("properties", *cx.selection));
    ui.data_mut(|d| {
        d.insert_temp(egui::Id::new("properties_step"), step_id);
        d.insert_temp(status_id(), status);
        // A step never accepted has nothing to compare with.
        if status == StepStatus::Pending {
            d.remove::<std::collections::HashMap<egui::Id, String>>(step_id.with("accepted"));
        }
    });
    // Status, the place of the step among those of its component, and what it does.
    ui.horizontal(|ui| {
        egui::Frame::new()
            .fill(color.gamma_multiply(0.14))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(8, 2))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{glyph} {}", status_text(status)))
                        .size(11.5)
                        .color(color),
                );
            });
        let kind = component.kind();
        let counted: Vec<DesignStep> = kind
            .steps()
            .iter()
            .copied()
            .filter(|&s| is_available(kind, s) && s != DesignStep::ModelSettings)
            .collect();
        if let Some(n) = counted.iter().position(|&s| s == step) {
            ui.label(
                RichText::new(tf("step {} of {}", &[&(n + 1), &counted.len()]))
                    .size(11.5)
                    .color(p.weak),
            );
        }
        if let Some(text) = intro(kind, step) {
            widgets::info(ui, text);
        }
    });
    problems(ui, cx, id, step);

    if !is_available(component.kind(), step) {
        ui.separator();
        ui.label(planned_phase(component.kind(), step));
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if step != DesignStep::ModelSettings {
                match component.kind() {
                    ComponentKind::Volute => {
                        crate::volute_steps::show(ui, cx, &project, id, step);
                        return;
                    }
                    ComponentKind::Stator { .. } => {
                        crate::diffuser_steps::show(ui, cx, &project, id, step);
                        return;
                    }
                    ComponentKind::Impeller => {}
                }
            }
            match step {
                DesignStep::MainDimensions => main_dimensions(ui, cx, &project, id),
                DesignStep::MeridionalContour => meridional(ui, cx, &project, id),
                DesignStep::BladeProperties => blade_properties(ui, cx, &project, id),
                DesignStep::BladeMeanLines => mean_lines(ui, cx, &project, id),
                DesignStep::BladeProfiles => profiles(ui, cx, &project, id),
                DesignStep::BladeEdges => edges(ui, cx, &project, id),
                DesignStep::ModelSettings => model(ui, cx, &project, id),
                _ => {}
            }
        });
}

pub(crate) fn reset(cx: &mut EditCtx<'_>, project: &Project, id: ComponentId, step: DesignStep) {
    cx.doc.end_gesture();
    let label = format!("reset {step}");
    match project.component(id).map(|c| c.kind()) {
        Some(ComponentKind::Volute) => {
            crate::volute_steps::reset(cx, project, id, step, &label);
            return;
        }
        Some(ComponentKind::Stator { .. }) => {
            crate::diffuser_steps::reset(cx, project, id, step, &label);
            return;
        }
        _ => {}
    }
    if step == DesignStep::MainDimensions {
        let old = project.pump_impeller_input(id).unwrap_or_default();
        let input = CentrifugalPumpInput {
            suction: old.suction,
            inlet_design: old.inlet_design,
            ..CentrifugalPumpInput::default()
        };
        cx.edit(&label, |p| {
            p.set_main_dimensions(id, MainDimensionsInput::CentrifugalPump(input))
        });
        return;
    }
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let defaults = ImpellerGeometryInput::default();
    match step {
        DesignStep::MeridionalContour => input.meridional = defaults.meridional,
        DesignStep::BladeProperties => input.properties = defaults.properties,
        DesignStep::BladeMeanLines => input.mean_lines = defaults.mean_lines,
        DesignStep::BladeProfiles => input.profiles = defaults.profiles,
        DesignStep::BladeEdges => input.edges = defaults.edges,
        _ => return,
    }
    cx.edit(&label, |p| p.set_impeller_geometry(id, input));
}

/// Errors and warnings of the computation behind `step`.
fn problems(ui: &mut Ui, cx: &EditCtx<'_>, id: ComponentId, step: DesignStep) {
    if let Some(result) = cx.derived.diffuser_result(id) {
        let (error, warnings) = match result {
            Ok(d) => (None, d.warnings.clone()),
            Err(e) => (Some(e), Vec::new()),
        };
        show_problems(ui, error, &warnings);
        return;
    }
    if let Some(result) = cx.derived.volute_result(id) {
        let (error, warnings) = match result {
            Ok(g) => (None, g.warnings.clone()),
            Err(e) => (Some(e), Vec::new()),
        };
        show_problems(ui, error, &warnings);
        return;
    }
    let Some(results) = cx.derived.impeller(id) else {
        return;
    };
    let (error, warnings) = match step {
        DesignStep::MainDimensions => (
            results.main.as_ref().err(),
            results
                .main
                .as_ref()
                .map(|d| d.warnings.clone())
                .unwrap_or_default(),
        ),
        _ => (
            results.geometry.as_ref().err(),
            results
                .geometry
                .as_ref()
                .map(|g| g.warnings.clone())
                .unwrap_or_default(),
        ),
    };
    show_problems(ui, error, &warnings);
}

/// The error and warnings of the step, in a strip under its heading: the error always
/// shows; the warnings fold into one line with the first of them.
fn show_problems(ui: &mut Ui, error: Option<&String>, warnings: &[String]) {
    if error.is_none() && warnings.is_empty() {
        return;
    }
    ui.add_space(6.0);
    if let Some(e) = error {
        messages::card(ui, Severity::Error, e);
        ui.add_space(3.0);
    }
    if warnings.is_empty() {
        return;
    }
    let id = egui::Id::new("properties_warnings");
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let first = crate::i18n::message(&warnings[0]);
    let text = if warnings.len() == 1 {
        tf("1 warning: {}", &[&first])
    } else {
        tf("{} warnings: {}", &[&warnings.len(), &first])
    };
    let caret = if open {
        icon::CARET_UP
    } else {
        icon::CARET_DOWN
    };
    let warn = ui.visuals().warn_fg_color;
    let head = ui.add(
        egui::Button::new(RichText::new(format!("{}  {text}  {caret}", icon::WARNING)).color(warn))
            .fill(Palette::of(ui).warning_bg)
            .truncate(),
    );
    head.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &text));
    if head
        .on_hover_text(tr("Show or fold the warnings"))
        .clicked()
    {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open {
        for w in warnings {
            messages::card(ui, Severity::Warning, w);
            ui.add_space(3.0);
        }
    }
}

fn main_dimensions(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    use QuantityKind::*;
    let units = cx.units();
    let mut input = project.pump_impeller_input(id).unwrap_or_default();
    let design = cx.derived.main(id).cloned();
    let d = design.as_ref();
    let mut changed = false;

    if section(ui, tr("Impeller type")) {
        props(ui, "impeller_type", |ui| {
            label(
                ui,
                tr("Outlet"),
                tr(
                    "Automatic: semi-axial above nq = 60, where it beats a radial outlet (Gülich Chap. 7.5)",
                ),
            );
            let semi = d.is_some_and(|d| d.semi_axial);
            ComboBox::from_id_salt("impeller_kind")
                .selected_text(match input.impeller_type {
                    ImpellerType::Auto if semi => tr("automatic: semi-axial"),
                    ImpellerType::Auto => tr("automatic: radial"),
                    ImpellerType::Radial => tr("radial"),
                    ImpellerType::SemiAxial => tr("semi-axial (mixed flow)"),
                })
                .show_ui(ui, |ui| {
                    for (kind, text) in [
                        (ImpellerType::Auto, tr("automatic")),
                        (ImpellerType::Radial, tr("radial")),
                        (ImpellerType::SemiAxial, tr("semi-axial (mixed flow)")),
                    ] {
                        changed |= ui
                            .selectable_value(&mut input.impeller_type, kind, text)
                            .changed();
                    }
                });
            ui.end_row();
            label(ui, tr("Suction"), "");
            ComboBox::from_id_salt("suction")
                .selected_text(tr(match input.suction {
                    Suction::Single => "single entry",
                    Suction::Double => "double entry",
                }))
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut input.suction, Suction::Single, tr("single entry"))
                        .changed();
                    changed |= ui
                        .selectable_value(&mut input.suction, Suction::Double, tr("double entry"))
                        .changed();
                });
            ui.end_row();
            label(ui, tr("Inlet design"), "");
            ComboBox::from_id_salt("inlet_design")
                .selected_text(tr(match input.inlet_design {
                    InletDesign::Normal => "normal impeller",
                    InletDesign::Suction => "suction impeller (low NPSH)",
                }))
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut input.inlet_design,
                            InletDesign::Normal,
                            tr("normal impeller"),
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut input.inlet_design,
                            InletDesign::Suction,
                            tr("suction impeller (low NPSH)"),
                        )
                        .changed();
                });
            ui.end_row();
        });
    }

    if section(ui, tr("Efficiencies")) {
        props(ui, "efficiencies", |ui| {
            changed |= param_f64(
                ui,
                tr("Hydraulic η_h"),
                tr("Gülich Table 3.9 (m = 0.08)"),
                &mut input.hydraulic_efficiency,
                Dimensionless,
                d.map(|d| d.efficiencies.hydraulic),
                &units,
                0.3..=0.99,
            );
            changed |= param_f64(
                ui,
                tr("Volumetric η_v"),
                tr("Leakage through the seals"),
                &mut input.volumetric_efficiency,
                Dimensionless,
                d.map(|d| d.efficiencies.volumetric),
                &units,
                0.5..=1.0,
            );
            changed |= param_f64(
                ui,
                tr("Overall η"),
                tr("Gülich Table 3.9 (m = 0.1)"),
                &mut input.overall_efficiency,
                Dimensionless,
                d.map(|d| d.efficiencies.overall),
                &units,
                0.2..=0.95,
            );
        });
    }

    if section(ui, tr("Dimensions")) {
        props(ui, "dimensions", |ui| {
            changed |= param_f64(
                ui,
                tr("Pressure coefficient ψ"),
                tr("ψ = 2gH/u₂², Gülich eq. 3.26; ignored when d₂ is manual"),
                &mut input.pressure_coefficient,
                Dimensionless,
                d.map(|d| d.pressure_coefficient),
                &units,
                0.3..=1.5,
            );
            changed |= param_f64(
                ui,
                tr("Outlet diameter d₂"),
                tr("From ψ and the head"),
                &mut input.outlet_diameter,
                Length,
                d.map(|d| d.outlet_diameter),
                &units,
                0.01..=5.0,
            );
            changed |= param_f64(
                ui,
                tr("Outlet width b₂"),
                tr("Gülich eq. 7.1"),
                &mut input.outlet_width,
                Length,
                d.map(|d| d.outlet_width),
                &units,
                0.0005..=1.0,
            );
            changed |= param_f64(
                ui,
                tr("Outlet hub diameter d₂i"),
                tr(
                    "Hub end of the outlet edge; d₂ is the shroud end d₂a. Automatic: Gülich eqs. 7.13–7.14 on a semi-axial impeller, d₂ on a radial one; set by hand on a radial one it slants the edge and the design takes the mean diameter",
                ),
                &mut input.outlet_hub_diameter,
                Length,
                d.map(|d| d.outlet_hub_diameter),
                &units,
                0.005..=5.0,
            );
            if d.is_some_and(|d| d.semi_axial) {
                changed |= param_f64(
                    ui,
                    tr("Outlet inclination ε_s"),
                    tr(
                        "Angle of the outer streamline at the outlet to a normal to the axis: 0° radial, 90° axial (Gülich eq. 7.12, ±5° in executed pumps)",
                    ),
                    &mut input.outlet_inclination,
                    Angle,
                    d.map(|d| d.outlet_inclination),
                    &units,
                    1f64.to_radians()..=80f64.to_radians(),
                );
            }
            let factor = match input.inlet_diameter_factor {
                Param::Manual(v) => Some(v),
                Param::Auto => d.map(|d| corr::inlet_diameter_factor(input.inlet_design, d.nq)),
            };
            changed |= param_f64(
                ui,
                tr("Eye factor f_d1"),
                tr("d₁ over the eye diameter of minimum w₁ (Gülich Table 7.1)"),
                &mut input.inlet_diameter_factor,
                Dimensionless,
                factor,
                &units,
                0.8..=1.6,
            );
            changed |= param_f64(
                ui,
                tr("Eye diameter d₁"),
                tr("Overrides the eye factor when manual"),
                &mut input.inlet_diameter,
                Length,
                d.map(|d| d.inlet_diameter),
                &units,
                0.005..=3.0,
            );
            changed |= param_f64(
                ui,
                tr("Hub diameter d_n"),
                tr("Hub diameter at the eye"),
                &mut input.hub_diameter,
                Length,
                d.map(|d| d.hub_diameter),
                &units,
                0.0..=2.0,
            );
            changed |= param_f64(
                ui,
                tr("Shaft diameter"),
                tr("From the torque and the allowable shear stress"),
                &mut input.shaft_diameter,
                Length,
                d.map(|d| d.shaft_diameter),
                &units,
                0.002..=1.0,
            );
            let tau = input
                .shaft_shear_stress
                .resolve(|| corr::SHAFT_SHEAR_STRESS);
            changed |= param_f64(
                ui,
                tr("Shaft shear stress"),
                tr("Allowable torsional stress"),
                &mut input.shaft_shear_stress,
                Pressure,
                Some(tau),
                &units,
                1e6..=300e6,
            );
            changed |= param_u32(
                ui,
                tr("Blades z"),
                tr(
                    "Pfleiderer: 5 to 7 for nq 10–120 (Gülich), 3 to 12 otherwise; the head rise picks 3 to 8 (Lobanoff & Ross Fig. 3-2)",
                ),
                &mut input.blade_count,
                d.map(|d| d.blade_count),
                3..=24,
            );
            changed |= param_f64(
                ui,
                tr("Head rise to shut-off"),
                tr(
                    "(H₀ − H)/H. Manual: sets the blades and β₂B (Lobanoff & Ross Figs. 3-2 and 3-13); fewer blades give a steeper curve",
                ),
                &mut input.head_rise,
                Dimensionless,
                d.map(|d| d.head_rise),
                &units,
                0.02..=1.0,
            );
            changed |= param_f64(
                ui,
                tr("Outlet blade angle β₂B"),
                tr(
                    "Manual: d₂ follows from the head, or b₂ when d₂ is manual; 90° gives radial blades",
                ),
                &mut input.outlet_blade_angle,
                Angle,
                d.map(|d| d.outlet.blade_angle),
                &units,
                5f64.to_radians()..=90f64.to_radians(),
            );
            changed |= param_f64(
                ui,
                tr("Blade thickness e"),
                tr("Normal thickness at inlet and outlet"),
                &mut input.blade_thickness,
                Length,
                d.map(|d| d.blade_thickness),
                &units,
                0.0003..=0.1,
            );
        });
    }

    if section(ui, tr("Inlet and cavitation")) {
        props(ui, "inlet", |ui| {
            let incidence = input.incidence.resolve(|| 0.0);
            changed |= param_f64(
                ui,
                tr("Incidence i"),
                tr("Between blade and inflow at the leading edge (0°–4°)"),
                &mut input.incidence,
                Angle,
                Some(incidence),
                &units,
                -5f64.to_radians()..=10f64.to_radians(),
            );
            let nss = input
                .suction_specific_speed
                .resolve(|| corr::SUCTION_SPECIFIC_SPEED);
            changed |= param_f64(
                ui,
                tr("Suction specific speed nss"),
                tr("rpm, m³/s, m; sets NPSH3"),
                &mut input.suction_specific_speed,
                Dimensionless,
                Some(nss),
                &units,
                100.0..=400.0,
            );
        });
    }
    if changed {
        cx.edit("edit main dimensions", |p| {
            p.set_main_dimensions(id, MainDimensionsInput::CentrifugalPump(input))
        });
    }

    if let Some(d) = d
        && section_kind(ui, tr("Results"), "", Kind::Results)
    {
        props(ui, "main_results", |ui| {
            value_row(ui, tr("Specific speed nq"), format!("{:.2}", d.nq));
            value_row(ui, tr("Shaft power"), units.show(Power, d.stage_power));
            value_row(
                ui,
                tr("Inlet blade angle β₁B (mean)"),
                units.show(Angle, d.inlet_mean.blade_angle),
            );
            let (lo, hi) = corr::inlet_area_ratio_band(d.nq);
            value_info(
                ui,
                tr("Inlet area ratio A_v/A_e"),
                &tf(
                    "Area between the blades at the inlet over the eye area. Lobanoff & Ross (Fig. 3-12) give {}–{} at this nq for large eyes (β₁B ≈ 15–20°); the eye of minimum w₁ gives more",
                    &[&format!("{lo:.2}"), &format!("{hi:.2}")],
                ),
                format!("{:.2}", d.inlet_area_ratio),
            );
            value_row(
                ui,
                tr("Slip factor γ"),
                format!("{:.3}", d.outlet.slip_factor),
            );
            value_row(ui, tr("NPSH3"), units.show(Head, d.npsh_required));
            value_row(ui, tr("NPSH available"), units.show(Head, d.npsh_available));
        });
    }
}

/// Geometry inputs of `id` and the closure that stores them.
fn edit_geometry(cx: &mut EditCtx<'_>, id: ComponentId, label: &str, input: ImpellerGeometryInput) {
    cx.edit(label, |p| p.set_impeller_geometry(id, input));
}

fn meridional(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    use QuantityKind::*;
    let units = cx.units();
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let g = cx.derived.geometry(id);
    let m: &mut MeridionalInput = &mut input.meridional;
    let mut changed = false;
    use tp_design::impeller::meridian::DesignMode;
    if section(ui, tr("Contour")) {
        props(ui, "contour", |ui| {
            label(
                ui,
                tr("Design mode"),
                tr(
                    "Hub and shroud drawn directly, or a center line and a cross-section \
                     distribution the walls follow (suited to mixed-flow impellers)",
                ),
            );
            let mode = m.design_mode;
            let mode_name = |mode: DesignMode| {
                tr(match mode {
                    DesignMode::Walls => "hub and shroud",
                    DesignMode::CenterLine => "center line and area",
                })
            };
            ComboBox::from_id_salt("design_mode")
                .selected_text(mode_name(mode))
                .show_ui(ui, |ui| {
                    for option in [DesignMode::Walls, DesignMode::CenterLine] {
                        ui.selectable_value(&mut m.design_mode, option, mode_name(option));
                    }
                });
            ui.end_row();
            changed |= m.design_mode != mode;
            changed |= param_f64(
                ui,
                tr("Axial length L"),
                tr("From the inlet plane to the hub at the outlet; default b₂ + (d₁ − d_n)/2"),
                &mut m.axial_length,
                Length,
                g.map(|g| g.contour.axial_length),
                &units,
                0.001..=5.0,
            );
            if let Some(g) = g {
                use tp_design::impeller::meridian::{AxialReference, InletTilt};
                let contour = &g.contour;
                label(
                    ui,
                    tr("Axial reference"),
                    tr(
                        "The point whose axial position is given: the middle of the inlet line or \
                         of the outlet line; the other end follows from L",
                    ),
                );
                let before = (m.axial_reference, m.axial_position);
                let name = |r: AxialReference| {
                    tr(match r {
                        AxialReference::Inlet => "middle of the inlet",
                        AxialReference::Outlet => "middle of the outlet",
                    })
                };
                ComboBox::from_id_salt("axial_reference")
                    .selected_text(name(m.axial_reference))
                    .show_ui(ui, |ui| {
                        for (reference, point) in [
                            (
                                AxialReference::Inlet,
                                contour.hub.start().lerp(contour.shroud.start(), 0.5),
                            ),
                            (
                                AxialReference::Outlet,
                                contour.hub.end().lerp(contour.shroud.end(), 0.5),
                            ),
                        ] {
                            // The meridian stays where it is: the position follows.
                            if ui
                                .selectable_label(m.axial_reference == reference, name(reference))
                                .clicked()
                            {
                                m.axial_reference = reference;
                                m.axial_position = point.x;
                            }
                        }
                    });
                ui.end_row();
                label(
                    ui,
                    tr("Axial position z"),
                    tr(
                        "Of the reference point; changing it moves the whole meridian, drawn points included",
                    ),
                );
                let mut position = m.axial_position;
                if widgets::quantity(ui, &mut position, Length, &units, -10.0..=10.0) {
                    m.shift(position - m.axial_position);
                    m.axial_position = position;
                }
                ui.end_row();
                changed |= (m.axial_reference, m.axial_position) != before;
                label(
                    ui,
                    tr("Inlet line"),
                    tr(
                        "Normal to the axis, or leaning: the shroud point an axial distance \
                         downstream of the hub point, or the line at an angle from the normal",
                    ),
                );
                let height = (contour.shroud.start().y - contour.hub.start().y).max(1e-9);
                let offset = m.inlet_tilt.offset(height);
                let tilt = m.inlet_tilt;
                let tilt_name = |t: &InletTilt| {
                    tr(match t {
                        InletTilt::Normal => "normal to the axis",
                        InletTilt::Axial(_) => "axial distance",
                        InletTilt::Angle(_) => "angle",
                    })
                };
                ComboBox::from_id_salt("inlet_tilt")
                    .selected_text(tilt_name(&tilt))
                    .show_ui(ui, |ui| {
                        for option in [
                            InletTilt::Normal,
                            InletTilt::Axial(offset),
                            InletTilt::Angle((offset / height).atan()),
                        ] {
                            let same =
                                std::mem::discriminant(&option) == std::mem::discriminant(&tilt);
                            if ui.selectable_label(same, tilt_name(&option)).clicked() && !same {
                                m.inlet_tilt = option;
                            }
                        }
                    });
                ui.end_row();
                match &mut m.inlet_tilt {
                    InletTilt::Normal => {}
                    InletTilt::Axial(dz) => {
                        label(ui, tr("Shroud point Δz"), tr("Downstream of the hub point"));
                        changed |= widgets::quantity(ui, dz, Length, &units, -1.0..=1.0);
                        ui.end_row();
                    }
                    InletTilt::Angle(angle) => {
                        label(
                            ui,
                            tr("Inlet angle"),
                            tr(
                                "From the normal to the axis; positive with the shroud point downstream",
                            ),
                        );
                        changed |= widgets::quantity(ui, angle, Angle, &units, -1.0..=1.0);
                        ui.end_row();
                    }
                }
                changed |= m.inlet_tilt != tilt;
            }
            let walls = [
                (
                    "Hub drawn by segments",
                    &mut m.hub_segments,
                    [
                        ("Hub inlet tangent", &mut m.hub_inlet_tangent),
                        ("Hub outlet tangent", &mut m.hub_outlet_tangent),
                    ],
                ),
                (
                    "Shroud drawn by segments",
                    &mut m.shroud_segments,
                    [
                        ("Shroud inlet tangent", &mut m.shroud_inlet_tangent),
                        ("Shroud outlet tangent", &mut m.shroud_outlet_tangent),
                    ],
                ),
            ];
            if m.design_mode == DesignMode::CenterLine {
                if let Some(list) = &m.center_line {
                    let count = list.len();
                    label(
                        ui,
                        tr("Center line drawn by segments"),
                        tr("Drawn in the editor; hub and shroud follow it"),
                    );
                    let automatic = ui
                        .horizontal(|ui| {
                            ui.label(tf("{} segments", &[&count]));
                            ui.button(tr("Automatic")).clicked()
                        })
                        .inner;
                    ui.end_row();
                    if automatic {
                        m.center_line = None;
                        changed = true;
                    }
                }
                label(
                    ui,
                    tr("Cross section"),
                    tr(
                        "A_rel along the center line, 0 at the inlet section and 1 at the outlet one",
                    ),
                );
                ui.label(if m.area_progression.is_empty() {
                    tr("linear").to_owned()
                } else {
                    tf("{} points", &[&m.area_progression.len()])
                });
                ui.end_row();
            }
            // Walls that follow the center line have no inputs of their own.
            let walls = if m.design_mode == DesignMode::CenterLine {
                Vec::new()
            } else {
                walls.into_iter().collect()
            };
            for (drawn, segments, tangents) in walls {
                if let Some(list) = segments {
                    let count = list.len();
                    widgets::label(
                        ui,
                        tr(drawn),
                        tr(
                            "Drawn segment by segment in the meridional view; its tangent lengths \
                            do not apply",
                        ),
                    );
                    let automatic = ui
                        .horizontal(|ui| {
                            ui.label(tf("{} segments", &[&count]));
                            ui.button(tr("Automatic")).clicked()
                        })
                        .inner;
                    ui.end_row();
                    if automatic {
                        *segments = None;
                        changed = true;
                    }
                    continue;
                }
                for (label, param) in tangents {
                    changed |= param_f64(
                        ui,
                        tr(label),
                        tr(
                            "Length of the Bézier tangent as a fraction of the distance to the corner",
                        ),
                        param,
                        Dimensionless,
                        Some(DEFAULT_TANGENT),
                        &units,
                        0.05..=0.95,
                    );
                }
            }
        });
        if m.design_mode == DesignMode::CenterLine {
            changed |= area_progression(ui, id, &mut m.area_progression);
        }
    }
    if section(ui, tr("Leading edge")) {
        props(ui, "leading_edge", |ui| {
            use tp_design::impeller::meridian::LeadingEdgeShape;
            let name = |shape: &LeadingEdgeShape| {
                tr(match shape {
                    LeadingEdgeShape::Channel => "Along the channel",
                    LeadingEdgeShape::Line => "Straight line",
                    LeadingEdgeShape::Bezier { .. } => "Bézier",
                })
            };
            label(
                ui,
                tr("Shape"),
                tr(
                    "Between its hub and shroud points: following the channel, straight, or a \
                    Bézier whose control points are dragged in the meridional view",
                ),
            );
            let before = std::mem::discriminant(&m.leading_edge_shape);
            ComboBox::from_id_salt("leading_edge_shape")
                .selected_text(name(&m.leading_edge_shape))
                .show_ui(ui, |ui| {
                    for shape in [LeadingEdgeShape::Channel, LeadingEdgeShape::Line] {
                        let text = name(&shape);
                        ui.selectable_value(&mut m.leading_edge_shape, shape, text);
                    }
                    let bezier = matches!(m.leading_edge_shape, LeadingEdgeShape::Bezier { .. });
                    if ui.selectable_label(bezier, tr("Bézier")).clicked() && !bezier {
                        m.leading_edge_shape = g.map_or(LeadingEdgeShape::Line, |g| {
                            crate::views::meridional::bezier_along_edge(g)
                        });
                    }
                });
            ui.end_row();
            changed |= std::mem::discriminant(&m.leading_edge_shape) != before;
            changed |= param_f64(
                ui,
                tr("On the hub"),
                tr("Fraction of the hub length"),
                &mut m.leading_edge_hub,
                Dimensionless,
                Some(g.map_or(DEFAULT_LEADING_EDGE_HUB, |g| g.contour.leading_edge.0)),
                &units,
                0.0..=0.9,
            );
            changed |= param_f64(
                ui,
                tr("On the shroud"),
                tr("Fraction of the shroud length"),
                &mut m.leading_edge_shroud,
                Dimensionless,
                Some(g.map_or(DEFAULT_LEADING_EDGE_SHROUD, |g| g.contour.leading_edge.1)),
                &units,
                0.0..=0.9,
            );
            use crate::views::meridional::{extent_like, extent_name};
            use tp_design::impeller::meridian::EdgeExtent;
            label(
                ui,
                tr("Hub point"),
                tr(
                    "Independent of the shroud point, or following it at a constant axial or \
                     radial distance: 0 puts the edge normal to the axis or parallel to it",
                ),
            );
            let before = m.leading_edge_extent;
            ComboBox::from_id_salt("leading_edge_extent")
                .selected_text(extent_name(before))
                .show_ui(ui, |ui| {
                    for kind in [
                        EdgeExtent::Independent,
                        EdgeExtent::Axial(0.0),
                        EdgeExtent::Radial(0.0),
                    ] {
                        let same = std::mem::discriminant(&kind) == std::mem::discriminant(&before);
                        if ui.selectable_label(same, extent_name(kind)).clicked() && !same {
                            m.leading_edge_extent = g.map_or(kind, |g| extent_like(kind, g));
                        }
                    }
                });
            ui.end_row();
            changed |= m.leading_edge_extent != before;
            let offset = match &mut m.leading_edge_extent {
                EdgeExtent::Independent => None,
                EdgeExtent::Axial(dz) => Some((tr("Axial offset Δz"), dz)),
                EdgeExtent::Radial(dr) => Some((tr("Radial offset Δr"), dr)),
            };
            if let Some((name, value)) = offset {
                label(ui, name, tr("From the shroud point to the hub point"));
                changed |= widgets::quantity(ui, value, Length, &units, -1.0..=1.0);
                ui.end_row();
            }
        });
    }
    if section_info(
        ui,
        tr("Material"),
        tr(
            "The closed contours of the hub disk and the shroud cover, for the CAD model: \
             automatic from these values, or drawn on the Materials page of the editor",
        ),
    ) {
        props(ui, "material", |ui| {
            let used = g.map(|g| g.contour.material);
            let settings = &mut m.material;
            changed |= param_f64(
                ui,
                tr("Wall thickness"),
                tr("Hub disk and shroud; automatic: max(3 mm, 0.02·d₂)"),
                &mut settings.wall_thickness,
                Length,
                used.map(|u| u.wall_thickness),
                &units,
                0.0005..=0.2,
            );
            changed |= param_f64(
                ui,
                tr("Bore diameter"),
                tr("Automatic: the hub diameter at the eye"),
                &mut settings.bore_diameter,
                Length,
                used.map(|u| u.bore_diameter),
                &units,
                0.0..=1.0,
            );
            changed |= param_f64(
                ui,
                tr("Boss diameter"),
                tr(
                    "Around the bore; automatic: the bore plus a wall of a quarter of it, no thinner than the wall",
                ),
                &mut settings.boss_diameter,
                Length,
                used.map(|u| u.boss_diameter),
                &units,
                0.0..=1.0,
            );
            changed |= param_f64(
                ui,
                tr("Neck length"),
                tr(
                    "Of the shroud upstream of the inlet, where the annular seal runs; automatic: 0.12·d_sp (Gülich Chap. 3.6.2); 0 for none",
                ),
                &mut settings.neck_length,
                Length,
                used.map(|u| u.neck_length),
                &units,
                0.0..=0.5,
            );
            for (name, wire) in [
                (tr("Hub disk drawn by segments"), &mut m.hub_material),
                (tr("Shroud cover drawn by segments"), &mut m.shroud_material),
            ] {
                let Some(list) = wire else {
                    continue;
                };
                let count = list.len();
                label(
                    ui,
                    name,
                    tr("Drawn on the Materials page of the editor; the values above do not apply"),
                );
                let automatic = ui
                    .horizontal(|ui| {
                        ui.label(tf("{} segments", &[&count]));
                        ui.button(tr("Automatic")).clicked()
                    })
                    .inner;
                ui.end_row();
                if automatic {
                    *wire = None;
                    changed = true;
                }
            }
        });
    }
    if section_info(
        ui,
        tr("Side chambers"),
        tr(
            "Between the impeller and the casing: the front one down to the annular seal on \
             the neck, the rear one down to the shaft seal. Once designed, the axial thrust \
             and the loss analysis take these seals and gaps wherever theirs are automatic",
        ),
    ) {
        props(ui, "chambers", |ui| {
            let built = g.map(|g| g.contour.chambers.values);
            let chambers = &mut m.chambers;
            label(
                ui,
                tr("Design the side chambers"),
                tr(
                    "Export their fluid with the CAD model, alone and, with a volute after them, joined to the impeller and the volute in one flow domain for CFD; shape them in the Side chambers scope of the Meridian view",
                ),
            );
            let enabled = chambers.enabled;
            let response = ui.checkbox(&mut chambers.enabled, "");
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    enabled,
                    tr("Design the side chambers"),
                )
            });
            changed |= response.changed();
            ui.end_row();
            changed |= param_f64(
                ui,
                tr("Axial gap s_ax"),
                tr(
                    "Between the impeller and the casing walls; automatic: 0.05·r₂ (our choice; Gülich finds its effect small, Chap. 3.6.1)",
                ),
                &mut chambers.axial_gap,
                Length,
                built.map(|v| v.axial_gap),
                &units,
                0.0001..=0.2,
            );
            changed |= param_f64(
                ui,
                tr("Seal clearance s"),
                tr("Radial, of the annular seal on the neck; automatic: Gülich eq. 3.12"),
                &mut chambers.seal_clearance,
                Length,
                built.map(|v| v.seal_clearance),
                &units,
                0.00001..=0.01,
            );
            changed |= param_f64(
                ui,
                tr("Shaft seal diameter d_D"),
                tr("Where the rear chamber ends; automatic: the hub diameter"),
                &mut chambers.shaft_seal_diameter,
                Length,
                built.map(|v| v.shaft_seal_diameter),
                &units,
                0.001..=2.0,
            );
            if let Some(v) = built {
                value_row(
                    ui,
                    tr("Seal diameter d_sp"),
                    units.show(Length, v.seal_diameter),
                );
                value_row(
                    ui,
                    tr("Seal length L_sp"),
                    units.show(Length, v.seal_length),
                );
            }
            for (name, wire) in [
                (tr("Front casing drawn by segments"), &mut chambers.front),
                (tr("Rear casing drawn by segments"), &mut chambers.rear),
            ] {
                let Some(list) = wire else {
                    continue;
                };
                let count = list.len();
                label(
                    ui,
                    name,
                    tr(
                        "Drawn on the Side chambers page of the editor; the gap above does not apply",
                    ),
                );
                let automatic = ui
                    .horizontal(|ui| {
                        ui.label(tf("{} segments", &[&count]));
                        ui.button(tr("Automatic")).clicked()
                    })
                    .inner;
                ui.end_row();
                if automatic {
                    *wire = None;
                    changed = true;
                }
            }
        });
    }
    if section_info(
        ui,
        tr("Inlet duct"),
        tr(
            "An axial duct ahead of the eye, for the inlet of a CFD domain. With the side chambers designed, it ends an axial gap short of the neck tip, so the leakage through the annular seal returns into the eye inside the same domain",
        ),
    ) {
        props(ui, "inlet_duct", |ui| {
            let built = g.map(|g| g.contour.inlet_duct.values);
            let duct = &mut m.inlet_duct;
            label(
                ui,
                tr("Model the inlet duct"),
                tr(
                    "Export its fluid with the CAD model, alone and inside the joined flow domain; it is drawn in the Side chambers scope of the Meridian view",
                ),
            );
            let enabled = duct.enabled;
            let response = ui.checkbox(&mut duct.enabled, "");
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    enabled,
                    tr("Model the inlet duct"),
                )
            });
            changed |= response.changed();
            ui.end_row();
            changed |= param_f64(
                ui,
                tr("Length L"),
                tr("Upstream of the eye; automatic: the eye diameter d₁ (our choice)"),
                &mut duct.length,
                Length,
                built.map(|v| v.length),
                &units,
                0.001..=5.0,
            );
            changed |= param_f64(
                ui,
                tr("Duct diameter"),
                tr("Automatic: the shroud diameter at the inlet, so the wall carries straight on"),
                &mut duct.diameter,
                Length,
                built.map(|v| v.diameter),
                &units,
                0.001..=5.0,
            );
            changed |= param_f64(
                ui,
                tr("Axial gap Δz"),
                tr(
                    "Between the suction cover at the end of the duct and the tip of the neck, where the leakage comes out; automatic: the axial gap of the side chambers",
                ),
                &mut duct.axial_gap,
                Length,
                built.map(|v| v.axial_gap),
                &units,
                0.00001..=0.2,
            );
            if let Some(v) = built {
                value_row(
                    ui,
                    tr("Hub diameter through the duct"),
                    units.show(Length, v.hub_diameter),
                );
            }
        });
    }
    if changed {
        edit_geometry(cx, id, "edit meridional contour", input);
    }
    if let Some(g) = g {
        let qos = &g.channel.quasi_orthogonals;
        if section_kind(ui, tr("Meridional flow"), "", Kind::Results) {
            props(ui, "meridional_results", |ui| {
                value_row(
                    ui,
                    tr("Hub length"),
                    units.show(Length, g.contour.hub_length()),
                );
                value_row(
                    ui,
                    tr("Shroud length"),
                    units.show(Length, g.contour.shroud_length()),
                );
                value_row(
                    ui,
                    tr("c_m inlet / outlet"),
                    format!(
                        "{} / {}",
                        units.show(Velocity, qos[0].cm),
                        units.show(Velocity, qos[qos.len() - 1].cm)
                    ),
                );
                value_row(
                    ui,
                    tr("Largest local rise of c_m"),
                    format!("{:.1} %", g.channel.max_cm_rise() * 100.0),
                );
            });
        }
        if section_kind(
            ui,
            tr("Check against Gülich"),
            tr(
                "Recommendations for the meridional section of radial impellers: Gülich, \
                 Centrifugal Pumps, chap. 7.2.2.1 and eq. 7.2",
            ),
            Kind::Checks,
        ) {
            crate::views::meridian_editor::guide_table(ui, g, &units);
        }
    }
}

/// The cross-section distribution of the center-line mode, pasted as (x, A_rel) lines;
/// true when it changed.
fn area_progression(ui: &mut Ui, id: ComponentId, table: &mut Vec<[f64; 2]>) -> bool {
    let key = egui::Id::new(("area_progression", id.0));
    let mut text = ui
        .ctx()
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| table.iter().map(|[x, a]| format!("{x} {a}\n")).collect());
    ui.add(
        egui::TextEdit::multiline(&mut text)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text(tr(
                "x A_rel per line, x from 0 at the inlet to 1 at the outlet",
            )),
    );
    let parsed: Vec<[f64; 2]> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<f64> = line
                .split(|c: char| c.is_whitespace() || c == ';' || c == '\t')
                .filter(|f| !f.is_empty())
                .filter_map(|f| f.replace(',', ".").parse().ok())
                .collect();
            (fields.len() == 2).then(|| [fields[0], fields[1]])
        })
        .collect();
    let mut changed = false;
    ui.horizontal(|ui| {
        let sorted = parsed.windows(2).all(|w| w[0][0] < w[1][0]);
        if ui
            .add_enabled(parsed.len() >= 2 && sorted, egui::Button::new(tr("Apply")))
            .on_hover_text(tr("At least two points with x rising"))
            .clicked()
        {
            *table = parsed.clone();
            changed = true;
        }
        if ui.button(tr("Linear")).clicked() {
            table.clear();
            text.clear();
            changed = true;
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(key, text));
    changed
}

fn blade_properties(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let units = cx.units();
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let g = cx.derived.geometry(id);
    let mut changed = false;
    if section(ui, tr("Sections")) {
        props(ui, "properties", |ui| {
            changed |= param_u32(
                ui,
                tr("Spans"),
                tr("Blade sections from hub to shroud"),
                &mut input.properties.spans,
                Some(g.map_or(DEFAULT_SPANS, |g| g.blade.sections.len() as u32)),
                2..=21,
            );
            label(
                ui,
                tr("Meridional flow"),
                tr(
                    "Stream surfaces and c_m the sections are built on. The potential flow is faster on the shroud and slower at the hub in the bend, which sets the inlet angle of each section; the quasi-1D flow keeps c_m uniform across the channel",
                ),
            );
            ComboBox::from_id_salt("meridional_flow")
                .selected_text(match input.properties.meridional_flow {
                    MeridionalFlow::Potential => tr("potential (2D)"),
                    MeridionalFlow::QuasiOneDimensional => tr("quasi-1D"),
                })
                .show_ui(ui, |ui| {
                    for (flow, text) in [
                        (MeridionalFlow::Potential, tr("potential (2D)")),
                        (MeridionalFlow::QuasiOneDimensional, tr("quasi-1D")),
                    ] {
                        changed |= ui
                            .selectable_value(&mut input.properties.meridional_flow, flow, text)
                            .changed();
                    }
                });
            ui.end_row();
            label(
                ui,
                tr("Inlet velocity"),
                tr(
                    "The c_m the leading-edge angles are designed with. Local: each section's own, from the meridional flow. Uniform: the eye value c1m = Q_La/A1 at every section, as Gülich designs the inlet angles (eq. T7.1.7)",
                ),
            );
            ComboBox::from_id_salt("inlet_velocity")
                .selected_text(match input.properties.inlet_velocity {
                    InletVelocity::Local => tr("local"),
                    InletVelocity::Uniform => tr("uniform c1m"),
                })
                .show_ui(ui, |ui| {
                    for (velocity, text) in [
                        (InletVelocity::Local, tr("local")),
                        (InletVelocity::Uniform, tr("uniform c1m")),
                    ] {
                        changed |= ui
                            .selectable_value(&mut input.properties.inlet_velocity, velocity, text)
                            .changed();
                    }
                });
            ui.end_row();
            label(
                ui,
                tr("Blade shape"),
                tr(
                    "Cylindrical (2D) blades share one plan view swept along the axis: no lean, a one-piece core, the leading edge parallel to the axis. They suit nq below about 20 (Lobanoff & Ross ch. 3; Gülich Chap. 7.3)",
                ),
            );
            ComboBox::from_id_salt("blade_shape")
                .selected_text(blade_shape_name(input.properties.shape))
                .show_ui(ui, |ui| {
                    for shape in [
                        BladeShape::FreeForm,
                        BladeShape::Cylindrical,
                        BladeShape::CircularArc,
                        BladeShape::Straight,
                    ] {
                        changed |= ui
                            .selectable_value(
                                &mut input.properties.shape,
                                shape,
                                blade_shape_name(shape),
                            )
                            .changed();
                    }
                });
            ui.end_row();
        });
    }
    if changed {
        edit_geometry(cx, id, "edit blade properties", input);
    }
    if let Some(g) = g
        && section_kind(
            ui,
            tr("Blade loading"),
            tr("Gülich eqs. 7.1.10–11"),
            Kind::Checks,
        )
    {
        let l = &g.blade_loading;
        props(ui, "loading", |ui| {
            value_row(ui, tr("Blades z"), g.blade.count.to_string());
            value_row(
                ui,
                tr("Outlet blade angle β₂B"),
                units.show(QuantityKind::Angle, g.blade.outlet_blade_angle),
            );
            value_row(ui, tr("ξ_eff"), format!("{:.3}", l.effective));
            value_row(ui, tr("ξ_al"), format!("{:.3}", l.allowable));
            value_info(
                ui,
                tr("ξ_eff / ξ_al"),
                tr("Recommended 0.85 to 1.15"),
                format!("{:.2}", l.ratio()),
            );
        });
    }
}

/// Name of a blade shape in the current language.
fn blade_shape_name(shape: BladeShape) -> &'static str {
    match shape {
        BladeShape::FreeForm => tr("free-form (3D)"),
        BladeShape::Cylindrical => tr("cylindrical (2D)"),
        BladeShape::CircularArc => tr("circular arc (2D)"),
        BladeShape::Straight => tr("straight (2D)"),
    }
}

fn mean_lines(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let units = cx.units();
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let g = cx.derived.geometry(id);
    let mut changed = false;
    if section(ui, tr("Blade angle distribution")) {
        props(ui, "beta", |ui| {
            let shape = input.properties.shape;
            if matches!(shape, BladeShape::CircularArc | BladeShape::Straight) {
                value_info(
                    ui,
                    tr("Blade shape"),
                    tr(
                        "The circular-arc and straight shapes fix β from the leading to the trailing edge; the distribution below does not apply",
                    ),
                    blade_shape_name(shape),
                );
            }
            label(ui, tr("Distribution"), "");
            let smooth = matches!(input.mean_lines.beta, BetaDistribution::Smooth { .. });
            ComboBox::from_id_salt("beta_kind")
                .selected_text(tr(if smooth { "smooth (Bézier)" } else { "linear" }))
                .show_ui(ui, |ui| {
                    if ui.selectable_label(!smooth, tr("linear")).clicked() && smooth {
                        input.mean_lines.beta = BetaDistribution::Linear;
                        changed = true;
                    }
                    if ui.selectable_label(smooth, tr("smooth (Bézier)")).clicked() && !smooth {
                        input.mean_lines.beta = BetaDistribution::Smooth {
                            inlet: 0.3,
                            outlet: 0.3,
                        };
                        changed = true;
                    }
                });
            ui.end_row();
            if let BetaDistribution::Smooth { inlet, outlet } = &mut input.mean_lines.beta {
                label(
                    ui,
                    tr("Inlet hold"),
                    tr("How long β stays near its inlet value"),
                );
                changed |= ui
                    .add(DragValue::new(inlet).range(0.0..=1.0).speed(0.005))
                    .changed();
                ui.end_row();
                label(
                    ui,
                    tr("Outlet hold"),
                    tr("How long β stays near its outlet value"),
                );
                changed |= ui
                    .add(DragValue::new(outlet).range(0.0..=1.0).speed(0.005))
                    .changed();
                ui.end_row();
            }
        });
        if input.properties.shape == BladeShape::FreeForm && section(ui, tr("Wrap angle")) {
            props(ui, "wrap", |ui| {
                label(
                    ui,
                    tr("Sections"),
                    tr(
                        "Common: every section wraps the same angle, so the leading edge has no tangential sweep; each keeps its edge angles and a bump in its β distribution takes it there. Linear: from the hub wrap to the shroud wrap with the flow fraction. Free: each wraps what its β distribution gives",
                    ),
                );
                ComboBox::from_id_salt("wrap_mode")
                    .selected_text(match input.mean_lines.wrap {
                        WrapMode::Common => tr("common wrap"),
                        WrapMode::Linear => tr("linear hub to shroud"),
                        WrapMode::Free => tr("each its own"),
                    })
                    .show_ui(ui, |ui| {
                        for (mode, text) in [
                            (WrapMode::Common, tr("common wrap")),
                            (WrapMode::Linear, tr("linear hub to shroud")),
                            (WrapMode::Free, tr("each its own")),
                        ] {
                            changed |= ui
                                .selectable_value(&mut input.mean_lines.wrap, mode, text)
                                .changed();
                        }
                    });
                ui.end_row();
                if input.mean_lines.wrap == WrapMode::Common {
                    changed |= param_f64(
                        ui,
                        tr("Wrap angle φ"),
                        tr(
                            "Shared by every section; automatic: the mean of the wraps their own β distributions give",
                        ),
                        &mut input.mean_lines.wrap_angle,
                        QuantityKind::Angle,
                        g.map(|g| g.blade.sections[0].wrap_angle()),
                        &units,
                        20f64.to_radians()..=360f64.to_radians(),
                    );
                }
                if input.mean_lines.wrap == WrapMode::Linear {
                    changed |= param_f64(
                        ui,
                        tr("Hub wrap φ_hub"),
                        tr("Automatic: what the hub section's own β distribution gives"),
                        &mut input.mean_lines.wrap_angle,
                        QuantityKind::Angle,
                        g.map(|g| g.blade.sections[0].wrap_angle()),
                        &units,
                        20f64.to_radians()..=360f64.to_radians(),
                    );
                    changed |= param_f64(
                        ui,
                        tr("Shroud wrap φ_shroud"),
                        tr("Automatic: what the shroud section's own β distribution gives"),
                        &mut input.mean_lines.wrap_angle_shroud,
                        QuantityKind::Angle,
                        g.and_then(|g| g.blade.sections.last())
                            .map(|s| s.wrap_angle()),
                        &units,
                        20f64.to_radians()..=360f64.to_radians(),
                    );
                }
            });
        }
    }
    if section(ui, tr("Resolution")) {
        props(ui, "points", |ui| {
            changed |= param_u32(
                ui,
                tr("Points per section"),
                tr("From leading to trailing edge"),
                &mut input.mean_lines.points,
                Some(g.map_or(DEFAULT_POINTS, |g| g.blade.sections[0].m.len() as u32)),
                11..=201,
            );
        });
    }
    if changed {
        edit_geometry(cx, id, "edit blade mean lines", input);
    }
    if let Some(g) = g
        && section(ui, tr("Sections"))
    {
        Grid::new("sections")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                for h in ["span", "β₁B", "wrap", "length"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for s in &g.blade.sections {
                    ui.label(format!("{:.2}", s.span));
                    ui.label(units.show(QuantityKind::Angle, s.leading_edge.blade_angle));
                    ui.label(units.show(QuantityKind::Angle, s.wrap_angle()));
                    ui.label(units.show(QuantityKind::Length, s.mean_line_length()));
                    ui.end_row();
                }
            });
    }
}

fn profiles(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let units = cx.units();
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let base = cx.derived.geometry(id).map(|g| g.basis.blade_thickness);
    let mut changed = false;
    if section(ui, tr("Thickness")) {
        props(ui, "profiles", |ui| {
            let leading = input.profiles.thickness_leading.manual_value().or(base);
            changed |= param_f64(
                ui,
                tr("At the leading edge"),
                tr("Default: the blade thickness of the main dimensions"),
                &mut input.profiles.thickness_leading,
                QuantityKind::Length,
                leading,
                &units,
                0.0002..=0.1,
            );
            let trailing = input.profiles.thickness_trailing.manual_value().or(base);
            changed |= param_f64(
                ui,
                tr("At the trailing edge"),
                tr("Default: the blade thickness of the main dimensions"),
                &mut input.profiles.thickness_trailing,
                QuantityKind::Length,
                trailing,
                &units,
                0.0002..=0.1,
            );
        });
    }
    if changed {
        edit_geometry(cx, id, "edit blade profiles", input);
    }
}

/// The name of an edge shape in the panel.
fn edge_shape_name(shape: &EdgeShape) -> &'static str {
    tr(match shape {
        EdgeShape::Cut => "blunt (cut)",
        EdgeShape::Ellipse { .. } => "elliptic",
        EdgeShape::Taper { .. } => "taper and rounding",
        EdgeShape::Bezier { .. } => "Bézier",
    })
}

/// One edge-shape parameter in blade thicknesses (or a plain ratio with `suffix`).
fn edge_value(
    ui: &mut Ui,
    name: &str,
    hint: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> bool {
    label(ui, name, hint);
    let changed = ui
        .add(
            DragValue::new(value)
                .range(range)
                .speed(0.01)
                .max_decimals(3)
                .suffix(suffix),
        )
        .changed();
    ui.end_row();
    changed
}

fn edge_shape(ui: &mut Ui, salt: &str, shape: &mut EdgeShape) -> bool {
    let mut changed = false;
    ComboBox::from_id_salt(salt)
        .selected_text(edge_shape_name(shape))
        .show_ui(ui, |ui| {
            for option in [
                EdgeShape::Ellipse { ratio: 4.0 },
                EdgeShape::TAPER,
                EdgeShape::BEZIER,
                EdgeShape::Cut,
            ] {
                let selected = shape.same_kind(&option);
                if ui
                    .selectable_label(selected, edge_shape_name(&option))
                    .clicked()
                    && !selected
                {
                    *shape = option;
                    changed = true;
                }
            }
        });
    ui.end_row();
    let thick = tr(" e");
    match shape {
        EdgeShape::Cut => {}
        EdgeShape::Ellipse { ratio } => {
            changed |= edge_value(
                ui,
                tr("Nose ratio"),
                tr("Length of the nose over the half thickness (1 = round)"),
                ratio,
                1.0..=8.0,
                " : 1",
            );
        }
        EdgeShape::Taper {
            length,
            nose,
            ratio,
            asymmetry,
        } => {
            changed |= edge_value(
                ui,
                tr("Taper length"),
                tr("From full thickness to the edge, in blade thicknesses e"),
                length,
                0.2..=20.0,
                thick,
            );
            changed |= edge_value(
                ui,
                tr("Nose length"),
                tr("Length of the rounding at the end of the taper, in e"),
                nose,
                0.05..=(*length - 0.01).max(0.05),
                thick,
            );
            changed |= edge_value(
                ui,
                tr("Nose ratio"),
                tr(
                    "Length of the rounding over its half width; the rounding is tangent \
                     to the taper",
                ),
                ratio,
                (2.0 * *nose + 0.01)..=20.0,
                " : 1",
            );
            changed |= edge_value(
                ui,
                tr("Asymmetry"),
                tr(
                    "Moves the tip toward the pressure face (+) or the suction face (−), \
                     as a fraction of the half width of the nose",
                ),
                asymmetry,
                -0.95..=0.95,
                "",
            );
        }
        EdgeShape::Bezier {
            start,
            pull,
            tip,
            symmetric,
        } => {
            label(
                ui,
                tr("Symmetric"),
                tr("The same on both faces, the tip on the mean line"),
            );
            changed |= ui.checkbox(symmetric, "").changed();
            ui.end_row();
            if *symmetric {
                changed |= edge_value(
                    ui,
                    tr("Length"),
                    tr("Where the nose meets the faces, from the edge, in e"),
                    &mut start[0],
                    0.1..=10.0,
                    thick,
                );
                changed |= edge_value(
                    ui,
                    tr("Pull"),
                    tr("How far points 1 and 3 reach toward the edge (0 to 1)"),
                    &mut pull[0],
                    0.0..=1.0,
                    "",
                );
            } else {
                changed |= edge_value(
                    ui,
                    tr("Length on the pressure face"),
                    tr("Where the nose meets the pressure face, from the edge, in e"),
                    &mut start[0],
                    0.1..=10.0,
                    thick,
                );
                changed |= edge_value(
                    ui,
                    tr("Length on the suction face"),
                    tr("Where the nose meets the suction face, from the edge, in e"),
                    &mut start[1],
                    0.1..=10.0,
                    thick,
                );
                changed |= edge_value(
                    ui,
                    tr("Pull on the pressure face"),
                    tr("How far point 1 reaches toward the edge (0 to 1)"),
                    &mut pull[0],
                    0.0..=1.0,
                    "",
                );
                changed |= edge_value(
                    ui,
                    tr("Pull on the suction face"),
                    tr("How far point 3 reaches toward the edge (0 to 1)"),
                    &mut pull[1],
                    0.0..=1.0,
                    "",
                );
                changed |= edge_value(
                    ui,
                    tr("Tip offset"),
                    tr("Point 2 off the mean line, in half thicknesses (+ pressure face)"),
                    tip,
                    -0.95..=0.95,
                    "",
                );
            }
        }
    }
    changed
}

/// One edge: hub to shroud mode, then the shape at the hub (or on every span) and, when
/// it varies, at the shroud.
fn edge_rows(ui: &mut Ui, salt: &str, hub: &mut EdgeShape, shroud: &mut Option<EdgeShape>) -> bool {
    let mut changed = false;
    label(
        ui,
        tr("Hub to shroud"),
        tr(
            "The same shape on every span, or one at the hub and one at the shroud with the spans between blended linearly",
        ),
    );
    let mut linear = shroud.is_some();
    ComboBox::from_id_salt((salt, "mode"))
        .selected_text(tr(if linear { "linear" } else { "uniform" }))
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut linear, false, tr("uniform"))
                .changed();
            changed |= ui
                .selectable_value(&mut linear, true, tr("linear"))
                .changed();
        });
    ui.end_row();
    if linear != shroud.is_some() {
        *shroud = linear.then_some(*hub);
    }
    if let Some(shroud) = shroud {
        label(ui, tr("At the hub"), "");
        changed |= edge_shape(ui, &format!("{salt}_hub"), hub);
        label(ui, tr("At the shroud"), "");
        changed |= edge_shape(ui, &format!("{salt}_shroud"), shroud);
        // Blending needs one kind of shape: the shroud follows a change at the hub.
        if !shroud.same_kind(hub) {
            *shroud = *hub;
            changed = true;
        }
    } else {
        label(ui, tr("Shape"), "");
        changed |= edge_shape(ui, salt, hub);
    }
    changed
}

fn edges(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let mut changed = false;
    let e = &mut input.edges;
    if section(ui, tr("Leading edge")) {
        props(ui, "leading_edge_shape_rows", |ui| {
            changed |= edge_rows(
                ui,
                "leading_edge_shape",
                &mut e.leading,
                &mut e.leading_shroud,
            );
        });
    }
    if section(ui, tr("Trailing edge")) {
        props(ui, "trailing_edge_shape_rows", |ui| {
            changed |= edge_rows(
                ui,
                "trailing_edge_shape",
                &mut e.trailing,
                &mut e.trailing_shroud,
            );
        });
    }
    if section_kind(ui, tr("Results"), "", Kind::Results) {
        props(ui, "edges_results", |ui| {
            let spans = [
                (tr("c_p,min of the leading edge"), Some(e.leading)),
                (tr("c_p,min at the shroud"), e.leading_shroud),
            ];
            for (name, shape) in spans {
                let Some(shape) = shape else { continue };
                match shape.min_pressure_coefficient() {
                    Some(cp) => value_info(
                        ui,
                        name,
                        tr(
                            "At shockless entry (Gülich eq. 7.1a, for elliptic noses; an \
                             estimate with the nose length for the other shapes)",
                        ),
                        format!("{cp:.2}"),
                    ),
                    None => value_info(
                        ui,
                        name,
                        tr("A blunt leading edge causes a strong local pressure drop."),
                        "—",
                    ),
                }
            }
        });
    }
    if changed {
        edit_geometry(cx, id, "edit blade edges", input);
    }
}

/// Whether the design steps a CAD model needs are done.
pub(crate) fn ready_for_cad(component: &tp_project::Component) -> bool {
    let done = |steps: &[DesignStep]| {
        steps
            .iter()
            .all(|&s| component.status(s) == StepStatus::Done)
    };
    match component.kind() {
        ComponentKind::Volute => done(&VOLUTE_STEPS),
        ComponentKind::Stator { vaned: true } => done(&VANED_STEPS),
        ComponentKind::Stator { vaned: false } => done(&VANELESS_STEPS),
        ComponentKind::Impeller => component.status(DesignStep::BladeEdges) == StepStatus::Done,
    }
}

fn model(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let Some(component) = project.component(id) else {
        return;
    };
    let volute = component.kind() == ComponentKind::Volute;
    let ready = ready_for_cad(component);
    if !crate::export::CAD_AVAILABLE {
        ui.label(tr(
            "This build has no CAD support (built without the `cad` feature).",
        ));
        return;
    }
    if !ready {
        ui.label(tr(if volute {
            "Accept the volute steps up to the wall thickness before exporting."
        } else {
            "Accept the design steps before exporting."
        }));
    }
    if ui
        .add_enabled(ready, egui::Button::new(tr("Export CAD solids…")))
        .clicked()
    {
        cx.exports.open_cad();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impeller_and_volute_steps_are_available() {
        assert!(is_available(
            ComponentKind::Impeller,
            DesignStep::MainDimensions
        ));
        assert!(is_available(
            ComponentKind::Impeller,
            DesignStep::BladeEdges
        ));
        assert!(!is_available(ComponentKind::Impeller, DesignStep::CfdSetup));
        assert!(is_available(ComponentKind::Volute, DesignStep::VoluteSetup));
        assert!(is_available(ComponentKind::Volute, DesignStep::CutWater));
        assert!(!is_available(ComponentKind::Volute, DesignStep::CfdSetup));
        assert!(planned_phase(ComponentKind::Volute, DesignStep::CfdSetup).contains("phase 8"));
        assert!(is_available(
            ComponentKind::Stator { vaned: true },
            DesignStep::BladeEdges
        ));
        assert!(!is_available(
            ComponentKind::Stator { vaned: false },
            DesignStep::BladeProperties
        ));
    }
}
