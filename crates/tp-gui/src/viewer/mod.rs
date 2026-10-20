//! 3D view of the impeller, rendered with wgpu from the design's own mesh, so it
//! follows every edit at once.

mod camera;
mod mesh;
mod render;

use eframe::egui::{self, Color32, ComboBox, Pos2, Rect, Sense, Ui};
use eframe::egui_wgpu::RenderState;
use tp_project::ComponentId;

use crate::derived::Derived;
use crate::views::unavailable;
use camera::Camera;
use mesh::{ShroudMode, ViewOptions};
use render::{Gpu, Uniforms};

/// State of the 3D view between frames.
#[derive(Debug, Default)]
pub(crate) struct Viewer {
    options: ViewOptions,
    camera: Option<Camera>,
    gpu: Option<Gpu>,
    /// What the uploaded mesh was built from.
    mesh_key: Option<(u64, ComponentId, ViewOptions)>,
    /// Center and radius of the model, for "fit".
    bounds: Option<(glam::Vec3, f32)>,
    /// Impeller the camera was fitted to.
    fitted: Option<ComponentId>,
}

impl Viewer {
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        render_state: Option<&RenderState>,
        derived: &Derived,
        id: ComponentId,
    ) {
        self.toolbar(ui);
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
        let key = (derived.revision(), id, self.options);
        if self.mesh_key != Some(key) {
            let mesh = mesh::build(g, &self.options);
            gpu.upload(&state.device, &mesh);
            self.bounds = Some((mesh.center, mesh.radius));
            if self.fitted != Some(id) || self.camera.is_none() {
                self.camera = Some(Camera::fit(mesh.center, mesh.radius));
                self.fitted = Some(id);
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
        let uniforms = Uniforms {
            view_projection: camera
                .view_projection(rect.width() / rect.height().max(1.0))
                .to_cols_array_2d(),
            eye: camera.eye().extend(1.0).to_array(),
        };
        let background = ui.visuals().extreme_bg_color;
        let texture = gpu.render(state, size, &uniforms, background);
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        ui.painter().text(
            rect.left_bottom() + egui::vec2(8.0, -6.0),
            egui::Align2::LEFT_BOTTOM,
            "drag: rotate · right-drag or shift-drag: pan · scroll: zoom · double-click: fit",
            egui::FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.options.hub, "hub");
            ui.label("shroud");
            ComboBox::from_id_salt("shroud_mode")
                .width(100.0)
                .selected_text(match self.options.shroud {
                    ShroudMode::Hidden => "hidden",
                    ShroudMode::Transparent => "transparent",
                    ShroudMode::Solid => "solid",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.options.shroud, ShroudMode::Hidden, "hidden");
                    ui.selectable_value(
                        &mut self.options.shroud,
                        ShroudMode::Transparent,
                        "transparent",
                    );
                    ui.selectable_value(&mut self.options.shroud, ShroudMode::Solid, "solid");
                });
            ui.checkbox(&mut self.options.all_blades, "all blades");
            ui.checkbox(&mut self.options.wireframe, "wireframe");
            ui.separator();
            if let Some(camera) = self.camera.as_mut() {
                if ui.button("Iso").clicked() {
                    camera.look_from(-0.9, 0.55);
                }
                if ui
                    .button("Inlet")
                    .on_hover_text("Seen from the inlet along the axis")
                    .clicked()
                {
                    camera.look_from(-std::f32::consts::FRAC_PI_2, 1.55);
                }
                if ui
                    .button("Side")
                    .on_hover_text("Seen across the axis")
                    .clicked()
                {
                    camera.look_from(-std::f32::consts::FRAC_PI_2, 0.0);
                }
                if ui.button("Fit").clicked()
                    && let Some((center, radius)) = self.bounds
                {
                    *camera = Camera::fit(center, radius);
                }
            }
        });
    }
}
