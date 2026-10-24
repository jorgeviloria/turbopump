//! 3D view of the stage, rendered with wgpu from the design's own mesh, so it follows
//! every edit at once. On top of the image: a view cube that turns the camera to a
//! face, the axes, a scale bar and the distance being measured. The part under the
//! pointer is tinted; a click selects it, a right-click offers to hide or isolate it.

mod camera;
mod mesh;
mod pick;
mod render;

use std::f32::consts::{FRAC_PI_2, PI};

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2,
};
use eframe::egui_wgpu::RenderState;
use glam::Mat4;
use glam::Vec3;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use crate::derived::Derived;
use crate::i18n::tr;
use crate::theme::{Palette, bold, icon};
use crate::views::unavailable;
use camera::Camera;
use mesh::MeshData;
pub(crate) use mesh::{Opacity, Part, PartId, ViewOptions};
use pick::{Hit, Ray};
use render::{Gpu, Highlight, Uniforms};

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
    /// Parallel projection instead of perspective.
    pub orthographic: bool,
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
    /// The free part of the view and the pixels per point, last frame, for captures.
    frame: Option<(Rect, f32)>,
    /// Angle about the axis of the normal of the meridional cut, rad, when the model is
    /// cut. The half the normal points to is removed.
    pub cut: Option<f32>,
    /// Clicks put points to measure between, instead of selecting parts.
    measuring: bool,
    /// The points measured: none, the first, or both.
    points: Vec<Vec3>,
    /// The part selected with a click.
    pub selected: Option<PartId>,
    /// The part under the pointer, last frame.
    pub hovered: Option<PartId>,
    /// The part the context menu is about.
    menu_part: Option<PartId>,
    /// The mesh last uploaded, for picking.
    mesh: Option<MeshData>,
}

/// What a right-click on the model asks of the scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PartAction {
    Hide(PartId),
    Isolate(PartId),
    Opacity(PartId, Opacity),
    ShowAll,
}

/// The name of a part, as the scene lists it.
pub(crate) fn part_name(id: PartId) -> &'static str {
    tr(match id {
        PartId::Blades => "Blades",
        PartId::Hub => "Hub (disk)",
        PartId::Shroud => "Shroud (cover)",
        PartId::Vanes => "Vanes",
        PartId::Walls => "Side walls",
        PartId::Volute => "Casing",
        PartId::Duct => "Inlet duct",
    })
}

/// The normal of a cut through the axis at `angle`.
fn cut_normal(angle: f32) -> Vec3 {
    Vec3::new(angle.cos(), angle.sin(), 0.0)
}

/// A pointer this close to a vertex of the mesh measures from the vertex, points.
const SNAP: f32 = 10.0;

/// A picture of the 3D view: `width` by `height` pixels, RGBA row by row.
#[derive(Debug, Clone)]
pub(crate) struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A capture is this many times as sharp as the screen.
const CAPTURE_SCALE: f32 = 2.0;

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
        }
    }

    /// Cuts the model through its axis, removing the half toward the camera, or puts
    /// it back whole.
    pub(crate) fn set_cut(&mut self, on: bool) {
        self.cut = on.then(|| self.camera.map_or(0.0, |c| c.yaw));
    }

    /// Turns the cut to remove the half toward the camera.
    pub(crate) fn aim_cut(&mut self) {
        if self.cut.is_some() {
            self.set_cut(true);
        }
    }

    /// Removes the other half instead.
    pub(crate) fn flip_cut(&mut self) {
        if let Some(angle) = self.cut.as_mut() {
            *angle = wrap_angle(*angle + PI);
        }
    }

    pub(crate) fn measuring(&self) -> bool {
        self.measuring
    }

    /// Starts or stops measuring; either way the points go.
    pub(crate) fn set_measuring(&mut self, on: bool) {
        self.measuring = on;
        self.points.clear();
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
        (left, right): (f32, f32),
    ) -> Option<PartAction> {
        let stator = stator.and_then(|s| derived.diffuser(s).map(|d| (s, d)));
        let volute = volute.and_then(|v| derived.volute(v).map(|g| (v, g)));
        let Some(state) = render_state else {
            unavailable(ui, tr("The 3D view needs the wgpu renderer."));
            return None;
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
            return None;
        };

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        // Panels float over the left and right of the view: center the model in the
        // free part between them.
        let free = Rect::from_min_max(
            pos2(rect.left() + left, rect.top()),
            pos2(rect.right() - right, rect.bottom()),
        );
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
            self.mesh = Some(mesh);
        }
        let camera = self.camera.as_mut()?;
        camera.orthographic = self.orthographic;
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

        let p = Palette::of(ui);
        let cut = self.cut.map(cut_normal);
        let matrix = clip_matrix(camera, rect, free);
        // What lies under the pointer, unless the camera is moving.
        let hit = match (response.hover_pos(), &self.mesh) {
            (Some(pos), Some(mesh)) if !response.dragged() && self.turn.is_none() => {
                let ndc = (
                    (pos.x - rect.left()) / rect.width().max(1.0) * 2.0 - 1.0,
                    1.0 - (pos.y - rect.top()) / rect.height().max(1.0) * 2.0,
                );
                pick::pick(mesh, &Ray::through(matrix, ndc), cut)
            }
            _ => None,
        };
        let target = hit.map(|h| snap(&h, matrix, rect, response.hover_pos()));
        self.hovered = hit.filter(|_| !self.measuring).map(|h| h.part);

        if response.clicked() {
            if self.measuring {
                if let Some((point, _)) = target {
                    if self.points.len() >= 2 {
                        self.points.clear();
                    }
                    self.points.push(point);
                }
            } else {
                self.selected = hit.map(|h| h.part);
            }
        }
        if response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.points.is_empty() {
                self.selected = None;
            } else {
                self.points.clear();
            }
        }
        if self.measuring && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if response.secondary_clicked() {
            self.menu_part = hit.map(|h| h.part);
        }
        let mut action = None;
        let menu_part = self.menu_part;
        let options = &self.options;
        response.context_menu(|ui| action = part_menu(ui, menu_part, options));
        if let Some(part) = self.hovered
            && !response.context_menu_opened()
        {
            response.clone().on_hover_text_at_pointer(part_name(part));
        }

        let range = |id: PartId| {
            self.mesh
                .as_ref()
                .and_then(|m| m.parts.iter().find(|s| s.id == id))
                .map(|s| s.range.clone())
        };
        let mut highlights = Vec::new();
        if let Some(range) = self.selected.and_then(range) {
            highlights.push(Highlight {
                range,
                selected: true,
            });
        }
        if let Some(id) = self.hovered.filter(|id| Some(*id) != self.selected)
            && let Some(range) = range(id)
        {
            highlights.push(Highlight {
                range,
                selected: false,
            });
        }

        let ppp = ui.ctx().pixels_per_point();
        self.frame = Some((free, ppp));
        let size = [
            ((rect.width() * ppp).round() as u32).max(1),
            ((rect.height() * ppp).round() as u32).max(1),
        ];
        let dark = ui.visuals().dark_mode;
        let uniforms = uniforms(camera, rect, free, dark, cut, p.accent);
        let texture = gpu.render(state, size, &uniforms, &highlights);
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        let camera = *camera;
        axes(ui, free, &camera);
        scale_bar(ui, free, &camera, units);
        measurement(
            ui,
            matrix,
            rect,
            &self.points,
            target.filter(|_| self.measuring),
            units,
        );
        if self.measuring {
            let hint = if self.points.len() == 1 {
                tr("Click the second point; Esc clears")
            } else {
                tr("Click two points of the model to measure between them")
            };
            chip(ui, free.center_top() + vec2(0.0, 18.0), hint, &p);
        }
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
        action
    }

    /// Renders the free part of the view, without the cube, axes and scale, sharper
    /// than on screen. `None` before the view has been drawn.
    pub(crate) fn capture(&mut self, state: &RenderState, dark: bool) -> Option<Capture> {
        let (free, ppp) = self.frame?;
        let camera = self.camera?;
        let cut = self.cut.map(cut_normal);
        let gpu = self.gpu.as_mut()?;
        let limit = state.device.limits().max_texture_dimension_2d as f32;
        let scale = (ppp * CAPTURE_SCALE).min(limit / free.width().max(free.height()).max(1.0));
        let size = [
            ((free.width() * scale).round() as u32).max(1),
            ((free.height() * scale).round() as u32).max(1),
        ];
        let uniforms = uniforms(&camera, free, free, dark, cut, Color32::TRANSPARENT);
        let rgba = gpu.capture(state, size, &uniforms)?;
        Some(Capture {
            width: size[0],
            height: size[1],
            rgba,
        })
    }
}

/// The right-click menu of a part, or of the model when `part` is `None`.
fn part_menu(ui: &mut Ui, part: Option<PartId>, options: &ViewOptions) -> Option<PartAction> {
    let mut action = None;
    if let Some(id) = part {
        ui.label(bold(part_name(id)));
        if ui
            .button(format!("{}  {}", icon::EYE_SLASH, tr("Hide")))
            .clicked()
        {
            action = Some(PartAction::Hide(id));
        }
        if ui
            .button(format!(
                "{}  {}",
                icon::CROSSHAIR_SIMPLE,
                tr("Show only this part")
            ))
            .clicked()
        {
            action = Some(PartAction::Isolate(id));
        }
        ui.horizontal(|ui| {
            for level in Opacity::ALL {
                let current = options.part(id).opacity == level;
                if ui.selectable_label(current, opacity_name(level)).clicked() {
                    action = Some(PartAction::Opacity(id, level));
                }
            }
        });
        ui.separator();
    }
    if ui.button(tr("Show all parts")).clicked() {
        action = Some(PartAction::ShowAll);
    }
    if action.is_some() {
        ui.close();
    }
    action
}

/// The name of an opacity level.
pub(crate) fn opacity_name(opacity: Opacity) -> &'static str {
    tr(match opacity {
        Opacity::Opaque => "Opaque",
        Opacity::Translucent => "Translucent",
        Opacity::Glass => "Glass",
    })
}

/// From model space to clip space, with the model centred in `free`, the part of
/// `rect` nothing covers.
fn clip_matrix(camera: &Camera, rect: Rect, free: Rect) -> Mat4 {
    // Normalized device x runs from −1 to 1 across the view.
    let shift = (free.center().x - rect.center().x) / (rect.width() / 2.0).max(1.0);
    Mat4::from_translation(Vec3::new(shift, 0.0, 0.0))
        * camera.view_projection(rect.width() / rect.height().max(1.0))
}

/// Where a point of the model lands in `rect`, or `None` behind the eye.
fn to_screen(matrix: Mat4, rect: Rect, point: Vec3) -> Option<Pos2> {
    let clip = matrix * point.extend(1.0);
    if clip.w <= 1e-9 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(pos2(
        rect.left() + (ndc.x + 1.0) / 2.0 * rect.width(),
        rect.top() + (1.0 - ndc.y) / 2.0 * rect.height(),
    ))
}

/// The point to measure from for `hit`: the nearest corner of the triangle when the
/// pointer is within [`SNAP`] points of it (then `true`), the surface point otherwise.
fn snap(hit: &Hit, matrix: Mat4, rect: Rect, pointer: Option<Pos2>) -> (Vec3, bool) {
    let Some(pointer) = pointer else {
        return (hit.point, false);
    };
    hit.corners
        .iter()
        .filter_map(|&c| to_screen(matrix, rect, c).map(|s| (c, s.distance(pointer))))
        .filter(|(_, d)| *d <= SNAP)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or((hit.point, false), |(c, _)| (c, true))
}

/// A short text on a rounded chip centred at `at`.
fn chip(ui: &Ui, at: Pos2, text: &str, p: &Palette) {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(12.0), p.text);
    let rect = Rect::from_center_size(at, galley.size() + vec2(16.0, 8.0));
    painter.rect(
        rect,
        rect.height() / 2.0,
        ui.visuals().window_fill,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(8.0, 4.0), galley, p.text);
}

/// The distance between the points measured, or from the first to the pointer, with
/// its radial and axial parts.
fn measurement(
    ui: &Ui,
    matrix: Mat4,
    rect: Rect,
    points: &[Vec3],
    pointer: Option<(Vec3, bool)>,
    units: &DisplayUnits,
) {
    let p = Palette::of(ui);
    let painter = ui.painter().with_clip_rect(rect);
    let end = points.get(1).copied().or(pointer.map(|t| t.0));
    if let (Some(&a), Some(b)) = (points.first(), end)
        && let (Some(sa), Some(sb)) = (to_screen(matrix, rect, a), to_screen(matrix, rect, b))
    {
        let stroke = Stroke::new(2.0, p.accent);
        if points.len() == 2 {
            painter.line_segment([sa, sb], stroke);
        } else {
            painter.add(Shape::dashed_line(&[sa, sb], stroke, 6.0, 4.0));
        }
        let length = |v: f32| units.show(QuantityKind::Length, f64::from(v));
        let radius = |q: Vec3| q.x.hypot(q.y);
        let text = format!(
            "{}\nΔr {}  ·  Δz {}",
            length(a.distance(b)),
            length((radius(b) - radius(a)).abs()),
            length((b.z - a.z).abs()),
        );
        chip(ui, sa.lerp(sb, 0.5) + vec2(0.0, -22.0), &text, &p);
    }
    for (i, q) in points.iter().enumerate() {
        if let Some(s) = to_screen(matrix, rect, *q) {
            painter.circle(s, 4.0, p.accent, Stroke::new(1.5, Color32::WHITE));
            let label = if i == 0 { "1" } else { "2" };
            // A halo in the window color keeps the number legible on any part.
            let halo = ui.visuals().window_fill;
            for offset in [
                vec2(-1.0, 0.0),
                vec2(1.0, 0.0),
                vec2(0.0, -1.0),
                vec2(0.0, 1.0),
            ] {
                painter.text(
                    s + vec2(8.0, -8.0) + offset,
                    Align2::LEFT_BOTTOM,
                    label,
                    FontId::new(12.0, crate::theme::bold_family()),
                    halo,
                );
            }
            painter.text(
                s + vec2(8.0, -8.0),
                Align2::LEFT_BOTTOM,
                label,
                FontId::new(12.0, crate::theme::bold_family()),
                p.accent,
            );
        }
    }
    // Where a click would put the next point: a dot on a vertex, a ring elsewhere.
    if let Some((q, snapped)) = pointer
        && let Some(s) = to_screen(matrix, rect, q)
    {
        if snapped {
            painter.circle_filled(s, 4.5, p.accent);
        } else {
            painter.circle_stroke(s, 5.0, Stroke::new(1.5, p.accent));
        }
    }
}

/// Lights fixed to the camera, and the background of the theme. The model is centered
/// in `free`, the part of `rect` nothing covers, and cut by the plane through the axis
/// with normal `cut`.
fn uniforms(
    camera: &Camera,
    rect: Rect,
    free: Rect,
    dark: bool,
    cut: Option<Vec3>,
    accent: Color32,
) -> Uniforms {
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
    // A parallel view looks along one direction: w = 0 tells the shader so.
    let eye = if camera.orthographic {
        camera.backward().extend(0.0)
    } else {
        camera.eye().extend(1.0)
    };
    let [r, g, b, _] = accent.to_array();
    Uniforms {
        view_projection: clip_matrix(camera, rect, free).to_cols_array_2d(),
        eye: eye.to_array(),
        key_light: key.extend(0.0).to_array(),
        fill_light: fill.extend(0.0).to_array(),
        up: up.extend(0.0).to_array(),
        sky,
        ground,
        clip: cut.map_or([0.0; 4], |n| n.extend(1.0).to_array()),
        accent: [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        ],
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

    /// The cut removes the half toward the camera, flips to the other, and measuring
    /// starts afresh each time it is switched.
    #[test]
    fn the_cut_faces_the_camera_and_flips() {
        let mut viewer = Viewer {
            camera: Some(Camera::fit(Vec3::ZERO, 1.0, 1.0)),
            ..Viewer::default()
        };
        viewer.set_cut(true);
        let camera = viewer.camera.unwrap();
        let normal = cut_normal(viewer.cut.unwrap());
        assert!(normal.dot(camera.eye()) > 0.0, "the eye's half goes");
        viewer.flip_cut();
        assert!(cut_normal(viewer.cut.unwrap()).dot(camera.eye()) < 0.0);
        viewer.aim_cut();
        assert!(cut_normal(viewer.cut.unwrap()).dot(camera.eye()) > 0.0);
        viewer.set_cut(false);
        viewer.aim_cut();
        assert!(viewer.cut.is_none(), "aiming does not cut a whole model");

        viewer.set_measuring(true);
        viewer.points.push(Vec3::ZERO);
        viewer.set_measuring(false);
        assert!(viewer.points.is_empty() && !viewer.measuring());
    }

    /// A point projected to the screen and a ray back through it meet again.
    #[test]
    fn screen_points_and_rays_agree() {
        let camera = Camera::fit(Vec3::ZERO, 0.1, 1.5);
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(600.0, 400.0));
        let free = Rect::from_min_max(pos2(200.0, 20.0), rect.max);
        let matrix = clip_matrix(&camera, rect, free);
        let point = Vec3::new(0.03, -0.02, 0.01);
        let s = to_screen(matrix, rect, point).unwrap();
        let ndc = (
            (s.x - rect.left()) / rect.width() * 2.0 - 1.0,
            1.0 - (s.y - rect.top()) / rect.height() * 2.0,
        );
        let ray = Ray::through(matrix, ndc);
        let closest = ray.direction.normalize().cross(point - ray.origin).length();
        assert!(closest < 1e-5, "{closest}");
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
