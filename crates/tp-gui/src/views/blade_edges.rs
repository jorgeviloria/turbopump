//! The blade edges in true shape: the leading and trailing edge of the hub section
//! (and of the shroud section when the shapes vary from hub to shroud), along and across
//! the mean line, with the points of each shape to drag.

use eframe::egui::{self, Color32, CursorIcon, Ui};
use egui_plot::{Line, LineStyle, Plot, PlotPoint, PlotPoints, PlotTransform};
use tp_design::impeller::blade::{EdgeInput, EdgeProfile, EdgeShape};
use tp_design::impeller::edge::bezier_points;
use tp_math::DVec2;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::{Mark, Palette, marker};
use crate::app::EditCtx;
use crate::i18n::{tf, tr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    Leading,
    Trailing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Span {
    Hub,
    Shroud,
}

/// A point of a shape to drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Point {
    /// Where the shape meets the faces (ellipse and taper).
    End,
    /// Where the rounding meets the taper.
    Nose,
    /// The tip of a taper, for its asymmetry.
    Tip,
    /// A control point of a Bézier edge.
    Bezier(usize),
}

/// One point of one plot.
pub(crate) type Grip = (Edge, Span, Point);

/// What the page keeps between frames.
#[derive(Debug, Default)]
pub(crate) struct EdgesPage {
    dragging: Option<Grip>,
    transforms: Vec<((Edge, Span), PlotTransform)>,
}

/// The shape of `edge` on `span`.
pub(crate) fn shape_of(input: &EdgeInput, edge: Edge, span: Span) -> EdgeShape {
    match (edge, span) {
        (Edge::Leading, Span::Hub) => input.leading,
        (Edge::Trailing, Span::Hub) => input.trailing,
        (Edge::Leading, Span::Shroud) => input.leading_shroud.unwrap_or(input.leading),
        (Edge::Trailing, Span::Shroud) => input.trailing_shroud.unwrap_or(input.trailing),
    }
}

/// `input` with `shape` on `edge` of `span`; a uniform edge takes it on every span.
pub(crate) fn with_shape(
    mut input: EdgeInput,
    edge: Edge,
    span: Span,
    shape: EdgeShape,
) -> EdgeInput {
    let (hub, shroud) = match edge {
        Edge::Leading => (&mut input.leading, &mut input.leading_shroud),
        Edge::Trailing => (&mut input.trailing, &mut input.trailing_shroud),
    };
    match (span, shroud.as_mut()) {
        (Span::Shroud, Some(s)) => *s = shape,
        _ => *hub = shape,
    }
    input
}

/// The points of `shape` to drag, (s in m from the edge, n in half thicknesses).
pub(crate) fn grips(shape: EdgeShape, thickness: f64) -> Vec<(Point, DVec2)> {
    let e = thickness;
    match shape {
        EdgeShape::Cut => Vec::new(),
        EdgeShape::Ellipse { ratio } => vec![(Point::End, DVec2::new(ratio * e / 2.0, 1.0))],
        EdgeShape::Taper {
            length,
            nose,
            ratio,
            asymmetry,
        } => {
            let q = 2.0 * nose / ratio;
            let shift = |s: f64| asymmetry * q * (1.0 - s / length);
            vec![
                (Point::End, DVec2::new(length * e, 1.0)),
                (Point::Nose, DVec2::new(nose * e, shift(nose) + q)),
                (Point::Tip, DVec2::new(0.0, shift(0.0))),
            ]
        }
        EdgeShape::Bezier { symmetric, .. } => bezier_points(shape, e)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .filter(|(i, _)| !(symmetric && *i == 2))
                    .map(|(i, p)| (Point::Bezier(i), *p))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// `shape` with `point` dragged to (`s` m, `n` half thicknesses), kept within the
/// ranges the panel allows.
pub(crate) fn dragged(shape: EdgeShape, point: Point, s: f64, n: f64, thickness: f64) -> EdgeShape {
    let along = (s / thickness).max(0.0);
    match (shape, point) {
        (EdgeShape::Ellipse { .. }, Point::End) => EdgeShape::Ellipse {
            ratio: (2.0 * along).clamp(1.0, 8.0),
        },
        (
            EdgeShape::Taper {
                nose,
                ratio,
                asymmetry,
                ..
            },
            Point::End,
        ) => EdgeShape::Taper {
            length: along.clamp(nose + 0.05, 20.0),
            nose,
            ratio,
            asymmetry,
        },
        (
            EdgeShape::Taper {
                length, asymmetry, ..
            },
            Point::Nose,
        ) => {
            let nose = along.clamp(0.05, length - 0.05);
            // n = q·(1 + a·(1 − R/L)) at the nose; its half width q sets the ratio.
            let q = (n / (1.0 + asymmetry * (1.0 - nose / length))).clamp(0.05, 0.95);
            EdgeShape::Taper {
                length,
                nose,
                ratio: 2.0 * nose / q,
                asymmetry,
            }
        }
        (
            EdgeShape::Taper {
                length,
                nose,
                ratio,
                ..
            },
            Point::Tip,
        ) => {
            let q = 2.0 * nose / ratio;
            EdgeShape::Taper {
                length,
                nose,
                ratio,
                asymmetry: (n / q).clamp(-0.95, 0.95),
            }
        }
        (
            EdgeShape::Bezier {
                mut start,
                mut pull,
                mut tip,
                symmetric,
            },
            Point::Bezier(i),
        ) => {
            // Points 0 and 4 slide on the faces, 1 and 3 on their tangents, 2 across.
            let face = if i < 2 { 0 } else { 1 };
            match i {
                0 | 4 => start[face] = along.clamp(0.1, 10.0),
                1 | 3 => pull[face] = (1.0 - along / start[face]).clamp(0.0, 1.0),
                _ => tip = n.clamp(-0.95, 0.95),
            }
            if symmetric {
                start = [start[face]; 2];
                pull = [pull[face]; 2];
            }
            EdgeShape::Bezier {
                start,
                pull,
                tip,
                symmetric,
            }
        }
        (shape, _) => shape,
    }
}

/// Plot coordinates of (s m, n half thicknesses) on `edge`: the leading edge points
/// to the left, the trailing edge to the right, the pressure face up.
pub(crate) fn to_plot(edge: Edge, p: DVec2, thickness: f64, scale: f64) -> PlotPoint {
    let sign = if edge == Edge::Leading { 1.0 } else { -1.0 };
    PlotPoint::new(sign * p.x * scale, p.y * thickness / 2.0 * scale)
}

/// The inverse of [`to_plot`].
fn from_plot(edge: Edge, p: PlotPoint, thickness: f64, scale: f64) -> DVec2 {
    let sign = if edge == Edge::Leading { 1.0 } else { -1.0 };
    DVec2::new(sign * p.x / scale, p.y / scale / (thickness / 2.0))
}

impl EdgesPage {
    /// The plot of `edge` on `span` as last drawn.
    #[cfg(test)]
    pub(crate) fn transform(&self, edge: Edge, span: Span) -> Option<PlotTransform> {
        self.transforms
            .iter()
            .find(|(key, _)| *key == (edge, span))
            .map(|(_, t)| *t)
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        id: ComponentId,
        units: &DisplayUnits,
    ) {
        let Some(g) = cx.derived.geometry(id) else {
            return;
        };
        let input = cx
            .doc
            .project()
            .impeller_geometry_input(id)
            .unwrap_or_default();
        let edges = input.edges.clone();
        let thickness = [
            input
                .profiles
                .thickness_leading
                .resolve(|| g.basis.blade_thickness),
            input
                .profiles
                .thickness_trailing
                .resolve(|| g.basis.blade_thickness),
        ];
        let varies = edges.leading_shroud.is_some() || edges.trailing_shroud.is_some();
        let spans: &[Span] = if varies {
            &[Span::Hub, Span::Shroud]
        } else {
            &[Span::Hub]
        };
        let palette = Palette::of(ui);
        let ink = ui.visuals().text_color();
        let scale = units.to_display(QuantityKind::Length, 1.0);
        let length = units.get(QuantityKind::Length).symbol;

        ui.horizontal_wrapped(|ui| {
            marker(ui, Mark::Line, ink, tr("suction face"));
            marker(ui, Mark::DashedLine, ink, tr("pressure face"));
            marker(ui, Mark::Circle, palette.handle, tr("points to drag"));
        });

        // Which point is under the pointer, on the plots of the last frame.
        let pick = |at: egui::Pos2| {
            let mut best: Option<(Grip, f32)> = None;
            for &((edge, span), t) in &self.transforms {
                if !t.frame().contains(at) {
                    continue;
                }
                let e = thickness[edge as usize];
                for (point, p) in grips(shape_of(&edges, edge, span), e) {
                    let d = t
                        .position_from_point(&to_plot(edge, p, e, scale))
                        .distance(at);
                    if d < 10.0 && best.is_none_or(|(_, b)| d < b) {
                        best = Some(((edge, span, point), d));
                    }
                }
            }
            best.map(|(grip, _)| grip)
        };
        let hovered = ui.ctx().pointer_latest_pos().and_then(pick);
        // A drag starts where the button went down, a few pixels before it is seen.
        let pressed = ui.input(|i| i.pointer.press_origin()).and_then(pick);
        let active = self.dragging.or(hovered);
        let mut transforms = Vec::new();
        let mut edit: Option<EdgeInput> = None;

        let spacing = ui.spacing().item_spacing.y;
        let label_height = ui.text_style_height(&egui::TextStyle::Body) + spacing;
        let height = ((ui.available_height() - 2.0 * label_height - spacing) / 2.0).max(140.0);
        ui.columns(spans.len(), |columns| {
            for (column, &span) in columns.iter_mut().zip(spans) {
                for edge in [Edge::Leading, Edge::Trailing] {
                    let e = thickness[edge as usize];
                    let shape = shape_of(&edges, edge, span);
                    let where_ = match (edge, span, varies) {
                        (Edge::Leading, _, false) => tr("Leading edge").to_owned(),
                        (Edge::Trailing, _, false) => tr("Trailing edge").to_owned(),
                        (Edge::Leading, Span::Hub, true) => {
                            tr("Leading edge at the hub").to_owned()
                        }
                        (Edge::Leading, Span::Shroud, true) => {
                            tr("Leading edge at the shroud").to_owned()
                        }
                        (Edge::Trailing, Span::Hub, true) => {
                            tr("Trailing edge at the hub").to_owned()
                        }
                        (Edge::Trailing, Span::Shroud, true) => {
                            tr("Trailing edge at the shroud").to_owned()
                        }
                    };
                    let profile = EdgeProfile::new(shape, e);
                    column.horizontal(|ui| {
                        ui.strong(where_);
                        match (&profile, shape.min_pressure_coefficient()) {
                            (Err(error), _) => {
                                ui.colored_label(
                                    ui.visuals().error_fg_color,
                                    crate::i18n::message(error),
                                );
                            }
                            (Ok(p), Some(cp)) if edge == Edge::Leading => {
                                ui.weak(tf(
                                    "nose {}, c_p,min {}",
                                    &[
                                        &format!("{:.2} {length}", p.length() * scale),
                                        &format!("{cp:.2}"),
                                    ],
                                ));
                            }
                            (Ok(p), _) if !p.is_blunt() => {
                                ui.weak(tf(
                                    "nose {}",
                                    &[&format!("{:.2} {length}", p.length() * scale)],
                                ));
                            }
                            _ => {
                                ui.weak(tr("blunt"));
                            }
                        }
                    });
                    let Ok(profile) = profile else {
                        continue;
                    };
                    let reach = (profile.length() * 1.3).max(1.5 * e);
                    let fill = palette.hub.gamma_multiply(0.18);
                    let response = Plot::new(("blade_edge", id.0, edge as u8, span as u8))
                        .height(height)
                        .data_aspect(1.0)
                        .allow_drag(false)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .allow_boxed_zoom(false)
                        .allow_double_click_reset(false)
                        .show_x(false)
                        .show_y(false)
                        .x_axis_label(format!("[{length}]"))
                        .include_x(to_plot(edge, DVec2::new(-0.25 * e, 0.0), e, scale).x)
                        .include_x(to_plot(edge, DVec2::new(reach, 0.0), e, scale).x)
                        .include_y(0.8 * e * scale)
                        .include_y(-0.8 * e * scale)
                        .y_axis_min_width(36.0)
                        .show(column, |plot| {
                            let [pressure, suction] = profile.faces();
                            let face = |points: &[DVec2], end: f64| -> PlotPoints<'static> {
                                points
                                    .iter()
                                    .copied()
                                    .chain([DVec2::new(reach, end)])
                                    .map(|p| {
                                        let q = to_plot(edge, p, e, scale);
                                        [q.x, q.y]
                                    })
                                    .collect()
                            };
                            plot.line(
                                Line::new(String::new(), face(&suction, -1.0))
                                    .color(ink)
                                    .width(1.6),
                            );
                            plot.line(
                                Line::new(String::new(), face(&pressure, 1.0))
                                    .color(ink)
                                    .width(1.6)
                                    .style(LineStyle::dashed_loose()),
                            );
                            let axis = |a: DVec2, b: DVec2| -> PlotPoints<'static> {
                                [a, b]
                                    .iter()
                                    .map(|&p| {
                                        let q = to_plot(edge, p, e, scale);
                                        [q.x, q.y]
                                    })
                                    .collect()
                            };
                            // The mean line and the meridional edge.
                            plot.line(
                                Line::new(
                                    String::new(),
                                    axis(DVec2::new(-0.25 * e, 0.0), DVec2::new(reach, 0.0)),
                                )
                                .color(palette.guide)
                                .style(LineStyle::dashed_dense()),
                            );
                            plot.line(
                                Line::new(
                                    String::new(),
                                    axis(DVec2::new(0.0, -1.5), DVec2::new(0.0, 1.5)),
                                )
                                .color(palette.leading_edge.gamma_multiply(0.7))
                                .width(1.0),
                            );
                            if let Some(c) = bezier_points(shape, e) {
                                plot.line(
                                    Line::new(
                                        String::new(),
                                        c.iter()
                                            .map(|&p| {
                                                let q = to_plot(edge, p, e, scale);
                                                [q.x, q.y]
                                            })
                                            .collect::<PlotPoints>(),
                                    )
                                    .color(palette.handle.gamma_multiply(0.6))
                                    .style(LineStyle::dashed_loose()),
                                );
                            }
                            plot.pointer_coordinate()
                        });
                    let t = response.transform;
                    super::settle(column, ("blade_edge", id.0, edge as u8, span as u8), &t);
                    paint_fill(column, &t, &profile, edge, e, reach, scale, fill);
                    let painter = column.painter_at(*t.frame());
                    for (point, p) in grips(shape, e) {
                        let at = t.position_from_point(&to_plot(edge, p, e, scale));
                        let current = active == Some((edge, span, point));
                        let color = if current {
                            palette.handle_active
                        } else {
                            palette.handle
                        };
                        painter.circle(
                            at,
                            if current { 6.0 } else { 4.5 },
                            column.visuals().extreme_bg_color,
                            egui::Stroke::new(1.8, color),
                        );
                        if let Point::Bezier(i) = point {
                            painter.text(
                                at + egui::vec2(7.0, -7.0),
                                egui::Align2::LEFT_BOTTOM,
                                i.to_string(),
                                egui::FontId::proportional(11.0),
                                color,
                            );
                        }
                    }
                    let plot_response = &response.response;
                    if plot_response.drag_started()
                        && let Some(grip) = pressed
                        && grip.0 == edge
                        && grip.1 == span
                    {
                        self.dragging = Some(grip);
                    }
                    if let (Some((de, ds, point)), Some(pointer)) = (self.dragging, response.inner)
                        && de == edge
                        && ds == span
                        && plot_response.dragged()
                    {
                        let p = from_plot(edge, pointer, e, scale);
                        let moved = dragged(shape, point, p.x, p.y, e);
                        if moved != shape {
                            edit = Some(with_shape(edges.clone(), edge, span, moved));
                        }
                    }
                    if active.is_some_and(|(ae, asp, _)| ae == edge && asp == span) {
                        column.ctx().set_cursor_icon(if self.dragging.is_some() {
                            CursorIcon::Grabbing
                        } else {
                            CursorIcon::Grab
                        });
                    }
                    transforms.push(((edge, span), t));
                }
            }
        });
        self.transforms = transforms;
        if !ui.input(|i| i.pointer.primary_down()) {
            self.dragging = None;
        }
        if let Some(edges) = edit {
            let mut input = input;
            input.edges = edges;
            cx.edit("edit blade edges", |p| p.set_impeller_geometry(id, input));
        }
    }
}

/// The blade between its faces, filled up to `reach` from the edge.
#[allow(clippy::too_many_arguments)]
fn paint_fill(
    ui: &Ui,
    t: &PlotTransform,
    profile: &EdgeProfile,
    edge: Edge,
    thickness: f64,
    reach: f64,
    scale: f64,
    color: Color32,
) {
    let mut mesh = egui::Mesh::default();
    let steps = 120;
    for k in 0..=steps {
        // Denser toward the tip, where the nose turns.
        let x = k as f64 / steps as f64;
        let s = reach * x * x;
        let (p, q) = profile.at(s);
        for n in [p, q] {
            let at = t.position_from_point(&to_plot(edge, DVec2::new(s, n), thickness, scale));
            mesh.colored_vertex(at, color);
        }
        if k > 0 {
            let i = 2 * k as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    ui.painter_at(*t.frame()).add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_a_grip_to_where_it_is_keeps_the_shape() {
        let e = 0.004;
        for shape in [
            EdgeShape::Ellipse { ratio: 4.0 },
            EdgeShape::TAPER,
            EdgeShape::Taper {
                length: 3.0,
                nose: 0.6,
                ratio: 2.0,
                asymmetry: 0.3,
            },
            EdgeShape::BEZIER,
            EdgeShape::Bezier {
                start: [2.0, 1.5],
                pull: [0.6, 0.4],
                tip: 0.3,
                symmetric: false,
            },
        ] {
            for (point, p) in grips(shape, e) {
                let again = dragged(shape, point, p.x, p.y, e);
                let close = match (shape, again) {
                    (
                        EdgeShape::Bezier {
                            start: a,
                            pull: b,
                            tip: c,
                            ..
                        },
                        EdgeShape::Bezier {
                            start: x,
                            pull: y,
                            tip: z,
                            ..
                        },
                    ) => {
                        (a[0] - x[0]).abs()
                            + (a[1] - x[1]).abs()
                            + (b[0] - y[0]).abs()
                            + (b[1] - y[1]).abs()
                            + (c - z).abs()
                            < 1e-9
                    }
                    (
                        EdgeShape::Taper {
                            length: a,
                            nose: b,
                            ratio: c,
                            asymmetry: d,
                        },
                        EdgeShape::Taper {
                            length: w,
                            nose: x,
                            ratio: y,
                            asymmetry: z,
                        },
                    ) => (a - w).abs() + (b - x).abs() + (c - y).abs() + (d - z).abs() < 1e-9,
                    (a, b) => a == b,
                };
                assert!(close, "{shape:?} {point:?} → {again:?}");
            }
        }
    }

    #[test]
    fn a_symmetric_bezier_moves_both_faces() {
        let e = 0.004;
        let moved = dragged(EdgeShape::BEZIER, Point::Bezier(4), 3.0 * e, -1.0, e);
        assert_eq!(
            moved,
            EdgeShape::Bezier {
                start: [3.0, 3.0],
                pull: [0.6, 0.6],
                tip: 0.0,
                symmetric: true,
            }
        );
        // Its point 2 stays on the mean line and is not offered.
        assert!(
            grips(EdgeShape::BEZIER, e)
                .iter()
                .all(|(p, _)| *p != Point::Bezier(2))
        );
    }

    #[test]
    fn a_uniform_edge_takes_a_shroud_edit_on_every_span() {
        let input = EdgeInput::default();
        let shape = EdgeShape::Ellipse { ratio: 2.0 };
        let edited = with_shape(input.clone(), Edge::Leading, Span::Shroud, shape);
        assert_eq!(edited.leading, shape);
        assert_eq!(edited.leading_shroud, None);
        let linear = EdgeInput {
            leading_shroud: Some(EdgeShape::Ellipse { ratio: 6.0 }),
            ..input
        };
        let edited = with_shape(linear, Edge::Leading, Span::Shroud, shape);
        assert_eq!(edited.leading, EdgeShape::Ellipse { ratio: 4.0 });
        assert_eq!(edited.leading_shroud, Some(shape));
    }
}
