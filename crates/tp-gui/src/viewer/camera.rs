//! Turntable camera around the impeller axis.

use glam::camera::rh::{proj::directx, view};
use glam::{Mat4, Vec3};

/// Vertical field of view, rad.
const FOV: f32 = 0.5;
const MAX_PITCH: f32 = 1.55;

/// Camera orbiting `target`. The model axis is z, pointing downstream; the screen's up
/// direction is −z, so the inlet is at the top and positive pitch looks from the inlet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Camera {
    pub target: Vec3,
    /// Angle about the axis, rad.
    pub yaw: f32,
    /// Elevation toward the inlet side, rad.
    pub pitch: f32,
    pub distance: f32,
    /// Radius of the model, for zoom limits and clipping planes.
    radius: f32,
}

impl Camera {
    /// Looks at a sphere of `radius` around `center` from the inlet side, at an angle.
    pub fn fit(center: Vec3, radius: f32) -> Self {
        let radius = radius.max(1e-6);
        Self {
            target: center,
            yaw: -0.9,
            pitch: 0.55,
            distance: 0.98 * radius / (FOV / 2.0).sin(),
            radius,
        }
    }

    /// Keeps the distance and target but looks from the given direction.
    pub fn look_from(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch.clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Looks at the target from `direction` (from the target toward the eye).
    pub fn look_along(&mut self, direction: Vec3) {
        let d = direction.normalize_or_zero();
        if d == Vec3::ZERO {
            return;
        }
        let pitch = (-d.z).clamp(-1.0, 1.0).asin();
        // Straight along the axis the yaw is free: keep it.
        if d.x.hypot(d.y) > 1e-4 {
            self.yaw = d.y.atan2(d.x);
        }
        self.pitch = pitch.clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Size of one point of the view at the target, in model units, for a view
    /// `height` points tall.
    pub fn units_per_point(&self, width: f32, height: f32) -> f32 {
        let fov = Self::vertical_fov(width / height.max(1.0));
        2.0 * self.distance * (fov / 2.0).tan() / height.max(1.0)
    }

    pub fn eye(&self) -> Vec3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        self.target + Vec3::new(cp * cy, cp * sy, -sp) * self.distance
    }

    pub fn view(&self) -> Mat4 {
        view::look_at_mat4(self.eye(), self.target, Vec3::NEG_Z)
    }

    /// Vertical field of view for `aspect`: in a narrow view the horizontal field keeps
    /// the nominal angle, so the model still fits.
    fn vertical_fov(aspect: f32) -> f32 {
        if aspect < 1.0 {
            2.0 * ((FOV / 2.0).tan() / aspect.max(1e-3)).atan()
        } else {
            FOV
        }
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let near = (self.distance - 2.0 * self.radius).max(self.distance * 0.01);
        let far = self.distance + 2.0 * self.radius;
        // wgpu clip space: z in [0, 1], y up.
        directx::perspective(Self::vertical_fov(aspect), aspect.max(1e-3), near, far)
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Rotates by a pointer drag of (`dx`, `dy`) pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Moves the target with the pointer: (`dx`, `dy`) pixels in a view of `width` by
    /// `height` pixels.
    pub fn pan(&mut self, dx: f32, dy: f32, width: f32, height: f32) {
        let per_pixel = self.units_per_point(width, height);
        let view = self.view();
        let right = view.row(0).truncate();
        let up = view.row(1).truncate();
        self.target += (-right * dx + up * dy) * per_pixel;
    }

    /// Multiplies the distance by `factor`, within sensible limits.
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(self.radius * 0.3, self.radius * 50.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_projects_to_the_centre() {
        let camera = Camera::fit(Vec3::new(0.0, 0.0, 0.05), 0.15);
        let clip = camera.view_projection(1.5) * camera.target.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x.abs() < 1e-5 && ndc.y.abs() < 1e-5);
        assert!((0.0..1.0).contains(&ndc.z));
    }

    #[test]
    fn the_inlet_is_up_on_screen() {
        let camera = Camera::fit(Vec3::ZERO, 0.1);
        let vp = camera.view_projection(1.0);
        let inlet = vp * Vec3::new(0.0, 0.0, -0.05).extend(1.0);
        let outlet = vp * Vec3::new(0.0, 0.0, 0.05).extend(1.0);
        assert!(inlet.y / inlet.w > outlet.y / outlet.w);
    }

    #[test]
    fn whole_model_is_in_view_in_wide_and_narrow_views() {
        let camera = Camera::fit(Vec3::ZERO, 0.1);
        for aspect in [0.5, 1.0, 2.0] {
            let vp = camera.view_projection(aspect);
            for p in [
                Vec3::X,
                Vec3::Y,
                Vec3::Z,
                Vec3::NEG_X,
                Vec3::NEG_Y,
                Vec3::NEG_Z,
            ] {
                let clip = vp * (p * 0.1).extend(1.0);
                let ndc = clip.truncate() / clip.w;
                assert!(
                    ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0 && (0.0..1.0).contains(&ndc.z),
                    "aspect {aspect}: {ndc:?}"
                );
            }
        }
    }

    #[test]
    fn looking_along_a_direction_puts_the_eye_there() {
        let mut camera = Camera::fit(Vec3::ZERO, 0.1);
        for d in [Vec3::X, Vec3::NEG_Y, Vec3::new(1.0, 1.0, -1.0)] {
            camera.look_along(d);
            let eye = (camera.eye() - camera.target).normalize();
            assert!(eye.distance(d.normalize()) < 1e-4, "{d:?}: {eye:?}");
        }
        // Along the axis the pitch stops just short of the pole.
        camera.look_along(Vec3::NEG_Z);
        assert!((camera.pitch - MAX_PITCH).abs() < 1e-6);
    }

    #[test]
    fn orbit_and_zoom_stay_in_range() {
        let mut camera = Camera::fit(Vec3::ZERO, 0.1);
        camera.orbit(0.0, 1e6);
        assert!(camera.pitch <= MAX_PITCH);
        camera.zoom(1e-9);
        assert!(camera.distance >= 0.03 - 1e-6);
        let before = camera.target;
        camera.pan(10.0, 0.0, 400.0, 500.0);
        assert!((camera.target - before).length() > 0.0);
    }
}
