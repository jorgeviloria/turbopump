//! Meridional view: hub and shroud contours, leading and trailing edges, stream lines,
//! and draggable handles that edit the meridional-contour inputs. On request, the
//! two-dimensional potential flow: its stream lines and the meridional velocity along
//! hub and shroud.

use eframe::egui::{CursorIcon, DragValue, Sense, Ui};
use egui_plot::{
    Corner, Legend, Line, LineStyle, MarkerShape, Plot, PlotPoint, PlotPoints, PlotTransform,
    Points,
};
use tp_design::Param;
use tp_design::impeller::meridian::{MeridionalContour, MeridionalInput};
use tp_design::impeller::potential::{self, PotentialFlow, PotentialMesh};
use tp_design::impeller::wall::{self, SegmentInput, SegmentKind, Wall, WallPoint};
use tp_design::impeller::{ImpellerBasis, ImpellerGeometry, ImpellerGeometryInput};
use tp_math::DVec2;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::{Palette, unavailable};
use crate::app::EditCtx;
use crate::i18n::{tf, tr};

/// Hub or shroud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Hub,
    Shroud,
}

impl Side {
    fn wall(self, contour: &MeridionalContour) -> &Wall {
        match self {
            Side::Hub => &contour.hub,
            Side::Shroud => &contour.shroud,
        }
    }

    fn segments(self, input: &mut MeridionalInput) -> &mut Option<Vec<SegmentInput>> {
        match self {
            Side::Hub => &mut input.hub_segments,
            Side::Shroud => &mut input.shroud_segments,
        }
    }

    fn drawn(self, input: &MeridionalInput) -> bool {
        match self {
            Side::Hub => input.hub_segments.is_some(),
            Side::Shroud => input.shroud_segments.is_some(),
        }
    }
}

/// A point of the meridional contour the user can drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    /// Tangent lengths of an automatic wall: they slide along their tangent line.
    HubInlet,
    HubOutlet,
    ShroudInlet,
    ShroudOutlet,
    /// A control point or joint of a wall drawn segment by segment.
    Point(Side, WallPoint),
    LeadingEdgeHub,
    LeadingEdgeShroud,
    AxialLength,
}

impl Handle {
    fn label(self) -> &'static str {
        tr(match self {
            Handle::HubInlet => "hub inlet tangent",
            Handle::HubOutlet => "hub outlet tangent",
            Handle::ShroudInlet => "shroud inlet tangent",
            Handle::ShroudOutlet => "shroud outlet tangent",
            Handle::Point(Side::Hub, WallPoint::Joint { .. }) => "hub joint",
            Handle::Point(Side::Hub, WallPoint::Control { .. }) => "hub control point",
            Handle::Point(Side::Shroud, WallPoint::Joint { .. }) => "shroud joint",
            Handle::Point(Side::Shroud, WallPoint::Control { .. }) => "shroud control point",
            Handle::LeadingEdgeHub => "leading edge on the hub",
            Handle::LeadingEdgeShroud => "leading edge on the shroud",
            Handle::AxialLength => "axial length",
        })
    }

    /// The wall point this handle stands for, if it is one.
    fn wall_point(self) -> Option<(Side, WallPoint)> {
        let control = |index| WallPoint::Control { segment: 0, index };
        match self {
            Handle::HubInlet => Some((Side::Hub, control(0))),
            Handle::HubOutlet => Some((Side::Hub, control(1))),
            Handle::ShroudInlet => Some((Side::Shroud, control(0))),
            Handle::ShroudOutlet => Some((Side::Shroud, control(1))),
            Handle::Point(side, at) => Some((side, at)),
            _ => None,
        }
    }
}

/// Every handle of the contour: the tangent handles of automatic walls, the points of
/// drawn ones, the leading edge and the axial length.
pub(crate) fn handles(contour: &MeridionalContour, input: &MeridionalInput) -> Vec<Handle> {
    let mut handles = Vec::new();
    for (side, automatic) in [
        (Side::Hub, [Handle::HubInlet, Handle::HubOutlet]),
        (Side::Shroud, [Handle::ShroudInlet, Handle::ShroudOutlet]),
    ] {
        if side.drawn(input) {
            let points = side.wall(contour).points();
            handles.extend(points.into_iter().map(|(at, _)| Handle::Point(side, at)));
        } else {
            handles.extend(automatic);
        }
    }
    handles.extend([
        Handle::LeadingEdgeHub,
        Handle::LeadingEdgeShroud,
        Handle::AxialLength,
    ]);
    handles
}

/// Where a handle sits, (z, r) in m.
pub(crate) fn handle_position(handle: Handle, contour: &MeridionalContour) -> DVec2 {
    match handle {
        Handle::LeadingEdgeHub => contour.hub_at(contour.leading_edge.0),
        Handle::LeadingEdgeShroud => contour.shroud_at(contour.leading_edge.1),
        Handle::AxialLength => contour.hub.end(),
        _ => {
            let (side, at) = handle.wall_point().expect("other handles are wall points");
            side.wall(contour)
                .point(at)
                .unwrap_or_else(|| side.wall(contour).end())
        }
    }
}

/// Moves a wall point to `to` exactly, turning an automatic wall into a drawn one.
fn place(
    side: Side,
    at: WallPoint,
    to: DVec2,
    contour: &MeridionalContour,
    input: &mut MeridionalInput,
) {
    *side.segments(input) = Some(wall::move_point(side.wall(contour), at, to));
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
    if let Handle::Point(side, at) = handle {
        place(side, at, pointer, contour, input);
        return;
    }
    let hub = contour.hub.segments()[0].control_points();
    let shroud = contour.shroud.segments()[0].control_points();
    // The tangents meet at the corner of the axial inlet and the outlet direction
    // (radial, or inclined at ε_s for a semi-axial impeller).
    let corner = |p: &[DVec2]| {
        let (a, b) = (p[1] - p[0], p[2] - p[3]);
        let det = a.perp_dot(b);
        if det.abs() < 1e-15 {
            DVec2::new(p[3].x, p[0].y)
        } else {
            p[0] + a * ((p[3] - p[0]).perp_dot(b) / det)
        }
    };
    let (hub_corner, shroud_corner) = (corner(hub), corner(shroud));
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
        Handle::Point(..) => {}
    }
}

/// State of the meridional view between frames.
#[derive(Debug)]
pub(crate) struct MeridionalView {
    dragging: Option<Handle>,
    /// What the context menu was opened on.
    menu: Option<MenuTarget>,
    /// Plot transform of the last frame, used to find the handle under the pointer
    /// before the plot is drawn.
    transform: Option<PlotTransform>,
    show_stream_lines: bool,
    show_quasi_orthogonals: bool,
    show_potential: bool,
    /// Potential flow of the last geometry it was solved for.
    potential: Option<((u64, ComponentId), Result<PotentialFlow, String>)>,
}

impl Default for MeridionalView {
    fn default() -> Self {
        Self {
            dragging: None,
            menu: None,
            transform: None,
            show_stream_lines: true,
            show_quasi_orthogonals: false,
            show_potential: false,
            potential: None,
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

    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        id: ComponentId,
        stator: Option<ComponentId>,
        volute: Option<ComponentId>,
    ) {
        let derived = cx.derived;
        let stator = stator.and_then(|s| derived.diffuser(s));
        let volute = volute.and_then(|v| derived.volute(v));
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
            ui.checkbox(&mut self.show_stream_lines, tr("stream lines"));
            ui.checkbox(&mut self.show_quasi_orthogonals, tr("quasi-orthogonals"));
            ui.checkbox(&mut self.show_potential, tr("potential flow"))
                .on_hover_text(tr(
                    "Two-dimensional potential flow between hub and shroud: its stream \
                     lines, and the meridional velocity along the walls against the \
                     quasi-one-dimensional flow of the design",
                ));
            ui.weak("(?)").on_hover_text(tr(
                "Drag the round handles to shape the contour; right-click a point or a \
                 wall to type exact values, split a wall into segments or join them. Drag \
                 the background to pan, scroll to zoom, double-click to fit.",
            ));
        });

        let current = cx
            .doc
            .project()
            .impeller_geometry_input(id)
            .unwrap_or_default();
        let handles: Vec<(Handle, DVec2)> = handles(&g.contour, &current.meridional)
            .into_iter()
            .map(|h| (h, handle_position(h, &g.contour)))
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

        // The potential flow, solved again only when the geometry changes.
        let potential = if self.show_potential {
            let key = (derived.revision(), id);
            if self.potential.as_ref().is_none_or(|(k, _)| *k != key) {
                let solution = potential::solve(&g.contour, g.basis.flow, PotentialMesh::default())
                    .map_err(|e| e.to_string());
                self.potential = Some((key, solution));
            }
            self.potential.as_ref().map(|(_, s)| s.clone())
        } else {
            None
        };
        let velocity_height = if potential.is_some() {
            (ui.available_height() * 0.3).max(120.0)
        } else {
            0.0
        };
        let plot_height = (ui.available_height() - velocity_height).max(150.0);

        let sections = &g.blade.sections;
        let response = Plot::new(("meridional", id.0))
            .height(plot_height)
            .data_aspect(1.0)
            .allow_drag(active.is_none())
            .sense(Sense::click_and_drag())
            .x_axis_label(format!("z [{unit}]"))
            .y_axis_label(format!("r [{unit}]"))
            .legend(Legend::default().position(Corner::RightBottom))
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
                if let Some(Ok(flow)) = &potential {
                    for fraction in [0.25, 0.5, 0.75] {
                        let stream = flow.streamline(fraction);
                        plot.line(
                            Line::new("potential flow", line(stream.points, scale))
                                .color(palette.stator)
                                .width(1.2)
                                .style(LineStyle::dashed_loose()),
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
                // Bézier control polygons: the tangent lines at both ends of each curve.
                for wall in [&g.contour.hub, &g.contour.shroud] {
                    for curve in wall.segments().iter().filter(|c| c.degree() > 1) {
                        let p = curve.control_points();
                        let n = p.len();
                        for pair in [[p[0], p[1]], [p[n - 2], p[n - 1]]] {
                            plot.line(
                                Line::new("", line(pair, scale))
                                    .color(palette.handle)
                                    .style(LineStyle::dashed_dense())
                                    .width(1.0),
                            );
                        }
                    }
                }
                if let Some(d) = stator {
                    // Side walls from the impeller outlet to d₄, and the vane edges.
                    let (r2, r3, r4) = (
                        d.impeller_diameter / 2.0,
                        d.inlet_diameter / 2.0,
                        d.outlet_diameter / 2.0,
                    );
                    for side in [-1.0, 1.0] {
                        let wall: Vec<DVec2> = [r2, r3, r4]
                            .iter()
                            .map(|&r| DVec2::new(d.midplane + side * d.half_width(r), r))
                            .collect();
                        plot.line(
                            Line::new("diffuser", line(wall, scale))
                                .color(palette.stator)
                                .width(1.5),
                        );
                    }
                    let edges: &[f64] = if d.vanes.is_some() { &[r3, r4] } else { &[r4] };
                    for &r in edges {
                        let h = d.half_width(r);
                        plot.line(
                            Line::new(
                                "diffuser",
                                line(
                                    [DVec2::new(d.midplane - h, r), DVec2::new(d.midplane + h, r)],
                                    scale,
                                ),
                            )
                            .color(palette.stator)
                            .width(1.0),
                        );
                    }
                }
                if let Some(v) = volute {
                    // Inlet ring walls and a few cross sections, folded into one plane.
                    let (inner, outer) = v.inlet_ring();
                    let zm = v.basis.midplane;
                    let half = v.design.inlet_width / 2.0;
                    for z in [zm - half, zm + half] {
                        plot.line(
                            Line::new(
                                "volute",
                                line([DVec2::new(z, inner), DVec2::new(z, outer)], scale),
                            )
                            .color(palette.volute)
                            .width(1.5),
                        );
                    }
                    // The bend and neck of an internal volute.
                    if let Some(bend) = &v.bend {
                        let rz = v.design.stagnation_radius;
                        for wall in [&bend.outer, &bend.inner] {
                            let points = wall
                                .iter()
                                .flatten()
                                .map(|p| DVec2::new(zm + p.x, rz + p.y));
                            plot.line(
                                Line::new("volute", line(points, scale))
                                    .color(palette.volute)
                                    .width(1.5),
                            );
                        }
                    }
                    let n = v.sections.len();
                    for index in [n / 4, n / 2, 3 * n / 4, n - 1] {
                        let s = &v.sections[index];
                        let mut points: Vec<DVec2> =
                            s.outline.iter().map(|p| DVec2::new(p.y, p.x)).collect();
                        points.push(points[0]);
                        plot.line(
                            Line::new("volute", line(points, scale))
                                .color(palette.volute)
                                .width(if index == n - 1 { 2.0 } else { 1.0 }),
                        );
                    }
                }
                for &(handle, p) in &handles {
                    let current = active == Some(handle);
                    let joint = matches!(handle, Handle::Point(_, WallPoint::Joint { .. }));
                    plot.points(
                        Points::new("", vec![[p.x * scale, p.y * scale]])
                            .shape(if joint {
                                MarkerShape::Square
                            } else {
                                MarkerShape::Circle
                            })
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

        super::settle(ui, ("meridional", id.0), &response.transform);
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
        if plot_response.secondary_clicked() {
            self.menu = Some(match hovered {
                Some(handle) => MenuTarget::Handle(handle),
                None => response
                    .inner
                    .and_then(|pointer| {
                        let pointer = DVec2::new(pointer.x / scale, pointer.y / scale);
                        wall_under(&g.contour, pointer, &response.transform, scale)
                    })
                    .map_or(MenuTarget::Empty, |(side, segment, t)| {
                        MenuTarget::Segment(side, segment, t)
                    }),
            });
        }
        if let Some(target) = self.menu {
            plot_response.context_menu(|ui| {
                if let Some(input) = context_menu(ui, target, g, &current, &units) {
                    let label = target.edit_label();
                    cx.edit(label, |p| p.set_impeller_geometry(id, input));
                }
            });
        }
        if let (Some(handle), Some(pointer)) = (self.dragging, response.inner)
            && plot_response.dragged()
        {
            let pointer = DVec2::new(pointer.x / scale, pointer.y / scale);
            let mut input = current.clone();
            apply_drag(handle, pointer, &g.contour, &g.basis, &mut input.meridional);
            if input != current {
                cx.edit(&tf("drag {}", &[&handle.label()]), |p| {
                    p.set_impeller_geometry(id, input)
                });
                // Show the new geometry even if the pointer stops here.
                ui.ctx().request_repaint();
            }
        }

        match &potential {
            Some(Ok(flow)) => wall_velocities(ui, flow, g, id, velocity_height, &units),
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("Potential flow: {e}"));
            }
            None => {}
        }
    }
}

/// What a right click landed on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuTarget {
    Handle(Handle),
    /// Segment `.1` of a wall, at curve parameter `.2`.
    Segment(Side, usize, f64),
    Empty,
}

impl MenuTarget {
    fn edit_label(self) -> &'static str {
        match self {
            MenuTarget::Handle(_) => "edit point",
            MenuTarget::Segment(..) | MenuTarget::Empty => "edit wall segments",
        }
    }
}

/// The wall segment within reach of `pointer` on screen, with its curve parameter.
fn wall_under(
    contour: &MeridionalContour,
    pointer: DVec2,
    transform: &PlotTransform,
    scale: f64,
) -> Option<(Side, usize, f64)> {
    let screen =
        |p: DVec2| transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
    let at = screen(pointer);
    [Side::Hub, Side::Shroud]
        .into_iter()
        .map(|side| {
            let wall = side.wall(contour);
            let (segment, t) = wall.nearest(pointer);
            let d = screen(wall.segments()[segment].eval(t)).distance(at);
            (side, segment, t, d)
        })
        .filter(|&(.., d)| d < 8.0)
        .min_by(|a, b| a.3.total_cmp(&b.3))
        .map(|(side, segment, t, _)| (side, segment, t))
}

/// A length typed in display units; true when it changed.
fn length_field(ui: &mut Ui, label: &str, value: &mut f64, units: &DisplayUnits) -> bool {
    let unit = units.get(QuantityKind::Length);
    let scale = units.to_display(QuantityKind::Length, 1.0);
    let mut shown = *value * scale;
    let changed = ui
        .horizontal(|ui| {
            ui.label(label);
            ui.add(
                DragValue::new(&mut shown)
                    .speed(0.1)
                    .max_decimals(4)
                    .suffix(format!(" {}", unit.symbol)),
            )
            .changed()
        })
        .inner;
    if changed {
        *value = shown / scale;
    }
    changed
}

/// The context menu of the meridional view; the new geometry input when it changed
/// something.
fn context_menu(
    ui: &mut Ui,
    target: MenuTarget,
    g: &ImpellerGeometry,
    current: &ImpellerGeometryInput,
    units: &DisplayUnits,
) -> Option<ImpellerGeometryInput> {
    let contour = &g.contour;
    let mut input = current.clone();
    let m = &mut input.meridional;
    match target {
        MenuTarget::Handle(Handle::AxialLength) => {
            ui.strong(tr("Axial length L"));
            let mut length = contour.axial_length;
            if length_field(ui, "L", &mut length, units) {
                m.axial_length = Param::Manual(length.max(g.basis.outlet_width * 1.01));
            }
        }
        MenuTarget::Handle(handle @ (Handle::LeadingEdgeHub | Handle::LeadingEdgeShroud)) => {
            ui.strong(handle.label());
            let (param, value) = if handle == Handle::LeadingEdgeHub {
                (&mut m.leading_edge_hub, contour.leading_edge.0)
            } else {
                (&mut m.leading_edge_shroud, contour.leading_edge.1)
            };
            let mut percent = value * 100.0;
            let changed = ui
                .horizontal(|ui| {
                    ui.label(tr("Position along the wall"));
                    ui.add(
                        DragValue::new(&mut percent)
                            .speed(0.2)
                            .range(0.0..=90.0)
                            .max_decimals(2)
                            .suffix(" %"),
                    )
                    .changed()
                })
                .inner;
            if changed {
                *param = Param::Manual(percent / 100.0);
            }
        }
        MenuTarget::Handle(handle) => {
            let (side, at) = handle.wall_point()?;
            let wall = side.wall(contour);
            ui.strong(handle.label());
            let Some(mut p) = wall.point(at) else {
                ui.weak(tr("This point no longer exists."));
                return None;
            };
            let z = length_field(ui, "z", &mut p.x, units);
            let r = length_field(ui, "r", &mut p.y, units);
            if z || r {
                place(side, at, p, contour, m);
            }
            if !side.drawn(&current.meridional) {
                ui.weak(tr("Typing a value turns this wall into drawn segments."));
            }
            if let WallPoint::Joint { segment } = at {
                ui.separator();
                let mut smooth = wall.smooth()[segment];
                if ui.checkbox(&mut smooth, tr("Tangent joint")).changed() {
                    *side.segments(m) = Some(wall::set_smooth(wall, segment, smooth));
                }
                if ui.button(tr("Join the two segments")).clicked() {
                    *side.segments(m) = Some(wall::merge(wall, segment));
                    ui.close();
                }
            }
        }
        MenuTarget::Segment(side, segment, t) => {
            let wall = side.wall(contour);
            let curve = wall.segments().get(segment)?;
            ui.strong(match side {
                Side::Hub => tr("Hub"),
                Side::Shroud => tr("Shroud"),
            });
            if wall.segments().len() > 1 {
                ui.weak(tf(
                    "segment {} of {}",
                    &[&(segment + 1), &wall.segments().len()],
                ));
            }
            if ui.button(tr("Split here")).clicked() {
                *side.segments(m) = Some(wall::split(wall, segment, t));
                ui.close();
            }
            let mut kind = if curve.degree() == 1 {
                SegmentKind::Line
            } else {
                SegmentKind::Bezier
            };
            let before = kind;
            ui.horizontal(|ui| {
                ui.radio_value(&mut kind, SegmentKind::Bezier, tr("Bézier"));
                ui.radio_value(&mut kind, SegmentKind::Line, tr("Straight line"));
            });
            if kind != before {
                *side.segments(m) = Some(wall::set_kind(wall, segment, kind));
            }
            if side.drawn(&current.meridional) {
                ui.separator();
                if ui
                    .button(tr("Back to the automatic contour"))
                    .on_hover_text(tr(
                        "One Bézier from the main dimensions, with its tangent lengths",
                    ))
                    .clicked()
                {
                    *side.segments(m) = None;
                    ui.close();
                }
            }
        }
        MenuTarget::Empty => {
            ui.weak(tr("Right-click a point or a wall to edit it."));
        }
    }
    (input != *current).then_some(input)
}

/// Meridional velocity along hub and shroud: the potential flow against the uniform
/// velocity of the quasi-one-dimensional flow.
fn wall_velocities(
    ui: &mut Ui,
    flow: &PotentialFlow,
    g: &tp_design::impeller::ImpellerGeometry,
    id: ComponentId,
    height: f32,
    units: &tp_units::DisplayUnits,
) {
    let palette = Palette::of(ui);
    let speed = |v: f64| units.to_display(QuantityKind::Velocity, v);
    let symbol = units.get(QuantityKind::Velocity).symbol;
    let wall = |points: Vec<[f64; 2]>| -> PlotPoints<'static> {
        points.into_iter().map(|[t, v]| [t, speed(v)]).collect()
    };
    let quasi: PlotPoints = (0..=100)
        .map(|i| {
            let t = f64::from(i) / 100.0;
            [t, speed(g.channel.cm_at(t))]
        })
        .collect();
    let response = Plot::new(("meridional_velocity", id.0))
        .height(height)
        .x_axis_label("fraction of the wall length (0 inlet, 1 outlet)")
        .y_axis_label(format!("c_m [{symbol}]"))
        .x_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
        .y_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
        .include_y(0.0)
        .allow_scroll(false)
        .legend(Legend::default().position(Corner::RightTop))
        .show(ui, |plot| {
            plot.line(
                Line::new("hub, potential flow", wall(flow.hub_velocity()))
                    .color(palette.hub)
                    .width(1.8),
            );
            plot.line(
                Line::new("shroud, potential flow", wall(flow.shroud_velocity()))
                    .color(palette.shroud)
                    .width(1.8),
            );
            plot.line(
                Line::new("quasi-one-dimensional", quasi)
                    .color(palette.guide)
                    .width(1.2)
                    .style(LineStyle::dashed_loose()),
            );
        });
    super::settle(ui, ("meridional_velocity", id.0), &response.transform);
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
    fn drawn_walls_offer_their_points_and_move_them_exactly() {
        let g = geometry();
        let mut input = tp_design::impeller::ImpellerGeometryInput::default();
        input.meridional.hub_segments = Some(wall::split(&g.contour.hub, 0, 0.5));
        let drawn = tp_design::impeller::build(&g.basis, &input).unwrap();
        let points: Vec<Handle> = handles(&drawn.contour, &input.meridional)
            .into_iter()
            .filter(|h| matches!(h, Handle::Point(Side::Hub, _)))
            .collect();
        // One joint and two control points on each side of it.
        assert_eq!(points.len(), 5);
        assert!(!handles(&drawn.contour, &input.meridional).contains(&Handle::HubInlet));
        // The split keeps the hub where it was.
        let (a, b) = (g.contour.hub.sample(50), drawn.contour.hub.sample(50));
        assert!(a.first().unwrap().distance(*b.first().unwrap()) < 1e-12);
        let joint = Handle::Point(Side::Hub, WallPoint::Joint { segment: 0 });
        let to = handle_position(joint, &drawn.contour) + DVec2::new(0.001, 0.002);
        let mut moved = input.meridional.clone();
        apply_drag(joint, to, &drawn.contour, &drawn.basis, &mut moved);
        input.meridional = moved;
        let after = tp_design::impeller::build(&drawn.basis, &input).unwrap();
        assert!(handle_position(joint, &after.contour).distance(to) < 1e-12);
    }

    #[test]
    fn dragging_a_handle_to_its_own_position_keeps_the_input() {
        let g = geometry();
        let mut input = MeridionalInput::default();
        for handle in handles(&g.contour, &MeridionalInput::default()) {
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
