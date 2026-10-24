//! Blade plots along the normalized meridional length m̄: blade angle, wrap angle,
//! thickness and the relative velocity on both faces of every section, two rows of
//! two under one legend, like the performance curves. A second page draws the blade
//! edges in true shape, to shape them by hand.

use eframe::egui::{self, Ui};
use egui_plot::{Line, LineStyle, Plot, PlotPoints};
use tp_design::impeller::blade::BladeSection;
use tp_design::impeller::loading::blade_velocities;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::blade_edges::EdgesPage;
use super::{Mark, Palette, marker, unavailable};
use crate::app::{EditCtx, Selection};
use crate::derived::Derived;
use crate::i18n::{tf, tr};
use tp_project::DesignStep;

/// The pages of the blade view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum BladePage {
    #[default]
    Plots,
    Edges,
}

/// The blade view between frames.
#[derive(Debug, Default)]
pub(crate) struct BladeView {
    pub page: BladePage,
    pub edges: EdgesPage,
    /// The step the page last followed.
    followed: Option<Selection>,
}

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

impl BladeView {
    /// The view, clear of the `cover` px the floating properties panel hides on the
    /// right. The blade-edge step opens the edges page, the other blade steps the plots.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        id: ComponentId,
        units: &DisplayUnits,
        cover: f32,
    ) {
        let selection = *cx.selection;
        if self.followed != Some(selection) {
            self.followed = Some(selection);
            if let Selection::Step(_, step) = selection {
                self.page = if step == DesignStep::BladeEdges {
                    BladePage::Edges
                } else {
                    BladePage::Plots
                };
            }
        }
        let area = ui.available_rect_before_wrap();
        let right = (area.max.x - cover).max(area.min.x + 320.0);
        let clear = egui::Rect::from_min_max(area.min, egui::pos2(right, area.max.y));
        ui.scope_builder(egui::UiBuilder::new().max_rect(clear), |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.page, BladePage::Plots, tr("Blade plots"));
                ui.selectable_value(&mut self.page, BladePage::Edges, tr("Edges"));
            });
            match self.page {
                BladePage::Plots => content(ui, cx.derived, id, units),
                BladePage::Edges => {
                    if cx.derived.geometry(id).is_some() {
                        self.edges.show(ui, cx, id, units);
                    } else {
                        content(ui, cx.derived, id, units);
                    }
                }
            }
        });
    }
}

fn content(ui: &mut Ui, derived: &Derived, id: ComponentId, units: &DisplayUnits) {
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
    let ink = ui.visuals().text_color();

    // The colors of the sections and the faces, explained once above the plots.
    ui.horizontal_wrapped(|ui| {
        for (i, s) in sections.iter().enumerate() {
            let name = match i {
                0 => tf("span {} (hub)", &[&format!("{:.2}", s.span)]),
                i if i + 1 == n => tf("span {} (shroud)", &[&format!("{:.2}", s.span)]),
                _ => tf("span {}", &[&format!("{:.2}", s.span)]),
            };
            marker(ui, Mark::Line, palette.span(i, n), &name);
        }
        marker(ui, Mark::Line, ink, tr("suction face"));
        marker(ui, Mark::DashedLine, ink, tr("pressure face"));
    });

    // Two rows of two plots, sharing the m̄ axis; it is labelled on the lower row.
    let spacing = ui.spacing().item_spacing.y;
    let height = ((ui.available_height() - spacing) / 2.0).max(150.0);
    let base = |name: &str, label: String, lower: bool| {
        let plot = Plot::new((name.to_owned(), id.0))
            .height(height)
            .y_axis_label(label)
            .link_axis("blade_plots", [true, false])
            .link_cursor("blade_plots", [true, false])
            .x_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
            .y_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
            .y_axis_min_width(44.0)
            .allow_scroll(false);
        if lower {
            plot.x_axis_label(tr("m̄ (0 leading edge, 1 trailing edge)"))
        } else {
            plot
        }
    };
    let section_plot = |ui: &mut Ui, name: &str, label: String, lower: bool, value: Quantity| {
        let response = base(name, label, lower).show(ui, |plot| {
            for (i, s) in sections.iter().enumerate() {
                plot.line(
                    Line::new(String::new(), along(s, |k| value(s, k)))
                        .color(palette.span(i, n))
                        .width(1.5),
                );
            }
        });
        super::settle(ui, (name, id.0), &response.transform);
    };
    let velocity_plot = |ui: &mut Ui| {
        // Relative velocity on the faces: suction solid, pressure dashed.
        let velocities = blade_velocities(g);
        let u2 = velocities.outlet_speed.max(f64::EPSILON);
        let response = base("blade_velocity", tr("w/u₂").to_owned(), true)
            .include_y(0.0)
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
                    plot.line(
                        Line::new(String::new(), face(&v.suction))
                            .color(color)
                            .width(1.5),
                    );
                    plot.line(
                        Line::new(String::new(), face(&v.pressure))
                            .color(color)
                            .width(1.2)
                            .style(LineStyle::dashed_loose()),
                    );
                }
            });
        super::settle(ui, ("blade_velocity", id.0), &response.transform);
    };
    ui.columns(2, |columns| {
        section_plot(
            &mut columns[0],
            "blade_beta",
            format!("β_B [{angle}]"),
            false,
            &|s, i| units.to_display(QuantityKind::Angle, s.beta[i]),
        );
        section_plot(
            &mut columns[1],
            "blade_theta",
            format!("θ [{angle}]"),
            false,
            &|s, i| units.to_display(QuantityKind::Angle, s.theta[i]),
        );
    });
    ui.columns(2, |columns| {
        section_plot(
            &mut columns[0],
            "blade_thickness",
            format!("e [{length}]"),
            true,
            &|s, i| units.to_display(QuantityKind::Length, s.thickness[i]),
        );
        velocity_plot(&mut columns[1]);
    });
}
