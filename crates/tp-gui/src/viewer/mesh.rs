//! Vertex data of the impeller for the 3D view, with smooth normals.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use tp_design::impeller::ImpellerGeometry;
use tp_design::volute::VoluteGeometry;
use tp_io::mesh::{Triangle, impeller_parts, volute_triangles};
use tp_math::DVec3;

/// How the shroud is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ShroudMode {
    Hidden,
    Transparent,
    Solid,
}

/// What the 3D view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ViewOptions {
    pub hub: bool,
    pub shroud: ShroudMode,
    pub all_blades: bool,
    pub wireframe: bool,
    /// How the volute is drawn, when the stage has one.
    pub volute: ShroudMode,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            hub: true,
            shroud: ShroudMode::Transparent,
            all_blades: true,
            wireframe: false,
            volute: ShroudMode::Transparent,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Linear RGB and opacity.
    pub color: [f32; 4],
}

/// Triangles split by how they are drawn, plus wireframe lines.
#[derive(Debug, Default)]
pub(crate) struct MeshData {
    /// Opaque triangles first, then transparent ones.
    pub vertices: Vec<Vertex>,
    pub opaque: u32,
    /// Pairs of vertices.
    pub lines: Vec<Vertex>,
    pub center: Vec3,
    pub radius: f32,
}

const BLADE: [f32; 4] = [0.10, 0.25, 0.55, 1.0];
const HUB: [f32; 4] = [0.42, 0.43, 0.45, 1.0];
const SHROUD_SOLID: [f32; 4] = [0.55, 0.30, 0.16, 1.0];
const SHROUD_GLASS: [f32; 4] = [0.60, 0.36, 0.20, 0.22];
const VOLUTE_SOLID: [f32; 4] = [0.36, 0.42, 0.22, 1.0];
const VOLUTE_GLASS: [f32; 4] = [0.45, 0.55, 0.25, 0.16];
/// Faces meeting at more than this angle keep a sharp edge.
const CREASE_DEGREES: f64 = 40.0;

/// Corner normals averaged over neighbouring faces that meet at less than the crease
/// angle, weighted by area, so curved surfaces shade smoothly and edges stay sharp.
pub(crate) fn smooth_normals(triangles: &[Triangle]) -> Vec<[DVec3; 3]> {
    let key = |p: DVec3| {
        let q = |v: f64| (v * 1e7).round() as i64;
        (q(p.x), q(p.y), q(p.z))
    };
    let faces: Vec<DVec3> = triangles
        .iter()
        .map(|t| (t[1] - t[0]).cross(t[2] - t[0]))
        .collect();
    let mut around: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, t) in triangles.iter().enumerate() {
        for p in t {
            around.entry(key(*p)).or_default().push(i);
        }
    }
    let cos_crease = CREASE_DEGREES.to_radians().cos();
    triangles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let own = faces[i].normalize_or_zero();
            t.map(|p| {
                let sum: DVec3 = around[&key(p)]
                    .iter()
                    .map(|&j| faces[j])
                    .filter(|n| {
                        let unit = n.normalize_or_zero();
                        // Neighbours wound the other way still belong to the surface.
                        unit.dot(own).abs() > cos_crease
                    })
                    .map(|n| if n.dot(own) < 0.0 { -n } else { n })
                    .sum();
                let n = sum.normalize_or_zero();
                if n == DVec3::ZERO { own } else { n }
            })
        })
        .collect()
}

fn push_triangles(out: &mut Vec<Vertex>, triangles: &[Triangle], color: [f32; 4]) {
    let normals = smooth_normals(triangles);
    for (t, n) in triangles.iter().zip(normals) {
        for k in 0..3 {
            out.push(Vertex {
                position: t[k].as_vec3().to_array(),
                normal: n[k].as_vec3().to_array(),
                color,
            });
        }
    }
}

fn push_edges(out: &mut Vec<Vertex>, triangles: &[Triangle], color: [f32; 4]) {
    let key = |p: DVec3| {
        let q = |v: f64| (v * 1e7).round() as i64;
        (q(p.x), q(p.y), q(p.z))
    };
    let mut seen = std::collections::HashSet::new();
    for t in triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (key(a), key(b));
            let edge = if ka < kb { (ka, kb) } else { (kb, ka) };
            if seen.insert(edge) {
                for p in [a, b] {
                    out.push(Vertex {
                        position: p.as_vec3().to_array(),
                        normal: [0.0; 3],
                        color,
                    });
                }
            }
        }
    }
}

/// Builds the vertices of the impeller, and of its volute if given, as `options` ask.
pub(crate) fn build(
    g: &ImpellerGeometry,
    volute: Option<&VoluteGeometry>,
    options: &ViewOptions,
) -> MeshData {
    let parts = impeller_parts(g);
    let blades: Vec<Triangle> = if options.all_blades {
        parts.blades.iter().flatten().copied().collect()
    } else {
        parts.blades.first().cloned().unwrap_or_default()
    };
    let mut opaque: Vec<(&[Triangle], [f32; 4])> = vec![(&blades, BLADE)];
    let mut transparent: Vec<(&[Triangle], [f32; 4])> = Vec::new();
    if options.hub {
        opaque.push((&parts.hub, HUB));
    }
    match options.shroud {
        ShroudMode::Hidden => {}
        ShroudMode::Solid => opaque.push((&parts.shroud, SHROUD_SOLID)),
        ShroudMode::Transparent => transparent.push((&parts.shroud, SHROUD_GLASS)),
    }
    let casing = volute.map(volute_triangles).unwrap_or_default();
    if !casing.is_empty() {
        match options.volute {
            ShroudMode::Hidden => {}
            ShroudMode::Solid => opaque.push((&casing, VOLUTE_SOLID)),
            ShroudMode::Transparent => transparent.push((&casing, VOLUTE_GLASS)),
        }
    }

    let mut mesh = MeshData::default();
    if options.wireframe {
        for (triangles, color) in opaque.iter().chain(&transparent) {
            push_edges(
                &mut mesh.lines,
                triangles,
                [color[0], color[1], color[2], 1.0],
            );
        }
    } else {
        for (triangles, color) in &opaque {
            push_triangles(&mut mesh.vertices, triangles, *color);
        }
        mesh.opaque = mesh.vertices.len() as u32;
        for (triangles, color) in &transparent {
            push_triangles(&mut mesh.vertices, triangles, *color);
        }
    }

    // Bounds of the whole stage, whatever is shown, so the camera stays put.
    let all = parts
        .blades
        .iter()
        .flatten()
        .chain(&parts.hub)
        .chain(&parts.shroud)
        .chain(&casing)
        .flatten();
    let (min, max) = all.clone().fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    if min.is_finite() && max.is_finite() {
        let center = (min + max) / 2.0;
        // The sphere through the farthest point is much tighter than the box diagonal
        // for a flat, round part.
        let radius = all.map(|p| p.distance(center)).fold(0.0, f64::max);
        mesh.center = center.as_vec3();
        mesh.radius = radius as f32;
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_project;
    use crate::derived::Derived;

    fn geometry() -> ImpellerGeometry {
        let project = default_project();
        let mut derived = Derived::default();
        derived.update(&project);
        derived
            .geometry(project.components()[0].id())
            .unwrap()
            .clone()
    }

    #[test]
    fn normals_are_unit_vectors() {
        let g = geometry();
        let mesh = build(&g, None, &ViewOptions::default());
        assert!(mesh.vertices.len() > 3000);
        for v in &mesh.vertices {
            let n = Vec3::from(v.normal);
            assert!((n.length() - 1.0).abs() < 1e-3, "{n:?}");
        }
        let r2 = (g.basis.outlet_diameter / 2.0) as f32;
        assert!(mesh.radius > r2 && mesh.radius < 1.2 * r2);
    }

    #[test]
    fn transparent_shroud_comes_last() {
        let g = geometry();
        let mesh = build(&g, None, &ViewOptions::default());
        assert!(mesh.opaque > 0 && (mesh.opaque as usize) < mesh.vertices.len());
        assert!(
            mesh.vertices[mesh.opaque as usize..]
                .iter()
                .all(|v| v.color[3] < 1.0)
        );
        let solid = build(
            &g,
            None,
            &ViewOptions {
                shroud: ShroudMode::Solid,
                ..ViewOptions::default()
            },
        );
        assert_eq!(solid.opaque as usize, solid.vertices.len());
    }

    #[test]
    fn wireframe_has_lines_only() {
        let g = geometry();
        let mesh = build(
            &g,
            None,
            &ViewOptions {
                wireframe: true,
                ..ViewOptions::default()
            },
        );
        assert!(mesh.vertices.is_empty());
        assert!(!mesh.lines.is_empty() && mesh.lines.len().is_multiple_of(2));
    }

    #[test]
    fn flat_strip_keeps_the_face_normal_and_creases_stay_sharp() {
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        // Two triangles in the plane z = 0, and one folded at 90° along x = 1.
        let tris = vec![
            [p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0)],
            [p(0.0, 0.0, 0.0), p(1.0, 1.0, 0.0), p(0.0, 1.0, 0.0)],
            [p(1.0, 0.0, 0.0), p(1.0, 0.0, 1.0), p(1.0, 1.0, 0.0)],
        ];
        let normals = smooth_normals(&tris);
        for n in normals[0].iter().chain(&normals[1]) {
            assert!((n.z.abs() - 1.0).abs() < 1e-12);
        }
        for n in &normals[2] {
            assert!((n.x.abs() - 1.0).abs() < 1e-12);
        }
    }
}
