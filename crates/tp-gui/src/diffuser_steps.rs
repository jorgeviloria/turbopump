//! Step editors of a stator: main dimensions, side walls and, for a vaned diffuser, the
//! vane number and throat, mean lines, thickness and edges.

use eframe::egui::{ComboBox, DragValue, Grid, Ui};
use tp_design::diffuser::{DiffuserDesign, DiffuserInput};
use tp_design::impeller::blade::EdgeShape;
use tp_project::{ComponentId, DesignStep, Project};
use tp_units::QuantityKind;

use crate::app::EditCtx;
use crate::i18n::{tf, tr};
use crate::steps::{grid, section};
use crate::widgets::{param_f64, param_u32, value_row};

pub(crate) fn show(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    project: &Project,
    id: ComponentId,
    step: DesignStep,
) {
    let Ok(mut input) = project.diffuser_input(id) else {
        return;
    };
    let before = input.clone();
    let design = cx.derived.diffuser(id).cloned();
    let d = design.as_ref();
    let label = match step {
        DesignStep::MainDimensions => main(ui, cx, d, &mut input),
        DesignStep::MeridionalContour => contour(ui, cx, d, &mut input),
        DesignStep::BladeProperties => properties(ui, cx, d, &mut input),
        DesignStep::BladeMeanLines => mean_lines(ui, cx, d, &mut input),
        DesignStep::BladeProfiles => profiles(ui, cx, d, &mut input),
        DesignStep::BladeEdges => edges(ui, &mut input),
        _ => return,
    };
    if input != before {
        cx.edit(label, |p| p.set_diffuser(id, input));
    }
}

pub(crate) fn reset(
    cx: &mut EditCtx<'_>,
    project: &Project,
    id: ComponentId,
    step: DesignStep,
    label: &str,
) {
    let Ok(mut input) = project.diffuser_input(id) else {
        return;
    };
    let defaults = DiffuserInput::default();
    match step {
        DesignStep::MainDimensions => input.main = defaults.main,
        DesignStep::MeridionalContour => input.contour = defaults.contour,
        DesignStep::BladeProperties => input.properties = defaults.properties,
        DesignStep::BladeMeanLines => input.mean_lines = defaults.mean_lines,
        DesignStep::BladeProfiles => input.profiles = defaults.profiles,
        DesignStep::BladeEdges => input.edges = defaults.edges,
        _ => return,
    }
    cx.edit(label, |p| p.set_diffuser(id, input));
}

fn main(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    d: Option<&DiffuserDesign>,
    input: &mut DiffuserInput,
) -> &'static str {
    use QuantityKind::*;
    let units = cx.units();
    let vaned = d.is_none_or(|d| d.vanes.is_some());
    ui.weak(tr(if vaned {
        "Vaned diffuser after the impeller (Gülich 2nd ed., Chap. 7.9 and Table 7.7)."
    } else {
        "Vaneless ring after the impeller: the flow keeps c_u·r constant and slows down."
    }));
    section(ui, tr("Main dimensions"));
    grid("diffuser_main").show(ui, |ui| {
        let flow_factor = input.main.flow_factor.resolve(|| 1.0);
        param_f64(
            ui,
            tr("Flow factor Q_Le/Q"),
            tr("Allowance for leakage that flows through the diffuser (eq. T7.7.1)"),
            &mut input.main.flow_factor,
            Dimensionless,
            Some(flow_factor),
            &units,
            0.8..=1.5,
        );
        param_f64(
            ui,
            tr("Inlet diameter d₃"),
            tr(if vaned {
                "Leading edge of the vanes; automatic: the smallest gap of Gülich Table 10.2"
            } else {
                "Start of the ring; automatic: d₂"
            }),
            &mut input.main.inlet_diameter,
            Length,
            d.map(|d| d.inlet_diameter),
            &units,
            0.01..=10.0,
        );
        param_f64(
            ui,
            tr("Inlet width b₃"),
            tr("b₃/b₂ = 1.05 to 1.3 (Gülich p. 425)"),
            &mut input.main.inlet_width,
            Length,
            d.map(|d| d.inlet_width),
            &units,
            0.001..=1.0,
        );
        param_f64(
            ui,
            tr("Outlet diameter d₄"),
            tr(if vaned {
                "Automatic: the channel length for Gülich criterion C"
            } else {
                "Automatic: 1.5·d₂"
            }),
            &mut input.main.outlet_diameter,
            Length,
            d.map(|d| d.outlet_diameter),
            &units,
            0.01..=10.0,
        );
        if vaned {
            let incidence = input.main.incidence.resolve(|| 0.0);
            param_f64(
                ui,
                tr("Incidence i₃"),
                tr("Between the vanes and the approaching flow, ±3° (eq. T7.7.6)"),
                &mut input.main.incidence,
                Angle,
                Some(incidence),
                &units,
                -10f64.to_radians()..=10f64.to_radians(),
            );
        }
    });
    if let Some(d) = d {
        section(ui, tr("Flow"));
        Grid::new("diffuser_flow")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                for h in ["", "c_u", "c_m", "α"] {
                    ui.strong(h);
                }
                ui.end_row();
                for (label, state) in [("inlet", &d.inlet), ("outlet", &d.outlet)] {
                    ui.label(label);
                    ui.label(units.show(Velocity, state.cu));
                    ui.label(units.show(Velocity, state.cm));
                    ui.label(units.show(Angle, state.angle));
                    ui.end_row();
                }
            });
        Grid::new("diffuser_main_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(
                    ui,
                    tr("d₄/d₂"),
                    format!("{:.3}", d.outlet_diameter / d.impeller_diameter),
                );
                value_row(
                    ui,
                    tr("Ideal pressure recovery"),
                    format!("{:.2}", d.ideal_pressure_recovery),
                );
                if d.vanes.is_none() {
                    value_row(ui, tr("Path wrap angle"), units.show(Angle, d.wrap_angle));
                    value_row(ui, tr("Path length"), units.show(Length, d.path_length));
                }
            });
    }
    "edit diffuser main dimensions"
}

fn contour(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    d: Option<&DiffuserDesign>,
    input: &mut DiffuserInput,
) -> &'static str {
    let units = cx.units();
    ui.weak(tr("The side walls run straight from b₃ at d₃ to b₄ at d₄."));
    section(ui, tr("Side walls"));
    grid("diffuser_contour").show(ui, |ui| {
        param_f64(
            ui,
            tr("Outlet width b₄"),
            tr("Automatic: b₃ (parallel walls)"),
            &mut input.contour.outlet_width,
            QuantityKind::Length,
            d.map(|d| d.outlet_width),
            &units,
            0.001..=1.0,
        );
    });
    "edit diffuser walls"
}

fn properties(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    d: Option<&DiffuserDesign>,
    input: &mut DiffuserInput,
) -> &'static str {
    use QuantityKind::*;
    let units = cx.units();
    let v = d.and_then(|d| d.vanes.as_ref());
    ui.weak(tr(
        "The throat width sets the best efficiency point (Gülich eq. T7.7.10).",
    ));
    section(ui, tr("Vanes"));
    grid("diffuser_properties").show(ui, |ui| {
        param_u32(
            ui,
            tr("Vanes z_Le"),
            tr("8 below nq = 20, 12 above, avoiding resonant pairs (Table 10.2)"),
            &mut input.properties.vane_count,
            v.map(|v| v.count),
            3..=40,
        );
        let throat_factor = input.properties.throat_factor.resolve(|| 1.15);
        param_f64(
            ui,
            tr("Throat factor f_a3"),
            tr("Throat over the width from angular momentum, 1.1 to 1.3"),
            &mut input.properties.throat_factor,
            Dimensionless,
            Some(throat_factor),
            &units,
            0.5..=2.0,
        );
        param_f64(
            ui,
            tr("Leading-edge thickness e₃"),
            tr("(0.01 to 0.015)·d₂ (Gülich Chap. 7.9.2)"),
            &mut input.properties.leading_thickness,
            Length,
            v.map(|v| v.leading_thickness),
            &units,
            0.0002..=0.1,
        );
    });
    if let Some(v) = v {
        section(ui, tr("Throat"));
        Grid::new("diffuser_throat").num_columns(2).show(ui, |ui| {
            value_row(
                ui,
                tr("Flow angle α₃'"),
                units.show(Angle, v.inlet_flow_angle),
            );
            value_row(ui, tr("Vane angle α₃B"), units.show(Angle, v.inlet_angle));
            value_row(
                ui,
                tr("Throat a₃ (eq. T7.7.10)"),
                units.show(Length, v.throat_target),
            );
            value_row(ui, tr("Throat of the vanes"), units.show(Length, v.throat));
            value_row(
                ui,
                tr("c₃q/c₂ (B)"),
                tf(
                    "{} (0.7 to 0.85)",
                    &[&format!("{:.2}", v.checks.deceleration)],
                ),
            );
            value_row(
                ui,
                tr("b₃/a₃ (D)"),
                tf("{} (0.8 to 2)", &[&format!("{:.2}", v.checks.aspect_ratio)]),
            );
        });
    }
    "edit diffuser vanes"
}

fn mean_lines(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    d: Option<&DiffuserDesign>,
    input: &mut DiffuserInput,
) -> &'static str {
    use QuantityKind::*;
    let units = cx.units();
    let v = d.and_then(|d| d.vanes.as_ref());
    ui.weak(tr(
        "Conformal mapping dθ = dr/(r·tan α). The vane angle grows from α₃B to α₄B; its law \
         is shaped to give the throat of the previous step.",
    ));
    section(ui, tr("Mean lines"));
    grid("diffuser_mean_lines").show(ui, |ui| {
        param_f64(
            ui,
            tr("Outlet vane angle α₄B"),
            tr("Automatic: the optimum area ratio of the channel (eq. 7.30)"),
            &mut input.mean_lines.outlet_angle,
            Angle,
            v.map(|v| v.outlet_angle),
            &units,
            1f64.to_radians()..=80f64.to_radians(),
        );
        param_u32(
            ui,
            tr("Points per vane"),
            tr("From leading to trailing edge"),
            &mut input.mean_lines.points,
            v.map(|v| v.geometry.mean_line.len() as u32),
            11..=201,
        );
    });
    if let Some(v) = v {
        let c = &v.checks;
        section(ui, tr("Diffusing channel"));
        Grid::new("diffuser_channel").num_columns(2).show(ui, |ui| {
            value_row(
                ui,
                tr("Outlet width a₄"),
                units.show(Length, v.outlet_throat),
            );
            value_row(ui, tr("Length L₃₋₄"), units.show(Length, v.channel_length));
            value_row(
                ui,
                tr("Wrap angle"),
                units.show(Angle, v.geometry.wrap_angle),
            );
            value_row(
                ui,
                tr("Area ratio (A)"),
                tf(
                    "{} (optimum {}, c_p,opt {})",
                    &[
                        &format!("{:.2}", c.area_ratio),
                        &format!("{:.2}", c.optimum_area_ratio),
                        &format!("{:.2}", c.optimum_pressure_recovery),
                    ],
                ),
            );
            value_row(
                ui,
                tr("L₃₋₄/a₃ (C)"),
                tf("{} (2.5 to 6)", &[&format!("{:.1}", c.length_ratio)]),
            );
            value_row(
                ui,
                tr("c₄²/(2gH) (E)"),
                tf("{} (0.02 to 0.04)", &[&format!("{:.3}", c.outlet_energy)]),
            );
            value_row(
                ui,
                tr("Opening angle (F)"),
                tf(
                    "{} (allowed {})",
                    &[
                        &units.show(Angle, c.opening_angle),
                        &units.show(Angle, c.allowable_angle),
                    ],
                ),
            );
        });
    }
    "edit diffuser mean lines"
}

fn profiles(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    d: Option<&DiffuserDesign>,
    input: &mut DiffuserInput,
) -> &'static str {
    let units = cx.units();
    let v = d.and_then(|d| d.vanes.as_ref());
    ui.weak(tr("The vane thickness varies linearly from e₃ to e₄."));
    section(ui, tr("Thickness"));
    grid("diffuser_profiles").show(ui, |ui| {
        param_f64(
            ui,
            tr("Trailing-edge thickness e₄"),
            tr("Automatic: 1.5·e₃"),
            &mut input.profiles.trailing_thickness,
            QuantityKind::Length,
            v.map(|v| v.trailing_thickness),
            &units,
            0.0002..=0.1,
        );
    });
    "edit diffuser profiles"
}

fn edge_shape(ui: &mut Ui, salt: &str, shape: &mut EdgeShape) {
    let ellipse = matches!(shape, EdgeShape::Ellipse { .. });
    ComboBox::from_id_salt(salt)
        .selected_text(tr(if ellipse { "elliptic" } else { "blunt (cut)" }))
        .show_ui(ui, |ui| {
            if ui.selectable_label(ellipse, tr("elliptic")).clicked() && !ellipse {
                *shape = EdgeShape::Ellipse { ratio: 3.0 };
            }
            if ui.selectable_label(!ellipse, tr("blunt (cut)")).clicked() && ellipse {
                *shape = EdgeShape::Cut;
            }
        });
    if let EdgeShape::Ellipse { ratio } = shape {
        ui.add(
            DragValue::new(ratio)
                .range(1.0..=8.0)
                .speed(0.02)
                .suffix(" : 1"),
        );
    } else {
        ui.label("");
    }
    ui.end_row();
}

fn edges(ui: &mut Ui, input: &mut DiffuserInput) -> &'static str {
    ui.weak(tr(
        "An elliptic leading edge makes the vane less sensitive to incidence (Gülich Chap. 7.9.2).",
    ));
    section(ui, tr("Edge shapes"));
    grid("diffuser_edges").show(ui, |ui| {
        ui.label(tr("Leading edge"));
        edge_shape(ui, "diffuser_le", &mut input.edges.leading);
        ui.label(tr("Trailing edge"));
        edge_shape(ui, "diffuser_te", &mut input.edges.trailing);
    });
    "edit diffuser edges"
}
