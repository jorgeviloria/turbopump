//! Design report of the active impeller: the same figures as `turbopump design` and
//! `turbopump geometry`, in tables.

use eframe::egui::{self, Grid, RichText, Ui};
use tp_design::impeller::BladeLoading;
use tp_design::impeller::loading::blade_velocities;
use tp_design::performance::ShutoffSource;
use tp_design::pump::{InletSection, correlations as corr};
use tp_design::radial_thrust::RadialCollector;
use tp_design::volute::{AreaLaw, CutwaterShape, DischargeShape, Progression};
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

/// The report of impeller `id` and the components after it, clear of the `cover` px
/// the floating properties panel hides on the right.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &mut Ui,
    project: &Project,
    derived: &Derived,
    id: ComponentId,
    stator: Option<ComponentId>,
    volute: Option<ComponentId>,
    units: &DisplayUnits,
    cover: f32,
) {
    let area = ui.available_rect_before_wrap();
    let right = (area.max.x - cover).max(area.min.x + 320.0);
    let clear = egui::Rect::from_min_max(area.min, egui::pos2(right, area.max.y));
    ui.scope_builder(egui::UiBuilder::new().max_rect(clear), |ui| {
        content(ui, project, derived, id, stator, volute, units);
    });
}

fn content(
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
            crate::widgets::table(ui, "report_fluid", |ui| {
                    value_row(
                        ui,
                        tr("Fluid"),
                        crate::i18n::fluid_name(&project.setup().fluid),
                    );
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
            crate::widgets::table(ui, "report_stage", |ui| {
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
        crate::widgets::table(ui, "report_eff", |ui| {
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
        crate::widgets::table(ui, "report_dims", |ui| {
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
                        "Outlet hub diameter d₂i",
                        s(Length, d.outlet_hub_diameter),
                        manual_tag(&input.outlet_hub_diameter),
                    ),
                    (
                        "Outlet inclination ε_s",
                        s(Angle, d.outlet_inclination),
                        manual_tag(&input.outlet_inclination),
                    ),
                    (
                        "Blades z",
                        d.blade_count.to_string(),
                        manual_tag(&input.blade_count),
                    ),
                    (
                        "Head rise to shut-off",
                        format!("{:.0} %", d.head_rise * 100.0),
                        manual_tag(&input.head_rise),
                    ),
                    (
                        "Outlet blade angle β₂B",
                        s(Angle, d.outlet.blade_angle),
                        manual_tag(&input.outlet_blade_angle),
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
        crate::widgets::table(ui, "report_outlet", |ui| {
                value_row(ui, tr("u₂"), s(Velocity, t.u));
                value_row(ui, tr("c_m2"), s(Velocity, t.cm));
                value_row(ui, tr("c_u2"), s(Velocity, t.cu));
                value_row(ui, tr("c₂"), s(Velocity, t.c()));
                value_row(ui, tr("w₂"), s(Velocity, t.w()));
                value_row(ui, tr("β₂ (flow)"), s(Angle, t.beta()));
                value_row(ui, tr("β₂B (blade)"), s(Angle, o.blade_angle));
                value_row(ui, "α₂", s(Angle, t.alpha()));
                value_row(ui, tr("Slip factor γ"), format!("{:.3}", o.slip_factor));
                let (q1, q2) = (d.at_pump_flow(&d.inlet_mean.triangle), d.at_pump_flow(t));
                value_row(
                    ui,
                    tr("With the pump flow Q"),
                    tf(
                        "c_m1 {}, c_m2 {}, w₂ {}, β₂ {} (Q = η_v·Q_La instead of Q_La)",
                        &[
                            &s(Velocity, q1.cm),
                            &s(Velocity, q2.cm),
                            &s(Velocity, q2.w()),
                            &s(Angle, q2.beta()),
                        ],
                    ),
                );
            });

        heading(ui, tr("Checks"));
        crate::widgets::table(ui, "report_checks", |ui| {
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
                let band = &d.npsh_band;
                value_row(
                    ui,
                    tr("NPSH3 band of the published methods"),
                    tf(
                        "{} to {} (estimates; NPSH3 is a test result)",
                        &[&s(Head, band.low), &s(Head, band.high)],
                    ),
                );
                for e in &band.estimates {
                    let value = if (e.high - e.low).abs() < 1e-9 {
                        s(Head, e.low)
                    } else {
                        tf("{} to {}", &[&s(Head, e.low), &s(Head, e.high)])
                    };
                    value_row(ui, &format!("  {}", tr(e.method.name())), value);
                }
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
        crate::widgets::table(ui, "report_geometry", |ui| {
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
                crate::widgets::table(ui, "report_performance", |ui| {
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
                                "{} (ψ₀ {}, {})",
                                &[
                                    &s(Head, c.shutoff_head),
                                    &format!("{:.3}", c.shutoff_coefficient),
                                    &match c.shutoff_source {
                                        ShutoffSource::Blades => {
                                            tr("head rise of z and β₂B, Lobanoff & Ross Fig. 3-2")
                                        }
                                        ShutoffSource::SpecificSpeed => tr("Gülich eqs. 3.27–3.28"),
                                        ShutoffSource::Curve => {
                                            tr("raised to fall into the computed curve")
                                        }
                                        ShutoffSource::Manual => tr("manual"),
                                    },
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
                        let v = &c.viscous;
                        if (v.viscosity / tp_design::viscous::WATER_VISCOSITY - 1.0).abs()
            >= tp_design::viscous::VISCOSITY_TOLERANCE {
                            value_row(
                                ui,
                                tr("Viscous correction"),
                                tf(
                                    "ν {} (Re {}): f_Q = f_H {}, f_η {} by loss analysis (Gülich Table 3.10); correlation f_H {}, f_η {} (Table 13.1)",
                                    &[
                                        &s(QuantityKind::KinematicViscosity, v.viscosity),
                                        &format!("{:.3e}", v.reynolds),
                                        &format!("{:.3}", v.head_factor),
                                        &format!("{:.3}", v.efficiency_factor),
                                        &format!("{:.3}", v.correlation_head_factor),
                                        &format!("{:.3}", v.correlation_efficiency_factor),
                                    ],
                                ),
                            );
                        }
                        match &c.losses {
                            Some(l) => {
                                value_row(
                                    ui,
                                    tr("Efficiency by losses"),
                                    tf(
                                        "η {} at design (statistics {} ± {}, Gülich Table 3.9), η_v {}",
                                        &[
                                            &format!("{:.3}", c.design.efficiency),
                                            &format!("{:.3}", c.statistical_efficiency),
                                            &format!("{:.3}", c.efficiency_tolerance),
                                            &format!("{:.3}", l.volumetric_efficiency),
                                        ],
                                    ),
                                );
                                value_row(
                                    ui,
                                    tr("Seal leakage"),
                                    tf(
                                        "{} through d_sp {} with s {} and L {}: ΔH {}, k {}, λ {} (Gülich Table 3.7)",
                                        &[
                                            &s(VolumeFlow, l.leakage),
                                            &s(Length, l.seal.diameter),
                                            &s(Length, l.seal.clearance),
                                            &s(Length, l.seal.length),
                                            &s(Head, l.seal_head),
                                            &format!("{:.2}", l.seal_rotation),
                                            &format!("{:.3}", l.seal_flow.friction),
                                        ],
                                    ),
                                );
                                value_row(
                                    ui,
                                    tr("Disk friction and mechanical losses"),
                                    tf(
                                        "front {}, rear {}{}, mechanical {} (Gülich eqs. T3.6.2, T3.6.3, T3.5.6a)",
                                        &[
                                            &s(Power, l.front_friction),
                                            &s(Power, l.rear_friction),
                                            &if l.back_vanes {
                                                tr(" (with back vanes, eq. T9.3.4)").to_owned()
                                            } else {
                                                String::new()
                                            },
                                            &s(Power, l.mechanical),
                                        ],
                                    ),
                                );
                                if let Some(b) = &c.breakdown {
                                    value_row(
                                        ui,
                                        tr("Efficiency breakdown"),
                                        tf(
                                            "η {} = η_h {} · η_v {} · η_RR {} (disk) · η_m {}, inner η_i {} (water, Gülich Table 3.5)",
                                            &[
                                                &format!("{:.3}", b.overall),
                                                &format!("{:.3}", b.hydraulic),
                                                &format!("{:.3}", b.volumetric),
                                                &format!("{:.3}", b.disk_friction),
                                                &format!("{:.3}", b.mechanical),
                                                &format!("{:.3}", b.inner),
                                            ],
                                        ),
                                    );
                                }
                                if let Some(h) = &c.hydraulic {
                                    value_row(
                                        ui,
                                        tr("Hydraulic losses"),
                                        tf(
                                            "η_h {} (statistics {}) of H_th {}: impeller friction {} (c_f {}), inlet shock {}, outlet shock {}, volute walls {}, volute {} (Gülich Table 3.8)",
                                            &[
                                                &format!("{:.3}", h.efficiency),
                                                &format!("{:.3}", h.statistical_efficiency),
                                                &s(Head, h.theoretical_head),
                                                &s(Head, h.impeller.friction),
                                                &format!("{:.4}", h.impeller.friction_coefficient),
                                                &s(Head, h.impeller.inlet_shock),
                                                &s(Head, h.outlet_shock),
                                                &s(Head, h.volute_friction),
                                                &s(Head, h.volute),
                                            ],
                                        ),
                                    );
                                }
                            }
                            None => value_row(
                                ui,
                                tr("Efficiency by losses"),
                                tf(
                                    "from the statistics of the design (η {})",
                                    &[&format!("{:.3}", c.statistical_efficiency)],
                                ),
                            ),
                        }
                        let o = &c.operation;
                        value_row(
                            ui,
                            tr("System curve"),
                            tf(
                                "H = {} + R·Q² through {} at {} (Gülich eqs. 11.1–11.3)",
                                &[
                                    &s(Head, o.system.static_head),
                                    &s(Head, o.system_head),
                                    &s(VolumeFlow, o.system_flow),
                                ],
                            ),
                        );
                        value_row(
                            ui,
                            tr("Speed and diameter"),
                            tf(
                                "{} ({} of design), d₂' {} (d₂'/d₂ {}, Δη {}, advised ≥ {})",
                                &[
                                    &s(QuantityKind::RotationalSpeed, o.speed),
                                    &format!("{:.3}", o.speed_ratio),
                                    &s(Length, o.diameter),
                                    &format!("{:.3}", o.trim_ratio),
                                    &format!("{:.3}", o.efficiency_drop),
                                    &format!("{:.2}", o.trim_limit),
                                ],
                            ),
                        );
                        value_row(
                            ui,
                            tr("Operating point"),
                            match &o.point {
                                Some(p) => tf(
                                    "{} · {} · {} · η {} % ({} % of the best flow)",
                                    &[
                                        &s(VolumeFlow, p.flow),
                                        &s(Head, p.head),
                                        &s(Power, p.power),
                                        &format!("{:.1}", 100.0 * p.efficiency),
                                        &format!("{:.0}", 100.0 * p.flow / o.best.flow),
                                    ],
                                ),
                                None => tr("the pump and system curves do not cross").to_owned(),
                            },
                        );
                        value_row(
                            ui,
                            tr("Continuous operation"),
                            tf(
                                "{} to {} (η ≥ 0.8·η_max)",
                                &[&s(VolumeFlow, o.continuous.0), &s(VolumeFlow, o.continuous.1)],
                            ),
                        );
                        value_row(
                            ui,
                            tr("Through the system point"),
                            tf(
                                "d₂' {} or {} at full diameter",
                                &[
                                    &o.required_trim.map_or("—".into(), |t| {
                                        s(Length, t * o.diameter / o.trim_ratio)
                                    }),
                                    &o.required_speed.map_or("—".into(), |r| {
                                        s(QuantityKind::RotationalSpeed, r * o.speed / o.speed_ratio)
                                    }),
                                ],
                            ),
                        );
                        let a = &c.axial;
                        let f = &a.design;
                        if a.law.eyes > 1.5 {
                            value_row(
                                ui,
                                tr("Axial force"),
                                tf(
                                    "about {} of either sign; double-entry impeller, Gülich eq. T9.2.15",
                                    &[&s(Force, f.total)],
                                ),
                            );
                        } else {
                            value_row(
                                ui,
                                tr("Axial force"),
                                tf(
                                    "{} at design {}, {} at shut-off, largest {}; Gülich Table 9.2",
                                    &[
                                        &s(Force, f.total.abs()),
                                        &tr(if f.total >= 0.0 {
                                            "toward the suction side"
                                        } else {
                                            "toward the pressure side"
                                        }),
                                        &s(Force, a.shutoff.total),
                                        &s(Force, a.largest),
                                    ],
                                ),
                            );
                            value_row(
                                ui,
                                tr("Axial force parts at design"),
                                tf(
                                    "shrouds {} (k {} front, {} rear{}), momentum {}, shaft {}",
                                    &[
                                        &s(Force, f.hydraulic),
                                        &format!("{:.2}", f.front_rotation),
                                        &format!("{:.2}", f.rear_rotation),
                                        &if a.law.rear_seal_diameter.is_some() {
                                            tr(", balance holes").to_owned()
                                        } else {
                                            String::new()
                                        },
                                        &s(Force, f.momentum),
                                        &s(Force, f.shaft),
                                    ],
                                ),
                            );
                            if a.law.back_vanes.is_some() {
                                value_row(
                                    ui,
                                    tr("Back vanes"),
                                    tf(
                                        "{} off the rear shroud (Gülich Table 9.3)",
                                        &[&s(Force, f.back_vanes)],
                                    ),
                                );
                            }
                            value_row(
                                ui,
                                tr("Annular and shaft seals"),
                                tf(
                                    "d_sp {}, d_D {}; rough estimate {} to {} (eq. T9.2.12)",
                                    &[
                                        &s(Length, a.law.seal_diameter),
                                        &s(Length, a.law.shaft_seal_diameter),
                                        &s(Force, a.estimate.0),
                                        &s(Force, a.estimate.1),
                                    ],
                                ),
                            );
                            if let Some(h) = &a.balance_holes {
                                value_row(
                                    ui,
                                    tr("Balance holes"),
                                    tf(
                                        "{} × {} inside the rear seal d_s2 {} (Gülich Chap. 9.2.2)",
                                        &[
                                            &h.count,
                                            &s(Length, h.diameter),
                                            &s(Length, h.seal_diameter),
                                        ],
                                    ),
                                );
                            }
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
                        if let Some(q) = c.throat_best_flow {
                            value_row(
                                ui,
                                tr("Best-efficiency flow of the volute throat"),
                                tf(
                                    "{} ({} of the design flow, Gülich eq. T4.1.3)",
                                    &[&s(VolumeFlow, q), &percent(q)],
                                ),
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
        let definitions = d.cross_sections.definitions();
        let shape = match definitions {
            [only] => crate::section_editor::describe(&only.shape, units),
            _ => definitions
                .iter()
                .map(|def| {
                    tf(
                        "{} at {}",
                        &[
                            &crate::section_editor::describe(&def.shape, units),
                            &s(Angle, def.angle),
                        ],
                    )
                })
                .collect::<Vec<_>>()
                .join(tr(", blending to ")),
        };
        let dev = &d.development;
        let progression = match dev.progression {
            Progression::Linear => tr("linearly"),
            Progression::Bezier { .. } => tr("along a Bézier curve"),
        };
        let law = match d.law {
            AreaLaw::ConstantVelocity => tf(
                "constant velocity {} (eqs. 7.26–7.27)",
                &[&s(Velocity, dev.velocity)],
            ),
            AreaLaw::Stepanoff => match dev.velocity_factor {
                Some(k) => tf(
                    "Stepanoff, c₃ = K₃·√(2gH) = {} with K₃ = {}",
                    &[&s(Velocity, dev.velocity), &format!("{k:.3}")],
                ),
                None => tf("Stepanoff, c₃ = {}", &[&s(Velocity, dev.velocity)]),
            },
            AreaLaw::Height => tf(
                "height growing {} to {}",
                &[&progression, &s(Length, dev.end_value)],
            ),
            AreaLaw::Area => tf(
                "area growing {} to {}",
                &[&progression, &s(Area, dev.end_value)],
            ),
            AreaLaw::AreaRadius => tf(
                "A/r_c growing {} to {}",
                &[&progression, &s(Length, dev.end_value)],
            ),
            #[allow(clippy::float_cmp)]
            _ if dev.exponent == 1.0 => tr("angular momentum (eqs. 7.24–7.25)").to_owned(),
            _ => tf(
                "Pfleiderer, c_u·r^x constant with x = {}",
                &[&format!("{:.2}", dev.exponent)],
            ),
        };
        crate::widgets::table(ui, "report_volute", |ui| {
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
                if dev.start > 0.0 {
                    value_row(
                        ui,
                        tr("Spiral wrap"),
                        tf(
                            "{}, starting {} past the cutwater with a blind flow of {}",
                            &[
                                &s(Angle, dev.wrap),
                                &s(Angle, dev.start),
                                &s(VolumeFlow, dev.blind_flow),
                            ],
                        ),
                    );
                }
                if let Some(c) = dev.compensation {
                    value_row(
                        ui,
                        tr("Cutwater compensation"),
                        tf(
                            "the inner radius rises {} from {} to the throat",
                            &[&s(Length, c.thickness), &s(Angle, c.start)],
                        ),
                    );
                }
                if let Some(arcs) = &dev.arcs {
                    value_row(
                        ui,
                        tr("Circular arcs"),
                        tf(
                            "{} arcs, at most {} from the spiral",
                            &[&arcs.arcs.len().to_string(), &s(Length, arcs.max_deviation)],
                        ),
                    );
                }
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
                    _ if dev.start > 0.0 => tf(
                        "blunt: the casing on r'_z from the throat to the start of the spiral at {}",
                        &[&s(Angle, v.start_angle)],
                    ),
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
                    DischargeShape::Spline { turn, .. } => {
                        tf("spline turning {}", &[&s(Angle, turn)])
                    }
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
                let w = &v.wall_stress;
                value_row(
                    ui,
                    tr("Wall stresses"),
                    tf(
                        "σ₁ {} round the section, σ₂ {} along, at {}; {} wall for {} (thin torus, our estimate)",
                        &[
                            &s(Pressure, w.hoop),
                            &s(Pressure, w.longitudinal),
                            &s(Pressure, w.pressure),
                            &s(Length, w.required_thickness),
                            &s(Pressure, w.allowable),
                        ],
                    ),
                );
                if let Some(f) = &d.flange {
                    value_row(
                        ui,
                        tr("Outlet flange"),
                        tf(
                            "{} × {}, {} bolt holes of {} on {} (our proportions, not a standard)",
                            &[
                                &s(Length, f.outer_diameter),
                                &s(Length, f.thickness),
                                &f.bolts,
                                &s(Length, f.bolt_hole),
                                &s(Length, f.bolt_circle),
                            ],
                        ),
                    );
                }
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
    crate::widgets::table(ui, "report_diffuser", |ui| {
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
