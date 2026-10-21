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
use crate::i18n::tr;
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

/// A camera turn in progress, from one yaw and pitch to another.
#[derive(Debug, Clone, Copy)]
struct Turn {
    from: (f32, f32),
    to: (f32, f32),
    /// Seconds since the turn began.
    elapsed: f32,
}

/// How long a camera turn takes, s.
const TURN_TIME: f32 = 0.35;

/// State of the 3D view between frames.
#[derive(Debug, Default)]
pub(crate) struct Viewer {
    pub options: ViewOptions,
    camera: Option<Camera>,
    /// The camera turning toward a standard view.
    turn: Option<Turn>,
    gpu: Option<Gpu>,
    /// What the uploaded mesh was built from.
    mesh_key: Option<MeshKey>,
    /// Center and radius of the model, for "fit".
    bounds: Option<(Vec3, f32)>,
    /// Impeller, stator and volute the camera was fitted to.
    fitted: Option<(ComponentId, bool, bool)>,
    /// Width over height of the part of the view nothing covers, last frame.
    free_aspect: f32,
    /// The free aspect the camera was framed for.
    framed_aspect: f32,
    /// The user zoomed or panned since the camera was last framed.
    moved: bool,
}

/// Labels of the faces of the view cube, by outward normal.
const FACES: [(Vec3, &str); 6] = [
    (Vec3::X, "+X"),
    (Vec3::NEG_X, "−X"),
    (Vec3::Y, "+Y"),
    (Vec3::NEG_Y, "−Y"),
    (Vec3::NEG_Z, "Inlet"),
    (Vec3::Z, "Rear"),
];

/// Half the edge of the view cube, in points.
const CUBE_HALF: f32 = 28.0;
/// Width of the bevels on the cube's edges and corners, as a fraction of its half edge.
const CUBE_BEVEL: f32 = 0.26;

/// What a click on the view cube asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CubeAction {
    /// Look from this direction: a face, an edge or a corner.
    Look(Vec3),
    /// Turn about the model axis by this angle, rad.
    Turn(f32),
    /// Back to the oblique view.
    Home,
}

impl Viewer {
    /// Moves the camera to a standard view.
    pub(crate) fn apply(&mut self, view: CameraView) {
        let Some(camera) = self.camera.as_mut() else {
            return;
        };
        match view {
            CameraView::Fit => {
                if let Some((center, radius)) = self.bounds {
                    *camera = Camera::fit(center, radius, self.free_aspect);
                    self.framed_aspect = self.free_aspect;
                    self.moved = false;
                }
            }
            CameraView::Iso => self.turn_to((-0.9, 0.55)),
            CameraView::Inlet => self.turn_to((-FRAC_PI_2, FRAC_PI_2)),
            CameraView::Side => self.turn_to((-FRAC_PI_2, 0.0)),
        }
    }

    /// Starts turning the camera to `yaw` and `pitch`, the short way round.
    fn turn_to(&mut self, (yaw, pitch): (f32, f32)) {
        let Some(camera) = self.camera.as_ref() else {
            return;
        };
        let from = (camera.yaw, camera.pitch);
        let yaw = from.0 + wrap_angle(yaw - from.0);
        self.turn = Some(Turn {
            from,
            to: (yaw, pitch),
            elapsed: 0.0,
        });
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
        cover: f32,
    ) {
        let stator = stator.and_then(|s| derived.diffuser(s).map(|d| (s, d)));
        let volute = volute.and_then(|v| derived.volute(v).map(|g| (v, g)));
        let Some(state) = render_state else {
            unavailable(ui, tr("The 3D view needs the wgpu renderer."));
            return;
        };
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

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        // A dialog over the right of the view: center the model in the free part.
        let free = Rect::from_min_max(rect.min, pos2(rect.right() - cover, rect.bottom()));
        self.free_aspect = free.width() / free.height().max(1.0);
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
                self.camera = Some(Camera::fit(mesh.center, mesh.radius, self.free_aspect));
                self.fitted = Some(content);
                self.framed_aspect = self.free_aspect;
                self.moved = false;
            }
            self.mesh_key = Some(key);
        }
        let Some(camera) = self.camera.as_mut() else {
            return;
        };
        // Until the user zooms or pans, the framing follows the free part of the view,
        // which changes as the dialog opens, closes or moves.
        if !self.moved
            && (self.free_aspect - self.framed_aspect).abs() > 0.02
            && let Some((center, radius)) = self.bounds
        {
            camera.reframe(center, radius, self.free_aspect);
            self.framed_aspect = self.free_aspect;
        }

        // Dragging takes the camera over from a turn in progress.
        if response.dragged() {
            self.turn = None;
        }
        let dt = ui.input(|i| i.stable_dt).min(0.25);
        if step_turn(&mut self.turn, camera, dt) {
            ui.ctx().request_repaint();
        }
        if response.dragged_by(egui::PointerButton::Primary) && !ui.input(|i| i.modifiers.shift) {
            let d = response.drag_delta();
            camera.orbit(d.x, d.y);
        } else if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Primary)
        {
            let d = response.drag_delta();
            camera.pan(d.x, d.y, rect.width(), rect.height());
            self.moved = true;
        }
        if response.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta().y, i.zoom_delta()));
            if scroll != 0.0 {
                camera.zoom((-scroll * 0.002).exp());
                self.moved = true;
            }
            if (pinch - 1.0).abs() > f32::EPSILON {
                camera.zoom(1.0 / pinch);
                self.moved = true;
            }
        }
        if response.double_clicked()
            && let Some((center, radius)) = self.bounds
        {
            *camera = Camera::fit(center, radius, self.free_aspect);
            self.framed_aspect = self.free_aspect;
            self.moved = false;
        }

        let ppp = ui.ctx().pixels_per_point();
        let size = [
            ((rect.width() * ppp).round() as u32).max(1),
            ((rect.height() * ppp).round() as u32).max(1),
        ];
        let uniforms = uniforms(camera, rect, free, ui.visuals().dark_mode);
        let texture = gpu.render(state, size, &uniforms);
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        let camera = *camera;
        axes(ui, free, &camera);
        scale_bar(ui, free, &camera, units);
        match view_cube(ui, free, &camera) {
            Some(CubeAction::Look(direction)) => {
                if let Some(target) = camera.orientation_along(direction) {
                    self.turn_to(target);
                }
            }
            Some(CubeAction::Turn(angle)) => self.turn_to((camera.yaw + angle, camera.pitch)),
            Some(CubeAction::Home) => self.apply(CameraView::Iso),
            None => {}
        }
        if let Some(view) = self.toolbar(ui, free) {
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
                        let tool = |ui: &mut Ui, glyph: &str, tip: &'static str, on: bool| {
                            let text = RichText::new(glyph).size(17.0);
                            ui.add(egui::Button::selectable(on, text).min_size(vec2(28.0, 26.0)))
                                .on_hover_text(tr(tip))
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
                            .on_hover_text(tr(
                                "Drag: rotate\nRight-drag or Shift-drag: pan\nScroll or pinch: \
                                 zoom\nDouble-click: fit\nView cube: click a face, edge or corner",
                            ));
                    });
                });
        });
        view
    }
}

/// Lights fixed to the camera, and the background of the theme. The model is centered
/// in `free`, the part of `rect` nothing covers.
fn uniforms(camera: &Camera, rect: Rect, free: Rect, dark: bool) -> Uniforms {
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
    // Normalized device x runs from −1 to 1 across the view.
    let shift = (free.center().x - rect.center().x) / (rect.width() / 2.0).max(1.0);
    let projection = glam::Mat4::from_translation(Vec3::new(shift, 0.0, 0.0))
        * camera.view_projection(rect.width() / rect.height().max(1.0));
    Uniforms {
        view_projection: projection.to_cols_array_2d(),
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

/// The view cube in the top-right corner: a bevelled cube whose faces, edges and
/// corners each pick a view, with a home button and two buttons that turn the model.
fn view_cube(ui: &mut Ui, rect: Rect, camera: &Camera) -> Option<CubeAction> {
    let p = Palette::of(ui);
    let dark = ui.visuals().dark_mode;
    let center = rect.right_top() + vec2(-62.0, 82.0);
    let area = Rect::from_center_size(center, Vec2::splat(CUBE_HALF * 3.4));
    let response = ui.interact(area, ui.id().with("view_cube"), Sense::click());
    let pointer = response.hover_pos();
    let to_screen = |q: Vec3| center + on_screen(camera, q, CUBE_HALF).0;
    let to_view = |d: Vec3| camera.view().transform_vector3(d.normalize());

    // The pieces that face the viewer: on a convex body they never overlap.
    let pieces: Vec<(Vec3, Vec<Pos2>, Vec3)> = cube_directions()
        .filter_map(|d| {
            let n = to_view(d);
            (n.z > 0.01).then(|| (d, cube_piece(d).into_iter().map(to_screen).collect(), n))
        })
        .collect();
    let hovered = pointer.and_then(|pt| {
        pieces
            .iter()
            .find(|(_, polygon, _)| inside(polygon, pt))
            .map(|(d, _, _)| *d)
    });

    let (lit, unlit, line, ink) = if dark {
        (
            Color32::from_rgb(122, 130, 142),
            Color32::from_rgb(46, 51, 59),
            Color32::from_rgb(20, 22, 26),
            Color32::from_rgb(238, 241, 245),
        )
    } else {
        (
            Color32::from_rgb(253, 253, 254),
            Color32::from_rgb(178, 186, 197),
            Color32::from_rgb(112, 121, 134),
            Color32::from_rgb(48, 55, 66),
        )
    };
    let painter = ui.painter();
    // A soft shadow under the cube.
    for (grow, alpha) in [(1.0, 10u8), (0.8, 12), (0.6, 14)] {
        painter.add(egui::epaint::EllipseShape::filled(
            center + vec2(0.0, CUBE_HALF * 1.45),
            vec2(CUBE_HALF * 1.25 * grow, CUBE_HALF * 0.3 * grow),
            Color32::from_black_alpha(if dark { alpha * 2 } else { alpha }),
        ));
    }
    let light = Vec3::new(-0.35, 0.8, 0.5).normalize();
    for (d, polygon, n) in &pieces {
        let shade = n.dot(light).clamp(0.0, 1.0).powf(1.2);
        let mut fill = lerp_color(unlit, lit, 0.15 + 0.85 * shade);
        if hovered == Some(*d) {
            fill = lerp_color(fill, p.accent, if dark { 0.6 } else { 0.45 });
        }
        painter.add(Shape::convex_polygon(
            polygon.clone(),
            fill,
            Stroke::new(0.6, line.gamma_multiply(0.35)),
        ));
        // The faces carry their names.
        if let Some((normal, label)) = FACES.iter().find(|(normal, _)| *normal == *d)
            && n.z > 0.3
        {
            let (a, b) = face_axes(*normal);
            let c = 1.0 - CUBE_BEVEL;
            let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(s, t)| to_screen(*normal + (a * s + b * t) * c));
            let alpha = ((n.z - 0.3) / 0.3).clamp(0.0, 1.0);
            let color = if hovered == Some(*d) {
                Color32::WHITE
            } else {
                ink
            };
            face_label(ui, tr(label), &corners, color.gamma_multiply(alpha));
        }
    }
    // A crisp outline round the whole cube.
    let outline: Vec<Pos2> = pieces
        .iter()
        .flat_map(|(_, polygon, _)| polygon.iter().copied())
        .collect();
    painter.add(Shape::closed_line(
        convex_hull(outline),
        Stroke::new(1.0, line),
    ));

    let mut action = None;
    if let Some(d) = hovered {
        let hint = match d.abs().cmpgt(Vec3::splat(0.5)).bitmask().count_ones() {
            1 => tr("Click to look at this face"),
            2 => tr("Click to look along this edge"),
            _ => tr("Click for the view from this corner"),
        };
        if response.clicked() {
            action = Some(CubeAction::Look(d));
        }
        response.on_hover_text(hint);
    }

    // Home and the turns show while the pointer is near the cube.
    let near = ui.rect_contains_pointer(area.expand(16.0));
    let buttons = [
        (
            icon::HOUSE,
            vec2(-1.75, -1.75),
            tr("Home view"),
            CubeAction::Home,
            true,
        ),
        (
            icon::ARROW_COUNTER_CLOCKWISE,
            vec2(-1.75, 1.6),
            tr("Turn 90° about the axis"),
            CubeAction::Turn(FRAC_PI_2),
            near,
        ),
        (
            icon::ARROW_CLOCKWISE,
            vec2(1.75, 1.6),
            tr("Turn 90° the other way"),
            CubeAction::Turn(-FRAC_PI_2),
            near,
        ),
    ];
    for (glyph, offset, tip, button_action, shown) in buttons {
        if !shown {
            continue;
        }
        let at = Rect::from_center_size(center + offset * CUBE_HALF, Vec2::splat(22.0));
        let response = ui
            .interact(at, ui.id().with(("cube_button", glyph)), Sense::click())
            .on_hover_text(tip);
        let painter = ui.painter();
        if response.hovered() {
            painter.circle_filled(at.center(), 11.0, p.hover);
        }
        painter.text(
            at.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(14.0),
            if response.hovered() { p.accent } else { p.weak },
        );
        if response.clicked() {
            action = Some(button_action);
        }
    }
    action
}

/// The 26 directions a piece of the bevelled cube looks along: six faces, twelve
/// edges and eight corners.
fn cube_directions() -> impl Iterator<Item = Vec3> {
    (0..27)
        .map(|i| {
            Vec3::new(
                (i % 3) as f32 - 1.0,
                (i / 3 % 3) as f32 - 1.0,
                (i / 9) as f32 - 1.0,
            )
        })
        .filter(|d| *d != Vec3::ZERO)
}

/// The polygon of the bevelled unit cube that faces `d`, its corners in order round
/// it: a square for a face, a strip for an edge, a triangle for a corner.
fn cube_piece(d: Vec3) -> Vec<Vec3> {
    let c = 1.0 - CUBE_BEVEL;
    let axes: Vec<usize> = (0..3).filter(|&k| d[k] != 0.0).collect();
    let point = |values: [(usize, f32); 3]| {
        let mut v = Vec3::ZERO;
        for (k, value) in values {
            v[k] = value;
        }
        v
    };
    match axes[..] {
        [k] => {
            let (i, j) = ((k + 1) % 3, (k + 2) % 3);
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(s, t)| point([(k, d[k]), (i, s * c), (j, t * c)]))
                .to_vec()
        }
        [i, j] => {
            let k = 3 - i - j;
            let (si, sj) = (d[i], d[j]);
            vec![
                point([(i, si), (j, sj * c), (k, -c)]),
                point([(i, si), (j, sj * c), (k, c)]),
                point([(i, si * c), (j, sj), (k, c)]),
                point([(i, si * c), (j, sj), (k, -c)]),
            ]
        }
        _ => (0..3)
            .map(|k| {
                Vec3::from_array(std::array::from_fn(
                    |m| if m == k { d[m] } else { d[m] * c },
                ))
            })
            .collect(),
    }
}

/// The convex hull of `points`, in order round it.
fn convex_hull(mut points: Vec<Pos2>) -> Vec<Pos2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| (*a - *b).length_sq() < 1e-6);
    if points.len() < 3 {
        return points;
    }
    let cross = |o: Pos2, a: Pos2, b: Pos2| (a - o).x * (b - o).y - (a - o).y * (b - o).x;
    let mut hull: Vec<Pos2> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &Pos2>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &q in ordered {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0
            {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull
}

/// Moves a turn in progress on by `dt` seconds. Returns whether it goes on.
fn step_turn(turn: &mut Option<Turn>, camera: &mut Camera, dt: f32) -> bool {
    let Some(t) = turn.as_mut() else {
        return false;
    };
    t.elapsed += dt;
    let x = (t.elapsed / TURN_TIME).clamp(0.0, 1.0);
    // Smooth start and stop.
    let e = x * x * (3.0 - 2.0 * x);
    let lerp = |a: f32, b: f32| a + (b - a) * e;
    camera.look_from(lerp(t.from.0, t.to.0), lerp(t.from.1, t.to.1));
    if x >= 1.0 {
        *turn = None;
    }
    turn.is_some()
}

/// An angle brought into (−π, π].
fn wrap_angle(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let a = (angle + PI).rem_euclid(TAU) - PI;
    if a <= -PI { a + TAU } else { a }
}

/// Two axes in the plane of the face with outward `normal`, so that a label drawn
/// along them reads upright from outside.
fn face_axes(normal: Vec3) -> (Vec3, Vec3) {
    // Up on the screen is −Z for the side faces; the end faces take +Y as up.
    let up = if normal.z == 0.0 {
        Vec3::NEG_Z
    } else {
        Vec3::Y
    };
    let right = up.cross(normal);
    (right, -up)
}

/// Draws `text` on a face, mapped onto its plane so it turns with the cube, and
/// sized to fit inside it. `corners` go round the face from its (−a, −b) corner.
fn face_label(ui: &Ui, text: &str, corners: &[Pos2; 4], color: Color32) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(15.0, crate::theme::bold_family()),
        color,
    );
    let size = galley.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    // Face coordinates: the galley's centre at the face's centre, a fraction of the
    // face wide at most.
    let scale = (1.5 / size.x).min(0.9 / size.y);
    let origin = corners[0];
    let (u, v) = ((corners[1] - origin) / 2.0, (corners[3] - origin) / 2.0);
    let map = |q: Pos2| {
        let (s, t) = (
            (q.x - size.x / 2.0) * scale + 1.0,
            (q.y - size.y / 2.0) * scale + 1.0,
        );
        origin + u * s + v * t
    };
    // Galleys keep their texture coordinates in texels of the font atlas.
    let [w, h] = ui.ctx().fonts(|f| f.font_image_size());
    let texel = vec2(1.0 / w as f32, 1.0 / h as f32);
    let mut mesh = egui::Mesh::default();
    for placed in &galley.rows {
        let row = &placed.row.visuals.mesh;
        let base = mesh.vertices.len() as u32;
        mesh.vertices
            .extend(row.vertices.iter().map(|vertex| egui::epaint::Vertex {
                pos: map(vertex.pos + placed.pos.to_vec2()),
                uv: (vertex.uv.to_vec2() * texel).to_pos2(),
                color: vertex.color,
            }));
        mesh.indices.extend(row.indices.iter().map(|i| base + i));
        mesh.texture_id = row.texture_id;
    }
    ui.painter().add(Shape::mesh(mesh));
}

/// The colour a fraction `t` of the way from `a` to `b`.
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
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

    /// Seen from outside, every face label reads left to right and top to bottom, not
    /// mirrored: on screen (y down) right × down points into the screen.
    #[test]
    fn face_labels_are_not_mirrored() {
        for (normal, _) in FACES {
            let (a, b) = face_axes(normal);
            assert_eq!(a.dot(normal), 0.0);
            assert_eq!(b.dot(normal), 0.0);
            assert!((a.cross(b) + normal).length() < 1e-6, "{normal}");
        }
    }

    /// Seen from a corner, the bevelled cube shows three faces, the three edges and
    /// the corner between them, and, at the outline, the three corners beside it.
    #[test]
    fn a_corner_view_shows_ten_pieces() {
        let mut camera = Camera::fit(Vec3::ZERO, 1.0, 1.0);
        let (yaw, pitch) = camera
            .orientation_along(Vec3::new(1.0, -1.0, -1.0))
            .unwrap();
        camera.look_from(yaw, pitch);
        let mut counts = [0; 3];
        for d in cube_directions() {
            if camera.view().transform_vector3(d.normalize()).z > 0.01 {
                counts[d.abs().cmpgt(Vec3::splat(0.5)).bitmask().count_ones() as usize - 1] += 1;
            }
        }
        assert_eq!(counts, [3, 3, 4]);
    }

    /// Each piece lies in the plane square to its direction, so it faces that way.
    #[test]
    fn cube_pieces_face_their_direction() {
        assert_eq!(cube_directions().count(), 26);
        for d in cube_directions() {
            let piece = cube_piece(d);
            let n = d.normalize();
            let level = piece[0].dot(n);
            assert!(piece.iter().all(|q| (q.dot(n) - level).abs() < 1e-5), "{d}");
        }
    }

    /// A turn ends on its target, the short way round, and then stops.
    #[test]
    fn camera_turns_end_on_their_target() {
        let mut viewer = Viewer {
            camera: Some(Camera::fit(Vec3::ZERO, 1.0, 1.0)),
            ..Viewer::default()
        };
        viewer.camera.as_mut().unwrap().look_from(3.0, 0.0);
        viewer.turn_to((-3.0, 0.5));
        let mut turn = viewer.turn;
        let camera = viewer.camera.as_mut().unwrap();
        assert!(step_turn(&mut turn, camera, TURN_TIME / 2.0));
        // Halfway the yaw has gone up through π, not down through zero.
        assert!(camera.yaw > 3.0);
        assert!(!step_turn(&mut turn, camera, TURN_TIME));
        assert!((wrap_angle(camera.yaw + 3.0)).abs() < 1e-5);
        assert!((camera.pitch - 0.5).abs() < 1e-6);
    }

    #[test]
    fn hull_and_angles() {
        let hull = convex_hull(vec![
            pos2(0.0, 0.0),
            pos2(2.0, 0.0),
            pos2(1.0, 1.0),
            pos2(2.0, 2.0),
            pos2(0.0, 2.0),
        ]);
        assert_eq!(hull.len(), 4);
        assert!((wrap_angle(3.0 * std::f32::consts::PI) - std::f32::consts::PI).abs() < 1e-5);
        assert!((wrap_angle(-0.5) + 0.5).abs() < 1e-6);
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
