//! The cutwater page of the front view: the cutwater in the planes across the inlet
//! width, with points to drag on it, the measures to check it by and a small 3D view of
//! its body.
//!
//! Three points edit the design: the innermost point of the nose moves the cutwater
//! circle d_z, the leading point of the nose its radius ρ, and a point on the camber
//! line its incidence i₃ against the flow angle α₃′.

use std::f64::consts::PI;

use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_plot::{
    HLine, Line, LineStyle, Plot, PlotBounds, PlotPoint, PlotPoints, PlotTransform, Points, VLine,
};
use tp_design::Param;
use tp_design::volute::{CutwaterShape, CutwaterSlice, VoluteGeometry, VoluteInput};
use tp_math::{DVec2, DVec3};
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::{Mark, Palette, marker, unavailable};
use crate::app::EditCtx;
use crate::i18n::{tf, tr};

/// Largest incidence a drag sets, rad (the warning starts at ±3°).
const MAX_INCIDENCE: f64 = 10.0 * PI / 180.0;
/// The camber point lies this many nose radii from the nose centre.
const CAMBER_REACH: f64 = 6.0;
/// Every this many points of an outline, in the 3D view.
const PREVIEW_STRIDE: usize = 3;

/// A point of the cutwater to drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grip {
    /// The innermost point of the nose, on the cutwater circle d_z.
    Nose,
    /// The leading point of the nose, ρ ahead of its centre.
    Radius,
    /// A point on the camber line, for the blade angle α₃B.
    Camber,
}

/// The cutwater page between frames.
#[derive(Debug)]
pub(crate) struct CutwaterPage {
    /// The plane shown, from −z; `None` for the midplane.
    pub plane: Option<usize>,
    /// Draw the body in every plane, faintly, behind the one shown.
    pub all_planes: bool,
    dragging: Option<Grip>,
    /// The plot as last drawn.
    transform: Option<PlotTransform>,
    /// Frame the plot on the cutwater next frame: when the page opens and on a
    /// double-click.
    pub frame: bool,
    /// Turn of the 3D view about the pump axis and its tilt, rad.
    yaw: f32,
    pitch: f32,
    /// The plane z to show in the 3D view, asked for with its button.
    pub open_3d: Option<f64>,
    /// Why the last obliquity asked for could not be built.
    lean_limit: Option<String>,
}

impl Default for CutwaterPage {
    fn default() -> Self {
        Self {
            plane: None,
            all_planes: true,
            dragging: None,
            transform: None,
            frame: true,
            lean_limit: None,
            yaw: -0.6,
            pitch: 0.9,
            open_3d: None,
        }
    }
}

/// Plot coordinates of a point of the volute, seen from the inlet as in the front
/// view: x points left.
fn to_plot(p: DVec2, scale: f64) -> PlotPoint {
    PlotPoint::new(-p.x * scale, p.y * scale)
}

fn from_plot(p: PlotPoint, scale: f64) -> DVec2 {
    DVec2::new(-p.x / scale, p.y / scale)
}

/// The frame at the nose: its centre, outward, downstream along the circumference and
/// along the camber line.
struct Nose {
    centre: DVec2,
    radial: DVec2,
    along: DVec2,
    camber: DVec2,
    radius: f64,
}

impl Nose {
    fn of(g: &VoluteGeometry) -> Self {
        let centre = g.cutwater.nose_center.truncate();
        let radial = centre.normalize();
        let along = DVec2::new(-radial.y, radial.x) * g.basis.rotation_sign;
        let (s, c) = g.design.cutwater_angle.sin_cos();
        Self {
            centre,
            radial,
            along,
            camber: along * c + radial * s,
            radius: g.cutwater.radius,
        }
    }

    /// Where each point to drag lies.
    fn grip(&self, grip: Grip) -> DVec2 {
        match grip {
            Grip::Nose => self.centre - self.radial * self.radius,
            Grip::Radius => self.centre - self.camber * self.radius,
            Grip::Camber => self.centre + self.camber * (CAMBER_REACH * self.radius),
        }
    }

    /// The blade angle α₃B of a camber line from the centre through `p`, rad.
    fn angle_to(&self, p: DVec2) -> f64 {
        let d = p - self.centre;
        d.dot(self.radial).atan2(d.dot(self.along))
    }
}

/// The inputs with `grip` dragged to `p`, or `None` when nothing changes.
fn dragged(
    g: &VoluteGeometry,
    input: &VoluteInput,
    nose: &Nose,
    grip: Grip,
    p: DVec2,
) -> Option<VoluteInput> {
    let mut next = input.clone();
    match grip {
        Grip::Nose => {
            let dz = 2.0 * p.length();
            if dz <= g.basis.impeller_diameter {
                return None;
            }
            next.setup.cutwater_diameter = Param::Manual(dz);
        }
        Grip::Radius => {
            // The innermost point stays on d_z while the radius changes, so the leading
            // point moves from it along r̂ − c, by the radius.
            let fixed = nose.centre - nose.radial * nose.radius;
            let way = nose.radial - nose.camber;
            let radius = ((p - fixed).dot(way) / way.length_squared()).max(1e-4);
            next.cutwater.shape = CutwaterShape::Rounded {
                radius: Param::Manual(radius),
            };
        }
        Grip::Camber => {
            let incidence = (nose.angle_to(p) - g.design.cutwater_flow_angle)
                .clamp(-MAX_INCIDENCE, MAX_INCIDENCE);
            next.cutwater.incidence = Param::Manual(incidence);
        }
    }
    (next != *input).then_some(next)
}

/// The grips a cutwater of this shape has.
fn grips(g: &VoluteGeometry) -> &'static [Grip] {
    if g.cutwater.radius > 0.0 {
        &[Grip::Nose, Grip::Radius, Grip::Camber]
    } else {
        &[]
    }
}

/// How a measure compares with its guideline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    Good,
    Watch,
    Bad,
    /// Information only.
    Info,
}

impl Check {
    fn glyph(self) -> &'static str {
        match self {
            Check::Good => "✔",
            Check::Watch => "⚠",
            Check::Bad => "✖",
            Check::Info => "•",
        }
    }

    fn color(self, ui: &Ui) -> Color32 {
        match self {
            Check::Good => Color32::from_rgb(60, 160, 90),
            Check::Watch => ui.visuals().warn_fg_color,
            Check::Bad => ui.visuals().error_fg_color,
            Check::Info => ui.visuals().weak_text_color(),
        }
    }
}

/// The lowest and highest of `f` over the planes.
fn range(slices: &[CutwaterSlice], f: impl Fn(&CutwaterSlice) -> f64) -> (f64, f64) {
    slices
        .iter()
        .map(f)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
            (lo.min(x), hi.max(x))
        })
}

impl CutwaterPage {
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        impeller: ComponentId,
        volute: ComponentId,
        units: &DisplayUnits,
    ) {
        let Some(g) = cx.derived.volute(volute) else {
            let reason = cx
                .derived
                .volute_result(volute)
                .and_then(|r| r.as_ref().err())
                .map_or_else(
                    || tr("No volute yet.").to_owned(),
                    |e| crate::i18n::message(e),
                );
            unavailable(ui, &reason);
            return;
        };
        let slices = &g.cutwater_slices;
        if slices.is_empty() {
            unavailable(
                ui,
                tr(
                    "This cutwater does not cut the diffuser wall: choose a sharp or rounded one to shape it here.",
                ),
            );
            return;
        }
        let input = cx.doc.project().volute_input(volute).unwrap_or_default();
        let plane = self.plane.unwrap_or(slices.len() / 2).min(slices.len() - 1);
        let scale = units.to_display(QuantityKind::Length, 1.0);
        let length = units.get(QuantityKind::Length).symbol;
        let mut edit: Option<VoluteInput> = None;

        // The plane shown and what to draw.
        ui.horizontal_wrapped(|ui| {
            let mut index = plane;
            let zm = g.basis.midplane;
            ui.label(tr("Plane"));
            let label = tf(
                "z − z_m = {}",
                &[&format!("{:+.2} {length}", (slices[plane].z - zm) * scale)],
            );
            ui.add(
                egui::Slider::new(&mut index, 0..=slices.len() - 1)
                    .show_value(false)
                    .text(label),
            );
            if index != plane {
                self.plane = Some(index);
            }
            if ui.button(tr("Midplane")).clicked() {
                self.plane = None;
            }
            ui.checkbox(&mut self.all_planes, tr("All planes"));
            if ui
                .button(format!("{}  {}", crate::theme::icon::CUBE, tr("View in 3D")))
                .on_hover_text(tr("The 3D view on the cutwater, cut across the axis at this plane"))
                .clicked()
            {
                self.open_3d = Some(slices[plane].z);
            }
            if let CutwaterShape::Rounded { .. } = input.cutwater.shape {
                ui.separator();
                let mut share = g.design.cutwater_share;
                let response = ui
                    .add(egui::Slider::new(&mut share, 0.0..=1.0).text(tr("spiral share")))
                    .on_hover_text(tr(
                        "Share of the missing thickness the spiral side of the body takes past the tip; the diffuser side takes the rest",
                    ));
                if response.changed() {
                    let mut next = input.clone();
                    next.cutwater.share = Param::Manual(share);
                    edit = Some(next);
                }
                // The obliquity only as far as the walls leave room: a lean they cannot
                // take keeps the last one and says how far it goes.
                let mut lean = g.design.cutwater_obliquity.to_degrees();
                let most = tp_design::volute::MAX_OBLIQUITY.to_degrees();
                let response = ui
                    .add(egui::Slider::new(&mut lean, -most..=most).suffix("°").text(tr("obliquity ε")))
                    .on_hover_text(tr(
                        "Angle between the edge and the pump axis, unrolled onto d_z; Gülich Table 10.2 asks for more than 35° against pressure pulsations",
                    ));
                if response.changed() {
                    let mut next = input.clone();
                    next.cutwater.obliquity = Param::Manual(lean.to_radians());
                    match VoluteGeometry::build(&g.basis, &next) {
                        Ok(_) => {
                            self.lean_limit = None;
                            edit = Some(next);
                        }
                        Err(e) => self.lean_limit = Some(crate::i18n::message(&e.to_string())),
                    }
                }
                if let Some(limit) = &self.lean_limit {
                    ui.colored_label(ui.visuals().warn_fg_color, limit);
                }
            }
        });

        let area = ui.available_rect_before_wrap();
        let side = (area.width() * 0.44).clamp(300.0, 520.0);
        let gap = 8.0;
        let left = Rect::from_min_max(area.min, pos2(area.max.x - side - gap, area.max.y));
        let right = Rect::from_min_max(pos2(area.max.x - side, area.min.y), area.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
            if let Some(next) = self.plot(ui, g, &input, plane, scale, length) {
                edit = Some(next);
            }
        });
        // Where the flow meets the cutwater within ±3°, as q*.
        let band = cx
            .derived
            .impeller(impeller)
            .and_then(|r| r.performance.as_ref().ok())
            .and_then(|p| {
                let (lo, hi) = p.volute.as_ref()?.incidence_range?;
                Some((lo / p.design.flow, hi / p.design.flow))
            });
        ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
            measures(ui, g, band, units);
            let height = ((ui.available_height() - gap) / 2.0).max(120.0);
            self.preview(ui, g, plane, height);
            ui.add_space(gap);
            incidence_plot(ui, cx, g, impeller, ui.available_height().max(120.0));
        });
        ui.allocate_rect(area, Sense::hover());

        if let Some(next) = edit {
            cx.edit("edit volute cutwater", |p| p.set_volute(volute, next));
        }
    }

    /// The cutwater in the plane shown, with its points to drag. Returns the inputs a
    /// drag changed.
    fn plot(
        &mut self,
        ui: &mut Ui,
        g: &VoluteGeometry,
        input: &VoluteInput,
        plane: usize,
        scale: f64,
        length: &str,
    ) -> Option<VoluteInput> {
        let palette = Palette::of(ui);
        let slices = &g.cutwater_slices;
        let shown = &slices[plane];
        let nose = Nose::of(g);
        let grips = grips(g);
        let ink = ui.visuals().text_color();

        // Which point is under the pointer, on the plot of the last frame.
        let pick = |at: Pos2| -> Option<Grip> {
            let t = self.transform?;
            if !t.frame().contains(at) {
                return None;
            }
            grips
                .iter()
                .map(|&grip| {
                    let p = t.position_from_point(&to_plot(nose.grip(grip), scale));
                    (grip, p.distance(at))
                })
                .filter(|(_, d)| *d < 10.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(grip, _)| grip)
        };
        let hovered = ui.ctx().pointer_latest_pos().and_then(pick);
        let pressed = ui.input(|i| i.pointer.press_origin()).and_then(pick);
        let active = self.dragging.or(hovered);

        ui.horizontal_wrapped(|ui| {
            marker(ui, Mark::Line, palette.hub, tr("spiral wall"));
            marker(ui, Mark::Line, palette.volute, tr("diffuser wall"));
            marker(ui, Mark::Square, palette.shroud, tr("cutwater"));
            marker(ui, Mark::Dashed, palette.guide, tr("cutwater circle d_z"));
            if !grips.is_empty() {
                marker(ui, Mark::Circle, palette.handle, tr("points to drag"));
            }
        });

        // Frame the body of the plane shown and some of the walls around it.
        let body: Vec<DVec2> = if shown.outline.is_empty() {
            vec![shown.tip.truncate()]
        } else {
            shown.outline.iter().map(|p| p.truncate()).collect()
        };
        let (lo, hi) = body.iter().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let centre = 0.5 * (lo + hi);
        let half = 0.5 * (hi - lo).max_element().max(0.01) * 1.4;
        let near = |p: &DVec3| (p.truncate() - centre).length() < 2.5 * half;
        let line = |points: &mut dyn Iterator<Item = DVec2>| -> PlotPoints<'static> {
            points
                .map(|p| {
                    let q = to_plot(p, scale);
                    [q.x, q.y]
                })
                .collect()
        };
        let circle = |r: f64| -> PlotPoints<'static> {
            let a0 = centre.y.atan2(centre.x);
            let span = 3.0 * half / r;
            line(&mut (0..=120).map(|i| {
                let a = a0 - span + 2.0 * span * f64::from(i) / 120.0;
                DVec2::new(r * a.cos(), r * a.sin())
            }))
        };
        let allow_drag = pressed.is_none() && self.dragging.is_none();
        let frame = std::mem::take(&mut self.frame);
        let (a, b) = (
            to_plot(centre - DVec2::splat(half), scale),
            to_plot(centre + DVec2::splat(half), scale),
        );
        let bounds =
            PlotBounds::from_min_max([a.x.min(b.x), a.y.min(b.y)], [a.x.max(b.x), a.y.max(b.y)]);
        let response = Plot::new(("cutwater_plot", g.basis.midplane.to_bits()))
            .data_aspect(1.0)
            .allow_drag(allow_drag)
            .allow_scroll(false)
            .allow_double_click_reset(false)
            .show_x(false)
            .show_y(false)
            .x_axis_label(format!("[{length}]"))
            .show(ui, |plot| {
                if frame {
                    plot.set_plot_bounds(bounds);
                }
                plot.line(
                    Line::new(
                        tr("cutwater circle d_z"),
                        circle(g.design.cutwater_diameter / 2.0),
                    )
                    .color(palette.guide)
                    .style(LineStyle::dashed_loose()),
                );
                plot.line(
                    Line::new(
                        tr("impeller outlet"),
                        circle(g.basis.impeller_diameter / 2.0),
                    )
                    .color(palette.guide)
                    .style(LineStyle::dotted_dense()),
                );
                if self.all_planes {
                    for (k, s) in slices.iter().enumerate() {
                        if k == plane || s.outline.is_empty() {
                            continue;
                        }
                        let mut closed = s.outline.iter().map(|p| p.truncate()).collect::<Vec<_>>();
                        closed.push(closed[0]);
                        plot.line(
                            Line::new(String::new(), line(&mut closed.into_iter()))
                                .color(palette.shroud.gamma_multiply(0.35))
                                .width(1.0),
                        );
                    }
                }
                plot.line(
                    Line::new(
                        tr("spiral wall"),
                        line(
                            &mut shown
                                .spiral_wall
                                .iter()
                                .filter(|p| near(p))
                                .map(|p| p.truncate()),
                        ),
                    )
                    .color(palette.hub)
                    .width(2.0),
                );
                plot.line(
                    Line::new(
                        tr("diffuser wall"),
                        line(
                            &mut shown
                                .diffuser_wall
                                .iter()
                                .filter(|p| near(p))
                                .map(|p| p.truncate()),
                        ),
                    )
                    .color(palette.volute)
                    .width(2.0),
                );
                if !shown.outline.is_empty() {
                    let mut closed = body.clone();
                    closed.push(body[0]);
                    plot.line(
                        Line::new(tr("cutwater"), line(&mut closed.into_iter()))
                            .color(palette.shroud)
                            .width(1.8),
                    );
                }
                let tip = to_plot(shown.tip.truncate(), scale);
                plot.points(
                    Points::new(tr("tip of the wedge"), vec![[tip.x, tip.y]])
                        .color(ink)
                        .radius(3.0),
                );
                if !grips.is_empty() {
                    // The camber line from the nose to its point.
                    plot.line(
                        Line::new(
                            String::new(),
                            line(&mut [nose.centre, nose.grip(Grip::Camber)].into_iter()),
                        )
                        .color(palette.handle.gamma_multiply(0.7))
                        .style(LineStyle::dashed_dense()),
                    );
                }
                plot.pointer_coordinate()
            });
        let t = response.transform;
        super::settle(ui, ("cutwater_plot", g.basis.midplane.to_bits()), &t);
        self.transform = Some(t);
        if !shown.outline.is_empty() {
            fill_strip(ui, &t, &body, scale, palette.shroud.gamma_multiply(0.25));
        }
        let painter = ui.painter_at(*t.frame());
        for &grip in grips {
            let at = t.position_from_point(&to_plot(nose.grip(grip), scale));
            let current = active == Some(grip);
            let color = if current {
                palette.handle_active
            } else {
                palette.handle
            };
            painter.circle(
                at,
                if current { 6.0 } else { 4.5 },
                ui.visuals().extreme_bg_color,
                Stroke::new(1.8, color),
            );
        }
        if let Some(grip) = active {
            response.response.clone().on_hover_text(match grip {
                Grip::Nose => tr("Innermost point of the nose: drag outward or inward to set the cutwater circle d_z"),
                Grip::Radius => tr("Leading point of the nose: drag to set its radius ρ"),
                Grip::Camber => tr("Camber line: turn it to set the incidence i₃ against the flow"),
            });
            ui.ctx().set_cursor_icon(if self.dragging.is_some() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        let plot_response = &response.response;
        if plot_response.double_clicked() {
            self.frame = true;
            ui.ctx().request_repaint();
        }
        if plot_response.drag_started()
            && let Some(grip) = pressed
        {
            self.dragging = Some(grip);
        }
        let mut edit = None;
        if let (Some(grip), Some(pointer)) = (self.dragging, response.inner)
            && plot_response.dragged()
        {
            edit = dragged(g, input, &nose, grip, from_plot(pointer, scale));
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.dragging = None;
        }
        edit
    }

    /// The body of the cutwater across the width, in 3D, the plane shown outlined. Drag
    /// to turn it.
    fn preview(&mut self, ui: &mut Ui, g: &VoluteGeometry, plane: usize, height: f32) {
        let palette = Palette::of(ui);
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4, ui.visuals().faint_bg_color);
        if response.dragged() {
            let d = response.drag_delta();
            self.yaw += d.x * 0.01;
            self.pitch = (self.pitch + d.y * 0.01).clamp(-1.5, 1.5);
        }
        let slices: Vec<&CutwaterSlice> = g
            .cutwater_slices
            .iter()
            .filter(|s| !s.outline.is_empty())
            .collect();
        let rings: Vec<Vec<DVec3>> = slices
            .iter()
            .map(|s| s.outline.iter().step_by(PREVIEW_STRIDE).copied().collect())
            .collect();
        let points: Vec<DVec3> = if rings.is_empty() {
            g.cutwater_slices
                .iter()
                .flat_map(|s| s.spiral_wall.iter().chain(&s.diffuser_wall))
                .filter(|p| p.distance(g.cutwater.tip) < 0.03)
                .copied()
                .collect()
        } else {
            rings.iter().flatten().copied().collect()
        };
        if points.is_empty() {
            return;
        }
        let centre = points.iter().copied().sum::<DVec3>() / points.len() as f64;
        let radius = points
            .iter()
            .map(|p| p.distance(centre))
            .fold(0.0, f64::max)
            .max(1e-3);
        let size = (rect.width().min(rect.height()) * 0.42) as f64 / radius;
        let (sy, cy) = (self.yaw as f64).sin_cos();
        let (sp, cp) = (self.pitch as f64).sin_cos();
        // Turned about the pump axis, then tilted toward the eye; x points left as in the
        // front view.
        let view = |p: DVec3| -> DVec3 {
            let q = p - centre;
            let (x, y) = (q.x * cy - q.y * sy, q.x * sy + q.y * cy);
            let (y, z) = (y * cp - q.z * sp, y * sp + q.z * cp);
            DVec3::new(-x, y, z)
        };
        let screen = |v: DVec3| {
            pos2(
                rect.center().x + (v.x * size) as f32,
                rect.center().y - (v.y * size) as f32,
            )
        };
        // Faces between neighbouring planes and the two end caps, far ones first.
        let mut triangles: Vec<[DVec3; 3]> = Vec::new();
        for pair in rings.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let n = a.len().min(b.len());
            for i in 0..n {
                let j = (i + 1) % n;
                triangles.push([a[i], a[j], b[j]]);
                triangles.push([a[i], b[j], b[i]]);
            }
        }
        for ring in [rings.first(), rings.last()].into_iter().flatten() {
            let n = ring.len();
            for i in 0..n / 2 {
                let (p, q) = (ring[i], ring[i + 1]);
                let (r, s) = (ring[n - 1 - i], ring[n - 2 - i]);
                triangles.push([p, q, s]);
                triangles.push([p, s, r]);
            }
        }
        let mut shaded: Vec<(f64, [Pos2; 3], f32)> = triangles
            .iter()
            .filter(|t| t.iter().all(|p| p.is_finite()))
            .map(|t| {
                let v = t.map(view);
                let normal = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
                let light = 0.35 + 0.65 * normal.z.abs() as f32;
                ((v[0].z + v[1].z + v[2].z) / 3.0, v.map(screen), light)
            })
            .collect();
        shaded.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut mesh = egui::Mesh::default();
        let base = palette.shroud;
        for (_, corners, light) in shaded {
            let color = Color32::from_rgb(
                (base.r() as f32 * light) as u8,
                (base.g() as f32 * light) as u8,
                (base.b() as f32 * light) as u8,
            );
            let first = mesh.vertices.len() as u32;
            for c in corners {
                mesh.colored_vertex(c, color);
            }
            mesh.add_triangle(first, first + 1, first + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
        // The walls of each plane near the cutwater, and the plane shown.
        let near = |p: &&DVec3| p.distance(centre) < 1.6 * radius;
        for s in &g.cutwater_slices {
            for (wall, color) in [
                (&s.spiral_wall, palette.hub),
                (&s.diffuser_wall, palette.volute),
            ] {
                let points: Vec<Pos2> =
                    wall.iter().filter(near).map(|p| screen(view(*p))).collect();
                if points.len() > 1 {
                    painter.add(egui::Shape::line(
                        points,
                        Stroke::new(1.0, color.gamma_multiply(0.6)),
                    ));
                }
            }
        }
        if let Some(s) = g.cutwater_slices.get(plane)
            && !s.outline.is_empty()
        {
            let mut points: Vec<Pos2> = s.outline.iter().map(|p| screen(view(*p))).collect();
            points.push(points[0]);
            painter.add(egui::Shape::line(points, Stroke::new(2.0, palette.handle)));
        }
        painter.text(
            rect.left_top() + vec2(6.0, 4.0),
            egui::Align2::LEFT_TOP,
            tr("Across the width: drag to turn"),
            egui::FontId::proportional(12.0),
            ui.visuals().weak_text_color(),
        );
    }
}

/// Fills a body outline whose first half runs back along one face and whose second
/// half runs forward along the other, as a strip between the two.
fn fill_strip(ui: &Ui, t: &PlotTransform, outline: &[DVec2], scale: f64, color: Color32) {
    let n = outline.len();
    if n < 4 {
        return;
    }
    let at = |p: DVec2| t.position_from_point(&to_plot(p, scale));
    let mut mesh = egui::Mesh::default();
    for i in 0..n / 2 {
        mesh.colored_vertex(at(outline[i]), color);
        mesh.colored_vertex(at(outline[n - 1 - i]), color);
        if i > 0 {
            let k = 2 * i as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    ui.painter_at(*t.frame()).add(egui::Shape::mesh(mesh));
}

/// The measures of the cutwater against the guidelines of Gülich.
fn measures(ui: &mut Ui, g: &VoluteGeometry, band: Option<(f64, f64)>, units: &DisplayUnits) {
    let d = &g.design;
    let slices = &g.cutwater_slices;
    let show = |kind: QuantityKind, v: f64| units.show(kind, v);
    let percent = |v: f64| format!("{:.1} %", 100.0 * v);
    let d2 = g.basis.impeller_diameter;
    let (near, _) = range(slices, |s| s.nose_distance);
    let mut rows: Vec<(Check, String, String, String)> = Vec::new();
    // The effective cutwater circle, against the one designed.
    let effective = 2.0 * near;
    rows.push((
        if effective >= d.cutwater_diameter * (1.0 - 1e-4) {
            Check::Good
        } else {
            Check::Bad
        },
        tr("Effective cutwater diameter").to_owned(),
        show(QuantityKind::Length, effective),
        tf(
            "designed d_z {}",
            &[&show(QuantityKind::Length, d.cutwater_diameter)],
        ),
    ));
    // Gap B to the impeller, against Table 10.2.
    let gap = d.cutwater_diameter / d2 - 1.0;
    let least = d.min_cutwater_diameter / d2 - 1.0;
    rows.push((
        if d.cutwater_diameter >= d.min_cutwater_diameter * (1.0 - 1e-9) {
            Check::Good
        } else {
            Check::Watch
        },
        tr("Gap d_z/d₂ − 1").to_owned(),
        percent(gap),
        tf(
            "at least {} against pulsations (Gülich Table 10.2); past 10 % it costs about 1 % of efficiency (p. 421)",
            &[&percent(least)],
        ),
    ));
    if g.cutwater.radius > 0.0 {
        let e = 2.0 * g.cutwater.radius;
        let (thin, _) = range(slices, |s| s.min_thickness);
        rows.push((
            if thin >= e * (1.0 - 2e-3) {
                Check::Good
            } else {
                Check::Bad
            },
            tr("Thinnest body").to_owned(),
            show(QuantityKind::Length, thin),
            tf("the nose {}: no neck", &[&show(QuantityKind::Length, e)]),
        ));
        let ratio = e / d.cutwater_diameter;
        rows.push((
            if ratio <= 0.04 {
                Check::Good
            } else {
                Check::Info
            },
            tr("Thickness e/d_z").to_owned(),
            format!("{ratio:.3}"),
            tr("up to 0.04 hardly affects the best efficiency (Gülich p. 421)").to_owned(),
        ));
    }
    let incidence = d.cutwater_angle - d.cutwater_flow_angle;
    rows.push((
        if incidence.abs() <= 3f64.to_radians() + 1e-12 {
            Check::Good
        } else {
            Check::Watch
        },
        tr("Incidence i₃ at design").to_owned(),
        show(QuantityKind::Angle, incidence),
        tf(
            "α₃B {} against α₃′ {}; ±3° (eq. T7.7.6)",
            &[
                &show(QuantityKind::Angle, d.cutwater_angle),
                &show(QuantityKind::Angle, d.cutwater_flow_angle),
            ],
        ),
    ));
    if let Some((lo, hi)) = band {
        rows.push((
            if lo < 0.8 && hi > 1.2 {
                Check::Good
            } else {
                Check::Watch
            },
            tr("Within ±3°").to_owned(),
            format!("q* {lo:.2}–{hi:.2}"),
            tr("the flow range over which the flow meets the cutwater within ±3°; it comes flatter at part load (Gülich Fig. 9.20)").to_owned(),
        ));
    }
    if g.cutwater.radius > 0.0 {
        let lean = d.cutwater_obliquity;
        rows.push((
            if lean.abs() >= 35f64.to_radians() - 1e-9 {
                Check::Good
            } else {
                Check::Info
            },
            tr("Obliquity ε").to_owned(),
            show(QuantityKind::Angle, lean),
            tr("more than 35° against pressure pulsations (Gülich Table 10.2); the walls set how far it can lean").to_owned(),
        ));
    }
    rows.push((
        if g.start_range.is_some() {
            Check::Info
        } else {
            Check::Watch
        },
        tr("Spiral start").to_owned(),
        show(QuantityKind::Angle, g.start_angle),
        match g.start_range {
            Some((lo, hi)) => tf(
                "before the walls cross in every plane: from {} to {}",
                &[
                    &show(QuantityKind::Angle, lo),
                    &show(QuantityKind::Angle, hi),
                ],
            ),
            None => tr("the walls do not cross in every plane: the spiral starts where it clears the diffuser").to_owned(),
        },
    ));
    let (wedge_lo, wedge_hi) = range(slices, |s| s.wedge);
    let degrees = |a: f64| units.to_display(QuantityKind::Angle, a);
    rows.push((
        Check::Info,
        tr("Wedge across the width").to_owned(),
        format!(
            "{:.1}–{:.1} {}",
            degrees(wedge_lo),
            degrees(wedge_hi),
            units.get(QuantityKind::Angle).symbol
        ),
        tr("angle between the spiral and the diffuser wall where they cross").to_owned(),
    ));
    if d.development.law.velocity_based() {
        rows.push((
            Check::Info,
            tr("Cutwater correction Δa₃").to_owned(),
            show(QuantityKind::Length, d.throat_correction),
            tr("the throat enlarged for the blockage of the cutwater (eq. 7.28)").to_owned(),
        ));
    }
    egui::Grid::new("cutwater_measures")
        .num_columns(3)
        .spacing([8.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            for (check, name, value, note) in rows {
                ui.colored_label(check.color(ui), check.glyph());
                ui.label(name).on_hover_text(&note);
                ui.strong(value).on_hover_text(&note);
                ui.end_row();
            }
        });
    ui.add_space(6.0);
}

/// The incidence on the cutwater over the range of flow: α₃B − α₃′(q*), with α₃′ from
/// the predicted theoretical head (eqs. T7.7.3 to T7.7.6).
fn incidence_plot(
    ui: &mut Ui,
    cx: &EditCtx<'_>,
    g: &VoluteGeometry,
    impeller: ComponentId,
    height: f32,
) {
    let palette = Palette::of(ui);
    let Some(performance) = cx
        .derived
        .impeller(impeller)
        .and_then(|r| r.performance.as_ref().ok())
    else {
        ui.weak(tr(
            "The incidence over the flow range needs the predicted characteristic.",
        ));
        return;
    };
    let design = &performance.design;
    let d = &g.design;
    let points: PlotPoints = performance
        .points
        .iter()
        .filter(|p| p.flow > 0.0 && design.theoretical_head > 0.0)
        .map(|p| {
            let q = p.flow / design.flow;
            let incidence =
                d.cutwater_incidence_at(q, p.theoretical_head / design.theoretical_head);
            [q, incidence.to_degrees()]
        })
        .filter(|[q, i]| q.is_finite() && i.is_finite())
        .collect();
    ui.horizontal_wrapped(|ui| {
        marker(
            ui,
            Mark::Line,
            palette.shroud,
            tr("incidence i₃ on the cutwater"),
        );
        marker(ui, Mark::Dashed, palette.guide, tr("±3°"));
    });
    Plot::new(("cutwater_incidence", g.basis.midplane.to_bits()))
        .height(height - 24.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .x_axis_label(tr("q* = Q/Q_design"))
        .y_axis_label(tr("i₃ [°]"))
        .include_y(-5.0)
        .include_y(5.0)
        .show(ui, |plot| {
            plot.hline(
                HLine::new(String::new(), 3.0)
                    .color(palette.guide)
                    .style(LineStyle::dashed_dense()),
            );
            plot.hline(
                HLine::new(String::new(), -3.0)
                    .color(palette.guide)
                    .style(LineStyle::dashed_dense()),
            );
            plot.vline(
                VLine::new(String::new(), 1.0)
                    .color(palette.guide)
                    .style(LineStyle::dotted_dense()),
            );
            plot.line(
                Line::new(tr("incidence i₃ on the cutwater"), points)
                    .color(palette.shroud)
                    .width(2.0),
            );
        });
}

#[cfg(test)]
impl CutwaterPage {
    /// The point to drag `grip` as last drawn, in screen points.
    pub(crate) fn grip_position(&self, g: &VoluteGeometry, grip: Grip, scale: f64) -> Option<Pos2> {
        self.screen(Nose::of(g).grip(grip), scale)
    }

    /// Where a point of the volute (x, y) lies on the plot as last drawn.
    pub(crate) fn screen(&self, p: DVec2, scale: f64) -> Option<Pos2> {
        Some(self.transform?.position_from_point(&to_plot(p, scale)))
    }
}

/// The nose of `g`: its centre, outward, downstream and along the camber line, and its
/// radius.
#[cfg(test)]
pub(crate) fn nose_frame(g: &VoluteGeometry) -> (DVec2, DVec2, DVec2, DVec2, f64) {
    let n = Nose::of(g);
    (n.centre, n.radial, n.along, n.camber, n.radius)
}
