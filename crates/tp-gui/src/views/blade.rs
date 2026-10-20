//! Blade plots along the normalized meridional length m̄: blade angle, wrap angle and
//! thickness of every section.

use eframe::egui::Ui;
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};
use tp_design::impeller::blade::BladeSection;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::{Palette, unavailable};
use crate::derived::Derived;

/// Value of a blade section at a station, in display units.
type Quantity<'a> = &'a dyn Fn(&BladeSection, usize) -> f64;

/// Points (m̄, value) of one section.
fn along(section: &BladeSection, value: impl Fn(usize) -> f64) -> PlotPoints<'static> {
    let total = section.m.last().copied().unwrap_or(1.0).max(f64::EPSILON);
    section
        .m
        .iter()
        .enumerate()
        .map(|(i, m)| [m / total, value(i)])
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
    let sections = &g.blade.sections;
    let n = sections.len();
    let angle = units.get(QuantityKind::Angle).symbol;
    let length = units.get(QuantityKind::Length).symbol;
    let spacing = ui.spacing().item_spacing.y;
    let height = ((ui.available_height() - 2.0 * spacing) / 3.0).max(120.0);

    let plots: [(&str, String, Quantity); 3] = [
        ("blade_beta", format!("β_B [{angle}]"), &|s, i| {
            units.to_display(QuantityKind::Angle, s.beta[i])
        }),
        ("blade_theta", format!("θ [{angle}]"), &|s, i| {
            units.to_display(QuantityKind::Angle, s.theta[i])
        }),
        ("blade_thickness", format!("e [{length}]"), &|s, i| {
            units.to_display(QuantityKind::Length, s.thickness[i])
        }),
    ];
    for (index, (name, label, value)) in plots.into_iter().enumerate() {
        let mut plot = Plot::new((name, id.0))
            .height(height)
            .y_axis_label(label)
            .link_axis("blade_plots", [true, false])
            .link_cursor("blade_plots", [true, false])
            .y_axis_min_width(40.0)
            .allow_scroll(false);
        if index == 0 {
            plot = plot.legend(Legend::default().position(Corner::RightTop));
        }
        if index == 2 {
            plot =
                plot.x_axis_label("relative meridional length (0 leading edge, 1 trailing edge)");
        }
        plot.show(ui, |plot| {
            for (i, s) in sections.iter().enumerate() {
                plot.line(
                    Line::new(format!("span {:.2}", s.span), along(s, |k| value(s, k)))
                        .color(palette.span(i, n))
                        .width(1.5),
                );
            }
        });
    }
}
