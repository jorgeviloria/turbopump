//! Design report of the active impeller: the same figures as `turbopump design` and
//! `turbopump geometry`, in tables.

use eframe::egui::{self, Grid, RichText, Ui};
use tp_design::impeller::BladeLoading;
use tp_design::impeller::loading::blade_velocities;
use tp_design::pump::{InletSection, correlations as corr};
use tp_design::radial_thrust::RadialCollector;
use tp_design::volute::{AreaLaw, CutwaterShape, DischargeShape, SectionShape};
use tp_project::{ComponentId, Project};
use tp_units::{DisplayUnits, QuantityKind};

use crate::derived::Derived;
use crate::i18n::{component_name, tf, tr};
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
        ui.heading(tf("{}: design report", &[&component_name(name)]));

        if let Some(Ok(liquid)) = &derived.liquid {
            heading(ui, tr("Fluid"));
            Grid::new("report_fluid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    value_row(ui, tr("Fluid"), project.setup().fluid.as_str());
                    value_row(ui, tr("Density ρ"), s(Density, liquid.density));
                    value_row(
                        ui,
                        tr("Kinematic viscosity ν"),
                        s(KinematicViscosity, liquid.kinematic_viscosity()),
                    );
                    value_row(
                        ui,
                        tr("Vapour pressure p_v"),
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
        heading(ui, tr("Stage"));
        if let Ok(duty) = project.pump_duty(id) {
            Grid::new("report_stage")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    value_row(ui, tr("Flow rate Q"), s(VolumeFlow, duty.flow));
                    value_row(ui, tr("Head H"), s(Head, duty.head));
                    value_row(ui, tr("Speed n"), s(RotationalSpeed, duty.speed));
                    value_row(ui, tr("Specific speed nq"), format!("{:.2}", d.nq));
                    value_row(
                        ui,
                        tr("Impeller flow Q/η_v"),
                        s(VolumeFlow, d.impeller_flow),
                    );
                    value_row(
                        ui,
                        tr("Theoretical head H/η_h"),
                        s(Head, d.theoretical_head),
                    );
                    value_row(ui, tr("Shaft power"), s(Power, d.stage_power));
                    value_row(ui, tr("Torque"), s(Torque, d.shaft_torque));
                });
        }

        heading(ui, tr("Efficiencies"));
        let e = &d.efficiencies;
        Grid::new("report_eff")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(
                    ui,
                    tr("Hydraulic η_h"),
                    format!(
                        "{:.3}{}",
                        e.hydraulic,
                        manual_tag(&input.hydraulic_efficiency)
                    ),
                );
                value_row(
                    ui,
                    tr("Volumetric η_v"),
                    format!(
                        "{:.3}{}",
                        e.volumetric,
                        manual_tag(&input.volumetric_efficiency)
                    ),
                );
                value_row(ui, tr("Mechanical η_m"), format!("{:.3}", e.mechanical));
                value_row(
                    ui,
                    tr("Overall η"),
                    format!("{:.3}{}", e.overall, manual_tag(&input.overall_efficiency)),
                );
                if input.overall_efficiency.is_auto() {
                    value_row(
                        ui,
                        tr("Expected scatter"),
                        tf(
                            "± {} ({} pump, Gülich eq. 3.9.10)",
                            &[
                                &format!("{:.3}", d.efficiency_tolerance),
                                &tr(d.layout.name()),
                            ],
                        ),
                    );
                }
            });

        heading(ui, tr("Dimensions"));
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
                    value_row(ui, tr(label), format!("{value}{tag}"));
                }
            });

        heading(ui, tr("Inlet velocity triangles"));
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
                row(ui, tr("hub"), &d.inlet_hub);
                row(ui, tr("mean"), &d.inlet_mean);
                row(ui, tr("shroud"), &d.inlet_shroud);
            });

        heading(ui, tr("Outlet"));
        let o = &d.outlet;
        let t = &o.triangle;
        Grid::new("report_outlet")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(ui, tr("u₂"), s(Velocity, t.u));
                value_row(ui, tr("c_m2"), s(Velocity, t.cm));
                value_row(ui, tr("c_u2"), s(Velocity, t.cu));
                value_row(ui, tr("c₂"), s(Velocity, t.c()));
                value_row(ui, tr("w₂"), s(Velocity, t.w()));
                value_row(ui, tr("β₂ (flow)"), s(Angle, t.beta()));
                value_row(ui, tr("β₂B (blade)"), s(Angle, o.blade_angle));
                value_row(ui, "α₂", s(Angle, t.alpha()));
                value_row(ui, tr("Slip factor γ"), format!("{:.3}", o.slip_factor));
            });

        heading(ui, tr("Checks"));
        Grid::new("report_checks")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(
                    ui,
                    tr("Deviation δ₂ = β₂B − β₂"),
                    format!(
                        "{} (≤ {}°)",
                        s(Angle, d.deviation_angle),
                        corr::MAX_DEVIATION_ANGLE
                    ),
                );
                value_row(
                    ui,
                    tr("De Haller w₂/w₁"),
                    format!("{:.2} (≥ {})", d.de_haller, corr::DE_HALLER_LIMIT),
                );
                value_row(ui, tr("NPSH3 (estimate)"), s(Head, d.npsh_required));
                value_row(ui, tr("NPSH available"), s(Head, d.npsh_available));
            });
        warnings(ui, &d.warnings);

        heading(ui, tr("Geometry"));
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
                value_row(ui, tr("Axial length L"), s(Length, g.contour.axial_length));
                value_row(ui, tr("Hub length"), s(Length, g.contour.hub_length()));
                value_row(
                    ui,
                    tr("Shroud length"),
                    s(Length, g.contour.shroud_length()),
                );
                value_row(
                    ui,
                    tr("c_m inlet / outlet"),
                    format!(
                        "{} / {}",
                        s(Velocity, qos[0].cm),
                        s(Velocity, qos[qos.len() - 1].cm)
                    ),
                );
                value_row(
                    ui,
                    tr("Area inlet / outlet"),
                    format!(
                        "{} / {}",
                        s(Area, qos[0].area),
                        s(Area, qos[qos.len() - 1].area)
                    ),
                );
                value_row(
                    ui,
                    tr("Largest local rise of c_m"),
                    format!("{:.1} %", g.channel.max_cm_rise() * 100.0),
                );
                let l = &g.blade_loading;
                value_row(
                    ui,
                    tr("Blade loading ξ_eff / ξ_al"),
                    tf(
                        "{} / {} = {} (band {}–{})",
                        &[
                            &format!("{:.3}", l.effective),
                            &format!("{:.3}", l.allowable),
                            &format!("{:.2}", l.ratio()),
                            &format!("{:.2}", 1.0 - BladeLoading::BAND),
                            &format!("{:.2}", 1.0 + BladeLoading::BAND),
                        ],
                    ),
                );
            });

        heading(ui, tr("Blade sections"));
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

        heading(ui, tr("Blade velocities"));
        ui.weak(tr(
            "Relative velocity on the faces over u₂, with the flow following the blades.",
        ));
        let velocities = blade_velocities(g);
        let u2 = velocities.outlet_speed;
        Grid::new("report_velocities")
            .num_columns(5)
            .striped(true)
            .show(ui, |ui| {
                for h in ["span", "w_SS max", "w_PS min", "w_LE (mean)", "w_TE (mean)"].map(tr) {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for v in &velocities.sections {
                    let max = v.suction.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let min = v.pressure.iter().copied().fold(f64::INFINITY, f64::min);
                    ui.label(format!("{:.2}", v.span));
                    ui.label(format!("{:.3}", max / u2));
                    ui.label(format!("{:.3}", min / u2));
                    ui.label(format!("{:.3}", v.mean[0] / u2));
                    ui.label(format!("{:.3}", v.mean[v.mean.len() - 1] / u2));
                    ui.end_row();
                }
            });
        warnings(ui, &velocities.warnings);

        heading(ui, tr("Predicted characteristic"));
        match &results.performance {
            Ok(c) => {
                let q = c.design.flow;
                Grid::new("report_performance")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        value_row(
                            ui,
                            tr("Best efficiency"),
                            tf(
                                "{} ({} % of design) · {} · η {}",
                                &[
                                    &s(VolumeFlow, c.best.flow),
                                    &format!("{:.0}", 100.0 * c.best.flow / q),
                                    &s(Head, c.best.head),
                                    &format!("{:.3}", c.best.efficiency),
                                ],
                            ),
                        );
                        value_row(
                            ui,
                            tr("Shut-off head"),
                            tf(
                                "{} (ψ₀ {}, Gülich eqs. 3.27–3.28)",
                                &[
                                    &s(Head, c.shutoff_head),
                                    &format!("{:.3}", c.shutoff_coefficient),
                                ],
                            ),
                        );
                        value_row(
                            ui,
                            tr("Shut-off power"),
                            tf("{} (estimate)", &[&s(Power, c.shutoff_power)]),
                        );
                        value_row(
                            ui,
                            tr("Recirculation below"),
                            tf(
                                "{} ({} % of design)",
                                &[
                                    &s(VolumeFlow, c.recirculation_flow),
                                    &format!("{:.0}", 100.0 * c.recirculation_flow / q),
                                ],
                            ),
                        );
                        value_row(
                            ui,
                            tr("Largest flow"),
                            tf(
                                "{} ({} % of design)",
                                &[
                                    &s(VolumeFlow, c.max_flow),
                                    &format!("{:.0}", 100.0 * c.max_flow / q),
                                ],
                            ),
                        );
                        let n = &c.npsh;
                        let percent = |flow: f64| format!("{:.0} %", 100.0 * flow / q);
                        value_row(
                            ui,
                            tr("NPSH₃ at design"),
                            tf(
                                "{} (Gülich eq. 6.10, λ_c {}, λ_w {}{}) · available {}",
                                &[
                                    &s(Head, c.npsh_required),
                                    &format!("{:.2}", n.inlet_coefficient),
                                    &format!("{:.3}", n.blade_coefficient),
                                    &if n.extrapolated {
                                        tr(", extrapolated").to_owned()
                                    } else {
                                        String::new()
                                    },
                                    &s(Head, c.npsh_available),
                                ],
                            ),
                        );
                        value_row(
                            ui,
                            tr("NPSH₃ rises steeply above"),
                            tf(
                                "{} ({}): shockless entry {}, w₁ = w₁q at {}, collector {}",
                                &[
                                    &s(VolumeFlow, n.knee_flow),
                                    &percent(n.knee_flow),
                                    &percent(n.shockless_flow),
                                    &n.throat_flow.map_or("—".into(), percent),
                                    &n.collector_flow.map_or("—".into(), percent),
                                ],
                            ),
                        );
                        if let Some(flow) = c.cavitation_flow {
                            value_row(
                                ui,
                                tr("NPSH₃ reaches the available"),
                                format!("{} ({})", s(VolumeFlow, flow), percent(flow)),
                            );
                        }
                        if let Some(r) = &c.radial {
                            let kind = match r.law.collector {
                                RadialCollector::SingleVolute => tr("single volute").to_owned(),
                                RadialCollector::DoubleVolute { wrap } => tf(
                                    "double volute of {}°",
                                    &[&format!("{:.0}", wrap.to_degrees())],
                                ),
                                RadialCollector::VanedDiffuser => tr("vaned diffuser").to_owned(),
                            };
                            value_row(
                                ui,
                                tr("Radial force"),
                                tf(
                                    "{} at design (k_R {}), {} at shut-off (k_R {}); {}, Gülich Table 9.4",
                                    &[
                                        &s(Force, r.design_force),
                                        &format!("{:.3}", r.law.best_coefficient),
                                        &s(Force, r.shutoff_force),
                                        &format!("{:.3}", r.law.shutoff_coefficient),
                                        &kind,
                                    ],
                                ),
                            );
                            value_row(
                                ui,
                                tr("Radial load for the mechanical design"),
                                tf(
                                    "{} (steady plus unsteady, k_R,tot ≥ 0.15)",
                                    &[&s(Force, r.design_load)],
                                ),
                            );
                        }
                        if let Some(v) = &c.volute {
                            let d = &v.design;
                            value_row(
                                ui,
                                tr("Volute loss at design"),
                                tf(
                                    "{} ({} % of the hydraulic losses): spiral {}, cone {}",
                                    &[
                                        &s(Head, d.total()),
                                        &format!("{:.0}", 100.0 * v.share),
                                        &s(Head, d.spiral_loss),
                                        &s(Head, d.cone_loss),
                                    ],
                                ),
                            );
                            value_row(
                                ui,
                                tr("Volute spiral"),
                                tf(
                                    "λ {} · AR {} · λ₆·AR {} · K {} · C_p {} (Japikse eqs. 4.6–4.8)",
                                    &[
                                        &format!("{:.2}", d.swirl),
                                        &format!("{:.3}", d.area_ratio),
                                        &format!("{:.2}", d.matching),
                                        &format!("{:.3}", d.loss_coefficient),
                                        &format!("{:.3}", d.recovery),
                                    ],
                                ),
                            );
                            value_row(
                                ui,
                                tr("Volute least loss at"),
                                v.matched_flow.map_or("—".into(), percent),
                            );
                        }
                    });
                warnings(ui, &c.warnings);
            }
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        }

        if let Some(stator) = stator {
            diffuser(ui, project, derived, stator, units);
        }

        let Some(volute) = volute else {
            return;
        };
        let name = project.component(volute).map_or("", |c| c.name.as_str());
        heading(ui, &tf("{}: volute", &[&component_name(name)]));
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
            SectionShape::Rectangular => tr("rectangular").to_owned(),
            SectionShape::Circular => tr("circular").to_owned(),
            SectionShape::Trapezoidal { flank_angle } => {
                tf("trapezoidal, flanks at {}", &[&s(Angle, flank_angle)])
            }
        };
        let law = tr(match d.law {
            AreaLaw::ConstantVelocity => "constant velocity (eqs. 7.26–7.27)",
            _ => "angular momentum (eqs. 7.24–7.25)",
        });
        Grid::new("report_volute")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                value_row(ui, tr("Design flow Q_Le"), s(VolumeFlow, d.flow));
                value_row(
                    ui,
                    tr("Cutwater diameter d_z"),
                    tf(
                        "{} (minimum {}, Table 10.2)",
                        &[
                            &s(Length, d.cutwater_diameter),
                            &s(Length, d.min_cutwater_diameter),
                        ],
                    ),
                );
                value_row(ui, tr("Inlet width b₃"), s(Length, d.inlet_width));
                value_row(
                    ui,
                    tr("Cutwater thickness e₃"),
                    s(Length, d.cutwater_thickness),
                );
                value_row(ui, tr("Cross sections"), shape);
                value_row(ui, tr("Area law"), law);
                if let Some(rib) = &d.rib {
                    value_row(
                        ui,
                        tr("Double volute"),
                        tf(
                            "inner volute of {} from the rib nose at {}, rib {}",
                            &[
                                &s(Angle, rib.wrap),
                                &s(Angle, rib.start),
                                &s(Length, rib.thickness),
                            ],
                        ),
                    );
                    value_row(
                        ui,
                        tr("Throats of the partial volutes"),
                        format!(
                            "{} · {}",
                            s(Area, rib.outer_throat.area),
                            s(Area, d.throat.area)
                        ),
                    );
                    value_row(
                        ui,
                        tr("Outer channel"),
                        format!(
                            "{} → {}",
                            s(Area, rib.channel_inlet_area),
                            s(Area, rib.channel_outlet_area)
                        ),
                    );
                    if let Some(r) = &v.rib {
                        value_row(
                            ui,
                            tr("Rib nose"),
                            tf(
                                "{}:1 ellipse {} long, tip at {}, {} from the axis",
                                &[
                                    &format!("{:.2}", rib.nose_ratio),
                                    &s(Length, rib.nose_length()),
                                    &s(Angle, r.nose_angle),
                                    &s(Length, r.nose_distance),
                                ],
                            ),
                        );
                    }
                }
                value_row(ui, tr("Throat area A₃q"), s(Area, d.throat.area));
                value_row(ui, tr("Throat height"), s(Length, d.throat.height));
                value_row(
                    ui,
                    tr("Throat B/H"),
                    tf("{} (2 to 3)", &[&format!("{:.2}", d.throat_aspect)]),
                );
                value_row(
                    ui,
                    tr("Throat velocity c₃q"),
                    s(Velocity, d.throat_velocity),
                );
                value_row(
                    ui,
                    tr("c₃q/c₂"),
                    tf(
                        "{} (not much below 0.8, Table 10.2)",
                        &[&format!("{:.2}", d.deceleration)],
                    ),
                );
                value_row(
                    ui,
                    tr("Cutwater angle α₃B"),
                    tf(
                        "{} (flow {})",
                        &[
                            &s(Angle, d.cutwater_angle),
                            &s(Angle, d.cutwater_flow_angle),
                        ],
                    ),
                );
                let c = &v.cutwater;
                let cutwater = match d.cutwater_shape {
                    CutwaterShape::Tangent => tr("tangent").to_owned(),
                    CutwaterShape::Sharp => tf(
                        "sharp, tip at {} with a wedge of {}",
                        &[&s(Angle, c.angle), &s(Angle, c.wedge)],
                    ),
                    CutwaterShape::Rounded { .. } => tf(
                        "rounded with ρ {}, tip at {} with a wedge of {}",
                        &[&s(Length, c.radius), &s(Angle, c.angle), &s(Angle, c.wedge)],
                    ),
                };
                value_row(ui, tr("Cutwater"), cutwater);
                value_row(
                    ui,
                    tr("Effective cutwater diameter"),
                    s(Length, 2.0 * c.nose_distance),
                );
                let dif = &d.diffuser;
                let shape = match dif.shape {
                    DischargeShape::Tangential { .. } => tr("tangential").to_owned(),
                    DischargeShape::Radial { axis_angle } => tf(
                        "radial, bend R_N {} turning {}, axis at {}",
                        &[
                            &s(Length, dif.bend_radius),
                            &s(Angle, v.nozzle_bend),
                            &s(Angle, axis_angle),
                        ],
                    ),
                };
                value_row(
                    ui,
                    tr("Discharge diffuser"),
                    tf(
                        "{}; {} long, outlet {}, area ratio {}",
                        &[
                            &shape,
                            &s(Length, v.nozzle_length),
                            &s(Length, dif.outlet_diameter),
                            &format!("{:.2}", dif.area_ratio),
                        ],
                    ),
                );
                value_row(
                    ui,
                    tr("Opening angle"),
                    tf(
                        "{} (allowed {}, eq. 1.44)",
                        &[&s(Angle, dif.opening_angle), &s(Angle, dif.allowable_angle)],
                    ),
                );
                value_row(ui, tr("Outlet velocity"), s(Velocity, dif.outlet_velocity));
                value_row(ui, tr("Wall thickness"), s(Length, d.wall_thickness));
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
            heading(ui, &tf("{}: diffuser", &[&component_name(name)]));
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
        None => return,
    };
    let title = if d.vanes.is_some() {
        "{}: vaned diffuser"
    } else {
        "{}: vaneless diffuser"
    };
    heading(ui, &tf(title, &[&component_name(name)]));
    Grid::new("report_diffuser")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            value_row(ui, tr("Design flow Q_Le"), s(VolumeFlow, d.flow));
            value_row(
                ui,
                tr("Inlet d₃ × b₃"),
                format!(
                    "{} × {}",
                    s(Length, d.inlet_diameter),
                    s(Length, d.inlet_width)
                ),
            );
            value_row(
                ui,
                tr("Outlet d₄ × b₄"),
                format!(
                    "{} × {} ({:.3}·d₂)",
                    s(Length, d.outlet_diameter),
                    s(Length, d.outlet_width),
                    d.outlet_diameter / d.impeller_diameter
                ),
            );
            value_row(
                ui,
                tr("Velocity in / out"),
                format!(
                    "{} / {}",
                    s(Velocity, d.inlet.velocity()),
                    s(Velocity, d.outlet.velocity())
                ),
            );
            value_row(
                ui,
                tr("Ideal pressure recovery"),
                format!("{:.2}", d.ideal_pressure_recovery),
            );
            if let Some(v) = &d.vanes {
                let c = &v.checks;
                value_row(ui, tr("Vanes"), v.count.to_string());
                value_row(
                    ui,
                    tr("Vane angles α₃B / α₄B"),
                    format!("{} / {}", s(Angle, v.inlet_angle), s(Angle, v.outlet_angle)),
                );
                value_row(
                    ui,
                    tr("Throat a₃"),
                    tf(
                        "{} (eq. T7.7.10: {})",
                        &[&s(Length, v.throat), &s(Length, v.throat_target)],
                    ),
                );
                value_row(
                    ui,
                    tr("A: area ratio"),
                    tf(
                        "{} (optimum {})",
                        &[
                            &format!("{:.2}", c.area_ratio),
                            &format!("{:.2}", c.optimum_area_ratio),
                        ],
                    ),
                );
                value_row(
                    ui,
                    tr("B: c₃q/c₂"),
                    tf("{} (0.7 to 0.85)", &[&format!("{:.2}", c.deceleration)]),
                );
                value_row(
                    ui,
                    tr("C: L₃₋₄/a₃"),
                    tf("{} (2.5 to 6)", &[&format!("{:.1}", c.length_ratio)]),
                );
                value_row(
                    ui,
                    tr("D: b₃/a₃"),
                    tf("{} (0.8 to 2)", &[&format!("{:.2}", c.aspect_ratio)]),
                );
                value_row(
                    ui,
                    tr("E: c₄²/(2gH)"),
                    tf("{} (0.02 to 0.04)", &[&format!("{:.3}", c.outlet_energy)]),
                );
                value_row(
                    ui,
                    tr("F: opening angle"),
                    tf(
                        "{} (allowed {})",
                        &[&s(Angle, c.opening_angle), &s(Angle, c.allowable_angle)],
                    ),
                );
            } else {
                value_row(ui, tr("Flow angle"), s(Angle, d.inlet.angle));
                value_row(
                    ui,
                    tr("Path"),
                    tf(
                        "{} around, {} long",
                        &[&s(Angle, d.wrap_angle), &s(Length, d.path_length)],
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
            format!(
                "{} {}",
                crate::theme::icon::WARNING,
                crate::i18n::message(w)
            ),
        );
    }
}
