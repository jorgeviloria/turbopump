//! Blade plots along the normalized meridional length m̄: blade angle, wrap angle,
//! thickness and the relative velocity on both faces of every section.

use eframe::egui::Ui;
use egui_plot::{Corner, Legend, Line, LineStyle, Plot, PlotPoints};
use tp_design::impeller::blade::BladeSection;
use tp_design::impeller::loading::blade_velocities;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::{Palette, unavailable};
use crate::derived::Derived;
use crate::i18n::{tf, tr};

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
            .map_or_else(
                || tr("No geometry yet.").to_owned(),
                |e| crate::i18n::message(e),
            );
        unavailable(ui, &reason);
        return;
    };
    let palette = Palette::of(ui);
    let sections = &g.blade.sections;
    let n = sections.len();
    let angle = units.get(QuantityKind::Angle).symbol;
    let length = units.get(QuantityKind::Length).symbol;
    let spacing = ui.spacing().item_spacing.y;
    let height = ((ui.available_height() - 3.0 * spacing) / 4.0).max(100.0);

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
            .x_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
            .y_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
            .y_axis_min_width(40.0)
            .allow_scroll(false);
        if index == 0 {
            plot = plot.legend(Legend::default().position(Corner::RightTop));
        }

        let response = plot.show(ui, |plot| {
            for (i, s) in sections.iter().enumerate() {
                plot.line(
                    Line::new(
                        tf("span {}", &[&format!("{:.2}", s.span)]),
                        along(s, |k| value(s, k)),
                    )
                    .color(palette.span(i, n))
                    .width(1.5),
                );
            }
        });
        super::settle(ui, (name, id.0), &response.transform);
    }

    // Relative velocity on the faces: suction solid, pressure dashed.
    let velocities = blade_velocities(g);
    let u2 = velocities.outlet_speed.max(f64::EPSILON);
    let response = Plot::new(("blade_velocity", id.0))
        .height(height)
        .y_axis_label(tr("w/u₂"))
        .x_axis_label(tr(
            "relative meridional length (0 leading edge, 1 trailing edge)",
        ))
        .link_axis("blade_plots", [true, false])
        .link_cursor("blade_plots", [true, false])
        .x_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
        .y_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
        .include_y(0.0)
        .y_axis_min_width(40.0)
        .allow_scroll(false)
        .legend(Legend::default().position(Corner::LeftBottom))
        .show(ui, |plot| {
            for (i, v) in velocities.sections.iter().enumerate() {
                let color = palette.span(i, n);
                let face = |values: &[f64]| -> PlotPoints<'static> {
                    v.position
                        .iter()
                        .zip(values)
                        .map(|(m, w)| [*m, w / u2])
                        .collect()
                };
                let (suction, pressure) = if i == 0 {
                    (
                        tr("suction face").to_owned(),
                        tr("pressure face").to_owned(),
                    )
                } else {
                    (String::new(), String::new())
                };
                plot.line(Line::new(suction, face(&v.suction)).color(color).width(1.5));
                plot.line(
                    Line::new(pressure, face(&v.pressure))
                        .color(color)
                        .width(1.2)
                        .style(LineStyle::dashed_loose()),
                );
            }
        });
    super::settle(ui, ("blade_velocity", id.0), &response.transform);
}
