//! Step editors of the volute: setup, cross section, spiral, discharge diffuser,
//! cutwater and wall thickness.

use eframe::egui::{ComboBox, DragValue, Grid, Ui};
use egui_plot::{Line, Plot, PlotPoints};
use tp_design::Param;
use tp_design::volute::{
    AreaLaw, CutwaterShape, DEFAULT_FLANK_ANGLE_DEG, DischargeShape, SectionShape, StraightWall,
    VoluteInput, VoluteKind,
};
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
    let Ok(mut input) = project.volute_input(id) else {
        return;
    };
    let before = input.clone();
    let label = match step {
        DesignStep::VoluteSetup => setup(ui, cx, id, &mut input),
        DesignStep::CrossSection => cross_section(ui, cx, id, &mut input),
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
    ui.weak(tr(
        "The volute collects the flow of the impeller before it: Q, d₂, b₂ and c₂u come from \
         there. Gülich 2nd ed., Chap. 7.8.",
    ));
    section(ui, tr("Main dimensions"));
    grid("volute_setup").show(ui, |ui| {
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
    section(ui, tr("Type"));
    grid("volute_kind").show(ui, |ui| {
        ui.label(tr("Volute")).on_hover_text(tr(
            "A double volute splits the spiral with a rib to cut the radial force \
             (Gülich Chap. 7.8.1 and Table 9.4)",
        ));
        let text = |k: VoluteKind| {
            tr(match k {
                VoluteKind::Single => "single",
                VoluteKind::Double => "double",
            })
        };
        ComboBox::from_id_salt("volute_kind")
            .selected_text(text(input.setup.kind))
            .show_ui(ui, |ui| {
                for kind in [VoluteKind::Single, VoluteKind::Double] {
                    ui.selectable_value(&mut input.setup.kind, kind, text(kind));
                }
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
                tr("Outer channel A_out/A_in"),
                tr(
                    "Area ratio of the outer channel from the rib nose to the discharge; \
                    automatic: constant area",
                ),
                &mut input.setup.channel_area_ratio,
                Dimensionless,
                rib.map(|r| r.channel_outlet_area / r.channel_inlet_area),
                &units,
                0.8..=3.0,
            );
        }
    });
    if let Some(g) = g {
        let d = &g.design;
        section(ui, tr("Results"));
        Grid::new("volute_setup_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(ui, tr("Design flow Q_Le"), units.show(VolumeFlow, d.flow));
                value_row(
                    ui,
                    tr("d_z/d₂"),
                    tf(
                        "{} (minimum {}, Table 10.2)",
                        &[
                            &format!("{:.3}", d.cutwater_diameter / g.basis.impeller_diameter),
                            &format!("{:.3}", d.min_cutwater_diameter / g.basis.impeller_diameter),
                        ],
                    ),
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
    "edit volute setup"
}

fn cross_section(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    id: ComponentId,
    input: &mut VoluteInput,
) -> &'static str {
    let units = cx.units();
    let angle_unit = units.get(QuantityKind::Angle).symbol;
    ui.weak(tr(
        "Flat sections with B/H = 2 to 3 lose least (Gülich p. 414).",
    ));
    section(ui, tr("Shape"));
    Grid::new("volute_shape")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            let shape = &mut input.cross_section.shape;
            ui.label(tr("Cross section"));
            let name = match shape {
                SectionShape::Rectangular => "rectangular",
                SectionShape::Trapezoidal { .. } => "trapezoidal",
                SectionShape::Circular => "circular",
            };
            ComboBox::from_id_salt("volute_shape")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(name == tr("rectangular"), tr("rectangular"))
                        .clicked()
                    {
                        *shape = SectionShape::Rectangular;
                    }
                    if ui
                        .selectable_label(name == tr("trapezoidal"), tr("trapezoidal"))
                        .clicked()
                        && name != "trapezoidal"
                    {
                        *shape = SectionShape::Trapezoidal {
                            flank_angle: DEFAULT_FLANK_ANGLE_DEG.to_radians(),
                        };
                    }
                    if ui
                        .selectable_label(name == tr("circular"), tr("circular"))
                        .clicked()
                    {
                        *shape = SectionShape::Circular;
                    }
                });
            ui.end_row();
            if let SectionShape::Trapezoidal { flank_angle } = shape {
                ui.label(tr("Flank angle δ"))
                    .on_hover_text(tr("Angle of the side walls from the radial plane"));
                let mut shown = units.to_display(QuantityKind::Angle, *flank_angle);
                let max = units.to_display(QuantityKind::Angle, 60f64.to_radians());
                if ui
                    .add(
                        DragValue::new(&mut shown)
                            .range(0.0..=max)
                            .speed(0.1)
                            .suffix(format!(" {angle_unit}"))
                            .update_while_editing(false),
                    )
                    .changed()
                {
                    *flank_angle = units.from_display(QuantityKind::Angle, shown);
                }
                ui.end_row();
            }
        });
    if let Some(g) = cx.derived.volute(id) {
        let d = &g.design;
        section(ui, tr("Throat"));
        Grid::new("volute_shape_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(
                    ui,
                    tr("Height"),
                    units.show(QuantityKind::Length, d.throat.height),
                );
                value_row(
                    ui,
                    tr("Width over height B/H"),
                    tf("{} (2 to 3)", &[&format!("{:.2}", d.throat_aspect)]),
                );
                value_row(
                    ui,
                    tr("Area A₃q"),
                    units.show(QuantityKind::Area, d.throat.area),
                );
            });
    }
    "edit volute cross section"
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
    ui.weak(tr("Below nq = 30 the sections grow linearly (constant velocity), above it they keep c_u·r constant."));
    section(ui, tr("Area law"));
    Grid::new("volute_law")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label(tr("Law"));
            let resolved = g.map(|g| g.design.law);
            let text = |law: AreaLaw| {
                tr(match law {
                    AreaLaw::Auto => "automatic",
                    AreaLaw::AngularMomentum => "angular momentum",
                    AreaLaw::ConstantVelocity => "constant velocity",
                })
            };
            let selected = match (input.spiral.law, resolved) {
                (AreaLaw::Auto, Some(law)) => tf("automatic: {}", &[&text(law)]),
                (law, _) => text(law).to_owned(),
            };
            ComboBox::from_id_salt("volute_law")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for law in [
                        AreaLaw::Auto,
                        AreaLaw::AngularMomentum,
                        AreaLaw::ConstantVelocity,
                    ] {
                        ui.selectable_value(&mut input.spiral.law, law, text(law));
                    }
                });
            ui.end_row();
            ui.label(tr("Cutwater correction")).on_hover_text(tr(
                "Enlarge the throat for the blockage of the cutwater (eq. 7.28)",
            ));
            ui.checkbox(&mut input.spiral.cutwater_correction, "");
            ui.end_row();
        });
    grid("volute_sections").show(ui, |ui| {
        param_u32(
            ui,
            tr("Sections"),
            tr("Cross sections from the start of the spiral to the throat"),
            &mut input.spiral.sections,
            g.map(|g| g.sections.len() as u32 - 1),
            4..=180,
        );
    });
    if let Some(g) = g {
        let d = &g.design;
        section(ui, tr("Throat"));
        Grid::new("volute_spiral_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(ui, tr("Area A₃q"), units.show(Area, d.throat.area));
                value_row(
                    ui,
                    tr("Outer radius r_a"),
                    units.show(Length, d.throat.outer_radius),
                );
                value_row(
                    ui,
                    tr("Cutwater correction Δa₃"),
                    units.show(Length, d.throat_correction),
                );
                value_row(
                    ui,
                    tr("Velocity c₃q"),
                    units.show(Velocity, d.throat_velocity),
                );
                value_row(
                    ui,
                    tr("c₃q/c₂"),
                    tf(
                        "{} (not much below 0.8, Table 10.2)",
                        &[&format!("{:.2}", d.deceleration)],
                    ),
                );
                value_row(ui, tr("Spiral starts at"), units.show(Angle, g.start_angle));
            });
        section(ui, tr("Area along the spiral"));
        let area_scale = units.to_display(Area, 1.0);
        let points: PlotPoints = g
            .sections
            .iter()
            .map(|s| [units.to_display(Angle, s.angle), s.size.area * area_scale])
            .collect();
        Plot::new(("volute_area", id.0))
            .height(160.0)
            .x_axis_label(format!("ε [{}]", units.get(Angle).symbol))
            .y_axis_label(format!("A [{}]", units.get(Area).symbol))
            .allow_scroll(false)
            .show(ui, |plot| {
                plot.line(Line::new(tr("area"), points).width(2.0));
            });
    }
    "edit volute spiral"
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
    ui.weak(tr("Conical diffuser from the throat to the discharge nozzle, straight or with a bend, checked against the allowable opening angle (Gülich eqs. 1.44, 1.45)."));
    section(ui, tr("Discharge diffuser"));
    grid("volute_diffuser").show(ui, |ui| {
        ui.label(tr("Shape")).on_hover_text(tr(
            "Tangential: straight along the flow at the throat. Radial: a bend turning the \
             flow away from the pump axis (Gülich Fig. 7.42)",
        ));
        let text = |shape: &DischargeShape| {
            tr(match shape {
                DischargeShape::Tangential {
                    straight_wall: StraightWall::Inner,
                } => "tangential, cutwater wall straight",
                DischargeShape::Tangential {
                    straight_wall: StraightWall::Centered,
                } => "tangential, centred",
                DischargeShape::Tangential {
                    straight_wall: StraightWall::Outer,
                } => "tangential, outer wall straight",
                DischargeShape::Radial { .. } => "radial",
            })
        };
        let current = input.diffuser.shape;
        ComboBox::from_id_salt("volute_discharge")
            .selected_text(text(&current))
            .show_ui(ui, |ui| {
                for option in [
                    DischargeShape::Tangential {
                        straight_wall: StraightWall::Inner,
                    },
                    DischargeShape::Tangential {
                        straight_wall: StraightWall::Centered,
                    },
                    DischargeShape::Tangential {
                        straight_wall: StraightWall::Outer,
                    },
                    DischargeShape::Radial { axis_angle: 0.0 },
                ] {
                    let same = std::mem::discriminant(&option) == std::mem::discriminant(&current)
                        && (matches!(option, DischargeShape::Radial { .. }) || option == current);
                    if ui.selectable_label(same, text(&option)).clicked() && !same {
                        input.diffuser.shape = option;
                    }
                }
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
            tr("Along the diffuser axis from the throat; automatic: four throat radii"),
            &mut input.diffuser.length,
            Length,
            dif.map(|d| d.length),
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
    });
    if let Some(d) = dif {
        section(ui, tr("Results"));
        Grid::new("volute_diffuser_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(ui, tr("Area ratio A_R"), format!("{:.2}", d.area_ratio));
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
    ui.weak(tr("The cutwater meets the flow leaving the impeller; its camber angle follows the flow angle plus a small incidence (Gülich eqs. T7.7.3 to T7.7.6)."));
    section(ui, tr("Cutwater"));
    grid("volute_cutwater").show(ui, |ui| {
        ui.label(tr("Shape")).on_hover_text(tr(
            "Tangent: the spiral starts where it clears the diffuser. Sharp: the spiral \
             cuts the diffuser wall. Rounded: a nose of radius ρ on the cutwater circle \
             (Gülich Fig. 7.41b)",
        ));
        let text = |shape: &CutwaterShape| {
            tr(match shape {
                CutwaterShape::Tangent => "tangent",
                CutwaterShape::Sharp => "sharp",
                CutwaterShape::Rounded { .. } => "rounded",
            })
        };
        let current = input.cutwater.shape;
        ComboBox::from_id_salt("volute_cutwater_shape")
            .selected_text(text(&current))
            .show_ui(ui, |ui| {
                for option in [
                    CutwaterShape::Tangent,
                    CutwaterShape::Sharp,
                    CutwaterShape::Rounded {
                        radius: Param::Auto,
                    },
                ] {
                    let same = std::mem::discriminant(&option) == std::mem::discriminant(&current);
                    if ui.selectable_label(same, text(&option)).clicked() && !same {
                        input.cutwater.shape = option;
                    }
                }
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
        section(ui, tr("Results"));
        Grid::new("volute_cutwater_results")
            .num_columns(2)
            .show(ui, |ui| {
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
                }
                value_row(
                    ui,
                    tr("Effective cutwater diameter"),
                    units.show(Length, 2.0 * c.nose_distance),
                );
            });
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
    ui.weak(tr("Casing wall around the flow domain."));
    section(ui, tr("Wall"));
    grid("volute_wall").show(ui, |ui| {
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
    });
    "edit volute wall"
}
