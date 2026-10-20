//! 3D view of the stage, rendered with wgpu from the design's own mesh, so it follows
//! every edit at once. On top of the image: a small toolbar, a view cube that turns
//! the camera to a face, the axes and a scale bar.

mod camera;
mod mesh;
mod render;

use std::f32::consts::FRAC_PI_2;

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, UiBuilder, Vec2,
    pos2, vec2,
};
use eframe::egui_wgpu::RenderState;
use glam::Vec3;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use crate::derived::Derived;
use crate::theme::{Palette, icon};
use crate::views::unavailable;
use camera::Camera;
pub(crate) use mesh::{Part, ViewOptions};
use render::{Gpu, Uniforms};

/// What the uploaded mesh was built from: results revision, impeller, stator, volute
/// and display options.
type MeshKey = (
    u64,
    ComponentId,
    Option<ComponentId>,
    Option<ComponentId>,
    ViewOptions,
);

/// Standard camera positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CameraView {
    Fit,
    /// Oblique, from the inlet side.
    Iso,
    /// Along the axis, from the inlet.
    Inlet,
    /// Across the axis.
    Side,
}

/// State of the 3D view between frames.
#[derive(Debug, Default)]
pub(crate) struct Viewer {
    pub options: ViewOptions,
    camera: Option<Camera>,
    gpu: Option<Gpu>,
    /// What the uploaded mesh was built from.
    mesh_key: Option<MeshKey>,
    /// Center and radius of the model, for "fit".
    bounds: Option<(Vec3, f32)>,
    /// Impeller, stator and volute the camera was fitted to.
    fitted: Option<(ComponentId, bool, bool)>,
}

/// Faces of the view cube: outward normal and label.
const FACES: [(Vec3, &str); 6] = [
    (Vec3::X, "X"),
    (Vec3::NEG_X, "−X"),
    (Vec3::Y, "Y"),
    (Vec3::NEG_Y, "−Y"),
    (Vec3::NEG_Z, "Inlet"),
    (Vec3::Z, "Back"),
];

impl Viewer {
    /// Moves the camera to a standard view.
    pub(crate) fn apply(&mut self, view: CameraView) {
        let Some(camera) = self.camera.as_mut() else {
            return;
        };
        match view {
            CameraView::Fit => {
                if let Some((center, radius)) = self.bounds {
                    *camera = Camera::fit(center, radius);
                }
            }
            CameraView::Iso => camera.look_from(-0.9, 0.55),
            CameraView::Inlet => camera.look_from(-FRAC_PI_2, FRAC_PI_2),
            CameraView::Side => camera.look_from(-FRAC_PI_2, 0.0),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        render_state: Option<&RenderState>,
        derived: &Derived,
        id: ComponentId,
        stator: Option<ComponentId>,
        volute: Option<ComponentId>,
        units: &DisplayUnits,
    ) {
        let stator = stator.and_then(|s| derived.diffuser(s).map(|d| (s, d)));
        let volute = volute.and_then(|v| derived.volute(v).map(|g| (v, g)));
        let Some(state) = render_state else {
            unavailable(ui, "The 3D view needs the wgpu renderer.");
            return;
        };
        let Some(g) = derived.geometry(id) else {
            let reason = derived
                .impeller(id)
                .and_then(|r| r.geometry.as_ref().err())
                .map_or("No geometry yet.", String::as_str);
            unavailable(ui, reason);
            return;
        };

        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(&state.device));
        let key = (
            derived.revision(),
            id,
            stator.map(|s| s.0),
            volute.map(|v| v.0),
            self.options,
        );
        if self.mesh_key != Some(key) {
            let mesh = mesh::build(g, stator.map(|s| s.1), volute.map(|v| v.1), &self.options);
            gpu.upload(&state.device, &mesh);
            self.bounds = Some((mesh.center, mesh.radius));
            let content = (id, stator.is_some(), volute.is_some());
            if self.fitted != Some(content) || self.camera.is_none() {
                self.camera = Some(Camera::fit(mesh.center, mesh.radius));
                self.fitted = Some(content);
            }
            self.mesh_key = Some(key);
        }
        let Some(camera) = self.camera.as_mut() else {
            return;
        };

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        if response.dragged_by(egui::PointerButton::Primary) && !ui.input(|i| i.modifiers.shift) {
            let d = response.drag_delta();
            camera.orbit(d.x, d.y);
        } else if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Primary)
        {
            let d = response.drag_delta();
            camera.pan(d.x, d.y, rect.width(), rect.height());
        }
        if response.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta().y, i.zoom_delta()));
            if scroll != 0.0 {
                camera.zoom((-scroll * 0.002).exp());
            }
            if (pinch - 1.0).abs() > f32::EPSILON {
                camera.zoom(1.0 / pinch);
            }
        }
        if response.double_clicked()
            && let Some((center, radius)) = self.bounds
        {
            *camera = Camera::fit(center, radius);
        }

        let ppp = ui.ctx().pixels_per_point();
        let size = [
            ((rect.width() * ppp).round() as u32).max(1),
            ((rect.height() * ppp).round() as u32).max(1),
        ];
        let uniforms = uniforms(camera, rect, ui.visuals().dark_mode);
        let texture = gpu.render(state, size, &uniforms);
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        let camera = *camera;
        axes(ui, rect, &camera);
        scale_bar(ui, rect, &camera, units);
        if let Some(direction) = view_cube(ui, rect, &camera)
            && let Some(c) = self.camera.as_mut()
        {
            c.look_along(direction);
        }
        if let Some(view) = self.toolbar(ui, rect) {
            self.apply(view);
        }
    }

    /// The floating toolbar in the top-left corner.
    fn toolbar(&mut self, ui: &mut Ui, rect: Rect) -> Option<CameraView> {
        let p = Palette::of(ui);
        let mut view = None;
        let area = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(rect.width() - 16.0, 34.0));
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            egui::Frame::new()
                .fill(p.ribbon.gamma_multiply(0.92))
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let tool = |ui: &mut Ui, glyph: &str, tip: &str, on: bool| {
                            let text = RichText::new(glyph).size(17.0);
                            ui.add(egui::Button::selectable(on, text).min_size(vec2(28.0, 26.0)))
                                .on_hover_text(tip)
                                .clicked()
                        };
                        for (glyph, tip, v) in [
                            (
                                icon::CORNERS_OUT,
                                "Fit the model (double-click)",
                                CameraView::Fit,
                            ),
                            (
                                icon::CUBE,
                                "Oblique view from the inlet side",
                                CameraView::Iso,
                            ),
                            (
                                icon::ARROW_LINE_DOWN,
                                "Along the axis, from the inlet",
                                CameraView::Inlet,
                            ),
                            (icon::COLUMNS, "Across the axis", CameraView::Side),
                        ] {
                            if tool(ui, glyph, tip, false) {
                                view = Some(v);
                            }
                        }
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                        let o = &mut self.options;
                        if tool(ui, icon::POLYGON, "Feature edges", o.edges) {
                            o.edges = !o.edges;
                        }
                        if tool(ui, icon::CUBE_TRANSPARENT, "Wireframe", o.wireframe) {
                            o.wireframe = !o.wireframe;
                        }
                        if tool(ui, icon::FAN, "All blades, or only one", o.all_blades) {
                            o.all_blades = !o.all_blades;
                        }
                        ui.add_space(4.0);
                        ui.separator();
                        ui.label(RichText::new(icon::QUESTION).size(16.0).color(p.weak))
                            .on_hover_text(
                                "Drag: rotate\nRight-drag or Shift-drag: pan\nScroll or pinch: \
                                 zoom\nDouble-click: fit\nView cube: click a face",
                            );
                    });
                });
        });
        view
    }
}

/// Lights fixed to the camera, and the background of the theme.
fn uniforms(camera: &Camera, rect: Rect, dark: bool) -> Uniforms {
    let view = camera.view();
    let right = view.row(0).truncate();
    let up = view.row(1).truncate();
    let back = view.row(2).truncate();
    let key = (-0.45 * right + 0.6 * up + 0.66 * back).normalize();
    let fill = (0.6 * right - 0.3 * up + 0.5 * back).normalize();
    let (sky, ground) = if dark {
        ([0.20, 0.22, 0.25, 1.0], [0.09, 0.10, 0.11, 1.0])
    } else {
        ([1.0, 1.0, 1.0, 1.0], [0.84, 0.87, 0.91, 1.0])
    };
    Uniforms {
        view_projection: camera
            .view_projection(rect.width() / rect.height().max(1.0))
            .to_cols_array_2d(),
        eye: camera.eye().extend(1.0).to_array(),
        key_light: key.extend(0.0).to_array(),
        fill_light: fill.extend(0.0).to_array(),
        up: up.extend(0.0).to_array(),
        sky,
        ground,
    }
}

/// Direction `d` of the model as an offset on screen, `scale` points per unit, and
/// its depth (positive toward the viewer).
fn on_screen(camera: &Camera, d: Vec3, scale: f32) -> (Vec2, f32) {
    let v = camera.view().transform_vector3(d);
    (vec2(v.x, -v.y) * scale, v.z)
}

/// The axes in the bottom-left corner.
fn axes(ui: &Ui, rect: Rect, camera: &Camera) {
    let p = Palette::of(ui);
    let origin = rect.left_bottom() + vec2(36.0, -34.0);
    let mut axes = [
        (Vec3::X, "X", Color32::from_rgb(214, 60, 52)),
        (Vec3::Y, "Y", Color32::from_rgb(46, 160, 67)),
        (Vec3::Z, "Z", Color32::from_rgb(52, 110, 220)),
    ]
    .map(|(d, label, color)| {
        let (offset, depth) = on_screen(camera, d, 24.0);
        (offset, depth, label, color)
    });
    axes.sort_by(|a, b| a.1.total_cmp(&b.1));
    let painter = ui.painter();
    painter.circle_filled(origin, 2.5, p.weak);
    for (offset, _, label, color) in axes {
        let tip = origin + offset;
        painter.line_segment([origin, tip], Stroke::new(2.0, color));
        painter.text(
            origin + offset * 1.32,
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.0),
            color,
        );
    }
}

/// Length of about `target` that reads well: 1, 2 or 5 times a power of ten.
fn round_length(target: f64) -> f64 {
    let power = 10f64.powf(target.log10().floor());
    [5.0, 2.0, 1.0]
        .into_iter()
        .map(|m| m * power)
        .find(|&l| l <= target)
        .unwrap_or(power)
}

/// A number without trailing zeros.
fn compact(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The scale bar in the bottom-right corner.
fn scale_bar(ui: &Ui, rect: Rect, camera: &Camera, units: &DisplayUnits) {
    let p = Palette::of(ui);
    let per_point = f64::from(camera.units_per_point(rect.width(), rect.height()));
    let unit = units.get(QuantityKind::Length);
    let target = units.to_display(QuantityKind::Length, per_point * 130.0);
    if !(target.is_finite() && target > 0.0) {
        return;
    }
    let length = round_length(target);
    let points = (units.from_display(QuantityKind::Length, length) / per_point) as f32;
    let right = rect.right() - 18.0;
    let top = rect.bottom() - 30.0;
    let left = right - points;
    let painter = ui.painter();
    let ink = p.text;
    for k in 0..4 {
        let x0 = left + points * k as f32 / 4.0;
        let segment = Rect::from_min_max(pos2(x0, top), pos2(x0 + points / 4.0, top + 6.0));
        let fill = if k % 2 == 0 { ink } else { p.ribbon };
        painter.rect_filled(segment, 0, fill);
    }
    painter.rect_stroke(
        Rect::from_min_max(pos2(left, top), pos2(right, top + 6.0)),
        0,
        Stroke::new(1.0, ink),
        egui::StrokeKind::Middle,
    );
    let font = FontId::proportional(10.5);
    let label = |x: f32, value: f64, suffix: &str| {
        let text = format!("{}{suffix}", compact(value));
        painter.text(
            pos2(x, top + 9.0),
            Align2::CENTER_TOP,
            text,
            font.clone(),
            ink,
        );
    };
    label(left, 0.0, "");
    label(left + points / 2.0, length / 2.0, "");
    label(right, length, &format!(" {}", unit.symbol));
}

/// The view cube in the top-right corner. Returns the direction to look from when a
/// face was clicked.
fn view_cube(ui: &mut Ui, rect: Rect, camera: &Camera) -> Option<Vec3> {
    let p = Palette::of(ui);
    let half = 21.0;
    let center = rect.right_top() + vec2(-48.0, 72.0);
    let area = Rect::from_center_size(center, Vec2::splat(2.0 * half * 1.8));
    let response = ui.interact(area, ui.id().with("view_cube"), Sense::click());
    let pointer = response.hover_pos();
    let corner =
        |sx: f32, sy: f32, sz: f32| center + on_screen(camera, Vec3::new(sx, sy, sz), half).0;
    let mut clicked = None;
    let painter = ui.painter();
    for (normal, label) in FACES {
        let (_, depth) = on_screen(camera, normal, 1.0);
        if depth <= 0.02 {
            continue;
        }
        // The face's corners, in order round it.
        let (a, b) = if normal.x != 0.0 {
            (Vec3::Y, Vec3::Z)
        } else if normal.y != 0.0 {
            (Vec3::Z, Vec3::X)
        } else {
            (Vec3::X, Vec3::Y)
        };
        let quad: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|&(s, t)| {
                let q = normal + a * s + b * t;
                corner(q.x, q.y, q.z)
            })
            .collect();
        let hovered = pointer.is_some_and(|pt| inside(&quad, pt));
        let base = if hovered { p.selected } else { p.strip };
        let shade = 0.82 + 0.18 * depth;
        let fill = Color32::from_rgb(
            (f32::from(base.r()) * shade) as u8,
            (f32::from(base.g()) * shade) as u8,
            (f32::from(base.b()) * shade) as u8,
        );
        painter.add(Shape::convex_polygon(
            quad.clone(),
            fill,
            Stroke::new(1.0, p.weak.gamma_multiply(0.8)),
        ));
        if depth > 0.3 {
            let mid = quad.iter().fold(Vec2::ZERO, |s, q| s + q.to_vec2()) / 4.0;
            painter.text(
                mid.to_pos2(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(8.0 + 3.0 * depth),
                if hovered { p.accent } else { p.text },
            );
        }
        if hovered && response.clicked() {
            clicked = Some(normal);
        }
    }
    if pointer.is_some() {
        response.on_hover_text("Click a face to look at it");
    }
    clicked
}

/// Whether `point` lies inside the convex polygon `quad`.
fn inside(quad: &[Pos2], point: Pos2) -> bool {
    let n = quad.len();
    let crosses = (0..n).map(|i| {
        let (a, b) = (quad[i], quad[(i + 1) % n]);
        (b - a).x * (point - a).y - (b - a).y * (point - a).x
    });
    let (mut left, mut right) = (false, false);
    for cross in crosses {
        left |= cross > f32::EPSILON;
        right |= cross < -f32::EPSILON;
    }
    !(left && right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_lengths_are_round() {
        assert_eq!(round_length(130.0), 100.0);
        assert_eq!(round_length(260.0), 200.0);
        assert_eq!(round_length(0.9), 0.5);
        assert_eq!(round_length(5.0), 5.0);
    }

    #[test]
    fn scale_labels_drop_trailing_zeros() {
        assert_eq!(compact(100.0), "100");
        assert_eq!(compact(2.5), "2.5");
        assert_eq!(compact(0.0), "0");
    }

    #[test]
    fn points_inside_a_square() {
        let quad = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 10.0),
            pos2(0.0, 10.0),
        ];
        assert!(inside(&quad, pos2(5.0, 5.0)));
        assert!(!inside(&quad, pos2(15.0, 5.0)));
    }
}
