//! Step editors of the volute: setup, cross section, spiral, discharge diffuser,
//! cutwater and wall thickness.

use eframe::egui::{ComboBox, DragValue, Grid, Ui};
use egui_plot::{Line, Plot, PlotPoints};
use tp_design::volute::{AreaLaw, DEFAULT_FLANK_ANGLE_DEG, SectionShape, VoluteInput};
use tp_project::{ComponentId, DesignStep, Project};
use tp_units::QuantityKind;

use crate::app::EditCtx;
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
    ui.weak(
        "The volute collects the flow of the impeller before it: Q, d₂, b₂ and c₂u come from \
         there. Gülich 2nd ed., Chap. 7.8.",
    );
    section(ui, "Main dimensions");
    grid("volute_setup").show(ui, |ui| {
        let flow_factor = g.map(|g| g.design.flow / g.basis.flow);
        param_f64(
            ui,
            "Flow factor Q_Le/Q",
            "Allowance for leakage that flows through the volute (eq. T7.7.1)",
            &mut input.setup.flow_factor,
            Dimensionless,
            flow_factor,
            &units,
            0.8..=1.5,
        );
        param_f64(
            ui,
            "Cutwater diameter d_z",
            "Base circle; automatic: the smallest gap of Gülich Table 10.2",
            &mut input.setup.cutwater_diameter,
            Length,
            d.map(|d| d.cutwater_diameter),
            &units,
            0.01..=10.0,
        );
        param_f64(
            ui,
            "Inlet width b₃",
            "Width of the volute at the base circle; b₃/b₂ from 2.0 at low nq to 1.2",
            &mut input.setup.inlet_width,
            Length,
            d.map(|d| d.inlet_width),
            &units,
            0.001..=1.0,
        );
        param_f64(
            ui,
            "Cutwater thickness e₃",
            "Nose of the cutwater; about 0.02·d₂ (Gülich Chap. 7.8.1)",
            &mut input.setup.cutwater_thickness,
            Length,
            d.map(|d| d.cutwater_thickness),
            &units,
            0.0002..=0.2,
        );
    });
    if let Some(g) = g {
        let d = &g.design;
        section(ui, "Results");
        Grid::new("volute_setup_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(ui, "Design flow Q_Le", units.show(VolumeFlow, d.flow));
                value_row(
                    ui,
                    "d_z/d₂",
                    format!(
                        "{:.3} (minimum {:.3}, Table 10.2)",
                        d.cutwater_diameter / g.basis.impeller_diameter,
                        d.min_cutwater_diameter / g.basis.impeller_diameter
                    ),
                );
                value_row(
                    ui,
                    "Stagnation radius r'_z",
                    units.show(Length, d.stagnation_radius),
                );
                value_row(ui, "c₂u of the impeller", units.show(Velocity, g.basis.cu));
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
    ui.weak("Flat sections with B/H = 2 to 3 lose least (Gülich p. 414).");
    section(ui, "Shape");
    Grid::new("volute_shape")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            let shape = &mut input.cross_section.shape;
            ui.label("Cross section");
            let name = match shape {
                SectionShape::Rectangular => "rectangular",
                SectionShape::Trapezoidal { .. } => "trapezoidal",
                SectionShape::Circular => "circular",
            };
            ComboBox::from_id_salt("volute_shape")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(name == "rectangular", "rectangular")
                        .clicked()
                    {
                        *shape = SectionShape::Rectangular;
                    }
                    if ui
                        .selectable_label(name == "trapezoidal", "trapezoidal")
                        .clicked()
                        && name != "trapezoidal"
                    {
                        *shape = SectionShape::Trapezoidal {
                            flank_angle: DEFAULT_FLANK_ANGLE_DEG.to_radians(),
                        };
                    }
                    if ui
                        .selectable_label(name == "circular", "circular")
                        .clicked()
                    {
                        *shape = SectionShape::Circular;
                    }
                });
            ui.end_row();
            if let SectionShape::Trapezoidal { flank_angle } = shape {
                ui.label("Flank angle δ")
                    .on_hover_text("Angle of the side walls from the radial plane");
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
        section(ui, "Throat");
        Grid::new("volute_shape_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(
                    ui,
                    "Height",
                    units.show(QuantityKind::Length, d.throat.height),
                );
                value_row(
                    ui,
                    "Width over height B/H",
                    format!("{:.2} (2 to 3)", d.throat_aspect),
                );
                value_row(
                    ui,
                    "Area A₃q",
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
    ui.weak("Below nq = 30 the sections grow linearly (constant velocity), above it they keep c_u·r constant.");
    section(ui, "Area law");
    Grid::new("volute_law")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label("Law");
            let resolved = g.map(|g| g.design.law);
            let text = |law: AreaLaw| match law {
                AreaLaw::Auto => "automatic",
                AreaLaw::AngularMomentum => "angular momentum",
                AreaLaw::ConstantVelocity => "constant velocity",
            };
            let selected = match (input.spiral.law, resolved) {
                (AreaLaw::Auto, Some(law)) => format!("automatic: {}", text(law)),
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
            ui.label("Cutwater correction")
                .on_hover_text("Enlarge the throat for the blockage of the cutwater (eq. 7.28)");
            ui.checkbox(&mut input.spiral.cutwater_correction, "");
            ui.end_row();
        });
    grid("volute_sections").show(ui, |ui| {
        param_u32(
            ui,
            "Sections",
            "Cross sections from the start of the spiral to the throat",
            &mut input.spiral.sections,
            g.map(|g| g.sections.len() as u32 - 1),
            4..=180,
        );
    });
    if let Some(g) = g {
        let d = &g.design;
        section(ui, "Throat");
        Grid::new("volute_spiral_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(ui, "Area A₃q", units.show(Area, d.throat.area));
                value_row(
                    ui,
                    "Outer radius r_a",
                    units.show(Length, d.throat.outer_radius),
                );
                value_row(
                    ui,
                    "Cutwater correction Δa₃",
                    units.show(Length, d.throat_correction),
                );
                value_row(ui, "Velocity c₃q", units.show(Velocity, d.throat_velocity));
                value_row(
                    ui,
                    "c₃q/c₂",
                    format!("{:.2} (not much below 0.8, Table 10.2)", d.deceleration),
                );
                value_row(ui, "Spiral starts at", units.show(Angle, g.start_angle));
            });
        section(ui, "Area along the spiral");
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
                plot.line(Line::new("area", points).width(2.0));
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
    ui.weak("Straight conical diffuser from the throat to the discharge nozzle, checked against the allowable opening angle (Gülich eqs. 1.44, 1.45).");
    section(ui, "Discharge diffuser");
    grid("volute_diffuser").show(ui, |ui| {
        param_f64(
            ui,
            "Length",
            "Along the diffuser axis from the throat; automatic: four throat radii",
            &mut input.diffuser.length,
            Length,
            dif.map(|d| d.length),
            &units,
            0.005..=10.0,
        );
        param_f64(
            ui,
            "Outlet diameter",
            "Automatic: 85 % of the allowable opening angle; no deceleration above nq = 80",
            &mut input.diffuser.outlet_diameter,
            Length,
            dif.map(|d| d.outlet_diameter),
            &units,
            0.005..=10.0,
        );
    });
    if let Some(d) = dif {
        section(ui, "Results");
        Grid::new("volute_diffuser_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(ui, "Area ratio A_R", format!("{:.2}", d.area_ratio));
                value_row(
                    ui,
                    "Opening angle ϑ_eq",
                    format!(
                        "{} (allowed {})",
                        units.show(Angle, d.opening_angle),
                        units.show(Angle, d.allowable_angle)
                    ),
                );
                value_row(
                    ui,
                    "Outlet velocity",
                    units.show(Velocity, d.outlet_velocity),
                );
                value_row(
                    ui,
                    "Ideal pressure recovery c_p,id",
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
    ui.weak("The cutwater meets the flow leaving the impeller; its camber angle follows the flow angle plus a small incidence (Gülich eqs. T7.7.3 to T7.7.6).");
    section(ui, "Cutwater");
    grid("volute_cutwater").show(ui, |ui| {
        let incidence = input.cutwater.incidence.resolve(|| 0.0);
        param_f64(
            ui,
            "Incidence i₃",
            "Between the cutwater and the approaching flow, ±3°",
            &mut input.cutwater.incidence,
            Angle,
            Some(incidence),
            &units,
            -10f64.to_radians()..=10f64.to_radians(),
        );
    });
    if let Some(g) = g {
        let d = &g.design;
        section(ui, "Results");
        Grid::new("volute_cutwater_results")
            .num_columns(2)
            .show(ui, |ui| {
                value_row(
                    ui,
                    "Flow angle α₃'",
                    units.show(Angle, d.cutwater_flow_angle),
                );
                value_row(
                    ui,
                    "Cutwater angle α₃B",
                    units.show(Angle, d.cutwater_angle),
                );
                value_row(ui, "Thickness e₃", units.show(Length, d.cutwater_thickness));
                value_row(ui, "Diameter d_z", units.show(Length, d.cutwater_diameter));
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
    ui.weak("Casing wall around the flow domain.");
    section(ui, "Wall");
    grid("volute_wall").show(ui, |ui| {
        param_f64(
            ui,
            "Wall thickness",
            "Automatic: 3 % of d₂, at least 6 mm",
            &mut input.wall.thickness,
            QuantityKind::Length,
            g.map(|g| g.design.wall_thickness),
            &units,
            0.0005..=0.2,
        );
    });
    "edit volute wall"
}
