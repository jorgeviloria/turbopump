//! What lies under the pointer in the 3D view: a ray from the eye through the pointer,
//! tested against the triangles of the shaded parts on the CPU.

use glam::{Mat4, Vec3};

use super::mesh::{MeshData, Opacity, PartId};

/// A point of the model under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Hit {
    pub part: PartId,
    pub point: Vec3,
    /// The corners of the triangle hit, for snapping to the mesh's vertices.
    pub corners: [Vec3; 3],
}

/// A ray in model space.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    /// The ray through a point of the screen, in normalized device coordinates, for
    /// `matrix` from model space to clip space. Works for both projections.
    pub fn through(matrix: Mat4, ndc: (f32, f32)) -> Self {
        let inverse = matrix.inverse();
        let near = inverse.project_point3(Vec3::new(ndc.0, ndc.1, 0.0));
        let far = inverse.project_point3(Vec3::new(ndc.0, ndc.1, 1.0));
        Self {
            origin: near,
            direction: far - near,
        }
    }

    /// Where the ray crosses the triangle, as a fraction of its direction
    /// (Möller–Trumbore), or `None` when it misses.
    pub fn triangle(&self, [a, b, c]: [Vec3; 3]) -> Option<f32> {
        let (ab, ac) = (b - a, c - a);
        let p = self.direction.cross(ac);
        let det = ab.dot(p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let s = self.origin - a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(ab);
        let v = self.direction.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = ac.dot(q) * inv;
        (t >= 0.0).then_some(t)
    }

    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.direction * t
    }
}

/// The nearest point of a shown part on `ray`. Glass parts only count where nothing
/// else lies behind them, so the parts inside the casing can be picked through it.
/// Points that `cut` removes are skipped.
pub(crate) fn pick(mesh: &MeshData, ray: &Ray, cut: Option<super::Plane>) -> Option<Hit> {
    let mut solid: Option<(f32, Hit)> = None;
    let mut glass: Option<(f32, Hit)> = None;
    for part in &mesh.parts {
        let best = if part.opacity == Opacity::Glass {
            &mut glass
        } else {
            &mut solid
        };
        let vertices = &mesh.vertices[part.range.start as usize..part.range.end as usize];
        for t in vertices.as_chunks::<3>().0 {
            let corners = t.map(|v| Vec3::from(v.position));
            let Some(along) = ray.triangle(corners) else {
                continue;
            };
            if best.is_some_and(|(d, _)| d <= along) {
                continue;
            }
            let point = ray.at(along);
            if cut.is_some_and(|plane| plane.removes(point)) {
                continue;
            }
            *best = Some((
                along,
                Hit {
                    part: part.id,
                    point,
                    corners,
                },
            ));
        }
    }
    solid.or(glass).map(|(_, hit)| hit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::mesh::{Shaded, Vertex};

    fn vertex(p: [f32; 3]) -> Vertex {
        Vertex {
            position: p,
            normal: [0.0, 0.0, 1.0],
            color: [1.0; 4],
        }
    }

    /// Two squares across the z axis, at z = 0 (glass) and z = 1 (opaque).
    fn two_squares() -> MeshData {
        let square = |z: f32| {
            [
                [-1.0, -1.0],
                [1.0, -1.0],
                [1.0, 1.0],
                [-1.0, -1.0],
                [1.0, 1.0],
                [-1.0, 1.0],
            ]
            .map(|[x, y]| vertex([x, y, z]))
        };
        let mut vertices = square(1.0).to_vec();
        vertices.extend(square(0.0));
        MeshData {
            vertices,
            opaque: 6,
            parts: vec![
                Shaded {
                    id: PartId::Hub,
                    range: 0..6,
                    opacity: Opacity::Opaque,
                },
                Shaded {
                    id: PartId::Volute,
                    range: 6..12,
                    opacity: Opacity::Glass,
                },
            ],
            ..MeshData::default()
        }
    }

    #[test]
    fn rays_hit_inside_triangles_only() {
        let ray = Ray {
            origin: Vec3::new(0.2, 0.2, -1.0),
            direction: Vec3::Z,
        };
        let t = ray.triangle([Vec3::ZERO, Vec3::X, Vec3::Y]).unwrap();
        assert!((ray.at(t) - Vec3::new(0.2, 0.2, 0.0)).length() < 1e-6);
        let beside = Ray {
            origin: Vec3::new(0.8, 0.8, -1.0),
            ..ray
        };
        assert!(beside.triangle([Vec3::ZERO, Vec3::X, Vec3::Y]).is_none());
        let away = Ray {
            direction: Vec3::NEG_Z,
            ..ray
        };
        assert!(away.triangle([Vec3::ZERO, Vec3::X, Vec3::Y]).is_none());
    }

    #[test]
    fn glass_in_front_lets_the_part_behind_be_picked() {
        let mesh = two_squares();
        let ray = Ray {
            origin: Vec3::new(0.1, 0.0, -5.0),
            direction: Vec3::Z,
        };
        assert_eq!(pick(&mesh, &ray, None).unwrap().part, PartId::Hub);
        // With nothing behind it, the glass is picked.
        let glass_only = MeshData {
            parts: mesh.parts[1..].to_vec(),
            vertices: mesh.vertices.clone(),
            ..MeshData::default()
        };
        let hit = pick(&glass_only, &ray, None).unwrap();
        assert_eq!(hit.part, PartId::Volute);
        assert!(hit.point.z.abs() < 1e-6);
    }

    #[test]
    fn the_cut_away_half_is_not_picked() {
        let mesh = two_squares();
        let ray = Ray {
            origin: Vec3::new(0.5, 0.0, -5.0),
            direction: Vec3::Z,
        };
        let plane = |normal: Vec3, offset: f32| Some(super::super::Plane { normal, offset });
        // The plane x = 0 removes x > 0: the ray at x = 0.5 meets nothing.
        assert!(pick(&mesh, &ray, plane(Vec3::X, 0.0)).is_none());
        assert!(pick(&mesh, &ray, plane(Vec3::NEG_X, 0.0)).is_some());
        // Moved past the ray, the plane leaves it its part.
        assert!(pick(&mesh, &ray, plane(Vec3::X, 0.6)).is_some());
    }

    #[test]
    fn a_ray_through_the_screen_centre_follows_the_view() {
        use glam::camera::rh::{proj::directx, view};
        let view = view::look_at_mat4(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let projection = directx::perspective(0.8, 1.0, 0.1, 20.0);
        let ray = Ray::through(projection * view, (0.0, 0.0));
        let direction = ray.direction.normalize();
        assert!((direction - Vec3::NEG_Z).length() < 1e-5);
        assert!((ray.origin.x.abs() + ray.origin.y.abs()) < 1e-5);
    }
}
