//! Cross-section view of a volute: its sections round the spiral in their own plane,
//! the width B across and the height H outward from r'_z, with the definition being
//! edited drawn bold at the height the spiral gives it. The control points of its
//! Bézier and line-segment sides can be dragged; a right click adds or removes a corner.
//!
//! Modifier keys constrain a dragged corner: Ctrl (⌘ on a Mac) keeps its distance to the
//! previous corner, with Shift to the next; Alt keeps it on the line through the
//! previous corner and where the drag started, with Shift through the next.

use std::f64::consts::TAU;

use eframe::egui::{Align2, CursorIcon, Sense, Ui};
use egui_plot::{
    Corner, Legend, Line, LineStyle, Plot, PlotPoint, PlotPoints, PlotTransform, Points, Text,
};
use tp_design::volute::section::segment_vertices;
use tp_design::volute::{SectionShape, SegmentPoint, SideShape, VoluteInput};
use tp_math::DVec2;
use tp_project::ComponentId;
use tp_units::QuantityKind;

use super::{Palette, unavailable};
use crate::app::EditCtx;
use crate::i18n::{tf, tr};
use crate::section_editor::selected;

/// A control point of the selected definition: on the −z or the +z side, Bézier point 1
/// to 3 or corner `index` of a line-segment side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Handle {
    right: bool,
    index: usize,
}

/// State of the cross-section view between frames.
#[derive(Debug, Default)]
pub(crate) struct CrossSectionView {
    dragging: Option<Handle>,
    /// Where the dragged point started, in the frame of its side.
    drag_start: Option<DVec2>,
    /// Plot transform of the last frame, to find the point under the pointer.
    transform: Option<PlotTransform>,
    /// Where the context menu was opened, (s, h) in m.
    menu_at: Option<DVec2>,
}

/// The −z side, and the +z side when it has its own shape.
fn sides(shape: &SectionShape) -> Vec<(bool, &SideShape)> {
    match shape {
        SectionShape::Sides { left, right } => {
            let mut out = vec![(false, left)];
            if let Some(r) = right {
                out.push((true, r));
            }
            out
        }
        SectionShape::RoundExternal(_) | SectionShape::RoundInternal(_) => Vec::new(),
    }
}

/// (s, h) of a point (x, y) in the frame of a side.
fn placed(right: bool, p: DVec2) -> DVec2 {
    DVec2::new(if right { p.x } else { -p.x }, p.y)
}

/// The draggable points of `shape` at height `h`, (s, h).
fn handles(shape: &SectionShape, b3: f64, h: f64) -> Vec<(Handle, DVec2)> {
    let mut out = Vec::new();
    for (right, side) in sides(shape) {
        match side {
            SideShape::Bezier(b) => {
                let p = b.control_points(b3, h);
                for (index, point) in p.iter().enumerate().take(4).skip(1) {
                    out.push((Handle { right, index }, placed(right, *point)));
                }
            }
            SideShape::LineSegments { points } => {
                let v = segment_vertices(points, b3, h);
                for index in 0..points.len() {
                    out.push((Handle { right, index }, placed(right, v[index + 1])));
                }
            }
            _ => {}
        }
    }
    out
}

/// The side of `shape` a handle on the `right` belongs to.
fn side_mut(shape: &mut SectionShape, right: bool) -> Option<&mut SideShape> {
    match shape {
        SectionShape::Sides { left, right: r } => {
            if right {
                r.as_mut()
            } else {
                Some(left)
            }
        }
        SectionShape::RoundExternal(_) | SectionShape::RoundInternal(_) => None,
    }
}

impl CrossSectionView {
    /// Shows the sections of `volute`, the plot kept clear of the `cover` px the
    /// properties panel hides on the right.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        volute: Option<ComponentId>,
        cover: f32,
    ) {
        let Some(id) = volute else {
            ui.centered_and_justified(|ui| {
                ui.weak(tr("Add a volute to see its cross sections."));
            });
            return;
        };
        let g = match cx.derived.volute_result(id) {
            Some(Ok(g)) => g,
            Some(Err(e)) => {
                unavailable(ui, e);
                return;
            }
            None => return,
        };
        let Ok(input) = cx.doc.project().volute_input(id) else {
            return;
        };
        let units = cx.units();
        let unit = units.get(QuantityKind::Length).symbol;
        let scale = units.to_display(QuantityKind::Length, 1.0);
        let palette = Palette::of(ui);
        let definitions = &input.cross_section.sections;
        let index = selected(ui.ctx(), id, definitions.len());
        let definition = &definitions[index];
        let b3 = g.design.inlet_width;
        let height_at = |angle: f64| {
            g.design
                .section(angle.clamp(0.0, TAU))
                .map_or(0.0, |s| s.height)
        };
        let h = height_at(definition.angle);
        if h <= 0.0 {
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.label(tf(
                "Definition at {}, shown {} high as the spiral sizes it there",
                &[
                    &units.show(QuantityKind::Angle, definition.angle),
                    &units.show(QuantityKind::Length, h),
                ],
            ));
        });

        let points = handles(&definition.shape, b3, h);
        let hovered = match (ui.ctx().pointer_hover_pos(), &self.transform) {
            (Some(pos), Some(t)) if t.frame().contains(pos) => points
                .iter()
                .map(|&(handle, p)| {
                    let screen = t.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
                    (handle, screen.distance(pos))
                })
                .filter(|&(_, d)| d < 10.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(handle, _)| handle),
            _ => None,
        };
        let active = self.dragging.or(hovered);
        let frozen = self
            .dragging
            .and(self.transform.as_ref())
            .map(|t| *t.bounds());
        let at = |p: DVec2| [p.x * scale, p.y * scale];
        let closed = |outline: &[DVec2]| -> PlotPoints<'static> {
            outline
                .iter()
                .chain(outline.first())
                .map(|p| at(*p))
                .collect()
        };

        let response = Plot::new(("cross_section", id.0))
            .width((ui.available_width() - cover).max(200.0))
            .data_aspect(1.0)
            .allow_drag(active.is_none())
            .sense(Sense::click_and_drag())
            .x_axis_label(format!("B [{unit}]"))
            .y_axis_label(format!("H [{unit}]"))
            .legend(Legend::default().position(Corner::LeftBottom))
            .show(ui, |plot| {
                if let Some(bounds) = frozen {
                    plot.set_plot_bounds(bounds);
                }
                // The spiral every 45°, shaped by the active definitions.
                for k in 1..=8 {
                    let angle = TAU * f64::from(k) / 8.0;
                    let Ok(size) = g.design.section(angle) else {
                        continue;
                    };
                    if size.height <= 0.0 {
                        continue;
                    }
                    let outline = g.design.section_outline(angle, &size);
                    plot.line(
                        Line::new(tr("sections round the spiral"), closed(&outline.points))
                            .color(palette.guide)
                            .width(1.0),
                    );
                    let top = outline.points[outline.top];
                    plot.text(
                        Text::new(
                            "",
                            PlotPoint::new(top.x * scale, top.y * scale),
                            format!("{:.0}°", angle.to_degrees()),
                        )
                        .anchor(Align2::CENTER_BOTTOM)
                        .color(palette.guide),
                    );
                }
                // The bend and neck of an internal section, with the limits the spiral
                // keeps within: the top of the neck and its face.
                if let SectionShape::RoundInternal(round) = &definition.shape {
                    let bend = round.bend(b3);
                    for wall in [&bend.outer, &bend.inner] {
                        let points: PlotPoints = wall.iter().flatten().map(|p| at(*p)).collect();
                        plot.line(
                            Line::new(tr("bend and neck"), points)
                                .color(palette.stator)
                                .width(2.0),
                        );
                    }
                    let away = bend.face.signum();
                    let reach = 1.2 * (height_at(TAU).max(h) + b3);
                    let far = bend.face + away * reach;
                    for (a, b) in [
                        (DVec2::new(bend.face, bend.top), DVec2::new(far, bend.top)),
                        (
                            DVec2::new(bend.face, bend.top),
                            DVec2::new(bend.face, bend.top - reach),
                        ),
                    ] {
                        plot.line(
                            Line::new(tr("limits of the spiral"), vec![at(a), at(b)])
                                .color(palette.guide)
                                .style(LineStyle::dashed_loose())
                                .width(1.0),
                        );
                    }
                }
                // Inlet and centre line.
                plot.line(
                    Line::new(
                        tr("inlet b₃"),
                        vec![
                            at(DVec2::new(-b3 / 2.0, 0.0)),
                            at(DVec2::new(b3 / 2.0, 0.0)),
                        ],
                    )
                    .color(palette.stator)
                    .width(2.0),
                );
                let reach = height_at(TAU).max(h) * 1.05;
                plot.line(
                    Line::new(
                        "",
                        vec![at(DVec2::new(0.0, 0.0)), at(DVec2::new(0.0, reach))],
                    )
                    .color(palette.guide)
                    .style(LineStyle::dashed_loose())
                    .width(0.8),
                );
                // The definition being edited, as the spiral sizes it at its angle.
                let outline = definition.shape.outline(b3, h);
                let line = Line::new(tr("selected definition"), closed(&outline.points))
                    .color(palette.volute)
                    .width(2.5);
                plot.line(if definition.active {
                    line
                } else {
                    line.style(LineStyle::dashed_dense())
                });
                // Control polygons.
                for (right, side) in sides(&definition.shape) {
                    let polygon: Vec<DVec2> = match side {
                        SideShape::Bezier(b) => {
                            let p = b.control_points(b3, h);
                            plot.points(
                                Points::new("", vec![at(placed(right, p[5]))])
                                    .radius(3.0)
                                    .color(palette.guide),
                            );
                            p[..5].iter().map(|q| placed(right, *q)).collect()
                        }
                        SideShape::LineSegments { points } => segment_vertices(points, b3, h)
                            .into_iter()
                            .map(|q| placed(right, q))
                            .collect(),
                        _ => continue,
                    };
                    plot.line(
                        Line::new("", polygon.iter().map(|p| at(*p)).collect::<PlotPoints>())
                            .color(palette.handle)
                            .style(LineStyle::dashed_loose())
                            .width(1.0),
                    );
                }
                for &(handle, p) in &points {
                    let current = active == Some(handle);
                    plot.points(
                        Points::new("", vec![at(p)])
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

        super::settle(ui, ("cross_section", id.0), &response.transform);
        self.transform = Some(response.transform);
        let plot_response = &response.response;
        let pointer = response.inner.map(|p| DVec2::new(p.x / scale, p.y / scale));
        if plot_response.drag_started()
            && let Some(handle) = hovered
        {
            self.dragging = Some(handle);
            self.drag_start = points
                .iter()
                .find(|(h, _)| *h == handle)
                .map(|(_, p)| DVec2::new(p.x.abs(), p.y));
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.dragging = None;
        }
        if active.is_some() {
            ui.ctx().set_cursor_icon(if self.dragging.is_some() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        if let (Some(handle), Some(pointer)) = (self.dragging, pointer)
            && plot_response.dragged()
        {
            let modifiers = ui.input(|i| i.modifiers);
            let mut edited = input.clone();
            let target = DVec2::new(if handle.right { pointer.x } else { -pointer.x }, pointer.y);
            if let Some(side) = side_mut(
                &mut edited.cross_section.sections[index].shape,
                handle.right,
            ) {
                drag(
                    side,
                    handle.index,
                    target,
                    self.drag_start,
                    modifiers,
                    b3,
                    h,
                );
            }
            self.commit(ui, cx, id, &input, edited, tr("drag a cross-section point"));
        }

        if plot_response.secondary_clicked() {
            self.menu_at = pointer;
        }
        let near = hovered.or(self.menu_at.and_then(|m| {
            points
                .iter()
                .filter(|(_, p)| p.distance(m) < 0.02 * h.max(b3))
                .map(|(handle, _)| *handle)
                .next()
        }));
        let mut menu_edit = None;
        plot_response.context_menu(|ui| {
            let Some(at) = self.menu_at else {
                ui.close();
                return;
            };
            let mut edited = input.clone();
            let shape = &mut edited.cross_section.sections[index].shape;
            if let Some(handle) = near
                && let Some(SideShape::LineSegments { points }) = side_mut(shape, handle.right)
            {
                if ui.button(tr("Remove this corner")).clicked() {
                    points.remove(handle.index);
                    menu_edit = Some((edited, tr("remove a cross-section corner")));
                    ui.close();
                }
                return;
            }
            match insert_corner(shape, at, b3, h) {
                Some(()) => {
                    if ui.button(tr("Add a corner here")).clicked() {
                        menu_edit = Some((edited, tr("add a cross-section corner")));
                        ui.close();
                    }
                }
                None => {
                    ui.weak(tr("Only line-segment sides have corners to add or remove."));
                }
            }
        });
        if let Some((edited, label)) = menu_edit {
            self.commit(ui, cx, id, &input, edited, label);
        }
    }

    fn commit(
        &self,
        ui: &Ui,
        cx: &mut EditCtx<'_>,
        id: ComponentId,
        input: &VoluteInput,
        edited: VoluteInput,
        label: &str,
    ) {
        if edited != *input {
            cx.edit(label, |p| p.set_volute(id, edited));
            // Show the new shape even if the pointer stops here.
            ui.ctx().request_repaint();
        }
    }
}

/// Moves point `index` of `side` toward `target` (frame of the side) for a section of
/// height `h` on an inlet of width `b3`, constrained by the modifier keys for corners.
fn drag(
    side: &mut SideShape,
    index: usize,
    target: DVec2,
    start: Option<DVec2>,
    modifiers: eframe::egui::Modifiers,
    b3: f64,
    h: f64,
) {
    match side {
        SideShape::Bezier(b) => b.drag(index, target, b3, h),
        SideShape::LineSegments { points } => {
            let v = segment_vertices(points, b3, h);
            let (previous, next) = (v[index], v[index + 2]);
            let start = start.unwrap_or(v[index + 1]);
            let anchor = if modifiers.shift { next } else { previous };
            let target = if modifiers.command {
                // On the circle about the neighbour through where the drag started.
                anchor + (target - anchor).normalize_or_zero() * start.distance(anchor)
            } else if modifiers.alt {
                // On the line through the neighbour and where the drag started.
                let along = (start - anchor).normalize_or_zero();
                anchor + along * (target - anchor).dot(along)
            } else {
                target
            };
            let radius = points[index].radius;
            points[index] = SegmentPoint::at(
                DVec2::new(target.x.max(0.0), target.y.max(1e-3 * h)),
                b3,
                h,
                radius,
            );
        }
        _ => {}
    }
}

/// Adds a sharp corner at `at` ((s, h)) to the line-segment side of `shape` on that side
/// of the centre line, on its segment nearest to `at`. `None` when that side has no line
/// segments.
fn insert_corner(shape: &mut SectionShape, at: DVec2, b3: f64, h: f64) -> Option<()> {
    let right = at.x > 0.0 && matches!(shape, SectionShape::Sides { right: Some(_), .. });
    let Some(SideShape::LineSegments { points }) = side_mut(shape, right) else {
        return None;
    };
    let p = DVec2::new(at.x.abs(), at.y);
    let v = segment_vertices(points, b3, h);
    let distance = |a: DVec2, b: DVec2| {
        let t = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
        p.distance(a + (b - a) * t)
    };
    let segment = (0..v.len() - 1)
        .min_by(|&i, &j| distance(v[i], v[i + 1]).total_cmp(&distance(v[j], v[j + 1])))?;
    points.insert(segment, SegmentPoint::at(p, b3, h, 0.0));
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_design::volute::section::default_segments;

    #[test]
    fn corners_go_on_the_nearest_segment() {
        let (b3, h) = (0.03, 0.05);
        let mut shape = SectionShape::symmetric(SideShape::LineSegments {
            points: default_segments(),
        });
        // Halfway along the flank, between the two default corners, on the −z side.
        let v = segment_vertices(&default_segments(), b3, h);
        let mid = 0.5 * (v[1] + v[2]);
        insert_corner(&mut shape, DVec2::new(-mid.x, mid.y), b3, h).unwrap();
        let SectionShape::Sides {
            left: SideShape::LineSegments { points },
            ..
        } = &shape
        else {
            panic!();
        };
        assert_eq!(points.len(), 3);
        let added = segment_vertices(points, b3, h)[2];
        assert!(added.distance(mid) < 1e-12);
        // No line segments on a trapezoid.
        let mut trapezoid = SectionShape::default();
        assert!(insert_corner(&mut trapezoid, mid, b3, h).is_none());
    }

    #[test]
    fn a_dragged_corner_keeps_its_distance_with_ctrl() {
        let (b3, h) = (0.03, 0.05);
        let mut side = SideShape::LineSegments {
            points: default_segments(),
        };
        let before = match &side {
            SideShape::LineSegments { points } => segment_vertices(points, b3, h),
            _ => unreachable!(),
        };
        let modifiers = eframe::egui::Modifiers {
            command: true,
            ..Default::default()
        };
        drag(
            &mut side,
            1,
            DVec2::new(0.04, 0.03),
            Some(before[2]),
            modifiers,
            b3,
            h,
        );
        let SideShape::LineSegments { points } = &side else {
            unreachable!()
        };
        let after = segment_vertices(points, b3, h);
        assert!((after[2].distance(after[1]) - before[2].distance(before[1])).abs() < 1e-12);
    }
}
