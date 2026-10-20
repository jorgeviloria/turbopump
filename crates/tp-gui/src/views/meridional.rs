//! Meridional view: hub and shroud contours, leading and trailing edges, stream lines,
//! and draggable handles that edit the meridional-contour inputs.

use eframe::egui::{CursorIcon, Sense, Ui};
use egui_plot::{
    Corner, Legend, Line, LineStyle, Plot, PlotPoint, PlotPoints, PlotTransform, Points,
};
use tp_design::Param;
use tp_design::impeller::ImpellerBasis;
use tp_design::impeller::meridian::{MeridionalContour, MeridionalInput};
use tp_math::DVec2;
use tp_project::ComponentId;
use tp_units::QuantityKind;

use super::{Palette, unavailable};
use crate::app::EditCtx;

/// A point of the meridional contour the user can drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    HubInlet,
    HubOutlet,
    ShroudInlet,
    ShroudOutlet,
    LeadingEdgeHub,
    LeadingEdgeShroud,
    AxialLength,
}

impl Handle {
    const ALL: [Handle; 7] = [
        Handle::HubInlet,
        Handle::HubOutlet,
        Handle::ShroudInlet,
        Handle::ShroudOutlet,
        Handle::LeadingEdgeHub,
        Handle::LeadingEdgeShroud,
        Handle::AxialLength,
    ];

    fn label(self) -> &'static str {
        match self {
            Handle::HubInlet => "hub inlet tangent",
            Handle::HubOutlet => "hub outlet tangent",
            Handle::ShroudInlet => "shroud inlet tangent",
            Handle::ShroudOutlet => "shroud outlet tangent",
            Handle::LeadingEdgeHub => "leading edge on the hub",
            Handle::LeadingEdgeShroud => "leading edge on the shroud",
            Handle::AxialLength => "axial length",
        }
    }
}

/// Where a handle sits, (z, r) in m.
pub(crate) fn handle_position(handle: Handle, contour: &MeridionalContour) -> DVec2 {
    let hub = contour.hub.control_points();
    let shroud = contour.shroud.control_points();
    match handle {
        Handle::HubInlet => hub[1],
        Handle::HubOutlet => hub[2],
        Handle::ShroudInlet => shroud[1],
        Handle::ShroudOutlet => shroud[2],
        Handle::LeadingEdgeHub => contour.hub_at(contour.leading_edge.0),
        Handle::LeadingEdgeShroud => contour.shroud_at(contour.leading_edge.1),
        Handle::AxialLength => contour.hub.end(),
    }
}

/// Fraction of the way from `origin` to `corner` of the projection of `pointer`.
fn tangent_fraction(origin: DVec2, corner: DVec2, pointer: DVec2) -> f64 {
    let axis = corner - origin;
    ((pointer - origin).dot(axis) / axis.length_squared()).clamp(0.05, 0.95)
}

/// Arc-length fraction of the contour point nearest to `pointer`.
fn nearest_fraction(at: impl Fn(f64) -> DVec2, pointer: DVec2) -> f64 {
    const SAMPLES: usize = 400;
    (0..=SAMPLES)
        .map(|i| i as f64 / SAMPLES as f64)
        .min_by(|&a, &b| {
            at(a)
                .distance_squared(pointer)
                .total_cmp(&at(b).distance_squared(pointer))
        })
        .unwrap_or(0.0)
        .min(0.9)
}

/// Moves `handle` to `pointer` ((z, r) in m) by changing the matching input.
pub(crate) fn apply_drag(
    handle: Handle,
    pointer: DVec2,
    contour: &MeridionalContour,
    basis: &ImpellerBasis,
    input: &mut MeridionalInput,
) {
    let hub = contour.hub.control_points();
    let shroud = contour.shroud.control_points();
    // The tangents meet at the corner of the axial inlet and radial outlet directions.
    let hub_corner = DVec2::new(hub[3].x, hub[0].y);
    let shroud_corner = DVec2::new(shroud[3].x, shroud[0].y);
    match handle {
        Handle::HubInlet => {
            input.hub_inlet_tangent = Param::Manual(tangent_fraction(hub[0], hub_corner, pointer));
        }
        Handle::HubOutlet => {
            input.hub_outlet_tangent = Param::Manual(tangent_fraction(hub[3], hub_corner, pointer));
        }
        Handle::ShroudInlet => {
            input.shroud_inlet_tangent =
                Param::Manual(tangent_fraction(shroud[0], shroud_corner, pointer));
        }
        Handle::ShroudOutlet => {
            input.shroud_outlet_tangent =
                Param::Manual(tangent_fraction(shroud[3], shroud_corner, pointer));
        }
        Handle::LeadingEdgeHub => {
            input.leading_edge_hub =
                Param::Manual(nearest_fraction(|t| contour.hub_at(t), pointer));
        }
        Handle::LeadingEdgeShroud => {
            input.leading_edge_shroud =
                Param::Manual(nearest_fraction(|t| contour.shroud_at(t), pointer));
        }
        Handle::AxialLength => {
            let min = basis.outlet_width * 1.1;
            let max = basis.outlet_diameter * 2.0;
            input.axial_length = Param::Manual(pointer.x.clamp(min, max));
        }
    }
}

/// State of the meridional view between frames.
#[derive(Debug)]
pub(crate) struct MeridionalView {
    dragging: Option<Handle>,
    /// Plot transform of the last frame, used to find the handle under the pointer
    /// before the plot is drawn.
    transform: Option<PlotTransform>,
    show_stream_lines: bool,
    show_quasi_orthogonals: bool,
}

impl Default for MeridionalView {
    fn default() -> Self {
        Self {
            dragging: None,
            transform: None,
            show_stream_lines: true,
            show_quasi_orthogonals: false,
        }
    }
}

fn line(points: impl IntoIterator<Item = DVec2>, scale: f64) -> PlotPoints<'static> {
    points
        .into_iter()
        .map(|p| [p.x * scale, p.y * scale])
        .collect()
}

impl MeridionalView {
    /// Plot transform of the last frame.
    #[cfg(test)]
    pub(crate) fn transform(&self) -> Option<&PlotTransform> {
        self.transform.as_ref()
    }

    pub(crate) fn show(&mut self, ui: &mut Ui, cx: &mut EditCtx<'_>, id: ComponentId) {
        let derived = cx.derived;
        let Some(results) = derived.impeller(id) else {
            return;
        };
        let g = match &results.geometry {
            Ok(g) => g,
            Err(e) => {
                unavailable(ui, e);
                return;
            }
        };
        let units = cx.units();
        let unit = units.get(QuantityKind::Length).symbol;
        let scale = units.to_display(QuantityKind::Length, 1.0);
        let palette = Palette::of(ui);

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.show_stream_lines, "stream lines");
            ui.checkbox(&mut self.show_quasi_orthogonals, "quasi-orthogonals");
            ui.weak("(?)").on_hover_text(
                "Drag the round handles to shape the contour. Drag the background to pan, \
                 scroll to zoom, double-click to fit.",
            );
        });

        let handles: Vec<(Handle, DVec2)> = Handle::ALL
            .iter()
            .map(|&h| (h, handle_position(h, &g.contour)))
            .collect();
        let hovered = match (ui.ctx().pointer_hover_pos(), &self.transform) {
            (Some(pos), Some(t)) if t.frame().contains(pos) => handles
                .iter()
                .map(|&(h, p)| {
                    let screen = t.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
                    (h, screen.distance(pos))
                })
                .filter(|&(_, d)| d < 10.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| h),
            _ => None,
        };
        let active = self.dragging.or(hovered);
        // Keep the view still while dragging, or the plot would refit under the pointer.
        let frozen = self
            .dragging
            .and(self.transform.as_ref())
            .map(|t| *t.bounds());

        let sections = &g.blade.sections;
        let response = Plot::new(("meridional", id.0))
            .data_aspect(1.0)
            .allow_drag(active.is_none())
            .sense(Sense::click_and_drag())
            .x_axis_label(format!("z [{unit}]"))
            .y_axis_label(format!("r [{unit}]"))
            .legend(Legend::default().position(Corner::LeftTop))
            .show(ui, |plot| {
                if let Some(bounds) = frozen {
                    plot.set_plot_bounds(bounds);
                }
                if self.show_quasi_orthogonals {
                    for qo in g.channel.quasi_orthogonals.iter().step_by(4) {
                        plot.line(
                            Line::new("quasi-orthogonals", line([qo.hub, qo.shroud], scale))
                                .color(palette.guide)
                                .width(0.5),
                        );
                    }
                }
                if self.show_stream_lines {
                    let n = g.channel.spans.len();
                    for span in g.channel.spans.iter().skip(1).take(n.saturating_sub(2)) {
                        plot.line(
                            Line::new("stream lines", line(span.points.iter().copied(), scale))
                                .color(palette.guide)
                                .width(0.8),
                        );
                    }
                }
                plot.line(
                    Line::new("hub", line(g.contour.hub.sample(240), scale))
                        .color(palette.hub)
                        .width(2.0),
                );
                plot.line(
                    Line::new("shroud", line(g.contour.shroud.sample(240), scale))
                        .color(palette.shroud)
                        .width(2.0),
                );
                plot.line(
                    Line::new(
                        "leading edge",
                        line(sections.iter().map(|s| s.meridional[0]), scale),
                    )
                    .color(palette.leading_edge)
                    .width(2.0),
                );
                plot.line(
                    Line::new(
                        "trailing edge",
                        line(
                            sections.iter().filter_map(|s| s.meridional.last().copied()),
                            scale,
                        ),
                    )
                    .color(palette.trailing_edge)
                    .width(2.0),
                );
                // Bézier control polygons: the tangent lines the handles slide on.
                for curve in [&g.contour.hub, &g.contour.shroud] {
                    let p = curve.control_points();
                    for pair in [[p[0], p[1]], [p[2], p[3]]] {
                        plot.line(
                            Line::new("", line(pair, scale))
                                .color(palette.handle)
                                .style(LineStyle::dashed_dense())
                                .width(1.0),
                        );
                    }
                }
                for &(handle, p) in &handles {
                    let current = active == Some(handle);
                    plot.points(
                        Points::new("", vec![[p.x * scale, p.y * scale]])
                            .radius(if current { 6.5 } else { 4.5 })
                            .color(if current {
                                palette.handle_active
                            } else {
                                palette.handle
                            })
                            .filled(true),
                    );
                }
                plot.pointer_coordinate()
            });

        self.transform = Some(response.transform);
        let plot_response = &response.response;
        if plot_response.drag_started() && hovered.is_some() {
            self.dragging = hovered;
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.dragging = None;
        }
        if let Some(handle) = active {
            ui.ctx().set_cursor_icon(if self.dragging.is_some() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
            plot_response.clone().on_hover_text(handle.label());
        }
        if let (Some(handle), Some(pointer)) = (self.dragging, response.inner)
            && plot_response.dragged()
        {
            let pointer = DVec2::new(pointer.x / scale, pointer.y / scale);
            let current = cx
                .doc
                .project()
                .impeller_geometry_input(id)
                .unwrap_or_default();
            let mut input = current.clone();
            apply_drag(handle, pointer, &g.contour, &g.basis, &mut input.meridional);
            if input != current {
                cx.edit(&format!("drag {}", handle.label()), |p| {
                    p.component_mut(id)?.set_geometry(input)
                });
                // Show the new geometry even if the pointer stops here.
                ui.ctx().request_repaint();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_project;
    use crate::derived::Derived;

    fn geometry() -> tp_design::impeller::ImpellerGeometry {
        let project = default_project();
        let mut derived = Derived::default();
        derived.update(&project);
        let id = project.components()[0].id();
        derived.geometry(id).unwrap().clone()
    }

    #[test]
    fn dragging_a_handle_to_its_own_position_keeps_the_input() {
        let g = geometry();
        let mut input = MeridionalInput::default();
        for handle in Handle::ALL {
            let at = handle_position(handle, &g.contour);
            apply_drag(handle, at, &g.contour, &g.basis, &mut input);
        }
        let close = |p: Param<f64>, v: f64, tol: f64| {
            let got = p.manual_value().unwrap();
            assert!((got - v).abs() < tol, "{got} vs {v}");
        };
        let d = tp_design::impeller::meridian::DEFAULT_TANGENT;
        close(input.hub_inlet_tangent, d, 1e-9);
        close(input.hub_outlet_tangent, d, 1e-9);
        close(input.shroud_inlet_tangent, d, 1e-9);
        close(input.shroud_outlet_tangent, d, 1e-9);
        close(
            input.leading_edge_hub,
            g.contour.leading_edge.0,
            1.0 / 400.0,
        );
        close(
            input.leading_edge_shroud,
            g.contour.leading_edge.1,
            1.0 / 400.0,
        );
        close(input.axial_length, g.contour.axial_length, 1e-12);
    }

    #[test]
    fn drags_are_clamped_to_valid_inputs() {
        let g = geometry();
        let mut input = MeridionalInput::default();
        let far = DVec2::new(10.0, 10.0);
        apply_drag(Handle::HubInlet, far, &g.contour, &g.basis, &mut input);
        assert_eq!(input.hub_inlet_tangent, Param::Manual(0.95));
        apply_drag(
            Handle::AxialLength,
            DVec2::ZERO,
            &g.contour,
            &g.basis,
            &mut input,
        );
        assert!(input.axial_length.manual_value().unwrap() > g.basis.outlet_width);
        apply_drag(
            Handle::LeadingEdgeHub,
            far,
            &g.contour,
            &g.basis,
            &mut input,
        );
        assert!(input.leading_edge_hub.manual_value().unwrap() <= 0.9);
    }
}
