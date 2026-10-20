//! Front view seen from the inlet looking downstream: blade outlines at hub and shroud,
//! and the outlet, eye and hub circles.

use std::f64::consts::PI;

use eframe::egui::Ui;
use egui_plot::{Corner, Legend, Line, LineStyle, Plot, PlotPoints};
use tp_math::DVec3;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::{Palette, unavailable};
use crate::derived::Derived;

/// Looking along +z the x axis points left, as in the SVG drawing.
fn view(x: f64, y: f64, scale: f64) -> [f64; 2] {
    [-x * scale, y * scale]
}

fn circle(r: f64, scale: f64) -> PlotPoints<'static> {
    (0..=180)
        .map(|i| {
            let a = 2.0 * PI * f64::from(i) / 180.0;
            view(r * a.cos(), r * a.sin(), scale)
        })
        .collect()
}

pub(crate) fn show(
    ui: &mut Ui,
    derived: &Derived,
    id: ComponentId,
    stator: Option<ComponentId>,
    volute: Option<ComponentId>,
    units: &DisplayUnits,
) {
    let stator = stator.and_then(|s| derived.diffuser(s));
    let volute = volute.and_then(|v| derived.volute(v));
    let Some(g) = derived.geometry(id) else {
        let reason = derived
            .impeller(id)
            .and_then(|r| r.geometry.as_ref().err())
            .map_or("No geometry yet.", String::as_str);
        unavailable(ui, reason);
        return;
    };
    let palette = Palette::of(ui);
    let scale = units.to_display(QuantityKind::Length, 1.0);
    let unit = units.get(QuantityKind::Length).symbol;
    let blade = &g.blade;
    let sign = blade.rotation_sign;
    let outline = |s: &tp_design::impeller::blade::BladeSection| -> Vec<DVec3> {
        let mut points = s.pressure_side(sign);
        points.extend(s.suction_side(sign).into_iter().rev());
        points.push(points[0]);
        points
    };
    let outlines = [
        (
            "hub section",
            blade.sections.first().map(outline),
            palette.hub,
        ),
        (
            "shroud section",
            blade.sections.last().map(outline),
            palette.shroud,
        ),
    ];

    let response = Plot::new(("front", id.0))
        .data_aspect(1.0)
        .x_axis_label(format!("x [{unit}]"))
        .y_axis_label(format!("y [{unit}]"))
        .y_axis_min_width(40.0)
        .legend(Legend::default().position(Corner::LeftTop))
        .show(ui, |plot| {
            let b = &g.basis;
            plot.line(
                Line::new("outlet d₂", circle(b.outlet_diameter / 2.0, scale))
                    .color(palette.guide)
                    .width(1.5),
            );
            for (name, d) in [("eye d₁", b.inlet_diameter), ("hub d_n", b.hub_diameter)] {
                plot.line(
                    Line::new(name, circle(d / 2.0, scale))
                        .color(palette.guide)
                        .style(LineStyle::dashed_loose())
                        .width(1.0),
                );
            }
            if let Some(d) = stator {
                for r in [d.inlet_diameter / 2.0, d.outlet_diameter / 2.0] {
                    plot.line(
                        Line::new("diffuser", circle(r, scale))
                            .color(palette.stator)
                            .style(LineStyle::dashed_loose())
                            .width(1.0),
                    );
                }
                if let Some(vanes) = &d.vanes {
                    for k in 0..vanes.count {
                        let mut outline = vanes.geometry.outline(k, d.rotation_sign);
                        outline.push(outline[0]);
                        let points: PlotPoints =
                            outline.iter().map(|p| view(p.x, p.y, scale)).collect();
                        plot.line(
                            Line::new("diffuser vanes", points)
                                .color(palette.stator)
                                .width(1.5),
                        );
                    }
                }
            }
            if let Some(v) = volute {
                let sign = v.basis.rotation_sign;
                // Outer wall of the spiral, from the start to the throat.
                let outer: PlotPoints = v
                    .sections
                    .iter()
                    .map(|s| {
                        let r = s.outline.iter().map(|p| p.x).fold(0.0, f64::max);
                        let t = sign * s.angle;
                        view(r * t.cos(), r * t.sin(), scale)
                    })
                    .collect();
                plot.line(Line::new("volute", outer).color(palette.volute).width(2.0));
                plot.line(
                    Line::new(
                        "cutwater d_z",
                        circle(v.design.cutwater_diameter / 2.0, scale),
                    )
                    .color(palette.volute)
                    .style(LineStyle::dashed_loose())
                    .width(1.0),
                );
                // Side walls of the discharge diffuser.
                let axis = v.nozzle_axis;
                let across = DVec3::new(-axis.y, axis.x, 0.0);
                let mut walls = (Vec::new(), Vec::new());
                for n in &v.nozzle {
                    let along = |p: &DVec3| (*p - n.center).dot(across);
                    let low = n.points.iter().min_by(|a, b| along(a).total_cmp(&along(b)));
                    let high = n.points.iter().max_by(|a, b| along(a).total_cmp(&along(b)));
                    if let (Some(l), Some(h)) = (low, high) {
                        walls.0.push(view(l.x, l.y, scale));
                        walls.1.push(view(h.x, h.y, scale));
                    }
                }
                if let (Some(&a), Some(&b)) = (walls.0.last(), walls.1.last()) {
                    plot.line(
                        Line::new("volute", vec![a, b])
                            .color(palette.volute)
                            .width(2.0),
                    );
                }
                plot.line(
                    Line::new("volute", walls.0)
                        .color(palette.volute)
                        .width(2.0),
                );
                plot.line(
                    Line::new("volute", walls.1)
                        .color(palette.volute)
                        .width(2.0),
                );
            }
            for i in 0..blade.count {
                let angle = 2.0 * PI * f64::from(i) / f64::from(blade.count);
                let (sin, cos) = angle.sin_cos();
                for (name, points, color) in &outlines {
                    let Some(points) = points else { continue };
                    let rotated: PlotPoints = points
                        .iter()
                        .map(|p| view(cos * p.x - sin * p.y, sin * p.x + cos * p.y, scale))
                        .collect();
                    plot.line(Line::new(*name, rotated).color(*color).width(1.5));
                }
            }
        });
    super::settle(ui, ("front", id.0), &response.transform);
}
