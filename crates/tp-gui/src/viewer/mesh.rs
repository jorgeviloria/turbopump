//! Vertex data of the impeller for the 3D view, with smooth normals.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use tp_design::diffuser::DiffuserDesign;
use tp_design::impeller::ImpellerGeometry;
use tp_design::impeller::duct::InletDuct;
use tp_design::volute::VoluteGeometry;
use tp_io::mesh::{
    Triangle, blade_solids, diffuser_parts, impeller_parts, volute_solids, volute_triangles,
};
use tp_math::{DVec2, DVec3};

/// How much of a part the eye sees through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Opacity {
    #[default]
    Opaque,
    Translucent,
    /// Mostly see-through: only the outline and grazing faces show.
    Glass,
}

impl Opacity {
    pub const ALL: [Opacity; 3] = [Opacity::Opaque, Opacity::Translucent, Opacity::Glass];

    fn alpha(self) -> f32 {
        match self {
            Opacity::Opaque => 1.0,
            Opacity::Translucent => 0.5,
            Opacity::Glass => 0.2,
        }
    }

    /// The next level, back to opaque after glass.
    pub fn next(self) -> Self {
        match self {
            Opacity::Opaque => Opacity::Translucent,
            Opacity::Translucent => Opacity::Glass,
            Opacity::Glass => Opacity::Opaque,
        }
    }
}

/// A shaded part of the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PartId {
    Blades,
    Hub,
    Shroud,
    Vanes,
    /// Side walls of a diffuser.
    Walls,
    Volute,
    /// The fluid of the inlet duct ahead of the eye.
    Duct,
}

/// How one part of the model is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Part {
    pub shown: bool,
    pub opacity: Opacity,
    /// Gamma-encoded RGB.
    pub color: [u8; 3],
}

impl Part {
    const fn solid(color: [u8; 3]) -> Part {
        Part {
            shown: true,
            opacity: Opacity::Opaque,
            color,
        }
    }

    const fn glass(color: [u8; 3]) -> Part {
        Part {
            shown: true,
            opacity: Opacity::Glass,
            color,
        }
    }
}

/// What the 3D view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ViewOptions {
    pub blades: Part,
    pub hub: Part,
    pub shroud: Part,
    pub vanes: Part,
    /// Side walls of a diffuser.
    pub walls: Part,
    pub volute: Part,
    /// The inlet duct, when modelled.
    pub duct: Part,
    /// Every blade, or only the first.
    pub all_blades: bool,
    /// Outlines and sharp edges of the parts.
    pub edges: bool,
    /// Cross sections of the volute, as lines.
    pub sections: bool,
    /// Triangle edges instead of shaded surfaces.
    pub wireframe: bool,
}

impl ViewOptions {
    pub fn part(&self, id: PartId) -> &Part {
        match id {
            PartId::Blades => &self.blades,
            PartId::Hub => &self.hub,
            PartId::Shroud => &self.shroud,
            PartId::Vanes => &self.vanes,
            PartId::Walls => &self.walls,
            PartId::Volute => &self.volute,
            PartId::Duct => &self.duct,
        }
    }

    pub fn part_mut(&mut self, id: PartId) -> &mut Part {
        match id {
            PartId::Blades => &mut self.blades,
            PartId::Hub => &mut self.hub,
            PartId::Shroud => &mut self.shroud,
            PartId::Vanes => &mut self.vanes,
            PartId::Walls => &mut self.walls,
            PartId::Volute => &mut self.volute,
            PartId::Duct => &mut self.duct,
        }
    }
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            blades: Part::solid(BLADE),
            hub: Part::solid(HUB),
            shroud: Part::glass(SHROUD),
            vanes: Part::solid(VANES),
            walls: Part::glass(WALLS),
            volute: Part::glass(VOLUTE),
            duct: Part::glass(DUCT),
            all_blades: true,
            edges: true,
            sections: true,
            wireframe: false,
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

/// The triangles of one part among the vertices.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shaded {
    pub id: PartId,
    pub range: Range<u32>,
    pub opacity: Opacity,
}

/// A closed volume of an opaque part, capped where the cut runs through it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cap {
    /// Its triangles among the closures.
    pub range: Range<u32>,
    /// Linear RGB of the cap.
    pub color: [f32; 4],
}

/// Triangles split by how they are drawn, plus wireframe lines.
#[derive(Debug, Default)]
pub(crate) struct MeshData {
    /// Opaque triangles first, then transparent ones.
    pub vertices: Vec<Vertex>,
    pub opaque: u32,
    /// Where each shaded part lies in `vertices`, for picking and highlights.
    pub parts: Vec<Shaded>,
    /// Pairs of vertices.
    pub lines: Vec<Vertex>,
    /// Closed volumes, never drawn: the cut counts their faces to find where the
    /// plane lies inside them. Overlapping volumes are separate caps.
    pub closures: Vec<Vertex>,
    pub caps: Vec<Cap>,
    pub center: Vec3,
    pub radius: f32,
}

/// Default materials, gamma-encoded: a bronze impeller, a teal diffuser and a blue
/// casing, in the colors of their components elsewhere in the editor.
const BLADE: [u8; 3] = [188, 137, 80];
const HUB: [u8; 3] = [158, 118, 75];
const SHROUD: [u8; 3] = [196, 153, 105];
const VANES: [u8; 3] = [69, 158, 144];
const WALLS: [u8; 3] = [89, 170, 160];
const VOLUTE: [u8; 3] = [85, 129, 188];
/// The inlet duct: a cool grey, a fluid rather than a part of the pump.
const DUCT: [u8; 3] = [120, 150, 170];
/// Steps round the axis of a revolved outline.
const REVOLVE_STEPS: usize = 72;
/// Lines are drawn in the part's color times this.
const LINE_SHADE: f32 = 0.35;
/// Caps are drawn in the part's color times this, so the cut face reads apart from
/// the surfaces.
const CAP_SHADE: f32 = 0.8;
/// Caps the shader takes at most.
pub(crate) const MAX_CAPS: usize = 8;
/// One volute section of this many is drawn as a line.
const SECTION_STRIDE: usize = 3;
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

fn line_vertex(p: DVec3, color: [f32; 4]) -> Vertex {
    Vertex {
        position: p.as_vec3().to_array(),
        normal: [0.0; 3],
        color,
    }
}

fn edge_key(p: DVec3) -> (i64, i64, i64) {
    let q = |v: f64| (v * 1e7).round() as i64;
    (q(p.x), q(p.y), q(p.z))
}

/// Every triangle edge once, for the wireframe.
fn push_all_edges(out: &mut Vec<Vertex>, triangles: &[Triangle], color: [f32; 4]) {
    let mut seen = HashSet::new();
    for t in triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (edge_key(a), edge_key(b));
            let edge = if ka < kb { (ka, kb) } else { (kb, ka) };
            if seen.insert(edge) {
                out.push(line_vertex(a, color));
                out.push(line_vertex(b, color));
            }
        }
    }
}

/// Edges that outline a part: those of only one triangle (open borders) and those
/// where the faces fold by more than the crease angle.
pub(crate) fn feature_edges(triangles: &[Triangle]) -> Vec<[DVec3; 2]> {
    type Key = (i64, i64, i64);
    let mut faces: HashMap<(Key, Key), (Vec<DVec3>, [DVec3; 2])> = HashMap::new();
    for t in triangles {
        let normal = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (edge_key(a), edge_key(b));
            let key = if ka < kb { (ka, kb) } else { (kb, ka) };
            faces
                .entry(key)
                .or_insert_with(|| (Vec::new(), [a, b]))
                .0
                .push(normal);
        }
    }
    let cos_crease = CREASE_DEGREES.to_radians().cos();
    let mut edges: Vec<[DVec3; 2]> = faces
        .into_values()
        .filter(|(normals, _)| match normals.as_slice() {
            [_] => true,
            [n1, n2] => n1.dot(*n2).abs() < cos_crease,
            _ => false,
        })
        .map(|(_, edge)| edge)
        .collect();
    // Hash maps iterate in any order; keep the vertex data reproducible.
    edges.sort_by(|a, b| {
        let key = |e: &[DVec3; 2]| (edge_key(e[0]), edge_key(e[1]));
        key(a).cmp(&key(b))
    });
    edges
}

/// Whether `triangles` close a volume: every edge is shared by an even number of
/// them, so that a ray crosses the surface an odd number of times from inside.
pub(crate) fn is_closed(triangles: &[Triangle]) -> bool {
    type Key = (i64, i64, i64);
    let mut count: HashMap<(Key, Key), u32> = HashMap::new();
    for t in triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ka, kb) = (edge_key(a), edge_key(b));
            if ka == kb {
                continue;
            }
            *count
                .entry(if ka < kb { (ka, kb) } else { (kb, ka) })
                .or_default() += 1;
        }
    }
    !triangles.is_empty() && count.values().all(|n| n % 2 == 0)
}

fn rgba(rgb: [f32; 3], alpha: f32) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
}

/// Linear RGB of a gamma-encoded color, which the shader lights.
fn linear(color: [u8; 3]) -> [f32; 3] {
    color.map(|c| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn line_color(rgb: [f32; 3]) -> [f32; 4] {
    rgba(rgb.map(|c| c * LINE_SHADE), 1.0)
}

/// The surface of a closed outline (z, r) revolved about the z axis.
fn revolved(outline: &[DVec2]) -> Vec<Triangle> {
    let n = outline.len();
    let mut triangles = Vec::with_capacity(n * REVOLVE_STEPS * 2);
    let at = |p: DVec2, k: usize| {
        let a = std::f64::consts::TAU * k as f64 / REVOLVE_STEPS as f64;
        DVec3::new(p.y * a.cos(), p.y * a.sin(), p.x)
    };
    for i in 0..n {
        let (p, q) = (outline[i], outline[(i + 1) % n]);
        for k in 0..REVOLVE_STEPS {
            let (a, b, c, d) = (at(p, k), at(q, k), at(q, k + 1), at(p, k + 1));
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    triangles
}

/// Builds the vertices of the stage as `options` ask.
pub(crate) fn build(
    g: &ImpellerGeometry,
    inlet_duct: Option<&InletDuct>,
    stator: Option<&DiffuserDesign>,
    volute: Option<&VoluteGeometry>,
    options: &ViewOptions,
) -> MeshData {
    let parts = impeller_parts(g);
    let blades: Vec<Triangle> = if options.all_blades {
        parts.blades.iter().flatten().copied().collect()
    } else {
        parts.blades.first().cloned().unwrap_or_default()
    };
    let diffuser = stator.map(diffuser_parts).unwrap_or_default();
    let casing = volute.map(volute_triangles).unwrap_or_default();
    let duct = inlet_duct.map(|d| revolved(&d.outline)).unwrap_or_default();
    let shown: [(&[Triangle], PartId); 7] = [
        (&blades, PartId::Blades),
        (&parts.hub, PartId::Hub),
        (&parts.shroud, PartId::Shroud),
        (&diffuser.vanes, PartId::Vanes),
        (&diffuser.walls, PartId::Walls),
        (&casing, PartId::Volute),
        (&duct, PartId::Duct),
    ];

    let mut mesh = MeshData::default();
    let visible = shown.iter().filter_map(|(t, id)| {
        let part = options.part(*id);
        (part.shown && !t.is_empty()).then_some((*t, *id, *part))
    });
    if options.wireframe {
        for (triangles, _, part) in visible.clone() {
            push_all_edges(&mut mesh.lines, triangles, rgba(linear(part.color), 1.0));
        }
    } else {
        let opaque = |part: &Part| part.opacity == Opacity::Opaque;
        // Opaque parts first, so the blended ones draw over them.
        let ordered = visible
            .clone()
            .filter(|(_, _, part)| opaque(part))
            .chain(visible.clone().filter(|(_, _, part)| !opaque(part)));
        for (triangles, id, part) in ordered {
            let start = mesh.vertices.len() as u32;
            push_triangles(
                &mut mesh.vertices,
                triangles,
                rgba(linear(part.color), part.opacity.alpha()),
            );
            if opaque(&part) {
                mesh.opaque = mesh.vertices.len() as u32;
            }
            mesh.parts.push(Shaded {
                id,
                range: start..mesh.vertices.len() as u32,
                opacity: part.opacity,
            });
        }
        // The closed volumes of the opaque parts, for the caps of a cut.
        for (_, id, part) in visible.clone().filter(|(_, _, part)| opaque(part)) {
            let volumes: Vec<Vec<Triangle>> = match id {
                PartId::Blades => {
                    let solids = blade_solids(g);
                    let count = if options.all_blades { solids.len() } else { 1 };
                    // Apart from each other: one volume.
                    vec![solids.into_iter().take(count).flatten().collect()]
                }
                PartId::Volute => volute.map(volute_solids).unwrap_or_default(),
                PartId::Duct => vec![duct.clone()],
                _ => Vec::new(),
            };
            let color = rgba(linear(part.color).map(|c| c * CAP_SHADE), 1.0);
            for volume in volumes.iter().filter(|v| is_closed(v)) {
                if mesh.caps.len() == MAX_CAPS {
                    break;
                }
                let start = mesh.closures.len() as u32;
                mesh.closures
                    .extend(volume.iter().flatten().map(|p| line_vertex(*p, color)));
                mesh.caps.push(Cap {
                    range: start..mesh.closures.len() as u32,
                    color,
                });
            }
        }
        if options.edges {
            for (triangles, _, part) in visible.clone() {
                let color = line_color(linear(part.color));
                for [a, b] in feature_edges(triangles) {
                    mesh.lines.push(line_vertex(a, color));
                    mesh.lines.push(line_vertex(b, color));
                }
            }
        }
    }
    if options.sections
        && options.volute.shown
        && let Some(v) = volute
    {
        let color = rgba(linear(options.volute.color).map(|c| c * 0.7), 1.0);
        let rings = v.rings();
        let last = rings.len().saturating_sub(1);
        // Every third section, and the last, so the casing still reads as a surface.
        for (_, ring) in rings
            .into_iter()
            .enumerate()
            .filter(|(i, _)| i % SECTION_STRIDE == 0 || *i == last)
        {
            let n = ring.len();
            for i in 0..n {
                mesh.lines.push(line_vertex(ring[i], color));
                mesh.lines.push(line_vertex(ring[(i + 1) % n], color));
            }
        }
    }

    // Bounds of the whole stage, whatever is shown, so the camera stays put.
    let all = parts
        .blades
        .iter()
        .flatten()
        .chain(&parts.hub)
        .chain(&parts.shroud)
        .chain(&diffuser.walls)
        .chain(&casing)
        .chain(&duct)
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
        let mesh = build(&g, None, None, None, &ViewOptions::default());
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
        let mesh = build(&g, None, None, None, &ViewOptions::default());
        assert!(mesh.opaque > 0 && (mesh.opaque as usize) < mesh.vertices.len());
        assert!(
            mesh.vertices[mesh.opaque as usize..]
                .iter()
                .all(|v| v.color[3] < 1.0)
        );
        let solid = build(
            &g,
            None,
            None,
            None,
            &ViewOptions {
                shroud: Part::solid(SHROUD),
                ..ViewOptions::default()
            },
        );
        assert_eq!(solid.opaque as usize, solid.vertices.len());
    }

    /// Translucent and glass parts blend after the opaque ones, each with its own
    /// opacity, in the color the scene gave it.
    #[test]
    fn opacity_levels_and_colors_reach_the_vertices() {
        let g = geometry();
        let options = ViewOptions {
            blades: Part {
                opacity: Opacity::Translucent,
                color: [255, 0, 0],
                ..Part::solid(BLADE)
            },
            ..ViewOptions::default()
        };
        let mesh = build(&g, None, None, None, &options);
        let blended = &mesh.vertices[mesh.opaque as usize..];
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(blended.iter().any(|v| near(v.color, [1.0, 0.0, 0.0, 0.5])));
        let alpha = |v: &Vertex, a: f32| (v.color[3] - a).abs() < 1e-6;
        assert!(
            blended.iter().any(|v| alpha(v, 0.2)),
            "the shroud stays glass"
        );
        assert!(
            mesh.vertices[..mesh.opaque as usize]
                .iter()
                .all(|v| alpha(v, 1.0))
        );
        let cycle: Vec<Opacity> = Opacity::ALL.iter().map(|o| o.next()).collect();
        assert_eq!(
            cycle,
            [Opacity::Translucent, Opacity::Glass, Opacity::Opaque]
        );
    }

    #[test]
    fn default_colors_survive_the_trip_to_linear() {
        for c in [BLADE, HUB, SHROUD, VANES, WALLS, VOLUTE] {
            let back = linear(c).map(|l| {
                let g = if l <= 0.003_130_8 {
                    12.92 * l
                } else {
                    1.055 * l.powf(1.0 / 2.4) - 0.055
                };
                (g * 255.0).round() as u8
            });
            assert_eq!(back, c);
        }
    }

    #[test]
    fn wireframe_has_lines_only() {
        let g = geometry();
        let mesh = build(
            &g,
            None,
            None,
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
    fn feature_edges_are_borders_and_creases() {
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        let flat = vec![
            [p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0)],
            [p(0.0, 0.0, 0.0), p(1.0, 1.0, 0.0), p(0.0, 1.0, 0.0)],
        ];
        // The shared diagonal is smooth: only the four borders remain.
        assert_eq!(feature_edges(&flat).len(), 4);
        let folded = vec![
            [p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0)],
            [p(0.0, 0.0, 0.0), p(1.0, 1.0, 0.0), p(0.0, 0.0, 1.0)],
        ];
        // Folded at 90°: the shared edge is a crease.
        assert_eq!(feature_edges(&folded).len(), 5);
    }

    #[test]
    fn parts_know_their_vertices() {
        let g = geometry();
        let mesh = build(&g, None, None, None, &ViewOptions::default());
        let ids: Vec<PartId> = mesh.parts.iter().map(|p| p.id).collect();
        assert_eq!(ids, [PartId::Blades, PartId::Hub, PartId::Shroud]);
        assert_eq!(mesh.parts[1].range.end, mesh.opaque);
        assert_eq!(mesh.parts[2].range.end as usize, mesh.vertices.len());
        assert!(mesh.parts.iter().all(|p| p.range.len().is_multiple_of(3)));
    }

    #[test]
    fn opaque_closed_volumes_are_capped() {
        let project = default_project();
        let mut derived = Derived::default();
        derived.update(&project);
        let g = geometry();
        let v = project
            .components()
            .iter()
            .find_map(|c| derived.volute(c.id()))
            .unwrap();
        // The casing is glass by default: only the blades are capped.
        let glass = build(&g, None, None, Some(v), &ViewOptions::default());
        assert_eq!(glass.caps.len(), 1);
        let mut options = ViewOptions::default();
        options.volute.opacity = Opacity::Opaque;
        let opaque = build(&g, None, None, Some(v), &options);
        // One cap for each channel: they overlap where the spiral meets the diffuser.
        assert_eq!(opaque.caps.len(), 1 + v.channels().len());
        for cap in &opaque.caps {
            let triangles: Vec<Triangle> = opaque.closures
                [cap.range.start as usize..cap.range.end as usize]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| t.map(|v| Vec3::from(v.position).as_dvec3()))
                .collect();
            assert!(is_closed(&triangles));
        }
        options.wireframe = true;
        assert!(build(&g, None, None, Some(v), &options).caps.is_empty());
    }

    #[test]
    fn an_inlet_duct_is_a_part_of_its_own() {
        use tp_design::impeller::duct::{self, DuctInput};
        let mut g = geometry();
        let without = build(&g, None, None, None, &ViewOptions::default());
        assert!(without.parts.iter().all(|p| p.id != PartId::Duct));
        let mut input = tp_design::impeller::ImpellerGeometryInput::default();
        input.meridional.chambers.enabled = true;
        g = tp_design::impeller::build(&g.basis, &input).unwrap();
        let built = duct::build(&g.contour, &g.basis, &DuctInput::default());
        let with = build(&g, Some(&built), None, None, &ViewOptions::default());
        let part = with.parts.iter().find(|p| p.id == PartId::Duct).unwrap();
        assert_eq!(part.opacity, Opacity::Glass);
        // The duct reaches upstream of the eye: the bounds grow that way.
        assert!(with.radius > without.radius);
    }

    #[test]
    fn hidden_parts_draw_nothing() {
        let g = geometry();
        let hidden = Part {
            shown: false,
            ..Part::solid(HUB)
        };
        let mesh = build(
            &g,
            None,
            None,
            None,
            &ViewOptions {
                blades: hidden,
                hub: hidden,
                shroud: hidden,
                ..ViewOptions::default()
            },
        );
        assert!(mesh.vertices.is_empty() && mesh.lines.is_empty());
        assert!(mesh.radius > 0.0, "the bounds stay for the camera");
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
