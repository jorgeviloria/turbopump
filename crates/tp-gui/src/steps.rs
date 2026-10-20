//! Step panel: the inputs of the selected design step, its results and its problems.

use eframe::egui::{self, ComboBox, DragValue, Grid, RichText, Ui};
use tp_design::MainDimensionsInput;
use tp_design::impeller::blade::{BetaDistribution, DEFAULT_POINTS, DEFAULT_SPANS, EdgeShape};
use tp_design::impeller::meridian::{
    DEFAULT_LEADING_EDGE_HUB, DEFAULT_LEADING_EDGE_SHROUD, DEFAULT_TANGENT,
};
use tp_design::impeller::{ImpellerGeometryInput, meridian::MeridionalInput};
use tp_design::pump::correlations as corr;
use tp_design::pump::{CentrifugalPumpInput, InletDesign, Suction};
use tp_project::{
    ComponentId, ComponentKind, DesignStep, EnergyTransfer, FlowRate, Param, Project, Rotation,
    StepStatus,
};
use tp_units::{DisplayUnits, QuantityKind};

use crate::app::{EditCtx, Selection, status_color};
use crate::tree::{status_marker, status_text, step_title};
use crate::widgets::{self, param_f64, param_u32, value_row};

/// Steps the editor can work on today.
pub(crate) fn is_available(kind: ComponentKind, step: DesignStep) -> bool {
    use DesignStep::*;
    kind == ComponentKind::Impeller
        && matches!(
            step,
            MainDimensions
                | MeridionalContour
                | BladeProperties
                | BladeMeanLines
                | BladeProfiles
                | BladeEdges
                | ModelSettings
        )
}

/// Why a step is not available yet.
pub(crate) fn planned_phase(kind: ComponentKind, step: DesignStep) -> String {
    match (kind, step) {
        (ComponentKind::Impeller, DesignStep::CfdSetup) => {
            "CFD setup is planned for phase 8 (meshing and CFD).".into()
        }
        (ComponentKind::Impeller, DesignStep::ModelFinishing) => {
            "Model finishing (fillets, trimming, balance holes) is planned for phase 9.".into()
        }
        (ComponentKind::Impeller, _) => String::new(),
        _ => format!(
            "The design of a {} is planned for phase 5.",
            kind.label().to_lowercase()
        ),
    }
}

/// Steps that can be accepted (marked done) from the editor.
fn can_accept(step: DesignStep) -> bool {
    step == DesignStep::MainDimensions || tp_project::GEOMETRY_STEPS.contains(&step)
}

pub(crate) fn show(ui: &mut Ui, cx: &mut EditCtx<'_>) {
    match *cx.selection {
        Selection::Setup => setup(ui, cx),
        Selection::Step(id, step) => step_panel(ui, cx, id, step),
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong());
    ui.add_space(2.0);
}

fn grid(id: &str) -> Grid {
    Grid::new(id)
        .num_columns(3)
        .spacing([10.0, 4.0])
        .striped(false)
}

// ---------------------------------------------------------------------------------------
// Global setup

const COMMON_FLUIDS: &[&str] = &[
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
    ui.add_space(4.0);
    ui.heading("Global setup");
    ui.weak("Shared by every component. Changing it makes every step outdated.");
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        section(ui, "Project");
        let mut info = project.info.clone();
        let mut renamed = false;
        Grid::new("project_info").num_columns(2).show(ui, |ui| {
            ui.label("Name");
            renamed |= ui.text_edit_singleline(&mut info.name).changed();
            ui.end_row();
            ui.label("Author");
            renamed |= ui.text_edit_singleline(&mut info.author).changed();
            ui.end_row();
            ui.label("Machine");
            ui.label(project.machine().to_string());
            ui.end_row();
        });
        if renamed {
            cx.edit("edit project information", |p| {
                p.info = info;
                Ok(())
            });
        }

        let mut setup = project.setup().clone();
        let mut changed = false;
        section(ui, "Design point");
        Grid::new("design_point")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label("Flow rate");
                ui.horizontal(|ui| {
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
                        .selected_text(if mass { "mass" } else { "volume" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut is_mass, false, "volume");
                            ui.selectable_value(&mut is_mass, true, "mass");
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

                ui.label("Energy");
                ui.horizontal(|ui| {
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
                        .selected_text(if pressure { "pressure rise" } else { "head" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut is_pressure, false, "head");
                            ui.selectable_value(&mut is_pressure, true, "pressure rise");
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

                ui.label("Rotational speed");
                changed |= widgets::quantity(
                    ui,
                    &mut setup.design_point.speed,
                    QuantityKind::RotationalSpeed,
                    &units,
                    0.1..=1e5,
                );
                ui.end_row();

                ui.label("Rotation");
                ui.horizontal(|ui| {
                    changed |= ui
                        .radio_value(&mut setup.rotation, Rotation::Clockwise, "clockwise")
                        .changed();
                    changed |= ui
                        .radio_value(
                            &mut setup.rotation,
                            Rotation::CounterClockwise,
                            "counter-clockwise",
                        )
                        .changed();
                })
                .response
                .on_hover_text("Seen from the inlet, looking downstream");
                ui.end_row();
            });

        section(ui, "Fluid and inlet conditions");
        Grid::new("fluid")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label("Fluid");
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut setup.fluid).desired_width(120.0))
                        .changed();
                    ComboBox::from_id_salt("common_fluids")
                        .selected_text("common…")
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
                ui.label("Inlet total pressure");
                changed |= widgets::quantity(
                    ui,
                    &mut setup.inlet.total_pressure,
                    QuantityKind::Pressure,
                    &units,
                    1.0..=1e9,
                );
                ui.end_row();
                ui.label("Inlet total temperature");
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
                Grid::new("liquid").num_columns(2).show(ui, |ui| {
                    value_row(
                        ui,
                        "Density ρ",
                        units.show(QuantityKind::Density, l.density),
                    );
                    value_row(
                        ui,
                        "Kinematic viscosity ν",
                        units.show(QuantityKind::KinematicViscosity, l.kinematic_viscosity()),
                    );
                    value_row(
                        ui,
                        "Vapour pressure p_v",
                        units.show(QuantityKind::Pressure, l.vapor_pressure),
                    );
                    let source = match l.source {
                        tp_fluids::PropertySource::CoolProp => "CoolProp",
                        tp_fluids::PropertySource::BuiltinWater => "built-in water model (IAPWS)",
                    };
                    value_row(ui, "Source", source);
                });
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            None => {}
        }

        section(ui, "Units");
        let presets = [("SI", DisplayUnits::si()), ("US", DisplayUnits::us())];
        ui.horizontal(|ui| {
            for (label, preset) in presets {
                let selected = project.display_units == preset;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    cx.edit("change units", |p| {
                        p.display_units = preset;
                        Ok(())
                    });
                }
            }
        });
    });
}

// ---------------------------------------------------------------------------------------
// Design steps

fn step_panel(ui: &mut Ui, cx: &mut EditCtx<'_>, id: ComponentId, step: DesignStep) {
    let project = cx.doc.project().clone();
    let Some(component) = project.component(id) else {
        return;
    };
    let status = component.status(step);
    ui.add_space(4.0);
    ui.weak(&component.name);
    ui.heading(step_title(step));
    ui.horizontal(|ui| {
        ui.colored_label(
            status_color(ui, status),
            format!("{}  {}", status_marker(status), status_text(status)),
        );
    });

    if !is_available(component.kind(), step) {
        ui.separator();
        ui.label(planned_phase(component.kind(), step));
        return;
    }

    buttons(ui, cx, &project, id, step);
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
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

fn buttons(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    project: &Project,
    id: ComponentId,
    step: DesignStep,
) {
    let Some(component) = project.component(id) else {
        return;
    };
    let kind = component.kind();
    let steps: Vec<DesignStep> = kind
        .steps()
        .iter()
        .copied()
        .filter(|&s| is_available(kind, s))
        .collect();
    let index = steps.iter().position(|&s| s == step).unwrap_or(0);
    let previous_done = kind
        .steps()
        .iter()
        .take_while(|&&s| s != step)
        .all(|&s| component.status(s) == StepStatus::Done);
    let computed = match step {
        DesignStep::MainDimensions => cx.derived.main(id).is_some(),
        _ => cx.derived.geometry(id).is_some(),
    };

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if can_accept(step) {
            let done = component.status(step) == StepStatus::Done;
            let enabled = previous_done && computed && !done;
            let reason = if done {
                "This step is done."
            } else if !previous_done {
                "Complete the earlier steps first."
            } else {
                "Fix the errors below first."
            };
            if ui
                .add_enabled(enabled, egui::Button::new("Accept step"))
                .on_hover_text("Mark this step as done with the current values")
                .on_disabled_hover_text(reason)
                .clicked()
            {
                cx.doc.end_gesture();
                match cx.doc.edit(&format!("accept {step}"), |p| {
                    p.design_impeller_step(id, step)
                }) {
                    Ok(()) => {
                        cx.info(format!("{} accepted", step_title(step)));
                        if let Some(&next) = steps.get(index + 1) {
                            *cx.selection = Selection::Step(id, next);
                        }
                    }
                    Err(e) => cx.error(e.to_string()),
                }
            }
            if ui
                .button("Reset to automatic")
                .on_hover_text("Return every value of this step to its automatic proposal")
                .clicked()
            {
                reset(cx, project, id, step);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(index + 1 < steps.len(), egui::Button::new("Next ⏵"))
                .clicked()
            {
                *cx.selection = Selection::Step(id, steps[index + 1]);
            }
            if ui
                .add_enabled(index > 0, egui::Button::new("⏴ Back"))
                .clicked()
            {
                *cx.selection = Selection::Step(id, steps[index - 1]);
            }
        });
    });
}

fn reset(cx: &mut EditCtx<'_>, project: &Project, id: ComponentId, step: DesignStep) {
    cx.doc.end_gesture();
    let label = format!("reset {step}");
    if step == DesignStep::MainDimensions {
        let old = project.pump_impeller_input(id).unwrap_or_default();
        let input = CentrifugalPumpInput {
            suction: old.suction,
            inlet_design: old.inlet_design,
            ..CentrifugalPumpInput::default()
        };
        cx.edit(&label, |p| {
            p.component_mut(id)?
                .set_main_dimensions(MainDimensionsInput::CentrifugalPump(input))
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
    cx.edit(&label, |p| p.component_mut(id)?.set_geometry(input));
}

/// Errors and warnings of the computation behind `step`.
fn problems(ui: &mut Ui, cx: &EditCtx<'_>, id: ComponentId, step: DesignStep) {
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
    if error.is_none() && warnings.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.separator();
    if let Some(e) = error {
        ui.colored_label(ui.visuals().error_fg_color, format!("🗙 {e}"));
    }
    for w in warnings {
        ui.colored_label(ui.visuals().warn_fg_color, format!("⚑ {w}"));
    }
}

fn main_dimensions(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    use QuantityKind::*;
    let units = cx.units();
    let mut input = project.pump_impeller_input(id).unwrap_or_default();
    let design = cx.derived.main(id).cloned();
    let d = design.as_ref();
    let mut changed = false;

    ui.weak("One-dimensional design with Gülich's correlations. Values in automatic mode follow the design point.");
    section(ui, "Impeller type");
    Grid::new("impeller_type").num_columns(2).show(ui, |ui| {
        ui.label("Suction");
        ComboBox::from_id_salt("suction")
            .selected_text(match input.suction {
                Suction::Single => "single entry",
                Suction::Double => "double entry",
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut input.suction, Suction::Single, "single entry")
                    .changed();
                changed |= ui
                    .selectable_value(&mut input.suction, Suction::Double, "double entry")
                    .changed();
            });
        ui.end_row();
        ui.label("Inlet design");
        ComboBox::from_id_salt("inlet_design")
            .selected_text(match input.inlet_design {
                InletDesign::Normal => "normal impeller",
                InletDesign::Suction => "suction impeller (low NPSH)",
            })
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(
                        &mut input.inlet_design,
                        InletDesign::Normal,
                        "normal impeller",
                    )
                    .changed();
                changed |= ui
                    .selectable_value(
                        &mut input.inlet_design,
                        InletDesign::Suction,
                        "suction impeller (low NPSH)",
                    )
                    .changed();
            });
        ui.end_row();
    });

    section(ui, "Efficiencies");
    grid("efficiencies").show(ui, |ui| {
        changed |= param_f64(
            ui,
            "Hydraulic η_h",
            "Gülich Table 3.9 (m = 0.08)",
            &mut input.hydraulic_efficiency,
            Dimensionless,
            d.map(|d| d.efficiencies.hydraulic),
            &units,
            0.3..=0.99,
        );
        changed |= param_f64(
            ui,
            "Volumetric η_v",
            "Leakage through the seals",
            &mut input.volumetric_efficiency,
            Dimensionless,
            d.map(|d| d.efficiencies.volumetric),
            &units,
            0.5..=1.0,
        );
        changed |= param_f64(
            ui,
            "Overall η",
            "Gülich Table 3.9 (m = 0.1)",
            &mut input.overall_efficiency,
            Dimensionless,
            d.map(|d| d.efficiencies.overall),
            &units,
            0.2..=0.95,
        );
    });

    section(ui, "Dimensions");
    grid("dimensions").show(ui, |ui| {
        changed |= param_f64(
            ui,
            "Pressure coefficient ψ",
            "ψ = 2gH/u₂², Gülich eq. 3.26; ignored when d₂ is manual",
            &mut input.pressure_coefficient,
            Dimensionless,
            d.map(|d| d.pressure_coefficient),
            &units,
            0.3..=1.5,
        );
        changed |= param_f64(
            ui,
            "Outlet diameter d₂",
            "From ψ and the head",
            &mut input.outlet_diameter,
            Length,
            d.map(|d| d.outlet_diameter),
            &units,
            0.01..=5.0,
        );
        changed |= param_f64(
            ui,
            "Outlet width b₂",
            "Gülich eq. 7.1",
            &mut input.outlet_width,
            Length,
            d.map(|d| d.outlet_width),
            &units,
            0.0005..=1.0,
        );
        let factor = match input.inlet_diameter_factor {
            Param::Manual(v) => Some(v),
            Param::Auto => d.map(|d| corr::inlet_diameter_factor(input.inlet_design, d.nq)),
        };
        changed |= param_f64(
            ui,
            "Eye factor f_d1",
            "d₁ over the eye diameter of minimum w₁ (Gülich Table 7.1)",
            &mut input.inlet_diameter_factor,
            Dimensionless,
            factor,
            &units,
            0.8..=1.6,
        );
        changed |= param_f64(
            ui,
            "Eye diameter d₁",
            "Overrides the eye factor when manual",
            &mut input.inlet_diameter,
            Length,
            d.map(|d| d.inlet_diameter),
            &units,
            0.005..=3.0,
        );
        changed |= param_f64(
            ui,
            "Hub diameter d_n",
            "Hub diameter at the eye",
            &mut input.hub_diameter,
            Length,
            d.map(|d| d.hub_diameter),
            &units,
            0.0..=2.0,
        );
        changed |= param_f64(
            ui,
            "Shaft diameter",
            "From the torque and the allowable shear stress",
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
            "Shaft shear stress",
            "Allowable torsional stress",
            &mut input.shaft_shear_stress,
            Pressure,
            Some(tau),
            &units,
            1e6..=300e6,
        );
        changed |= param_u32(
            ui,
            "Blades z",
            "5 to 7 (Gülich)",
            &mut input.blade_count,
            d.map(|d| d.blade_count),
            3..=12,
        );
        changed |= param_f64(
            ui,
            "Blade thickness e",
            "Normal thickness at inlet and outlet",
            &mut input.blade_thickness,
            Length,
            d.map(|d| d.blade_thickness),
            &units,
            0.0003..=0.1,
        );
    });

    section(ui, "Inlet and cavitation");
    grid("inlet").show(ui, |ui| {
        let incidence = input.incidence.resolve(|| 0.0);
        changed |= param_f64(
            ui,
            "Incidence i",
            "Between blade and inflow at the leading edge (0°–4°)",
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
            "Suction specific speed nss",
            "rpm, m³/s, m; sets NPSH3",
            &mut input.suction_specific_speed,
            Dimensionless,
            Some(nss),
            &units,
            100.0..=400.0,
        );
    });

    if changed {
        cx.edit("edit main dimensions", |p| {
            p.component_mut(id)?
                .set_main_dimensions(MainDimensionsInput::CentrifugalPump(input))
        });
    }

    if let Some(d) = d {
        section(ui, "Results");
        Grid::new("main_results").num_columns(2).show(ui, |ui| {
            value_row(ui, "Specific speed nq", format!("{:.2}", d.nq));
            value_row(ui, "Shaft power", units.show(Power, d.stage_power));
            value_row(
                ui,
                "Outlet blade angle β₂B",
                units.show(Angle, d.outlet.blade_angle),
            );
            value_row(
                ui,
                "Inlet blade angle β₁B (mean)",
                units.show(Angle, d.inlet_mean.blade_angle),
            );
            value_row(ui, "Slip factor γ", format!("{:.3}", d.outlet.slip_factor));
            value_row(ui, "NPSH3", units.show(Head, d.npsh_required));
            value_row(ui, "NPSH available", units.show(Head, d.npsh_available));
        });
    }
}

/// Geometry inputs of `id` and the closure that stores them.
fn edit_geometry(cx: &mut EditCtx<'_>, id: ComponentId, label: &str, input: ImpellerGeometryInput) {
    cx.edit(label, |p| p.component_mut(id)?.set_geometry(input));
}

fn meridional(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    use QuantityKind::*;
    let units = cx.units();
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let g = cx.derived.geometry(id);
    let m: &mut MeridionalInput = &mut input.meridional;
    let mut changed = false;
    ui.weak("Hub and shroud are cubic Bézier curves. Drag their control points in the meridional view, or type the values here.");
    section(ui, "Contour");
    grid("contour").show(ui, |ui| {
        changed |= param_f64(
            ui,
            "Axial length L",
            "From the inlet plane to the hub at the outlet; default b₂ + (d₁ − d_n)/2",
            &mut m.axial_length,
            Length,
            g.map(|g| g.contour.axial_length),
            &units,
            0.001..=5.0,
        );
        let tangents: [(&str, &mut Param<f64>); 4] = [
            ("Hub inlet tangent", &mut m.hub_inlet_tangent),
            ("Hub outlet tangent", &mut m.hub_outlet_tangent),
            ("Shroud inlet tangent", &mut m.shroud_inlet_tangent),
            ("Shroud outlet tangent", &mut m.shroud_outlet_tangent),
        ];
        for (label, param) in tangents {
            changed |= param_f64(
                ui,
                label,
                "Length of the Bézier tangent as a fraction of the distance to the corner",
                param,
                Dimensionless,
                Some(DEFAULT_TANGENT),
                &units,
                0.05..=0.95,
            );
        }
    });
    section(ui, "Leading edge");
    grid("leading_edge").show(ui, |ui| {
        changed |= param_f64(
            ui,
            "On the hub",
            "Fraction of the hub length",
            &mut m.leading_edge_hub,
            Dimensionless,
            Some(g.map_or(DEFAULT_LEADING_EDGE_HUB, |g| g.contour.leading_edge.0)),
            &units,
            0.0..=0.9,
        );
        changed |= param_f64(
            ui,
            "On the shroud",
            "Fraction of the shroud length",
            &mut m.leading_edge_shroud,
            Dimensionless,
            Some(g.map_or(DEFAULT_LEADING_EDGE_SHROUD, |g| g.contour.leading_edge.1)),
            &units,
            0.0..=0.9,
        );
    });
    if changed {
        edit_geometry(cx, id, "edit meridional contour", input);
    }
    if let Some(g) = g {
        let qos = &g.channel.quasi_orthogonals;
        section(ui, "Meridional flow");
        Grid::new("meridional_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(ui, "Hub length", units.show(Length, g.contour.hub_length()));
                value_row(
                    ui,
                    "Shroud length",
                    units.show(Length, g.contour.shroud_length()),
                );
                value_row(
                    ui,
                    "c_m inlet / outlet",
                    format!(
                        "{} / {}",
                        units.show(Velocity, qos[0].cm),
                        units.show(Velocity, qos[qos.len() - 1].cm)
                    ),
                );
                value_row(
                    ui,
                    "Largest local rise of c_m",
                    format!("{:.1} %", g.channel.max_cm_rise() * 100.0),
                );
            });
    }
}

fn blade_properties(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let units = cx.units();
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let g = cx.derived.geometry(id);
    let mut changed = false;
    ui.weak("Blade sections are built on stream surfaces that split the flow into equal parts.");
    section(ui, "Sections");
    grid("properties").show(ui, |ui| {
        changed |= param_u32(
            ui,
            "Spans",
            "Blade sections from hub to shroud",
            &mut input.properties.spans,
            Some(g.map_or(DEFAULT_SPANS, |g| g.blade.sections.len() as u32)),
            2..=21,
        );
    });
    if changed {
        edit_geometry(cx, id, "edit blade properties", input);
    }
    if let Some(g) = g {
        section(ui, "Blade loading (Gülich eqs. 7.1.10–11)");
        let l = &g.blade_loading;
        Grid::new("loading").num_columns(2).show(ui, |ui| {
            value_row(ui, "Blades z", g.blade.count.to_string());
            value_row(
                ui,
                "Outlet blade angle β₂B",
                units.show(QuantityKind::Angle, g.blade.outlet_blade_angle),
            );
            value_row(ui, "ξ_eff", format!("{:.3}", l.effective));
            value_row(ui, "ξ_al", format!("{:.3}", l.allowable));
            value_row(ui, "ξ_eff / ξ_al", format!("{:.2} (0.85–1.15)", l.ratio()));
        });
    }
}

fn mean_lines(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let units = cx.units();
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let g = cx.derived.geometry(id);
    let mut changed = false;
    ui.weak("Conformal mapping dθ = dm/(r·tan β), stacked at the trailing edge.");
    section(ui, "Blade angle distribution");
    Grid::new("beta")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label("Distribution");
            let smooth = matches!(input.mean_lines.beta, BetaDistribution::Smooth { .. });
            ComboBox::from_id_salt("beta_kind")
                .selected_text(if smooth { "smooth (Bézier)" } else { "linear" })
                .show_ui(ui, |ui| {
                    if ui.selectable_label(!smooth, "linear").clicked() && smooth {
                        input.mean_lines.beta = BetaDistribution::Linear;
                        changed = true;
                    }
                    if ui.selectable_label(smooth, "smooth (Bézier)").clicked() && !smooth {
                        input.mean_lines.beta = BetaDistribution::Smooth {
                            inlet: 0.3,
                            outlet: 0.3,
                        };
                        changed = true;
                    }
                });
            ui.end_row();
            if let BetaDistribution::Smooth { inlet, outlet } = &mut input.mean_lines.beta {
                ui.label("Inlet hold")
                    .on_hover_text("How long β stays near its inlet value");
                changed |= ui
                    .add(DragValue::new(inlet).range(0.0..=1.0).speed(0.005))
                    .changed();
                ui.end_row();
                ui.label("Outlet hold")
                    .on_hover_text("How long β stays near its outlet value");
                changed |= ui
                    .add(DragValue::new(outlet).range(0.0..=1.0).speed(0.005))
                    .changed();
                ui.end_row();
            }
        });
    section(ui, "Resolution");
    grid("points").show(ui, |ui| {
        changed |= param_u32(
            ui,
            "Points per section",
            "From leading to trailing edge",
            &mut input.mean_lines.points,
            Some(g.map_or(DEFAULT_POINTS, |g| g.blade.sections[0].m.len() as u32)),
            11..=201,
        );
    });
    if changed {
        edit_geometry(cx, id, "edit blade mean lines", input);
    }
    if let Some(g) = g {
        section(ui, "Sections");
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
    ui.weak("Normal thickness, varying linearly from leading to trailing edge before the edges are shaped.");
    section(ui, "Thickness");
    grid("profiles").show(ui, |ui| {
        let leading = input.profiles.thickness_leading.manual_value().or(base);
        changed |= param_f64(
            ui,
            "At the leading edge",
            "Default: the blade thickness of the main dimensions",
            &mut input.profiles.thickness_leading,
            QuantityKind::Length,
            leading,
            &units,
            0.0002..=0.1,
        );
        let trailing = input.profiles.thickness_trailing.manual_value().or(base);
        changed |= param_f64(
            ui,
            "At the trailing edge",
            "Default: the blade thickness of the main dimensions",
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
        .selected_text(if ellipse { "elliptic" } else { "blunt (cut)" })
        .show_ui(ui, |ui| {
            if ui.selectable_label(ellipse, "elliptic").clicked() && !ellipse {
                *shape = EdgeShape::Ellipse { ratio: 4.0 };
                changed = true;
            }
            if ui.selectable_label(!ellipse, "blunt (cut)").clicked() && ellipse {
                *shape = EdgeShape::Cut;
                changed = true;
            }
        });
    if let EdgeShape::Ellipse { ratio } = shape {
        changed |= ui
            .add(
                DragValue::new(ratio)
                    .range(1.0..=8.0)
                    .speed(0.02)
                    .suffix(" : 1"),
            )
            .on_hover_text("Length of the nose over the half thickness (1 = round)")
            .changed();
    } else {
        ui.label("");
    }
    ui.end_row();
    changed
}

fn edges(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let mut input = project.impeller_geometry_input(id).unwrap_or_default();
    let mut changed = false;
    ui.weak(
        "A blunt trailing edge leaves material to under-file after casting (Gülich pp. 347–349).",
    );
    section(ui, "Edge shapes");
    grid("edges").show(ui, |ui| {
        ui.label("Leading edge");
        changed |= edge_shape(ui, "leading_edge_shape", &mut input.edges.leading);
        ui.label("Trailing edge");
        changed |= edge_shape(ui, "trailing_edge_shape", &mut input.edges.trailing);
    });
    match input.edges.leading.min_pressure_coefficient() {
        Some(cp) => {
            ui.label(format!(
                "c_p,min of the leading edge at shockless entry: {cp:.2} (Gülich eq. 7.1a)"
            ));
        }
        None => {
            ui.label("A blunt leading edge causes a strong local pressure drop.");
        }
    }
    if changed {
        edit_geometry(cx, id, "edit blade edges", input);
    }
}

fn model(ui: &mut Ui, cx: &mut EditCtx<'_>, project: &Project, id: ComponentId) {
    let ready = project
        .component(id)
        .is_some_and(|c| c.status(DesignStep::BladeEdges) == StepStatus::Done);
    ui.weak("CAD solids of the impeller and of its flow domain, built with Open CASCADE.");
    ui.add_space(8.0);
    if !crate::export::CAD_AVAILABLE {
        ui.label("This build has no CAD support (built without the `cad` feature).");
        return;
    }
    if !ready {
        ui.label("Accept the geometry steps up to the blade edges before exporting.");
    }
    if ui
        .add_enabled(ready, egui::Button::new("Export CAD solids…"))
        .clicked()
    {
        cx.exports.open_cad();
    }
    ui.add_space(8.0);
    ui.weak("CFD setup and model finishing (fillets, trimming, balance holes) arrive in phases 8 and 9.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_impeller_steps_of_this_phase_are_available() {
        assert!(is_available(
            ComponentKind::Impeller,
            DesignStep::MainDimensions
        ));
        assert!(is_available(
            ComponentKind::Impeller,
            DesignStep::BladeEdges
        ));
        assert!(!is_available(ComponentKind::Impeller, DesignStep::CfdSetup));
        assert!(!is_available(
            ComponentKind::Volute,
            DesignStep::VoluteSetup
        ));
        assert!(planned_phase(ComponentKind::Volute, DesignStep::CutWater).contains("phase 5"));
    }
}
