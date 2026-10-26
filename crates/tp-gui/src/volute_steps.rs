//! Step editors of the volute: setup, cross section, spiral, discharge diffuser,
//! cutwater and wall thickness.

use std::f64::consts::{PI, TAU};

use eframe::egui::{self, ComboBox, DragValue, Grid, Ui};
use egui_plot::{Line, Plot, PlotPoints};
use tp_design::Param;
use tp_design::volute::stress::StressPlace;
use tp_design::volute::{
    AreaLaw, CutwaterShape, DiffuserGrowth, DischargeShape, MAX_ARCS, MIN_ARCS, MIN_WRAP_DEG,
    Progression, StraightWall, VoluteGeometry, VoluteInput, VoluteKind, ZAlignment,
};
use tp_project::{ComponentId, DesignStep, Project};
use tp_units::{DisplayUnits, QuantityKind};

use crate::app::EditCtx;
use crate::i18n::{tf, tr};
use crate::steps::{Kind, section, section_info, section_kind};
use crate::widgets::{label, param_f64, param_u32, props, value_info, value_row};

pub(crate) fn show(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    project: &Project,
    id: ComponentId,
    step: DesignStep,
) {
    let Ok(mut input) = project.volute_input(id) else {
        return;
    };
    let before = input.clone();
    let label = match step {
        DesignStep::VoluteSetup => setup(ui, cx, id, &mut input),
        DesignStep::CrossSection => crate::section_editor::step(ui, cx, id, &mut input),
        DesignStep::SpiralDevelopment => spiral(ui, cx, id, &mut input),
        DesignStep::Diffuser => diffuser(ui, cx, id, &mut input),
        DesignStep::CutWater => cutwater(ui, cx, id, &mut input),
        DesignStep::WallThickness => wall(ui, cx, id, &mut input),
        _ => return,
    };
    if input != before {
        cx.edit(label, |p| p.set_volute(id, input));
    }
}

pub(crate) fn reset(
    cx: &mut EditCtx<'_>,
    project: &Project,
    id: ComponentId,
    step: DesignStep,
    label: &str,
) {
    let Ok(mut input) = project.volute_input(id) else {
        return;
    };
    let defaults = VoluteInput::default();
    match step {
        DesignStep::VoluteSetup => input.setup = defaults.setup,
        DesignStep::CrossSection => input.cross_section = defaults.cross_section,
        DesignStep::SpiralDevelopment => input.spiral = defaults.spiral,
        DesignStep::Diffuser => input.diffuser = defaults.diffuser,
        DesignStep::CutWater => input.cutwater = defaults.cutwater,
        DesignStep::WallThickness => input.wall = defaults.wall,
        _ => return,
    }
    cx.edit(label, |p| p.set_volute(id, input));
}

fn setup(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    id: ComponentId,
    input: &mut VoluteInput,
) -> &'static str {
    use QuantityKind::*;
    let units = cx.units();
    let g = cx.derived.volute(id);
    let d = g.map(|g| &g.design);
    if section(ui, tr("Main dimensions")) {
        props(ui, "volute_setup", |ui| {
            let flow_factor = g.map(|g| g.design.flow / g.basis.flow);
            param_f64(
                ui,
                tr("Flow factor Q_Le/Q"),
                tr("Allowance for leakage that flows through the volute (eq. T7.7.1)"),
                &mut input.setup.flow_factor,
                Dimensionless,
                flow_factor,
                &units,
                0.8..=1.5,
            );
            param_f64(
                ui,
                tr("Cutwater diameter d_z"),
                tr("Base circle; automatic: the smallest gap of Gülich Table 10.2"),
                &mut input.setup.cutwater_diameter,
                Length,
                d.map(|d| d.cutwater_diameter),
                &units,
                0.01..=10.0,
            );
            param_f64(
                ui,
                tr("Inlet width b₃"),
                tr("Width of the volute at the base circle; b₃/b₂ from 2.0 at low nq to 1.2"),
                &mut input.setup.inlet_width,
                Length,
                d.map(|d| d.inlet_width),
                &units,
                0.001..=1.0,
            );
            param_f64(
                ui,
                tr("Cutwater thickness e₃"),
                tr("Nose of the cutwater; about 0.02·d₂ (Gülich Chap. 7.8.1)"),
                &mut input.setup.cutwater_thickness,
                Length,
                d.map(|d| d.cutwater_thickness),
                &units,
                0.0002..=0.2,
            );
        });
    }
    if section(ui, tr("Type")) {
        props(ui, "volute_kind", |ui| {
            label(
                ui,
                tr("Volute"),
                tr(
                    "A double volute splits the spiral with a rib to cut the radial force \
                 (Gülich Chap. 7.8.1 and Table 9.4)",
                ),
            );
            let text = |k: VoluteKind| {
                tr(match k {
                    VoluteKind::Single => "single",
                    VoluteKind::Double => "double",
                })
            };
            crate::widgets::list(ui, |ui| {
                ComboBox::from_id_salt("volute_kind")
                    .selected_text(crate::widgets::noted(ui, text(input.setup.kind)))
                    .show_ui(ui, |ui| {
                        for kind in [VoluteKind::Single, VoluteKind::Double] {
                            ui.selectable_value(&mut input.setup.kind, kind, text(kind));
                        }
                    })
            });
            ui.end_row();
            if input.setup.kind == VoluteKind::Double {
                let rib = d.and_then(|d| d.rib);
                param_f64(
                    ui,
                    tr("Inner volute wrap ε_sp"),
                    tr(
                        "From the rib nose to the throat; 180°, or 170° with an even number of \
                        blades (Gülich Chap. 7.8.1)",
                    ),
                    &mut input.setup.rib_wrap,
                    Angle,
                    rib.map(|r| r.wrap),
                    &units,
                    (90.5f64).to_radians()..=(240f64).to_radians(),
                );
                param_f64(
                    ui,
                    tr("Rib thickness"),
                    tr("Wall between the inner volute and the outer channel; automatic: e₃"),
                    &mut input.setup.rib_thickness,
                    Length,
                    rib.map(|r| r.thickness),
                    &units,
                    0.0005..=0.1,
                );
                param_f64(
                    ui,
                    tr("Rib nose length/thickness"),
                    tr(
                        "Elliptic nose and tail of the rib: 1 for a semicircle, longer for a \
                        nose less sensitive to the flow angle (Gülich Chap. 7.8.1, item 7)",
                    ),
                    &mut input.setup.rib_nose_ratio,
                    Dimensionless,
                    rib.map(|r| r.nose_ratio),
                    &units,
                    1.0..=5.0,
                );
                param_f64(
                    ui,
                    tr("Outer channel outlet/first throat"),
                    tr(
                        "Area of the outer channel at the discharge over the throat of the first \
                        partial volute; automatic: 1",
                    ),
                    &mut input.setup.channel_area_ratio,
                    Dimensionless,
                    rib.map(|r| r.channel_outlet_area / r.outer_throat.area),
                    &units,
                    0.8..=3.0,
                );
                label(
                    ui,
                    tr("Compensate the rib"),
                    tr(
                        "From φ_C to the rib nose the first partial volute grows by the rib, so the \
                        outer channel starts with the flow area of the rule instead of accelerating \
                        past the rib. Our construction, in place of eq. 7.28 for the rib",
                    ),
                );
                ui.checkbox(&mut input.setup.rib_compensation, "");
                ui.end_row();
                if input.setup.rib_compensation {
                    param_f64(
                        ui,
                        tr("Rib compensation start φ_C"),
                        tr(
                            "Wrap angle from the cutwater where the base starts to rise; \
                            automatic: halfway to the rib nose",
                        ),
                        &mut input.setup.rib_compensation_start,
                        Angle,
                        rib.and_then(|r| r.compensation_start),
                        &units,
                        0.0..=359f64.to_radians(),
                    );
                }
            }
        });
        if let Some(g) = g {
            let d = &g.design;
            if section_kind(ui, tr("Results"), "", Kind::Results) {
                props(ui, "volute_setup_results", |ui| {
                    value_row(ui, tr("Design flow Q_Le"), units.show(VolumeFlow, d.flow));
                    value_info(
                        ui,
                        tr("d_z/d₂"),
                        &tf(
                            "Minimum {} (Gülich Table 10.2)",
                            &[&format!(
                                "{:.3}",
                                d.min_cutwater_diameter / g.basis.impeller_diameter
                            )],
                        ),
                        format!("{:.3}", d.cutwater_diameter / g.basis.impeller_diameter),
                    );
                    value_row(
                        ui,
                        tr("Stagnation radius r'_z"),
                        units.show(Length, d.stagnation_radius),
                    );
                    value_row(
                        ui,
                        tr("c₂u of the impeller"),
                        units.show(Velocity, g.basis.cu),
                    );
                    if let Some(rib) = &d.rib {
                        value_row(ui, tr("Rib nose at"), units.show(Angle, rib.start));
                        if let Some(r) = &g.rib {
                            value_row(
                                ui,
                                tr("Tip of the rib nose"),
                                tf(
                                    "{}, {} from the axis",
                                    &[
                                        &units.show(Angle, r.nose_angle),
                                        &units.show(Length, r.nose_distance),
                                    ],
                                ),
                            );
                        }
                        value_row(
                            ui,
                            tr("Throats of the partial volutes"),
                            format!(
                                "{} · {}",
                                units.show(Area, rib.outer_throat.area),
                                units.show(Area, d.throat.area)
                            ),
                        );
                    }
                });
            }
        }
    }
    "edit volute setup"
}

fn spiral(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    id: ComponentId,
    input: &mut VoluteInput,
) -> &'static str {
    use QuantityKind::*;
    let units = cx.units();
    let g = cx.derived.volute(id);
    let dev = g.map(|g| &g.design.development);
    let spiral = &mut input.spiral;
    if section(ui, tr("Design rule")) {
        let law = g.map_or(
            match spiral.law {
                AreaLaw::Auto => AreaLaw::AngularMomentum,
                law => law,
            },
            |g| g.design.law,
        );
        props(ui, "volute_rule", |ui| {
            param_f64(
                ui,
                tr("Wrap angle φ_wrap"),
                tr(
                    "From the start of the spiral to the throat. A shorter spiral starts past the cutwater, which becomes the casing between the throat and that start. Automatic: 360°",
                ),
                &mut spiral.wrap,
                Angle,
                dev.map(|d| d.wrap),
                &units,
                MIN_WRAP_DEG.to_radians()..=TAU,
            );
            label(ui, tr("Law"), "");
            let selected = match (spiral.law, g.map(|g| g.design.law)) {
                (AreaLaw::Auto, Some(law)) => tf("automatic: {}", &[&law_name(law)]),
                (law, _) => law_name(law).to_owned(),
            };
            crate::widgets::list(ui, |ui| {
                ComboBox::from_id_salt("volute_law")
                    .selected_text(crate::widgets::noted(ui, selected))
                    .height(400.0)
                    .show_ui(ui, |ui| {
                        ui.weak(tr("Velocity-based"));
                        for law in [
                            AreaLaw::Auto,
                            AreaLaw::AngularMomentum,
                            AreaLaw::ConstantVelocity,
                            AreaLaw::Stepanoff,
                        ] {
                            ui.selectable_value(&mut spiral.law, law, law_name(law));
                        }
                        ui.separator();
                        ui.weak(tr("Geometry-based"));
                        for law in [AreaLaw::Height, AreaLaw::Area, AreaLaw::AreaRadius] {
                            ui.selectable_value(&mut spiral.law, law, law_name(law));
                        }
                    })
            });
            ui.end_row();
            param_u32(
                ui,
                tr("Sections"),
                tr("Cross sections from the start of the spiral to the throat"),
                &mut spiral.sections,
                g.map(|g| g.sections.len() as u32 - 1),
                4..=180,
            );
        });

        if law.velocity_based() {
            if section(ui, tr("Velocity and flow")) {
                props(ui, "volute_velocity", |ui| {
                    if law == AreaLaw::AngularMomentum {
                        param_f64(
                            ui,
                            tr("Swirl exponent x"),
                            tr(
                                "c_u·r^x stays constant over every section: 1 conserves the angular momentum (Gülich eq. 7.24), 0 keeps the velocity of the volute inlet. Automatic: 1",
                            ),
                            &mut spiral.exponent,
                            Dimensionless,
                            dev.map(|d| d.exponent),
                            &units,
                            0.0..=2.0,
                        );
                    }
                    if law == AreaLaw::Stepanoff && spiral.reference_velocity.is_auto() {
                        param_f64(
                            ui,
                            tr("Velocity factor K₃"),
                            tr(
                                "c₃/√(2gH); automatic: from the specific speed (Stepanoff Fig. 7.5)",
                            ),
                            &mut spiral.velocity_factor,
                            Dimensionless,
                            dev.and_then(|d| d.velocity_factor),
                            &units,
                            0.05..=1.0,
                        );
                    }
                    let (velocity_label, hint) = if law == AreaLaw::Stepanoff {
                        (
                            tr("Velocity c₃"),
                            tr("Mean velocity in every section; automatic: K₃·√(2gH)"),
                        )
                    } else {
                        (
                            tr("Reference velocity c_u,4"),
                            tr(
                                "Circumferential velocity at the volute inlet r₄ = r'_z; automatic: c₂u·r₂/r₄ from the impeller",
                            ),
                        )
                    };
                    param_f64(
                        ui,
                        velocity_label,
                        hint,
                        &mut spiral.reference_velocity,
                        Velocity,
                        dev.map(|d| d.reference_velocity),
                        &units,
                        0.1..=500.0,
                    );
                    param_f64(
                        ui,
                        tr("Reference flow Q_i"),
                        tr("Flow the impeller delivers round the spiral; automatic: Q_Le"),
                        &mut spiral.reference_flow,
                        VolumeFlow,
                        dev.map(|d| d.reference_flow),
                        &units,
                        1e-6..=100.0,
                    );
                    param_f64(
                        ui,
                        tr("Blind flow Q₀"),
                        tr(
                            "Flow already in the spiral at its start; automatic: what the impeller delivers ahead of a spiral shorter than 360°",
                        ),
                        &mut spiral.blind_flow,
                        VolumeFlow,
                        dev.map(|d| d.blind_flow),
                        &units,
                        0.0..=100.0,
                    );
                    label(
                        ui,
                        tr("Cutwater correction"),
                        tr("Enlarge the throat for the blockage of the cutwater (eq. 7.28)"),
                    );
                    ui.checkbox(&mut spiral.cutwater_correction, "");
                    ui.end_row();
                });
            }
        } else {
            if section(ui, tr("End section and progression")) {
                props(ui, "volute_geometry_rule", |ui| {
                    let hint =
                        tr("At the throat; automatic: that of the throat by angular momentum");
                    let end = dev.map(|d| d.end_value);
                    match law {
                        AreaLaw::Height => param_f64(
                            ui,
                            tr("End height H"),
                            hint,
                            &mut spiral.end_height,
                            Length,
                            end,
                            &units,
                            1e-4..=10.0,
                        ),
                        AreaLaw::Area => param_f64(
                            ui,
                            tr("End area A"),
                            hint,
                            &mut spiral.end_area,
                            Area,
                            end,
                            &units,
                            1e-8..=10.0,
                        ),
                        _ => param_f64(
                            ui,
                            tr("End A/r_c"),
                            hint,
                            &mut spiral.end_area_radius,
                            Length,
                            end,
                            &units,
                            1e-6..=10.0,
                        ),
                    };
                    label(
                        ui,
                        tr("Progression"),
                        tr(
                            "How the value grows from the start of the spiral to its end value at the throat",
                        ),
                    );
                    let bezier = matches!(spiral.progression, Progression::Bezier { .. });
                    crate::widgets::list(ui, |ui| {
                        ComboBox::from_id_salt("volute_progression")
                            .selected_text(crate::widgets::noted(
                                ui,
                                if bezier {
                                    tr("Bézier curve")
                                } else {
                                    tr("linear")
                                },
                            ))
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(!bezier, tr("linear")).clicked() {
                                    spiral.progression = Progression::Linear;
                                }
                                if ui.selectable_label(bezier, tr("Bézier curve")).clicked()
                                    && !bezier
                                {
                                    spiral.progression = Progression::bezier();
                                }
                            })
                    });
                    ui.end_row();
                });
                if let Progression::Bezier { points } = &mut spiral.progression {
                    Grid::new("volute_progression_points")
                        .num_columns(3)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            for (k, p) in points.iter_mut().enumerate() {
                                ui.label(tf("Point {}", &[&(k + 1).to_string()]));
                                ui.add(
                                    DragValue::new(&mut p[0])
                                        .speed(0.005)
                                        .range(0.0..=1.0)
                                        .prefix("φ/φ_wrap "),
                                );
                                ui.add(
                                    DragValue::new(&mut p[1])
                                        .speed(0.005)
                                        .range(0.0..=2.0)
                                        .prefix(tr("of the end ")),
                                );
                                ui.end_row();
                            }
                        });
                    let progression = spiral.progression;
                    let curve: PlotPoints = (0..=50)
                        .map(|i| {
                            let x = f64::from(i) / 50.0;
                            [x, progression.value(x)]
                        })
                        .collect();
                    Plot::new(("volute_progression_plot", id.0))
                        .height(120.0)
                        .data_aspect(1.0)
                        .allow_scroll(false)
                        .x_axis_label("φ/φ_wrap")
                        .show(ui, |plot| {
                            plot.line(Line::new(tr("progression"), curve).width(2.0));
                        });
                }
            }
        }
    }

    if section_info(
        ui,
        tr("Cutwater compensation"),
        tr(
            "Past φ_C the inner radius of the sections rises linearly to r'_z + e at the throat: the end section gains a strip as thick as the cutwater. An alternative to the correction of eq. 7.28, for single external volutes.",
        ),
    ) {
        props(ui, "volute_compensation", |ui| {
            label(ui, tr("Compensate the cutwater"), "");
            ui.checkbox(&mut spiral.compensation.enabled, "");
            ui.end_row();
            if spiral.compensation.enabled {
                let resolved = dev.and_then(|d| d.compensation);
                param_f64(
                    ui,
                    tr("Thickness e"),
                    tr(
                        "Rise of the inner radius at the throat; automatic: the cutwater thickness e₃",
                    ),
                    &mut spiral.compensation.thickness,
                    Length,
                    resolved.map(|c| c.thickness),
                    &units,
                    1e-4..=1.0,
                );
                param_f64(
                    ui,
                    tr("Start φ_C"),
                    tr(
                        "Wrap angle from the start of the spiral where the inner radius starts to rise; automatic: 270°",
                    ),
                    &mut spiral.compensation.start,
                    Angle,
                    resolved.map(|c| c.start),
                    &units,
                    0.0..=359f64.to_radians(),
                );
            }
        });
    }

    if section(ui, tr("Circular arcs")) {
        props(ui, "volute_arcs", |ui| {
            label(
                ui,
                tr("Use circular arcs"),
                tr(
                    "For rectangular and trapezoidal sections: tangent circular arcs take the place of the outer contour, as volutes are laid out on the drawing board. They start at the start of the spiral and end at the throat.",
                ),
            );
            ui.checkbox(&mut spiral.arcs.enabled, "");
            ui.end_row();
            if spiral.arcs.enabled {
                label(ui, tr("Number of arcs"), "");
                ui.add(DragValue::new(&mut spiral.arcs.segments).range(MIN_ARCS..=MAX_ARCS));
                ui.end_row();
                if let Some(arcs) = dev.and_then(|d| d.arcs.as_ref()) {
                    value_row(
                        ui,
                        tr("Largest deviation"),
                        units.show(Length, arcs.max_deviation),
                    );
                }
            }
        });
        if let Some(arcs) = dev.and_then(|d| d.arcs.as_ref()) {
            Grid::new("volute_arc_list")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong(tr("Arc"));
                    ui.strong(tr("Radius"));
                    ui.strong(tr("Center"));
                    ui.strong(tr("From – to"));
                    ui.end_row();
                    for (k, arc) in arcs.arcs.iter().enumerate() {
                        let (from, to) = arc.angles();
                        ui.label((k + 1).to_string());
                        ui.label(units.show(Length, arc.radius));
                        ui.label(tf(
                            "{} at {}",
                            &[
                                &units.show(Length, arc.center.length()),
                                &units
                                    .show(Angle, arc.center.y.atan2(arc.center.x).rem_euclid(TAU)),
                            ],
                        ));
                        ui.label(format!(
                            "{} – {}",
                            units.show(Angle, from),
                            units.show(Angle, to)
                        ));
                        ui.end_row();
                    }
                });
        }

        if let Some(g) = g {
            let d = &g.design;
            if section(ui, tr("Throat")) {
                props(ui, "volute_spiral_results", |ui| {
                    value_row(ui, tr("Area A₃q"), units.show(Area, d.throat.area));
                    value_row(
                        ui,
                        tr("Outer radius r_a"),
                        units.show(Length, d.throat.outer_radius),
                    );
                    if d.development.law.velocity_based() {
                        value_row(
                            ui,
                            tr("Cutwater correction Δa₃"),
                            units.show(Length, d.throat_correction),
                        );
                    }
                    value_row(
                        ui,
                        tr("Velocity c₃q"),
                        units.show(Velocity, d.throat_velocity),
                    );
                    value_info(
                        ui,
                        tr("c₃q/c₂"),
                        tr("Not much below 0.8 (Gülich Table 10.2)"),
                        format!("{:.2}", d.deceleration),
                    );
                    value_row(ui, tr("Spiral starts at"), units.show(Angle, g.start_angle));
                });
                if let Ok(p) = d.end_section()
                    && section(ui, tr("End section"))
                {
                    props(ui, "volute_end_section", |ui| {
                        let height = p.outer_radius - p.inner_radius;
                        value_row(
                            ui,
                            tr("Inner radius r'"),
                            units.show(Length, p.inner_radius),
                        );
                        value_row(ui, tr("Outer radius r"), units.show(Length, p.outer_radius));
                        value_row(
                            ui,
                            tr("Lowest axial coordinate z"),
                            units.show(Length, p.axial_min),
                        );
                        value_row(ui, tr("Height H"), units.show(Length, height));
                        value_row(ui, tr("Width B"), units.show(Length, p.width));
                        value_row(ui, tr("Side ratio H/B"), format!("{:.3}", height / p.width));
                        value_row(
                            ui,
                            tr("Equivalent diameter D"),
                            units.show(Length, (4.0 * p.area / PI).sqrt()),
                        );
                        value_row(ui, tr("Area A"), units.show(Area, p.area));
                        value_row(
                            ui,
                            tr("Area over centroid radius A/r_c"),
                            units.show(Length, p.area / p.centroid_radius),
                        );
                    });
                }
                progressions(ui, id, g, &units);
            }
        }
    }
    "edit volute spiral"
}

/// Name of an area law in the interface.
fn law_name(law: AreaLaw) -> &'static str {
    tr(match law {
        AreaLaw::Auto => "automatic",
        AreaLaw::AngularMomentum => "Pfleiderer: c_u·r^x constant",
        AreaLaw::ConstantVelocity => "constant velocity, throat by angular momentum (Gülich)",
        AreaLaw::Stepanoff => "Stepanoff: c₃ = K₃·√(2gH)",
        AreaLaw::Height => "height H",
        AreaLaw::Area => "area A",
        AreaLaw::AreaRadius => "area over centroid radius A/r_c",
    })
}

/// The quantities along the spiral that [`progressions`] can plot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Progress {
    Area,
    Radius,
    AreaRadius,
    ContourAngle,
    Velocity,
}

/// Diagrams of the spiral against the wrap angle: area, outer radius, A/r_c and the
/// angle of the outer contour to the circumferential direction; and the
/// circumferential velocity against the radius for the velocity rules.
fn progressions(ui: &mut Ui, id: ComponentId, g: &VoluteGeometry, units: &DisplayUnits) {
    use QuantityKind::*;
    if section_kind(ui, tr("Along the spiral"), "", Kind::Results) {
        let key = egui::Id::new(("volute_progress", id.0));
        let mut shown = ui.data(|d| d.get_temp(key)).unwrap_or(Progress::Area);
        let dev = &g.design.development;
        let velocity = dev.law.velocity_based();
        ui.horizontal_wrapped(|ui| {
            let options = [
                (Progress::Area, tr("area")),
                (Progress::Radius, tr("outer radius")),
                (Progress::AreaRadius, "A/r_c"),
                (Progress::ContourAngle, tr("contour angle")),
                (Progress::Velocity, tr("circumferential velocity")),
            ];
            for (option, label) in options {
                if option == Progress::Velocity && !velocity {
                    continue;
                }
                ui.selectable_value(&mut shown, option, label);
            }
        });
        if shown == Progress::Velocity && !velocity {
            shown = Progress::Area;
        }
        ui.data_mut(|d| d.insert_temp(key, shown));

        let angle = |a: f64| units.to_display(Angle, a);
        // The outer wall is sampled evenly from the start of the spiral to the throat.
        let radii: Vec<f64> = g.outer_wall.iter().map(|p| p.truncate().length()).collect();
        let step = (TAU - g.start_angle) / (radii.len().max(2) - 1) as f64;
        let wall_angle = |i: usize| g.start_angle + step * i as f64;
        let (points, x_label, y_label): (PlotPoints, String, String) = match shown {
            Progress::Area => (
                g.sections
                    .iter()
                    .map(|s| [angle(s.angle), units.to_display(Area, s.size.area)])
                    .collect(),
                format!("ε [{}]", units.get(Angle).symbol),
                format!("A [{}]", units.get(Area).symbol),
            ),
            Progress::AreaRadius => (
                g.sections
                    .iter()
                    .map(|s| {
                        let ratio = s.size.area / s.size.centroid_radius;
                        [angle(s.angle), units.to_display(Length, ratio)]
                    })
                    .collect(),
                format!("ε [{}]", units.get(Angle).symbol),
                format!("A/r_c [{}]", units.get(Length).symbol),
            ),
            Progress::Radius => (
                radii
                    .iter()
                    .enumerate()
                    .map(|(i, &r)| [angle(wall_angle(i)), units.to_display(Length, r)])
                    .collect(),
                format!("ε [{}]", units.get(Angle).symbol),
                format!("r_a [{}]", units.get(Length).symbol),
            ),
            Progress::ContourAngle => (
                (1..radii.len().saturating_sub(1))
                    .map(|i| {
                        // tan α = (dr/dε)/r between the circumferential direction and the wall.
                        let slope = (radii[i + 1] - radii[i - 1]) / (2.0 * step);
                        [angle(wall_angle(i)), angle((slope / radii[i]).atan())]
                    })
                    .collect(),
                format!("ε [{}]", units.get(Angle).symbol),
                format!("α [{}]", units.get(Angle).symbol),
            ),
            Progress::Velocity => {
                let r4 = dev.reference_radius;
                let outer = g.design.throat.outer_radius;
                (
                    (0..=40)
                        .map(|i| {
                            let r = r4 + (outer - r4) * f64::from(i) / 40.0;
                            let cu = match dev.law {
                                AreaLaw::AngularMomentum => {
                                    dev.reference_velocity * (r4 / r).powf(dev.exponent)
                                }
                                _ => dev.velocity,
                            };
                            [units.to_display(Length, r), units.to_display(Velocity, cu)]
                        })
                        .collect(),
                    format!("r [{}]", units.get(Length).symbol),
                    format!("c_u [{}]", units.get(Velocity).symbol),
                )
            }
        };
        Plot::new(("volute_progress_plot", id.0))
            .height(160.0)
            .x_axis_label(x_label)
            .y_axis_label(y_label)
            .allow_scroll(false)
            .show(ui, |plot| {
                plot.line(Line::new(tr("spiral"), points).width(2.0));
            });
    }
}

fn diffuser(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    id: ComponentId,
    input: &mut VoluteInput,
) -> &'static str {
    use QuantityKind::*;
    let units = cx.units();
    let g = cx.derived.volute(id);
    let dif = g.map(|g| &g.design.diffuser);
    if section(ui, tr("Discharge diffuser")) {
        props(ui, "volute_diffuser", |ui| {
            label(
                ui,
                tr("Shape"),
                tr(
                    "Tangential: straight along the flow at the throat. Radial: a bend turning the \
                 flow away from the pump axis; at 0° the outlet stands on the pump centerline \
                 (Gülich Fig. 7.42)",
                ),
            );
            let text = |shape: &DischargeShape| {
                tr(match shape {
                    DischargeShape::Tangential {
                        straight_wall: StraightWall::Inner,
                    } => "tangential, cutwater wall straight",
                    DischargeShape::Tangential {
                        straight_wall: StraightWall::Centered,
                    } => "tangential, opening to both sides",
                    DischargeShape::Tangential {
                        straight_wall: StraightWall::Outer,
                    } => "tangential, outer wall straight",
                    DischargeShape::Radial { .. } => "radial",
                })
            };
            let current = input.diffuser.shape;
            crate::widgets::list(ui, |ui| {
                ComboBox::from_id_salt("volute_discharge")
                    .selected_text(crate::widgets::noted(ui, text(&current)))
                    .show_ui(ui, |ui| {
                        for option in [
                            DischargeShape::Radial { axis_angle: 0.0 },
                            DischargeShape::Tangential {
                                straight_wall: StraightWall::Centered,
                            },
                            DischargeShape::Tangential {
                                straight_wall: StraightWall::Inner,
                            },
                            DischargeShape::Tangential {
                                straight_wall: StraightWall::Outer,
                            },
                        ] {
                            let same = std::mem::discriminant(&option)
                                == std::mem::discriminant(&current)
                                && (matches!(option, DischargeShape::Radial { .. })
                                    || option == current);
                            if ui.selectable_label(same, text(&option)).clicked() && !same {
                                input.diffuser.shape = option;
                            }
                        }
                    })
            });
            ui.end_row();
            if let DischargeShape::Radial { axis_angle } = &mut input.diffuser.shape {
                let mut angle = Param::Manual(*axis_angle);
                param_f64(
                    ui,
                    tr("Axis angle"),
                    tr(
                        "Between the straight part and the line from the pump axis; 0: the \
                        discharge points straight away from the pump axis",
                    ),
                    &mut angle,
                    Angle,
                    Some(*axis_angle),
                    &units,
                    0.0..=80f64.to_radians(),
                );
                *axis_angle = angle.resolve(|| 0.0);
                param_f64(
                    ui,
                    tr("Bend radius R_N"),
                    tr("Mean radius of the bend; automatic: 1.5·√(4·A₃q/π) (Gülich eq. 7.29)"),
                    &mut input.diffuser.bend_radius,
                    Length,
                    dif.map(|d| d.bend_radius),
                    &units,
                    0.001..=10.0,
                );
            }
            param_f64(
                ui,
                tr("Length"),
                tr(
                    "Along the centerline from the throat, the bend included; automatic: four \
                    throat radii",
                ),
                &mut input.diffuser.length,
                Length,
                dif.map(|d| d.length),
                &units,
                0.005..=10.0,
            );
            param_f64(
                ui,
                tr("Outlet face height H"),
                tr(
                    "Distance from the pump axis to the outlet face; when set, it fixes the \
                    length of the diffuser",
                ),
                &mut input.diffuser.flange_height,
                Length,
                g.map(|g| g.outlet_height),
                &units,
                0.005..=10.0,
            );
            param_f64(
                ui,
                tr("Outlet diameter"),
                tr("Automatic: 85 % of the allowable opening angle; no deceleration above nq = 80"),
                &mut input.diffuser.outlet_diameter,
                Length,
                dif.map(|d| d.outlet_diameter),
                &units,
                0.005..=10.0,
            );
            label(
                ui,
                tr("Nominal size DN"),
                tr("Round the automatic outlet diameter to the nearest nominal size"),
            );
            ui.checkbox(&mut input.diffuser.nominal_size, "");
            ui.end_row();
            label(
                ui,
                tr("Area growth"),
                tr(
                    "How the area grows from the throat to the outlet. Radius: the equivalent \
                    radius grows linearly, so the area gains slope from the first step. Matched: \
                    quadratic area that starts with the slope of the spiral, with no break at the \
                    throat",
                ),
            );
            let growth = |g: DiffuserGrowth| {
                tr(match g {
                    DiffuserGrowth::Linear => "linear in the radius",
                    DiffuserGrowth::Matched => "matched to the spiral",
                })
            };
            crate::widgets::list(ui, |ui| {
                ComboBox::from_id_salt("volute_growth")
                    .selected_text(crate::widgets::noted(ui, growth(input.diffuser.growth)))
                    .show_ui(ui, |ui| {
                        for option in [DiffuserGrowth::Linear, DiffuserGrowth::Matched] {
                            let same = option == input.diffuser.growth;
                            if ui.selectable_label(same, growth(option)).clicked() && !same {
                                input.diffuser.growth = option;
                            }
                        }
                    })
            });
            ui.end_row();
            param_f64(
                ui,
                tr("Height offset Δr_C"),
                tr(
                    "Shifts the diffuser across its centerline, away from the pump axis when \
                    positive, over the first 40 % of its length: room for the cutwater, whose \
                    tip moves toward the base circle",
                ),
                &mut input.diffuser.height_offset,
                Length,
                Some(0.0),
                &units,
                -0.5..=0.5,
            );
            param_f64(
                ui,
                tr("Base form factor"),
                tr(
                    "How round the diffuser is on its base side, toward the pump axis: 0 keeps \
                    the corners of the last section of the spiral, 1 would round that side to the \
                    circle (the limit is 0.9). It builds up over the first 30 % of the length and changes where the \
                    spiral wall crosses the diffuser wall",
                ),
                &mut input.diffuser.base_form,
                Dimensionless,
                Some(0.0),
                &units,
                0.0..=tp_design::volute::MAX_BASE_FORM,
            );
            label(
                ui,
                tr("Axial alignment"),
                tr(
                    "Where the sections of the diffuser sit along the pump axis. Centric: on the \
                    centerline. Manual: the outlet is shifted by the offset, linearly or \
                    quadratically with the length",
                ),
            );
            let alignment = |a: &ZAlignment| {
                tr(match a {
                    ZAlignment::Centric => "centric",
                    ZAlignment::Manual { .. } => "manual",
                })
            };
            let current = input.diffuser.z_alignment;
            crate::widgets::list(ui, |ui| {
                ComboBox::from_id_salt("volute_z_alignment")
                    .selected_text(crate::widgets::noted(ui, alignment(&current)))
                    .show_ui(ui, |ui| {
                        for option in [
                            ZAlignment::Centric,
                            ZAlignment::Manual {
                                offset: 0.0,
                                quadratic: false,
                            },
                        ] {
                            let same =
                                std::mem::discriminant(&option) == std::mem::discriminant(&current);
                            if ui.selectable_label(same, alignment(&option)).clicked() && !same {
                                input.diffuser.z_alignment = option;
                            }
                        }
                    })
            });
            ui.end_row();
            if let ZAlignment::Manual { offset, quadratic } = &mut input.diffuser.z_alignment {
                let mut value = Param::Manual(*offset);
                param_f64(
                    ui,
                    tr("Axial offset"),
                    tr("Shift of the outlet along the pump axis, positive toward +z"),
                    &mut value,
                    Length,
                    Some(*offset),
                    &units,
                    -0.5..=0.5,
                );
                *offset = value.resolve(|| 0.0);
                label(
                    ui,
                    tr("Quadratic progression"),
                    tr("The shift grows with the square of the length instead of linearly"),
                );
                ui.checkbox(quadratic, "");
                ui.end_row();
            }
            param_f64(
                ui,
                tr("Volute rotation"),
                tr(
                    "Turns the whole volute about the pump axis: the angle of the cutwater from \
                    the x axis; automatic: the discharge points vertically up",
                ),
                &mut input.diffuser.rotation,
                Angle,
                g.map(|g| g.rotation),
                &units,
                (-180f64).to_radians()..=180f64.to_radians(),
            );
        });
        if let Some(d) = dif
            && section_kind(ui, tr("Results"), "", Kind::Results)
        {
            props(ui, "volute_diffuser_results", |ui| {
                let equivalent = |area: f64| (4.0 * area / std::f64::consts::PI).sqrt();
                value_row(
                    ui,
                    tr("Inlet diameter D₁"),
                    units.show(Length, equivalent(d.inlet_area)),
                );
                value_row(ui, tr("Inlet area A₁"), units.show(Area, d.inlet_area));
                value_row(
                    ui,
                    tr("Outlet area A₂"),
                    units.show(Area, std::f64::consts::PI * d.outlet_diameter.powi(2) / 4.0),
                );
                value_row(ui, tr("Area ratio A_R"), format!("{:.2}", d.area_ratio));
                value_row(
                    ui,
                    tr("Velocity ratio c_out/c_in"),
                    format!("{:.3}", 1.0 / d.area_ratio),
                );
                if let Some(g) = g {
                    value_row(
                        ui,
                        tr("Outlet face from the pump axis"),
                        units.show(Length, g.outlet_height),
                    );
                    value_row(
                        ui,
                        tr("Outlet axis offset C"),
                        units.show(Length, g.outlet_offset()),
                    );
                    value_row(
                        ui,
                        tr("Outlet centre angle φ"),
                        units.show(Angle, g.outlet_angle()),
                    );
                }
                if let Some(g) = g
                    && g.nozzle_bend > 0.0
                {
                    value_row(ui, tr("Bend turns"), units.show(Angle, g.nozzle_bend));
                    value_row(
                        ui,
                        tr("Centerline length"),
                        units.show(Length, g.nozzle_length),
                    );
                }
                value_row(
                    ui,
                    tr("Opening angle ϑ_eq"),
                    tf(
                        "{} (allowed {})",
                        &[
                            &units.show(Angle, d.opening_angle),
                            &units.show(Angle, d.allowable_angle),
                        ],
                    ),
                );
                value_row(
                    ui,
                    tr("Outlet velocity"),
                    units.show(Velocity, d.outlet_velocity),
                );
                value_row(
                    ui,
                    tr("Ideal pressure recovery c_p,id"),
                    format!("{:.2}", d.ideal_pressure_recovery),
                );
            });
        }
    }
    "edit volute diffuser"
}

fn cutwater(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    id: ComponentId,
    input: &mut VoluteInput,
) -> &'static str {
    use QuantityKind::*;
    let units = cx.units();
    let g = cx.derived.volute(id);
    if section(ui, tr("Cutwater")) {
        props(ui, "volute_cutwater", |ui| {
            label(
                ui,
                tr("Shape"),
                tr(
                    "Tangent: the spiral starts where it clears the diffuser. Sharp: the spiral \
                 cuts the diffuser wall. Rounded: a nose of radius ρ on the cutwater circle, \
                 its body never thinner than the nose and running smoothly into both walls \
                 (Gülich Fig. 7.41b)",
                ),
            );
            let text = |shape: &CutwaterShape| {
                tr(match shape {
                    CutwaterShape::Tangent => "tangent",
                    CutwaterShape::Sharp => "sharp",
                    CutwaterShape::Rounded { .. } => "rounded",
                })
            };
            let current = input.cutwater.shape;
            crate::widgets::list(ui, |ui| {
                ComboBox::from_id_salt("volute_cutwater_shape")
                    .selected_text(crate::widgets::noted(ui, text(&current)))
                    .show_ui(ui, |ui| {
                        for option in [
                            CutwaterShape::Tangent,
                            CutwaterShape::Sharp,
                            CutwaterShape::Rounded {
                                radius: Param::Auto,
                            },
                        ] {
                            let same =
                                std::mem::discriminant(&option) == std::mem::discriminant(&current);
                            if ui.selectable_label(same, text(&option)).clicked() && !same {
                                input.cutwater.shape = option;
                            }
                        }
                    })
            });
            ui.end_row();
            if let CutwaterShape::Rounded { radius } = &mut input.cutwater.shape {
                param_f64(
                    ui,
                    tr("Nose radius ρ"),
                    tr(
                        "Automatic: e₃/2, a nose as thick as the cutwater; its innermost point \
                        stays on the cutwater circle d_z",
                    ),
                    radius,
                    Length,
                    g.map(|g| g.cutwater.radius),
                    &units,
                    0.0001..=0.1,
                );
                param_f64(
                    ui,
                    tr("Spiral share"),
                    tr(
                        "Share of the thickness the wedge lacks past its tip that the spiral side of the body takes; the diffuser side takes the rest. Automatic: ½",
                    ),
                    &mut input.cutwater.share,
                    Dimensionless,
                    g.map(|g| g.design.cutwater_share),
                    &units,
                    0.0..=1.0,
                );
                param_f64(
                    ui,
                    tr("Obliquity ε"),
                    tr(
                        "Angle between the edge of the cutwater and the pump axis, unrolled onto d_z; positive leans it downstream toward +z. Gülich Table 10.2 asks for more than 35° against pressure pulsations; the walls set how far it can lean. Automatic: 0",
                    ),
                    &mut input.cutwater.obliquity,
                    Angle,
                    g.map(|g| g.design.cutwater_obliquity),
                    &units,
                    -tp_design::volute::MAX_OBLIQUITY..=tp_design::volute::MAX_OBLIQUITY,
                );
                label(
                    ui,
                    tr("Nose shape"),
                    tr(
                        "As a blade edge on a body as thick as the nose: elliptic (round by default; Gülich asks for an elliptic profile), tapered, Bézier or blunt. Its pressure face is the diffuser side",
                    ),
                );
                crate::steps::edge_shape(
                    ui,
                    "volute_cutwater_nose",
                    &mut input.cutwater.nose_shape,
                );
                label(
                    ui,
                    tr("Join to the walls"),
                    tr(
                        "How the faces run into the spiral and diffuser walls: G1 tangent, G2 with the curvature of the wall too",
                    ),
                );
                crate::widgets::list(ui, |ui| {
                    ComboBox::from_id_salt("volute_cutwater_join")
                        .selected_text(match input.cutwater.join {
                            tp_design::volute::Continuity::G1 => "G1",
                            tp_design::volute::Continuity::G2 => "G2",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut input.cutwater.join,
                                tp_design::volute::Continuity::G1,
                                tr("G1, tangent"),
                            );
                            ui.selectable_value(
                                &mut input.cutwater.join,
                                tp_design::volute::Continuity::G2,
                                tr("G2, same curvature"),
                            );
                        })
                });
                ui.end_row();
                param_f64(
                    ui,
                    tr("Side endings R"),
                    tr(
                        "Toward each side wall of the channel the body widens with this round between its faces and the wall, so it meets the wall without a sharp corner; 0 leaves the corner. Automatic: the nose radius ρ for rectangular and trapezoidal sections, 0 for the others",
                    ),
                    &mut input.cutwater.side_radius,
                    Length,
                    g.map(|g| g.design.cutwater_side_radius),
                    &units,
                    0.0..=0.02,
                );
            }
            if !matches!(input.cutwater.shape, CutwaterShape::Tangent) {
                // The starts that see the walls cross in every plane.
                let range = g.and_then(|g| g.start_range);
                let hint = match range {
                    Some((lo, hi)) => tf(
                        "Where the spiral starts, before its wall crosses the diffuser's in every plane: from {} to {} here. Automatic: 3°",
                        &[&units.show(Angle, lo), &units.show(Angle, hi)],
                    ),
                    None => tr(
                        "Where the spiral starts. Its wall does not cross the diffuser's in every plane: it starts where it clears it",
                    )
                    .to_owned(),
                };
                param_f64(
                    ui,
                    tr("Spiral start"),
                    &hint,
                    &mut input.cutwater.spiral_start,
                    Angle,
                    g.map(|g| g.start_angle),
                    &units,
                    0.0..=std::f64::consts::FRAC_PI_2,
                );
            }
            let incidence = input.cutwater.incidence.resolve(|| 0.0);
            param_f64(
                ui,
                tr("Incidence i₃"),
                tr("Between the cutwater and the approaching flow, ±3°"),
                &mut input.cutwater.incidence,
                Angle,
                Some(incidence),
                &units,
                -10f64.to_radians()..=10f64.to_radians(),
            );
        });
        if let Some(g) = g {
            let d = &g.design;
            if section_kind(ui, tr("Results"), "", Kind::Results) {
                props(ui, "volute_cutwater_results", |ui| {
                    value_row(
                        ui,
                        tr("Flow angle α₃'"),
                        units.show(Angle, d.cutwater_flow_angle),
                    );
                    value_row(
                        ui,
                        tr("Cutwater angle α₃B"),
                        units.show(Angle, d.cutwater_angle),
                    );
                    value_row(
                        ui,
                        tr("Thickness e₃"),
                        units.show(Length, d.cutwater_thickness),
                    );
                    value_row(
                        ui,
                        tr("Diameter d_z"),
                        units.show(Length, d.cutwater_diameter),
                    );
                    let c = &g.cutwater;
                    if !matches!(d.cutwater_shape, CutwaterShape::Tangent) {
                        value_row(
                            ui,
                            tr("Tip of the wedge"),
                            tf(
                                "{} from the cutwater, {} from the axis",
                                &[
                                    &units.show(Angle, c.angle),
                                    &units.show(Length, c.tip.truncate().length()),
                                ],
                            ),
                        );
                        value_row(ui, tr("Wedge angle"), units.show(Angle, c.wedge));
                        value_row(
                            ui,
                            tr("Spiral wall angle αIn"),
                            units.show(Angle, c.spiral_angle),
                        );
                        value_row(
                            ui,
                            tr("Diffuser wall angle αOut"),
                            units.show(Angle, c.diffuser_angle),
                        );
                        value_row(
                            ui,
                            tr("Mean wall angle αAvg"),
                            units.show(Angle, c.mean_angle()),
                        );
                        value_row(
                            ui,
                            tr("Walls off the flow angle"),
                            units.show(Angle, g.wall_misalignment()),
                        );
                        value_row(
                            ui,
                            tr("Spiral start φC,0"),
                            units.show(Angle, g.start_angle),
                        );
                    }
                    value_row(
                        ui,
                        tr("Effective cutwater diameter"),
                        units.show(Length, 2.0 * c.nose_distance),
                    );
                    // Across the inlet width.
                    let slices = &g.cutwater_slices;
                    let range = |f: &dyn Fn(&tp_design::volute::CutwaterSlice) -> f64| {
                        slices
                            .iter()
                            .map(f)
                            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                                (lo.min(x), hi.max(x))
                            })
                    };
                    if !slices.is_empty() {
                        let (lo, hi) = range(&|c| c.tip.truncate().length());
                        value_row(
                            ui,
                            tr("Tip across the width"),
                            tf(
                                "{} to {} from the axis",
                                &[&units.show(Length, lo), &units.show(Length, hi)],
                            ),
                        );
                    }
                    if !slices.is_empty() && c.radius > 0.0 {
                        value_row(
                            ui,
                            tr("Thinnest body"),
                            units.show(Length, range(&|c| c.min_thickness).0),
                        );
                        let (lo, hi) = range(&|c| c.join_length);
                        value_row(
                            ui,
                            tr("Joins the walls"),
                            tf(
                                "{} to {} past the tip",
                                &[&units.show(Length, lo), &units.show(Length, hi)],
                            ),
                        );
                    }
                });
            }
        }
    }
    "edit volute cutwater"
}

fn wall(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    id: ComponentId,
    input: &mut VoluteInput,
) -> &'static str {
    let units = cx.units();
    let g = cx.derived.volute(id);
    if section(ui, tr("Wall")) {
        props(ui, "volute_wall", |ui| {
            param_f64(
                ui,
                tr("Wall thickness"),
                tr("Automatic: 3 % of d₂, at least 6 mm"),
                &mut input.wall.thickness,
                QuantityKind::Length,
                g.map(|g| g.design.wall_thickness),
                &units,
                0.0005..=0.2,
            );
            param_f64(
                ui,
                tr("Fillet over the cutwater"),
                tr(
                    "Fills and rounds the outside of the casing where the back of the start of the spiral meets the diffuser, so no sharp groove is left over the cutwater; 0 leaves it. Automatic: the wall thickness",
                ),
                &mut input.wall.cutwater_fillet,
                QuantityKind::Length,
                g.map(|g| g.design.cutwater_fillet),
                &units,
                0.0..=0.1,
            );
        });
    }
    if section_info(
        ui,
        tr("Wall stresses"),
        tr("Each section as a thin torus of the same area: an estimate to choose the thickness"),
    ) {
        let w = g.map(|g| g.wall_stress);
        props(ui, "volute_wall_stress", |ui| {
            param_f64(
                ui,
                tr("Casing pressure"),
                tr(
                    "Over the suction pressure; automatic: the rise at shut-off (Gülich eqs. 3.26 and 3.28)",
                ),
                &mut input.wall.pressure,
                QuantityKind::Pressure,
                w.map(|w| w.pressure),
                &units,
                1e3..=1e9,
            );
            param_f64(
                ui,
                tr("Allowable stress"),
                tr(
                    "Of the casing material; automatic: 50 MPa, about a fifth of a grey cast iron of 250 MPa",
                ),
                &mut input.wall.allowable_stress,
                QuantityKind::Pressure,
                w.map(|w| w.allowable),
                &units,
                1e5..=1e10,
            );
        });
        if let Some(w) = w {
            props(ui, "volute_wall_stress_results", |ui| {
                let place = match w.place {
                    StressPlace::Spiral { angle } => {
                        tf("section at {}", &[&units.show(QuantityKind::Angle, angle)])
                    }
                    StressPlace::Bend => tr("bend of the discharge").to_owned(),
                    StressPlace::Straight => tr("straight discharge").to_owned(),
                };
                value_row(
                    ui,
                    tr("Stress round the section σ₁"),
                    tf(
                        "{} at the {}",
                        &[&units.show(QuantityKind::Pressure, w.hoop), &place],
                    ),
                );
                value_row(
                    ui,
                    tr("Stress along σ₂"),
                    units.show(QuantityKind::Pressure, w.longitudinal),
                );
                value_row(
                    ui,
                    tr("Wall for the allowable stress"),
                    units.show(QuantityKind::Length, w.required_thickness),
                );
            });
        }
    }
    if section(ui, tr("Outlet flange")) {
        props(ui, "volute_flange_on", |ui| {
            label(
                ui,
                tr("Flange on the casing"),
                tr(
                    "Automatic sizes from proportions of our own, not a standard: set them to the \
                    standard in use",
                ),
            );
            ui.checkbox(&mut input.wall.flange.enabled, "");
            ui.end_row();
        });
        if input.wall.flange.enabled {
            let f = g.and_then(|g| g.design.flange);
            let flange = &mut input.wall.flange;
            props(ui, "volute_flange", |ui| {
                let rows: [(&str, &str, &mut Param<f64>, Option<f64>); 4] = [
                    (
                        tr("Outer diameter"),
                        tr("Automatic: the bolt circle plus three holes"),
                        &mut flange.outer_diameter,
                        f.map(|f| f.outer_diameter),
                    ),
                    (
                        tr("Thickness"),
                        tr("Automatic: twice the wall, at least 0.15·d + 6 mm"),
                        &mut flange.thickness,
                        f.map(|f| f.thickness),
                    ),
                    (
                        tr("Bolt circle"),
                        tr("Automatic: the bore plus twice the wall and three holes"),
                        &mut flange.bolt_circle,
                        f.map(|f| f.bolt_circle),
                    ),
                    (
                        tr("Bolt hole"),
                        tr("Automatic: 12 % of the bore, at least 14 mm"),
                        &mut flange.bolt_hole,
                        f.map(|f| f.bolt_hole),
                    ),
                ];
                for (label, hint, param, value) in rows {
                    param_f64(
                        ui,
                        label,
                        hint,
                        param,
                        QuantityKind::Length,
                        value,
                        &units,
                        0.001..=5.0,
                    );
                }
                param_u32(
                    ui,
                    tr("Bolts"),
                    tr("Automatic: in fours, about six holes apart"),
                    &mut flange.bolts,
                    f.map(|f| f.bolts),
                    3..=64,
                );
            });
        }
    }
    "edit volute wall"
}
