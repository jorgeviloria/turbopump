//! Design report of the active impeller: the same figures as `turbopump design` and
//! `turbopump geometry`, in tables.

use eframe::egui::{self, Grid, RichText, Ui};
use tp_design::impeller::BladeLoading;
use tp_design::pump::{InletSection, correlations as corr};
use tp_design::volute::{AreaLaw, SectionShape};
use tp_project::{ComponentId, Project};
use tp_units::{DisplayUnits, QuantityKind};

use crate::derived::Derived;
use crate::widgets::{manual_tag, value_row};

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text).heading().size(16.0));
    ui.add_space(2.0);
}

pub(crate) fn show(
    ui: &mut Ui,
    project: &Project,
    derived: &Derived,
    id: ComponentId,
    stator: Option<ComponentId>,
    volute: Option<ComponentId>,
    units: &DisplayUnits,
) {
    use QuantityKind::*;
    let s = |kind, value| units.show(kind, value);
    let Some(results) = derived.impeller(id) else {
        return;
    };
    let name = project.component(id).map_or("", |c| c.name.as_str());
    let input = project.pump_impeller_input(id).unwrap_or_default();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(4.0);
        ui.heading(format!("{name}: design report"));

        if let Some(Ok(liquid)) = &derived.liquid {
            heading(ui, "Fluid");
            Grid::new("report_fluid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    value_row(ui, "Fluid", project.setup().fluid.as_str());
                    value_row(ui, "Density ρ", s(Density, liquid.density));
                    value_row(
                        ui,
                        "Kinematic viscosity ν",
                        s(KinematicViscosity, liquid.kinematic_viscosity()),
                    );
                    value_row(
                        ui,
                        "Vapour pressure p_v",
                        s(Pressure, liquid.vapor_pressure),
                    );
                });
        }

        let d = match &results.main {
            Ok(d) => d,
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                return;
            }
        };
        heading(ui, "Stage");
        if let Ok(duty) = project.pump_duty(id) {
            Grid::new("report_stage")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    value_row(ui, "Flow rate Q", s(VolumeFlow, duty.flow));
                    value_row(ui, "Head H", s(Head, duty.head));
                    value_row(ui, "Speed n", s(RotationalSpeed, duty.speed));
                    value_row(ui, "Specific speed nq", format!("{:.2}", d.nq));
                    value_row(ui, "Impeller flow Q/η_v", s(VolumeFlow, d.impeller_flow));
                    value_row(ui, "Theoretical head H/η_h", s(Head, d.theoretical_head));
                    value_row(ui, "Shaft power", s(Power, d.stage_power));
                    value_row(ui, "Torque", s(Torque, d.shaft_torque));
                });
        }

        heading(ui, "Efficiencies");
        let e = &d.efficiencies;
        Grid::new("report_eff")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(
                    ui,
                    "Hydraulic η_h",
                    format!(
                        "{:.3}{}",
                        e.hydraulic,
                        manual_tag(&input.hydraulic_efficiency)
                    ),
                );
                value_row(
                    ui,
                    "Volumetric η_v",
                    format!(
                        "{:.3}{}",
                        e.volumetric,
                        manual_tag(&input.volumetric_efficiency)
                    ),
                );
                value_row(ui, "Mechanical η_m", format!("{:.3}", e.mechanical));
                value_row(
                    ui,
                    "Overall η",
                    format!("{:.3}{}", e.overall, manual_tag(&input.overall_efficiency)),
                );
                if input.overall_efficiency.is_auto() {
                    value_row(
                        ui,
                        "Expected scatter",
                        format!(
                            "± {:.3} ({} pump, Gülich eq. 3.9.10)",
                            d.efficiency_tolerance,
                            d.layout.name()
                        ),
                    );
                }
            });

        heading(ui, "Dimensions");
        Grid::new("report_dims")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                let rows = [
                    (
                        "Shaft diameter",
                        s(Length, d.shaft_diameter),
                        manual_tag(&input.shaft_diameter),
                    ),
                    (
                        "Hub diameter d_n",
                        s(Length, d.hub_diameter),
                        manual_tag(&input.hub_diameter),
                    ),
                    (
                        "Eye diameter d₁",
                        s(Length, d.inlet_diameter),
                        manual_tag(&input.inlet_diameter),
                    ),
                    (
                        "Outlet diameter d₂",
                        s(Length, d.outlet_diameter),
                        manual_tag(&input.outlet_diameter),
                    ),
                    (
                        "Outlet width b₂",
                        s(Length, d.outlet_width),
                        manual_tag(&input.outlet_width),
                    ),
                    (
                        "Blades z",
                        d.blade_count.to_string(),
                        manual_tag(&input.blade_count),
                    ),
                    (
                        "Blade thickness e",
                        s(Length, d.blade_thickness),
                        manual_tag(&input.blade_thickness),
                    ),
                    (
                        "Pressure coefficient ψ",
                        format!("{:.4}", d.pressure_coefficient),
                        manual_tag(&input.pressure_coefficient),
                    ),
                ];
                for (label, value, tag) in rows {
                    value_row(ui, label, format!("{value}{tag}"));
                }
            });

        heading(ui, "Inlet velocity triangles");
        Grid::new("report_inlet")
            .num_columns(7)
            .striped(true)
            .show(ui, |ui| {
                for h in ["", "d", "u", "c_m", "w", "β", "β_B"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                let row = |ui: &mut Ui, label: &str, sec: &InletSection| {
                    let t = &sec.triangle;
                    ui.label(label);
                    ui.label(s(Length, sec.diameter));
                    ui.label(s(Velocity, t.u));
                    ui.label(s(Velocity, t.cm));
                    ui.label(s(Velocity, t.w()));
                    ui.label(s(Angle, t.beta()));
                    ui.label(s(Angle, sec.blade_angle));
                    ui.end_row();
                };
                row(ui, "hub", &d.inlet_hub);
                row(ui, "mean", &d.inlet_mean);
                row(ui, "shroud", &d.inlet_shroud);
            });

        heading(ui, "Outlet");
        let o = &d.outlet;
        let t = &o.triangle;
        Grid::new("report_outlet")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(ui, "u₂", s(Velocity, t.u));
                value_row(ui, "c_m2", s(Velocity, t.cm));
                value_row(ui, "c_u2", s(Velocity, t.cu));
                value_row(ui, "c₂", s(Velocity, t.c()));
                value_row(ui, "w₂", s(Velocity, t.w()));
                value_row(ui, "β₂ (flow)", s(Angle, t.beta()));
                value_row(ui, "β₂B (blade)", s(Angle, o.blade_angle));
                value_row(ui, "α₂", s(Angle, t.alpha()));
                value_row(ui, "Slip factor γ", format!("{:.3}", o.slip_factor));
            });

        heading(ui, "Checks");
        Grid::new("report_checks")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(
                    ui,
                    "Deviation δ₂ = β₂B − β₂",
                    format!(
                        "{} (≤ {}°)",
                        s(Angle, d.deviation_angle),
                        corr::MAX_DEVIATION_ANGLE
                    ),
                );
                value_row(
                    ui,
                    "De Haller w₂/w₁",
                    format!("{:.2} (≥ {})", d.de_haller, corr::DE_HALLER_LIMIT),
                );
                value_row(ui, "NPSH3 (estimate)", s(Head, d.npsh_required));
                value_row(ui, "NPSH available", s(Head, d.npsh_available));
            });
        warnings(ui, &d.warnings);

        heading(ui, "Geometry");
        let g = match &results.geometry {
            Ok(g) => g,
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                return;
            }
        };
        let qos = &g.channel.quasi_orthogonals;
        Grid::new("report_geometry")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(ui, "Axial length L", s(Length, g.contour.axial_length));
                value_row(ui, "Hub length", s(Length, g.contour.hub_length()));
                value_row(ui, "Shroud length", s(Length, g.contour.shroud_length()));
                value_row(
                    ui,
                    "c_m inlet / outlet",
                    format!(
                        "{} / {}",
                        s(Velocity, qos[0].cm),
                        s(Velocity, qos[qos.len() - 1].cm)
                    ),
                );
                value_row(
                    ui,
                    "Area inlet / outlet",
                    format!(
                        "{} / {}",
                        s(Area, qos[0].area),
                        s(Area, qos[qos.len() - 1].area)
                    ),
                );
                value_row(
                    ui,
                    "Largest local rise of c_m",
                    format!("{:.1} %", g.channel.max_cm_rise() * 100.0),
                );
                let l = &g.blade_loading;
                value_row(
                    ui,
                    "Blade loading ξ_eff / ξ_al",
                    format!(
                        "{:.3} / {:.3} = {:.2} (band {:.2}–{:.2})",
                        l.effective,
                        l.allowable,
                        l.ratio(),
                        1.0 - BladeLoading::BAND,
                        1.0 + BladeLoading::BAND
                    ),
                );
            });

        heading(ui, "Blade sections");
        Grid::new("report_sections")
            .num_columns(7)
            .striped(true)
            .show(ui, |ui| {
                for h in ["span", "r_LE", "c_m,LE", "β₁", "β₁B", "wrap", "length"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for section in &g.blade.sections {
                    let le = &section.leading_edge;
                    ui.label(format!("{:.2}", section.span));
                    ui.label(s(Length, le.position.y));
                    ui.label(s(Velocity, le.cm));
                    ui.label(s(Angle, le.flow_angle));
                    ui.label(s(Angle, le.blade_angle));
                    ui.label(s(Angle, section.wrap_angle()));
                    ui.label(s(Length, section.mean_line_length()));
                    ui.end_row();
                }
            });
        warnings(ui, &g.warnings);

        if let Some(stator) = stator {
            diffuser(ui, project, derived, stator, units);
        }

        let Some(volute) = volute else {
            return;
        };
        let name = project.component(volute).map_or("", |c| c.name.as_str());
        heading(ui, &format!("{name}: volute"));
        let v = match derived.volute_result(volute) {
            Some(Ok(v)) => v,
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                return;
            }
            None => return,
        };
        let d = &v.design;
        let shape = match d.shape {
            SectionShape::Rectangular => "rectangular".to_owned(),
            SectionShape::Circular => "circular".to_owned(),
            SectionShape::Trapezoidal { flank_angle } => {
                format!("trapezoidal, flanks at {}", s(Angle, flank_angle))
            }
        };
        let law = match d.law {
            AreaLaw::ConstantVelocity => "constant velocity (eqs. 7.26–7.27)",
            _ => "angular momentum (eqs. 7.24–7.25)",
        };
        Grid::new("report_volute")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(ui, "Design flow Q_Le", s(VolumeFlow, d.flow));
                value_row(
                    ui,
                    "Cutwater diameter d_z",
                    format!(
                        "{} (minimum {}, Table 10.2)",
                        s(Length, d.cutwater_diameter),
                        s(Length, d.min_cutwater_diameter)
                    ),
                );
                value_row(ui, "Inlet width b₃", s(Length, d.inlet_width));
                value_row(ui, "Cutwater thickness e₃", s(Length, d.cutwater_thickness));
                value_row(ui, "Cross sections", shape);
                value_row(ui, "Area law", law);
                value_row(ui, "Throat area A₃q", s(Area, d.throat.area));
                value_row(ui, "Throat height", s(Length, d.throat.height));
                value_row(ui, "Throat B/H", format!("{:.2} (2 to 3)", d.throat_aspect));
                value_row(ui, "Throat velocity c₃q", s(Velocity, d.throat_velocity));
                value_row(
                    ui,
                    "c₃q/c₂",
                    format!("{:.2} (not much below 0.8, Table 10.2)", d.deceleration),
                );
                value_row(
                    ui,
                    "Cutwater angle α₃B",
                    format!(
                        "{} (flow {})",
                        s(Angle, d.cutwater_angle),
                        s(Angle, d.cutwater_flow_angle)
                    ),
                );
                let dif = &d.diffuser;
                value_row(
                    ui,
                    "Discharge diffuser",
                    format!(
                        "{} long, outlet {}, area ratio {:.2}",
                        s(Length, dif.length),
                        s(Length, dif.outlet_diameter),
                        dif.area_ratio
                    ),
                );
                value_row(
                    ui,
                    "Opening angle",
                    format!(
                        "{} (allowed {}, eq. 1.44)",
                        s(Angle, dif.opening_angle),
                        s(Angle, dif.allowable_angle)
                    ),
                );
                value_row(ui, "Outlet velocity", s(Velocity, dif.outlet_velocity));
                value_row(ui, "Wall thickness", s(Length, d.wall_thickness));
            });
        warnings(ui, &v.warnings);
    });
}

fn diffuser(
    ui: &mut Ui,
    project: &Project,
    derived: &Derived,
    id: ComponentId,
    units: &DisplayUnits,
) {
    use QuantityKind::*;
    let s = |kind, value| units.show(kind, value);
    let name = project.component(id).map_or("", |c| c.name.as_str());
    let d = match derived.diffuser_result(id) {
        Some(Ok(d)) => d,
        Some(Err(e)) => {
            heading(ui, &format!("{name}: diffuser"));
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
        None => return,
    };
    let kind = if d.vanes.is_some() {
        "vaned"
    } else {
        "vaneless"
    };
    heading(ui, &format!("{name}: {kind} diffuser"));
    Grid::new("report_diffuser")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            value_row(ui, "Design flow Q_Le", s(VolumeFlow, d.flow));
            value_row(
                ui,
                "Inlet d₃ × b₃",
                format!(
                    "{} × {}",
                    s(Length, d.inlet_diameter),
                    s(Length, d.inlet_width)
                ),
            );
            value_row(
                ui,
                "Outlet d₄ × b₄",
                format!(
                    "{} × {} ({:.3}·d₂)",
                    s(Length, d.outlet_diameter),
                    s(Length, d.outlet_width),
                    d.outlet_diameter / d.impeller_diameter
                ),
            );
            value_row(
                ui,
                "Velocity in / out",
                format!(
                    "{} / {}",
                    s(Velocity, d.inlet.velocity()),
                    s(Velocity, d.outlet.velocity())
                ),
            );
            value_row(
                ui,
                "Ideal pressure recovery",
                format!("{:.2}", d.ideal_pressure_recovery),
            );
            if let Some(v) = &d.vanes {
                let c = &v.checks;
                value_row(ui, "Vanes", v.count.to_string());
                value_row(
                    ui,
                    "Vane angles α₃B / α₄B",
                    format!("{} / {}", s(Angle, v.inlet_angle), s(Angle, v.outlet_angle)),
                );
                value_row(
                    ui,
                    "Throat a₃",
                    format!(
                        "{} (eq. T7.7.10: {})",
                        s(Length, v.throat),
                        s(Length, v.throat_target)
                    ),
                );
                value_row(
                    ui,
                    "A: area ratio",
                    format!("{:.2} (optimum {:.2})", c.area_ratio, c.optimum_area_ratio),
                );
                value_row(
                    ui,
                    "B: c₃q/c₂",
                    format!("{:.2} (0.7 to 0.85)", c.deceleration),
                );
                value_row(
                    ui,
                    "C: L₃₋₄/a₃",
                    format!("{:.1} (2.5 to 6)", c.length_ratio),
                );
                value_row(ui, "D: b₃/a₃", format!("{:.2} (0.8 to 2)", c.aspect_ratio));
                value_row(
                    ui,
                    "E: c₄²/(2gH)",
                    format!("{:.3} (0.02 to 0.04)", c.outlet_energy),
                );
                value_row(
                    ui,
                    "F: opening angle",
                    format!(
                        "{} (allowed {})",
                        s(Angle, c.opening_angle),
                        s(Angle, c.allowable_angle)
                    ),
                );
            } else {
                value_row(ui, "Flow angle", s(Angle, d.inlet.angle));
                value_row(
                    ui,
                    "Path",
                    format!(
                        "{} around, {} long",
                        s(Angle, d.wrap_angle),
                        s(Length, d.path_length)
                    ),
                );
            }
        });
    warnings(ui, &d.warnings);
}

fn warnings(ui: &mut Ui, warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }
    ui.add_space(6.0);
    for w in warnings {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{} {w}", crate::theme::icon::WARNING),
        );
    }
}
