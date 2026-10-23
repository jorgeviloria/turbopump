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
    match kind {
        ComponentKind::Impeller => {
            step == DesignStep::MainDimensions || tp_project::GEOMETRY_STEPS.contains(&step)
        }
        ComponentKind::Volute => VOLUTE_STEPS.contains(&step),
        ComponentKind::Stator { vaned: true } => VANED_STEPS.contains(&step),
        ComponentKind::Stator { vaned: false } => VANELESS_STEPS.contains(&step),
    }
}

/// Title of the dialog for `selection`: icon, component and step.
pub(crate) fn title(project: &Project, selection: Selection) -> String {
    match selection {
        Selection::Setup => format!("{}  {}", icon::GEAR, tr("Global setup")),
        Selection::Step(id, step) => {
            let name = project.component(id).map_or("", |c| c.name.as_str());
            format!(
                "{}  {} · {}",
                icons::step(step),
                component_name(name),
                step_title(step)
            )
        }
    }
}

/// The inputs and results of the selected step, or of the global setup.
pub(crate) fn show(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    match *cx.selection {
        Selection::Setup => setup(ui, cx),
        Selection::Step(id, step) => step_panel(ui, cx, id, step),
    }
}

/// The buttons under the step: back and next.
pub(crate) fn footer(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    let p = Palette::of(ui);
    let Selection::Step(id, step) = *cx.selection else {
        ui.label(
            RichText::new(tr(
                "Shared by every component. Changing it makes every step outdated.",
            ))
            .color(p.weak),
        );
        return;
    };
    let project = cx.doc.project().clone();
    let Some(kind) = project.component(id).map(|c| c.kind()) else {
        return;
    };
    let (back, next) = neighbours(kind, step);
    let target = ui.horizontal(|ui| navigation(ui, back, next)).inner;
    if let Some(target) = target {
        *cx.selection = Selection::Step(id, target);
    }
}

fn next_text() -> String {
    format!("{} {}", tr("Next"), icon::CARET_RIGHT)
}

fn back_text() -> String {
    format!("{} {}", icon::CARET_LEFT, tr("Back"))
}

fn accept_text() -> String {
    format!("{}  {}", icon::CHECK, tr("Accept step"))
}

fn reset_text() -> String {
    format!(
        "{}  {}",
        icon::ARROW_COUNTER_CLOCKWISE,
        tr("Reset to automatic")
    )
}

/// Width the accept and reset buttons take.
fn buttons_width(ui: &Ui) -> f32 {
    let text_width = |text: String| {
        egui::WidgetText::from(text)
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Button,
            )
            .size()
            .x
    };
    let spacing = ui.spacing();
    text_width(accept_text())
        + text_width(reset_text())
        + 4.0 * spacing.button_padding.x
        + 2.0 * spacing.item_spacing.x
}

/// The back and next buttons, right-aligned. Returns the step clicked.
fn navigation(
    ui: &mut Ui,
    back: Option<DesignStep>,
    next: Option<DesignStep>,
) -> Option<DesignStep> {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let next_clicked = ui
            .add_enabled(next.is_some(), egui::Button::new(next_text()))
            .clicked();
        let back_clicked = ui
            .add_enabled(back.is_some(), egui::Button::new(back_text()))
            .clicked();
        if next_clicked {
            next
        } else if back_clicked {
            back
        } else {
            None
        }
    })
    .inner
}

/// Heading of a group of inputs, with a rule after it.
pub(crate) fn section(ui: &mut Ui, title: &str) {
    section_info(ui, title, "");
}

/// Heading of a group of inputs with an ⓘ for `hint`, and a rule after it.
pub(crate) fn section_info(ui: &mut Ui, title: &str, hint: &str) {
    let p = Palette::of(ui);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(bold(title).size(13.0));
        if !hint.is_empty() {
            widgets::info(ui, hint);
        }
        let rect = ui.available_rect_before_wrap();
        ui.painter().hline(
            (rect.left() + 4.0)..=rect.right(),
            rect.center().y,
            egui::Stroke::new(1.0, p.border),
        );
    });
    ui.add_space(2.0);
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
    let project = cx.doc.project().clone();
    let units = cx.units();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            section(ui, tr("Project"));
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

            let mut setup = project.setup().clone();
            let mut changed = false;
            section(ui, tr("Design point"));
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

            section(ui, tr("Fluid and inlet conditions"));
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
            if changed {
                cx.edit("edit global setup", |p| p.set_setup(setup));
            }

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
                            units.show(QuantityKind::KinematicViscosity, l.kinematic_viscosity()),
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
        });
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
    // Accept and reset sit on the right of the status when they fit, else just below.
    let actions = accept_state(&project, cx.derived, id, step) != AcceptState::NotApplicable;
    let placed = ui.horizontal(|ui| {
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
        if let Some(text) = intro(component.kind(), step) {
            widgets::info(ui, text);
        }
        let fits = actions && ui.available_width() >= buttons_width(ui) + 8.0;
        if fits {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                buttons(ui, cx, &project, id, step);
            });
        }
        fits
    });
    if actions && !placed.inner {
        ui.horizontal(|ui| buttons(ui, cx, &project, id, step));
    }

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
                        problems(ui, cx, id, step);
                        return;
                    }
                    ComponentKind::Stator { .. } => {
                        crate::diffuser_steps::show(ui, cx, &project, id, step);
                        problems(ui, cx, id, step);
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
            problems(ui, cx, id, step);
        });
}

/// Accept and reset, for the steps that can be accepted.
fn buttons(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    project: &Project,
    id: ComponentId,
    step: DesignStep,
) {
    let p = Palette::of(ui);
    let state = accept_state(project, cx.derived, id, step);
    if state == AcceptState::NotApplicable {
        return;
    }
    let ready = state == AcceptState::Ready;
    let text = accept_text();
    let button = if ready {
        egui::Button::new(RichText::new(text).color(p.on_accent)).fill(p.accent)
    } else {
        egui::Button::new(text)
    };
    let reason = match state {
        AcceptState::Blocked(reason) => reason,
        _ => "",
    };
    if ui
        .add_enabled(ready, button)
        .on_hover_text(tr("Mark this step as done with the current values"))
        .on_disabled_hover_text(tr(reason))
        .clicked()
    {
        accept(cx, id, step);
    }
    if ui
        .button(reset_text())
        .on_hover_text(tr(
            "Return every value of this step to its automatic proposal",
        ))
        .clicked()
    {
        reset(cx, project, id, step);
    }
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

fn show_problems(ui: &mut Ui, error: Option<&String>, warnings: &[String]) {
    if error.is_none() && warnings.is_empty() {
        return;
    }
    ui.add_space(10.0);
    if let Some(e) = error {
        messages::card(ui, Severity::Error, e);
        ui.add_space(3.0);
    }
    for w in warnings {
        messages::card(ui, Severity::Warning, w);
        ui.add_space(3.0);
    }
}

fn main_dimensions(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    use QuantityKind::*;
    let units = cx.units();
    let mut input = project.pump_impeller_input(id).unwrap_or_default();
    let design = cx.derived.main(id).cloned();
    let d = design.as_ref();
    let mut changed = false;

    section(ui, tr("Impeller type"));
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

    section(ui, tr("Efficiencies"));
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

    section(ui, tr("Dimensions"));
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
        if d.is_some_and(|d| d.semi_axial) {
            changed |= param_f64(
                ui,
                tr("Outlet hub diameter d₂i"),
                tr("Hub end of the outlet edge; d₂ is the shroud end d₂a (Gülich eqs. 7.13–7.14)"),
                &mut input.outlet_hub_diameter,
                Length,
                d.map(|d| d.outlet_hub_diameter),
                &units,
                0.005..=5.0,
            );
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

    section(ui, tr("Inlet and cavitation"));
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

    if changed {
        cx.edit("edit main dimensions", |p| {
            p.set_main_dimensions(id, MainDimensionsInput::CentrifugalPump(input))
        });
    }

    if let Some(d) = d {
        section(ui, tr("Results"));
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
    section(ui, tr("Contour"));
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
                tr("A_rel along the center line, 0 at the inlet section and 1 at the outlet one"),
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
                    tr("Length of the Bézier tangent as a fraction of the distance to the corner"),
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
    section(ui, tr("Leading edge"));
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
    section_info(
        ui,
        tr("Material"),
        tr(
            "The closed contours of the hub disk and the shroud cover, for the CAD model: \
             automatic from these values, or drawn on the Materials page of the editor",
        ),
    );
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
    section_info(
        ui,
        tr("Side chambers"),
        tr(
            "Between the impeller and the casing: the front one down to the annular seal on \
             the neck, the rear one down to the shaft seal. Their values follow the loss \
             analysis but are not shared with it",
        ),
    );
    props(ui, "chambers", |ui| {
        let built = g.map(|g| g.contour.chambers.values);
        let chambers = &mut m.chambers;
        label(
            ui,
            tr("Design the side chambers"),
            tr(
                "Export their fluid with the CAD model; shape them on the Side chambers page of the editor",
            ),
        );
        changed |= ui.checkbox(&mut chambers.enabled, "").changed();
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
                tr("Drawn on the Side chambers page of the editor; the gap above does not apply"),
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
    if changed {
        edit_geometry(cx, id, "edit meridional contour", input);
    }
    if let Some(g) = g {
        let qos = &g.channel.quasi_orthogonals;
        section(ui, tr("Meridional flow"));
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
        section_info(
            ui,
            tr("Check against Gülich"),
            tr(
                "Recommendations for the meridional section of radial impellers: Gülich, \
                 Centrifugal Pumps, chap. 7.2.2.1 and eq. 7.2",
            ),
        );
        crate::views::meridian_editor::guide_table(ui, g, &units);
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
    section(ui, tr("Sections"));
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
    if changed {
        edit_geometry(cx, id, "edit blade properties", input);
    }
    if let Some(g) = g {
        section_info(ui, tr("Blade loading"), tr("Gülich eqs. 7.1.10–11"));
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
    section(ui, tr("Blade angle distribution"));
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
    if input.properties.shape == BladeShape::FreeForm {
        section(ui, tr("Wrap angle"));
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
    section(ui, tr("Resolution"));
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
    if changed {
        edit_geometry(cx, id, "edit blade mean lines", input);
    }
    if let Some(g) = g {
        section(ui, tr("Sections"));
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
    section(ui, tr("Thickness"));
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
    if changed {
        edit_geometry(cx, id, "edit blade profiles", input);
    }
}

fn edge_shape(ui: &mut Ui, salt: &str, shape: &mut EdgeShape) -> bool {
    let mut changed = false;
    let ellipse = matches!(shape, EdgeShape::Ellipse { .. });
    ComboBox::from_id_salt(salt)
        .selected_text(tr(if ellipse { "elliptic" } else { "blunt (cut)" }))
        .show_ui(ui, |ui| {
            if ui.selectable_label(ellipse, tr("elliptic")).clicked() && !ellipse {
                *shape = EdgeShape::Ellipse { ratio: 4.0 };
                changed = true;
            }
            if ui.selectable_label(!ellipse, tr("blunt (cut)")).clicked() && ellipse {
                *shape = EdgeShape::Cut;
                changed = true;
            }
        });
    ui.end_row();
    if let EdgeShape::Ellipse { ratio } = shape {
        label(
            ui,
            tr("Nose ratio"),
            tr("Length of the nose over the half thickness (1 = round)"),
        );
        changed |= ui
            .add(
                DragValue::new(ratio)
                    .range(1.0..=8.0)
                    .speed(0.02)
                    .suffix(" : 1"),
            )
            .changed();
        ui.end_row();
    }
    changed
}

fn edges(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let mut changed = false;
    section(ui, tr("Edge shapes"));
    props(ui, "edges", |ui| {
        label(ui, tr("Leading edge"), "");
        changed |= edge_shape(ui, "leading_edge_shape", &mut input.edges.leading);
        label(ui, tr("Trailing edge"), "");
        changed |= edge_shape(ui, "trailing_edge_shape", &mut input.edges.trailing);
    });
    section(ui, tr("Results"));
    props(ui, "edges_results", |ui| {
        match input.edges.leading.min_pressure_coefficient() {
            Some(cp) => value_info(
                ui,
                tr("c_p,min of the leading edge"),
                tr("At shockless entry (Gülich eq. 7.1a)"),
                format!("{cp:.2}"),
            ),
            None => value_info(
                ui,
                tr("c_p,min of the leading edge"),
                tr("A blunt leading edge causes a strong local pressure drop."),
                "—",
            ),
        }
    });
    if changed {
        edit_geometry(cx, id, "edit blade edges", input);
    }
}

fn model(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let Some(component) = project.component(id) else {
        return;
    };
    let kind = component.kind();
    let volute = kind == ComponentKind::Volute;
    let done = |steps: &[DesignStep]| {
        steps
            .iter()
            .all(|&s| component.status(s) == StepStatus::Done)
    };
    let ready = match kind {
        ComponentKind::Volute => done(&VOLUTE_STEPS),
        ComponentKind::Stator { vaned: true } => done(&VANED_STEPS),
        ComponentKind::Stator { vaned: false } => done(&VANELESS_STEPS),
        ComponentKind::Impeller => component.status(DesignStep::BladeEdges) == StepStatus::Done,
    };
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
