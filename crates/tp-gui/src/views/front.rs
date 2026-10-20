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

pub(crate) fn show(ui: &mut Ui, derived: &Derived, id: ComponentId, units: &DisplayUnits) {
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

    Plot::new(("front", id.0))
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
}
