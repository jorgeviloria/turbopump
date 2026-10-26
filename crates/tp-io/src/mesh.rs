//! Triangle meshes of the impeller for visual checks.
//!
//! The mesh is made of separate surfaces (blade faces, hub and shroud walls) and is not
//! a closed solid; the CAD solids come from the CAD kernel in a later phase.

use std::f64::consts::PI;

use tp_design::diffuser::DiffuserDesign;
use tp_design::impeller::ImpellerGeometry;
use tp_design::volute::{RibPlane, VoluteGeometry};
use tp_math::{DVec2, DVec3};

pub type Triangle = [DVec3; 3];

/// Segments around the circumference of the hub and shroud walls.
const WALL_SEGMENTS: usize = 96;
/// Points along the hub and shroud contours.
const WALL_POINTS: usize = 60;

fn rotate_z(p: DVec3, angle: f64) -> DVec3 {
    let (s, c) = angle.sin_cos();
    DVec3::new(c * p.x - s * p.y, s * p.x + c * p.y, p.z)
}

/// Two triangles for the quad a-b-c-d, skipping degenerate ones.
fn quad(out: &mut Vec<Triangle>, a: DVec3, b: DVec3, c: DVec3, d: DVec3) {
    for t in [[a, b, c], [a, c, d]] {
        if (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-24 {
            out.push(t);
        }
    }
}

/// Surface through a grid of points `rows[j][k]`.
fn grid(out: &mut Vec<Triangle>, rows: &[Vec<DVec3>]) {
    for j in 0..rows.len() - 1 {
        for k in 0..rows[j].len() - 1 {
            quad(
                out,
                rows[j][k],
                rows[j][k + 1],
                rows[j + 1][k + 1],
                rows[j + 1][k],
            );
        }
    }
}

/// Surface of revolution of a meridional curve (z, r).
fn revolve(out: &mut Vec<Triangle>, curve: &[DVec2]) {
    let rows: Vec<Vec<DVec3>> = (0..=WALL_SEGMENTS)
        .map(|i| {
            let phi = 2.0 * PI * i as f64 / WALL_SEGMENTS as f64;
            curve
                .iter()
                .map(|p| DVec3::new(p.y * phi.cos(), p.y * phi.sin(), p.x))
                .collect()
        })
        .collect();
    grid(out, &rows);
}

/// Triangles of one blade: pressure and suction faces, closed at both edges.
pub fn blade_triangles(g: &ImpellerGeometry) -> Vec<Triangle> {
    let sign = g.blade.rotation_sign;
    let ps: Vec<Vec<DVec3>> = g
        .blade
        .sections
        .iter()
        .map(|s| s.pressure_side(sign))
        .collect();
    let ss: Vec<Vec<DVec3>> = g
        .blade
        .sections
        .iter()
        .map(|s| s.suction_side(sign))
        .collect();
    let mut out = Vec::new();
    grid(&mut out, &ps);
    grid(&mut out, &ss);
    for edge in [0, ps[0].len() - 1] {
        for j in 0..ps.len() - 1 {
            quad(
                &mut out,
                ps[j][edge],
                ps[j + 1][edge],
                ss[j + 1][edge],
                ss[j][edge],
            );
        }
    }
    out
}

/// Every blade as a closed solid: its faces and edges, closed also at the hub and at
/// the shroud, where it stands on the walls. For viewers that need closed volumes, such
/// as the caps of a cut; the ends lie on the walls and are not drawn.
pub fn blade_solids(g: &ImpellerGeometry) -> Vec<Vec<Triangle>> {
    let sign = g.blade.rotation_sign;
    let mut one = blade_triangles(g);
    for s in [g.blade.sections.first(), g.blade.sections.last()]
        .into_iter()
        .flatten()
    {
        let (ps, ss) = (s.pressure_side(sign), s.suction_side(sign));
        for k in 0..ps.len().min(ss.len()).saturating_sub(1) {
            quad(&mut one, ps[k], ps[k + 1], ss[k + 1], ss[k]);
        }
    }
    let z = g.blade.count;
    (0..z)
        .map(|i| {
            let angle = 2.0 * PI * f64::from(i) / f64::from(z);
            one.iter().map(|t| t.map(|p| rotate_z(p, angle))).collect()
        })
        .collect()
}

/// The impeller mesh split by part, for viewers that color or hide each one.
#[derive(Debug, Clone, Default)]
pub struct ImpellerMesh {
    /// One mesh per blade, in angular order.
    pub blades: Vec<Vec<Triangle>>,
    /// Hub wall (surface of revolution of the hub contour).
    pub hub: Vec<Triangle>,
    /// Shroud wall.
    pub shroud: Vec<Triangle>,
}

/// Every blade and the hub and shroud walls, as separate meshes.
pub fn impeller_parts(g: &ImpellerGeometry) -> ImpellerMesh {
    let one = blade_triangles(g);
    let z = g.blade.count;
    let blades = (0..z)
        .map(|i| {
            let angle = 2.0 * PI * f64::from(i) / f64::from(z);
            one.iter().map(|t| t.map(|p| rotate_z(p, angle))).collect()
        })
        .collect();
    let mut hub = Vec::new();
    revolve(&mut hub, &g.contour.hub.sample(WALL_POINTS));
    let mut shroud = Vec::new();
    revolve(&mut shroud, &g.contour.shroud.sample(WALL_POINTS));
    ImpellerMesh {
        blades,
        hub,
        shroud,
    }
}

/// All blades plus the hub and shroud walls.
pub fn impeller_triangles(g: &ImpellerGeometry) -> Vec<Triangle> {
    let parts = impeller_parts(g);
    let mut out: Vec<Triangle> = parts.blades.into_iter().flatten().collect();
    out.extend(parts.hub);
    out.extend(parts.shroud);
    out
}

/// Surface between consecutive closed rings with the same number of points.
fn loft_rings(out: &mut Vec<Triangle>, rings: &[Vec<DVec3>]) {
    for pair in rings.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let n = a.len().min(b.len());
        for i in 0..n {
            let j = (i + 1) % n;
            quad(out, a[i], a[j], b[j], b[i]);
        }
    }
}

/// Flat cap over a closed ring, fanned from its mean point.
fn cap(out: &mut Vec<Triangle>, ring: &[DVec3]) {
    let n = ring.len();
    let centre = ring.iter().copied().sum::<DVec3>() / n as f64;
    for i in 0..n {
        let t = [centre, ring[i], ring[(i + 1) % n]];
        if (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-24 {
            out.push(t);
        }
    }
}

/// Inner surface of the volute: spiral channel and discharge diffuser, closed at the
/// start of the spiral and at the outlet. A double volute gives one closed surface for
/// each channel and one for the diffuser.
pub fn volute_triangles(g: &VoluteGeometry) -> Vec<Triangle> {
    let mut out = Vec::new();
    // The walls of the bend and neck of an internal volute, all round.
    if let Some(bend) = &g.bend {
        let rz = g.design.stagnation_radius;
        let zm = g.basis.midplane;
        for wall in [&bend.outer, &bend.inner] {
            let mut curve: Vec<DVec2> = wall
                .iter()
                .flatten()
                .map(|p| DVec2::new(zm + p.x, rz + p.y))
                .collect();
            // The pieces share their ends.
            curve.dedup_by(|a, b| a.distance(*b) < 1e-12);
            revolve(&mut out, &curve);
        }
    }
    let channels = g.channels();
    let count = channels.len();
    for (i, rings) in channels.into_iter().enumerate() {
        loft_rings(&mut out, &rings);
        // The other channels end inside the diffuser: only the outlet is closed.
        if let Some(first) = rings.first() {
            cap(&mut out, first);
        }
        if i + 1 == count
            && let Some(last) = rings.last()
        {
            cap(&mut out, last);
        }
    }
    out
}

/// The body of a rounded cutwater as a closed surface: lofted through its outlines
/// across the inlet width ([`VoluteGeometry::cutwater_body`]) and closed at the
/// outermost planes. Empty unless rounded.
pub fn cutwater_triangles(g: &VoluteGeometry) -> Vec<Triangle> {
    let body = g.cutwater_body();
    let rings: Vec<&Vec<DVec3>> = body.iter().collect();
    let mut out = Vec::new();
    let Some(&first) = rings.first() else {
        return out;
    };
    let n = first.len();
    if rings.len() < 2 || rings.iter().any(|r| r.len() != n) {
        return out;
    }
    // Outward normals: the planes run from −z to +z, so the loft faces out when the
    // outlines run counter-clockwise, and the ends face −z and +z.
    let counter_clockwise = (0..n)
        .map(|i| first[i].truncate().perp_dot(first[(i + 1) % n].truncate()))
        .sum::<f64>()
        > 0.0;
    for pair in rings.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        for i in 0..n {
            let j = (i + 1) % n;
            if counter_clockwise {
                quad(&mut out, a[i], a[j], b[j], b[i]);
            } else {
                quad(&mut out, b[i], b[j], a[j], a[i]);
            }
        }
    }
    // Each end as a strip between the two faces: the outline runs back along one face
    // and forward along the other.
    for (ring, up) in [(first, false), (rings[rings.len() - 1], true)] {
        let mut end = Vec::new();
        for i in 0..(n - 1) / 2 {
            quad(
                &mut end,
                ring[i],
                ring[i + 1],
                ring[n - 2 - i],
                ring[n - 1 - i],
            );
        }
        out.extend(end.into_iter().map(|t| {
            let facing_up = (t[1] - t[0]).cross(t[2] - t[0]).z > 0.0;
            if facing_up == up {
                t
            } else {
                [t[0], t[2], t[1]]
            }
        }));
    }
    out
}

/// The rib of a double volute as a closed surface: lofted through its planes across
/// the height of the sections and closed at the outermost planes, with the outward
/// normals. Empty for a single volute.
pub fn rib_triangles(g: &VoluteGeometry) -> Vec<Triangle> {
    let Some(rib) = &g.rib else {
        return Vec::new();
    };
    if rib.planes.is_empty() {
        return Vec::new();
    }
    // The rib runs the full height of the tallest section, but the channel is lower
    // toward the rib nose: every point is kept inside the channel, so that the volute
    // trims the rib and it does not stick out of it. The planes above or below the
    // channel collapse onto its top and bottom and add no surface.
    let sign = g.basis.rotation_sign;
    let trim = |p: &DVec3| -> DVec3 {
        let angle = (sign * (p.y.atan2(p.x) - g.rotation)).rem_euclid(std::f64::consts::TAU);
        match g.envelope_z_range(angle, p.truncate().length()) {
            Some((lo, hi)) => DVec3::new(p.x, p.y, p.z.clamp(lo, hi)),
            None => *p,
        }
    };
    let trimmed: Vec<RibPlane> = rib
        .planes
        .iter()
        .map(|p| RibPlane {
            z: p.z,
            inner_face: p.inner_face.iter().map(trim).collect(),
            outer_face: p.outer_face.iter().map(trim).collect(),
            nose: p.nose.iter().map(trim).collect(),
            tail: p.tail.iter().map(trim).collect(),
        })
        .collect();
    let (first, last) = (&trimmed[0], &trimmed[trimmed.len() - 1]);
    let rings: Vec<Vec<DVec3>> = trimmed.iter().map(|p| p.outline()).collect();
    let n = rings[0].len();
    // Outward when the outlines run counter-clockwise, as the planes run up.
    let counter_clockwise = (0..n)
        .map(|i| {
            rings[0][i]
                .truncate()
                .perp_dot(rings[0][(i + 1) % n].truncate())
        })
        .sum::<f64>()
        > 0.0;
    let mut out = Vec::new();
    for pair in rings.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        for i in 0..n {
            let j = (i + 1) % n;
            if counter_clockwise {
                quad(&mut out, a[i], a[j], b[j], b[i]);
            } else {
                quad(&mut out, b[i], b[j], a[j], a[i]);
            }
        }
    }
    // Each end: the band between the faces, and the nose and the tail fanned from
    // their middles.
    for (plane, up) in [(first, false), (last, true)] {
        let mut end = Vec::new();
        for (i, o) in plane.inner_face.windows(2).zip(plane.outer_face.windows(2)) {
            quad(&mut end, i[0], i[1], o[1], o[0]);
        }
        cap(&mut end, &plane.nose);
        cap(&mut end, &plane.tail);
        out.extend(end.into_iter().map(|t| {
            let facing_up = (t[1] - t[0]).cross(t[2] - t[0]).z > 0.0;
            if facing_up == up {
                t
            } else {
                [t[0], t[2], t[1]]
            }
        }));
    }
    out
}

/// The inner surface of the volute for viewing: as [`volute_triangles`], without the
/// pieces of wall that lie inside another channel or inside the body of the cutwater
/// near it, so that the cutwater shows as it is built. Not a closed surface.
pub fn volute_view_triangles(g: &VoluteGeometry) -> Vec<Triangle> {
    let all = volute_triangles(g);
    let slices = &g.cutwater_slices;
    let channels = g.channels();
    if slices.is_empty() || channels.len() < 2 || g.bend.is_some() {
        return all;
    }
    // Near the cutwater only: within twice the reach of its body from the tip.
    let reach = slices
        .iter()
        .flat_map(|s| s.outline.iter().map(move |p| p.distance(s.tip)))
        .fold(0.01f64, f64::max);
    let near = |p: DVec3| slices.iter().any(|s| p.distance(s.tip) < 2.0 * reach);
    let plane = |z: f64| {
        slices
            .iter()
            .min_by(|a, b| (a.z - z).abs().total_cmp(&(b.z - z).abs()))
            .expect("some planes")
    };
    // The body between the planes too, as the CAD lofts it.
    let fine = g.cutwater_body();
    let body: Vec<Vec<DVec2>> = fine
        .iter()
        .map(|o| o.iter().map(|q| q.truncate()).collect())
        .collect();
    let levels: Vec<f64> = fine.iter().map(|o| o[0].z).collect();
    let outline = |z: f64| {
        levels
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - z).abs().total_cmp(&(b.1 - z).abs()))
            .map(|(k, _)| &body[k])
    };
    // The channels again, each with its own triangles, the diffuser last.
    let mut out = Vec::new();
    let count = channels.len();
    for (i, rings) in channels.into_iter().enumerate() {
        let diffuser = i + 1 == count;
        // The last ring of a spiral reaches into the diffuser so that the solids overlap
        // for the CAD kernel; seen through the casing, its walls inside the diffuser
        // show as a sliver at the neck.
        let shown = if diffuser {
            &rings[..]
        } else {
            &rings[..rings.len().saturating_sub(1)]
        };
        let mut own = Vec::new();
        loft_rings(&mut own, shown);
        if let Some(first) = shown.first() {
            cap(&mut own, first);
        }
        if diffuser && let Some(last) = shown.last() {
            cap(&mut own, last);
        }
        // Only triangles wholly inside go, so that no hole opens along the edge: what
        // is left pokes a little into the opaque body or into the other channel.
        out.extend(own.into_iter().filter(|t| {
            let c = (t[0] + t[1] + t[2]) / 3.0;
            if !near(c) {
                return true;
            }
            let s = plane(c.z);
            if outline(c.z).is_some_and(|body| t.iter().all(|q| inside(body, q.truncate()))) {
                return false;
            }
            // A spiral wall inside the diffuser, or a diffuser wall inside the spiral.
            let (other, toward_axis) = if diffuser {
                (&s.spiral_wall, true)
            } else {
                (&s.diffuser_wall, false)
            };
            let other: Vec<DVec2> = other.iter().map(|q| q.truncate()).collect();
            !t.iter().all(|q| beside(&other, q.truncate(), toward_axis))
        }));
    }
    out
}

/// Whether `p` lies inside the closed polygon `outline`.
fn inside(outline: &[DVec2], p: DVec2) -> bool {
    let n = outline.len();
    let mut odd = false;
    for i in 0..n {
        let (a, b) = (outline[i], outline[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            odd = !odd;
        }
    }
    odd
}

/// Whether `p` lies beside the wall `wall`, within its length, on the side toward the
/// pump axis (`toward_axis`) or away from it.
fn beside(wall: &[DVec2], p: DVec2, toward_axis: bool) -> bool {
    let Some((k, t, q)) = (0..wall.len().saturating_sub(1))
        .map(|k| {
            let (a, b) = (wall[k], wall[k + 1]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-30)).clamp(0.0, 1.0);
            (k, t, a + ab * t)
        })
        .min_by(|x, y| x.2.distance(p).total_cmp(&y.2.distance(p)))
    else {
        return false;
    };
    // Past either end of the wall there is nothing to be beside.
    if (k == 0 && t <= 0.0) || (k + 2 == wall.len() && t >= 1.0) {
        return false;
    }
    let along = wall[k + 1] - wall[k];
    let mut out = DVec2::new(along.y, -along.x);
    if out.dot(q) < 0.0 {
        out = -out;
    }
    let away = (p - q).dot(out) > 0.0;
    away != toward_axis
}

/// Each channel of the volute as a closed solid, capped at both ends. The channels
/// overlap where the spiral runs on into the discharge diffuser, so each one closes a
/// volume of its own; the walls of the bend of an internal volute are left out.
pub fn volute_solids(g: &VoluteGeometry) -> Vec<Vec<Triangle>> {
    g.channels()
        .into_iter()
        .filter(|rings| rings.len() > 1)
        .map(|rings| {
            let mut out = Vec::new();
            loft_rings(&mut out, &rings);
            for ring in [&rings[0], &rings[rings.len() - 1]] {
                cap(&mut out, ring);
            }
            out
        })
        .collect()
}

/// A radial diffuser split by part: the two side walls and the vanes.
#[derive(Debug, Clone, Default)]
pub struct DiffuserMesh {
    pub walls: Vec<Triangle>,
    pub vanes: Vec<Triangle>,
}

/// Side walls from the impeller outlet to d₄ and, for a vaned diffuser, the vanes
/// standing between them.
pub fn diffuser_parts(d: &DiffuserDesign) -> DiffuserMesh {
    let r2 = d.impeller_diameter / 2.0;
    let r4 = d.outlet_diameter / 2.0;
    let zm = d.midplane;
    let radii: Vec<f64> = (0..=WALL_POINTS)
        .map(|i| r2 + (r4 - r2) * i as f64 / WALL_POINTS as f64)
        .collect();
    let mut walls = Vec::new();
    for side in [-1.0, 1.0] {
        let curve: Vec<DVec2> = radii
            .iter()
            .map(|&r| DVec2::new(zm + side * d.half_width(r), r))
            .collect();
        revolve(&mut walls, &curve);
    }
    let mut vanes = Vec::new();
    if let Some(v) = &d.vanes {
        for k in 0..v.count {
            let outline = v.geometry.outline(k, d.rotation_sign);
            let n = outline.len();
            for i in 0..n {
                let (a, b) = (outline[i], outline[(i + 1) % n]);
                let at = |p: DVec2, side: f64| {
                    DVec3::new(p.x, p.y, zm + side * d.half_width(p.length()))
                };
                quad(&mut vanes, at(a, -1.0), at(b, -1.0), at(b, 1.0), at(a, 1.0));
            }
        }
    }
    DiffuserMesh { walls, vanes }
}

/// The vanes of a vaned diffuser as closed solids: their sides, closed also where they
/// stand on the walls, by a fan over each end. For viewers that need closed volumes,
/// such as the caps of a cut; the ends lie on the walls and are not drawn. A fan over a
/// vane outline that is not convex covers some points more than once, but always an
/// odd number of times inside and an even number outside, which is all a parity count
/// needs. Empty for a vaneless diffuser.
pub fn vane_solids(d: &DiffuserDesign) -> Vec<Triangle> {
    let mut out = diffuser_parts(d).vanes;
    let Some(v) = &d.vanes else {
        return out;
    };
    let zm = d.midplane;
    for k in 0..v.count {
        let outline = v.geometry.outline(k, d.rotation_sign);
        for side in [-1.0, 1.0] {
            let ring: Vec<DVec3> = outline
                .iter()
                .map(|p| DVec3::new(p.x, p.y, zm + side * d.half_width(p.length())))
                .collect();
            cap(&mut out, &ring);
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::f64::consts::PI;
    use tp_design::impeller::{self, ImpellerBasis, ImpellerGeometryInput};
    use tp_design::pump::{CentrifugalPumpInput, PumpDuty, design};

    pub(crate) fn reference() -> ImpellerGeometry {
        let duty = PumpDuty {
            flow: 100.0 / 3600.0,
            head: 32.0,
            machine_head: 32.0,
            stages: 1,
            speed: 2900.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
            kinematic_viscosity: 1.0e-6,
        };
        let input = CentrifugalPumpInput::default();
        let main = design(&duty, &input).unwrap();
        let basis = ImpellerBasis::from_pump(&main, &input, 1.0);
        impeller::build(&basis, &ImpellerGeometryInput::default(), 1.0).unwrap()
    }

    #[test]
    fn mesh_lies_inside_the_impeller_envelope() {
        let g = reference();
        let r2 = g.basis.outlet_diameter / 2.0;
        let tris = impeller_triangles(&g);
        assert!(tris.len() > 1000);
        for t in &tris {
            for p in t {
                assert!(p.truncate().length() <= r2 * (1.0 + 1e-9));
                assert!(p.z >= -1e-12 && p.z <= g.contour.axial_length + 1e-12);
            }
        }
    }

    #[test]
    fn blades_are_rotated_copies() {
        let g = reference();
        let one = blade_triangles(&g).len();
        let all = impeller_triangles(&g).len();
        assert!(all >= one * g.blade.count as usize);
    }

    /// Whether every edge of `triangles` is shared by an even number of them.
    fn closed(triangles: &[Triangle]) -> bool {
        let key = |p: DVec3| [p.x, p.y, p.z].map(|v| (v * 1e7).round() as i64);
        let mut count = std::collections::HashMap::new();
        for t in triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (ka, kb) = (key(a), key(b));
                if ka != kb {
                    *count
                        .entry(ka.min(kb))
                        .or_insert_with(std::collections::HashMap::new)
                        .entry(ka.max(kb))
                        .or_insert(0u32) += 1;
                }
            }
        }
        count.values().flat_map(|m| m.values()).all(|n| n % 2 == 0)
    }

    #[test]
    fn blade_and_volute_solids_are_closed() {
        use tp_design::outlet::ImpellerOutlet;
        use tp_design::volute::{CrossSectionInput, InternalRound, SectionShape, VoluteInput};
        let g = reference();
        let blades = blade_solids(&g);
        assert_eq!(blades.len(), g.blade.count as usize);
        assert!(!closed(&blade_triangles(&g)), "open at the walls");
        assert!(blades.iter().all(|b| closed(b)));
        let basis = ImpellerOutlet {
            flow: 100.0 / 3600.0,
            impeller_diameter: 0.172,
            impeller_width: 0.0174,
            cu: 13.27,
            cm: 3.03,
            nq: 35.9,
            stage_head: 32.0,
            density: 998.2,
            midplane: 0.0378,
            rotation_sign: 1.0,
            impeller_blades: 7,
            sides: None,
        };
        let internal = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::RoundInternal(
                InternalRound::default(),
            )),
            ..VoluteInput::default()
        };
        let mut double = VoluteInput::default();
        double.setup.kind = tp_design::volute::VoluteKind::Double;
        for input in [VoluteInput::default(), internal, double] {
            let v = VoluteGeometry::build(&basis, &input).unwrap();
            let solids = volute_solids(&v);
            assert_eq!(solids.len(), v.channels().len());
            assert!(solids.iter().all(|s| closed(s)), "{:?}", input.setup.kind);
        }
    }

    fn reference_volute(edit: impl Fn(&mut tp_design::volute::VoluteInput)) -> VoluteGeometry {
        use tp_design::outlet::ImpellerOutlet;
        let basis = ImpellerOutlet {
            flow: 100.0 / 3600.0,
            impeller_diameter: 0.172,
            impeller_width: 0.0174,
            cu: 13.27,
            cm: 3.03,
            nq: 35.9,
            stage_head: 32.0,
            density: 998.2,
            midplane: 0.0378,
            rotation_sign: 1.0,
            impeller_blades: 7,
            sides: None,
        };
        let mut input = tp_design::volute::VoluteInput::default();
        edit(&mut input);
        VoluteGeometry::build(&basis, &input).unwrap()
    }

    #[test]
    fn the_cutwater_body_is_a_closed_surface_as_thick_as_its_nose() {
        use tp_design::volute::{CrossSectionInput, CutwaterShape, SectionShape};
        for sections in [
            CrossSectionInput::default(),
            CrossSectionInput::single(SectionShape::round()),
        ] {
            let volume = |triangles: &[Triangle]| {
                triangles
                    .iter()
                    .map(|t| t[0].dot(t[1].cross(t[2])))
                    .sum::<f64>()
                    .abs()
                    / 6.0
            };
            // With its side endings the body is closed and a little larger; round
            // sections have none by default.
            let ended = reference_volute(|v| v.cross_section = sections.clone());
            let with_endings = cutwater_triangles(&ended);
            assert!(closed(&with_endings));
            let g = reference_volute(|v| {
                v.cross_section = sections.clone();
                v.cutwater.side_radius = tp_design::Param::Manual(0.0);
            });
            let body = cutwater_triangles(&g);
            assert!(!body.is_empty());
            assert!(closed(&body));
            if ended.design.cutwater_side_radius > 0.0 {
                assert!(volume(&with_endings) > volume(&body));
            } else {
                assert!((volume(&with_endings) - volume(&body)).abs() < 1e-15);
            }
            // Without them, about the area of the midplane outline times the width of
            // the planes.
            let volume = volume(&body);
            let mid = &g.cutwater_slices[g.cutwater_slices.len() / 2];
            let n = mid.outline.len();
            let area = 0.5
                * (0..n)
                    .map(|i| {
                        mid.outline[i]
                            .truncate()
                            .perp_dot(mid.outline[(i + 1) % n].truncate())
                    })
                    .sum::<f64>()
                    .abs();
            let first = g.cutwater_slices.first().unwrap().z;
            let last = g.cutwater_slices.last().unwrap().z;
            let estimate = area * (last - first);
            assert!(
                (volume / estimate - 1.0).abs() < 0.25,
                "{volume} vs {estimate}"
            );
        }
        let sharp = reference_volute(|v| v.cutwater.shape = CutwaterShape::Sharp);
        assert!(cutwater_triangles(&sharp).is_empty());
    }

    #[test]
    fn the_rib_is_a_closed_surface_across_the_height_of_the_sections() {
        let g = reference_volute(|v| v.setup.kind = tp_design::volute::VoluteKind::Double);
        let mesh = rib_triangles(&g);
        assert!(closed(&mesh), "open edges");
        // Its volume: the area of its outline times its height.
        let rib = g.rib.as_ref().unwrap();
        let outline = rib.outline();
        let area = 0.5
            * (0..outline.len())
                .map(|i| {
                    outline[i]
                        .truncate()
                        .perp_dot(outline[(i + 1) % outline.len()].truncate())
                })
                .sum::<f64>()
                .abs();
        let height = rib.planes[rib.planes.len() - 1].z - rib.planes[0].z;
        // Positive: the normals point out. The volute trims it: it is the part of that
        // prism that lies inside the channel, which is lower toward the nose.
        let volume = mesh.iter().map(|t| t[0].dot(t[1].cross(t[2]))).sum::<f64>() / 6.0;
        let share = volume / (area * height);
        assert!(share > 0.4 && share < 0.9, "{volume} of {}", area * height);
        assert!(rib_triangles(&reference_volute(|_| {})).is_empty());
    }

    #[test]
    fn the_view_of_the_volute_leaves_out_the_walls_the_cutwater_replaces() {
        let g = reference_volute(|_| {});
        let all = volute_triangles(&g);
        let view = volute_view_triangles(&g);
        assert!(view.len() < all.len());
        // Only near the cutwater: most of the volute stays.
        assert!(
            view.len() as f64 > 0.9 * all.len() as f64,
            "{} of {}",
            view.len(),
            all.len()
        );
        // No triangle of the walls is left wholly inside the body in the midplane.
        let mid = &g.cutwater_slices[g.cutwater_slices.len() / 2];
        let body: Vec<DVec2> = mid.outline.iter().map(|p| p.truncate()).collect();
        for t in &view {
            let c = (t[0] + t[1] + t[2]) / 3.0;
            if (c.z - mid.z).abs() < 1e-3 {
                assert!(!t.iter().all(|q| inside(&body, q.truncate())), "{c}");
            }
        }
        // A tangent cutwater is drawn as it is.
        let tangent =
            reference_volute(|v| v.cutwater.shape = tp_design::volute::CutwaterShape::Tangent);
        assert_eq!(
            volute_view_triangles(&tangent).len(),
            volute_triangles(&tangent).len()
        );
    }

    #[test]
    fn the_rib_of_a_double_volute_stays_inside_the_channel() {
        let g = reference_volute(|v| v.setup.kind = tp_design::volute::VoluteKind::Double);
        let rib = g.rib.as_ref().unwrap();
        // The planes run the full height of the tallest section: above the channel at the
        // rib nose, where the sections are small.
        let top = rib.planes.last().unwrap().z;
        let nose = rib.nose_tip;
        let nose_angle = (g.basis.rotation_sign * (nose.y.atan2(nose.x) - g.rotation))
            .rem_euclid(std::f64::consts::TAU);
        let (_, channel_top) = g
            .envelope_z_range(nose_angle, nose.truncate().length())
            .unwrap();
        assert!(top > channel_top + 6e-3, "{top} vs {channel_top}");
        // Every vertex of the mesh is inside the channel there, within rounding.
        let tris = rib_triangles(&g);
        assert!(!tris.is_empty());
        for t in &tris {
            for p in t {
                let angle = (g.basis.rotation_sign * (p.y.atan2(p.x) - g.rotation))
                    .rem_euclid(std::f64::consts::TAU);
                if let Some((lo, hi)) = g.envelope_z_range(angle, p.truncate().length()) {
                    // The centres of the fans that close the nose and the tail are averages
                    // of trimmed points and may lie a little off: within 2.5 mm.
                    assert!(
                        p.z >= lo - 2.5e-3 && p.z <= hi + 2.5e-3,
                        "z {} outside {lo}..{hi} at {angle}",
                        p.z
                    );
                }
            }
        }
        // And it still reaches the top of the channel at the throat, where it is full
        // height: the rib is a band, not a sliver.
        let highest = tris
            .iter()
            .flat_map(|t| t.iter().map(|p| p.z))
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(highest > channel_top + 1e-3, "{highest} vs {channel_top}");
    }

    #[test]
    fn the_view_leaves_out_the_end_of_the_spiral_that_reaches_into_the_diffuser() {
        let g = reference_volute(|_| {});
        let tail: Vec<DVec3> = g.channels()[0].last().unwrap().clone();
        // The walls of that last piece lie inside the diffuser: seen through the casing
        // they are a sliver at the neck.
        assert!(
            volute_triangles(&g)
                .iter()
                .any(|t| t.iter().any(|q| tail.contains(q)))
        );
        assert!(
            !volute_view_triangles(&g)
                .iter()
                .any(|t| t.iter().any(|q| tail.contains(q)))
        );
    }

    #[test]
    fn volute_mesh_is_closed_and_encloses_the_channel() {
        use tp_design::outlet::ImpellerOutlet;
        use tp_design::volute::{CrossSectionInput, InternalRound, SectionShape, VoluteInput};
        let basis = ImpellerOutlet {
            flow: 100.0 / 3600.0,
            impeller_diameter: 0.172,
            impeller_width: 0.0174,
            cu: 13.27,
            cm: 3.03,
            nq: 35.9,
            stage_head: 32.0,
            density: 998.2,
            midplane: 0.0378,
            rotation_sign: 1.0,
            impeller_blades: 7,
            sides: None,
        };
        let g = VoluteGeometry::build(&basis, &VoluteInput::default()).unwrap();
        let tris = volute_triangles(&g);
        // Divergence theorem: a closed mesh gives the enclosed volume.
        let volume: f64 = tris.iter().map(|t| t[0].dot(t[1].cross(t[2]))).sum::<f64>() / 6.0;
        let spiral: f64 = g
            .sections
            .windows(2)
            .map(|w| {
                0.5 * (w[0].size.area + w[1].size.area)
                    * 0.5
                    * (w[0].size.centroid_radius + w[1].size.centroid_radius)
                    * (w[1].angle - w[0].angle)
            })
            .sum();
        assert!(volume.abs() > spiral, "{volume} vs spiral {spiral}");
        assert!(volume.abs() < 1.6 * spiral);
        // An internal volute adds the walls of its bend, all round.
        let internal = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::RoundInternal(
                InternalRound::default(),
            )),
            ..VoluteInput::default()
        };
        let g = VoluteGeometry::build(&basis, &internal).unwrap();
        let channel = {
            let mut bare = g.clone();
            bare.bend = None;
            volute_triangles(&bare).len()
        };
        assert!(volute_triangles(&g).len() > channel + 2 * WALL_SEGMENTS);
        let text = crate::points::volute_txt(&g);
        assert!(text.contains("[bend outer wall"), "{text}");
        assert!(text.contains("[bend inner wall"));
    }

    #[test]
    fn diffuser_mesh_has_walls_and_vanes() {
        use tp_design::diffuser::DiffuserInput;
        use tp_design::outlet::ImpellerOutlet;
        let outlet = ImpellerOutlet {
            flow: 100.0 / 3600.0,
            impeller_diameter: 0.172,
            impeller_width: 0.0174,
            cu: 13.27,
            cm: 3.03,
            nq: 35.9,
            stage_head: 32.0,
            density: 998.2,
            midplane: 0.0378,
            rotation_sign: 1.0,
            impeller_blades: 7,
            sides: None,
        };
        let vaned = DiffuserDesign::new(&outlet, &DiffuserInput::default(), true).unwrap();
        let mesh = diffuser_parts(&vaned);
        assert!(!mesh.walls.is_empty() && !mesh.vanes.is_empty());
        // The blunt trailing edge reaches past d₄ by part of its thickness.
        let reach = vaned.outlet_diameter / 2.0 + vaned.vanes.as_ref().unwrap().trailing_thickness;
        for t in mesh.walls.iter().chain(&mesh.vanes) {
            for p in t {
                assert!(p.truncate().length() <= reach);
                assert!((p.z - 0.0378).abs() <= vaned.half_width(0.1) * 1.01 + 1e-9);
            }
        }
        let vaneless = DiffuserDesign::new(&outlet, &DiffuserInput::default(), false).unwrap();
        assert!(diffuser_parts(&vaneless).vanes.is_empty());
        // Closed at the walls, the vanes are solids: every edge shared by an even
        // number of triangles.
        assert!(!closed(&mesh.vanes));
        assert!(closed(&vane_solids(&vaned)));
        assert!(vane_solids(&vaneless).is_empty());
    }

    #[test]
    fn parts_add_up_to_the_whole_mesh() {
        let g = reference();
        let parts = impeller_parts(&g);
        assert_eq!(parts.blades.len(), g.blade.count as usize);
        assert!(!parts.hub.is_empty() && !parts.shroud.is_empty());
        let count =
            parts.blades.iter().map(Vec::len).sum::<usize>() + parts.hub.len() + parts.shroud.len();
        assert_eq!(count, impeller_triangles(&g).len());
        // The second blade is the first one turned by 2π/z about the axis.
        let angle = 2.0 * PI / f64::from(g.blade.count);
        let p = parts.blades[0][0][0];
        let q = parts.blades[1][0][0];
        assert!((rotate_z(p, angle) - q).length() < 1e-12);
    }
}
