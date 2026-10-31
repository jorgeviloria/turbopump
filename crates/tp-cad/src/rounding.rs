//! The cutwater of a fillet rounded without the kernel's fillet.
//!
//! The design draws the round in every plane across the inlet width: an arc of radius ρ
//! tangent to both walls, and the sliver of the sharp wedge it cuts off. The flow gains
//! that sliver, so the rounded flow is the sharp one with the sliver added. The sliver
//! is lofted through its sections, a slab between each two neighbouring planes, and
//! fused with the flow. It reaches a little past the walls into the flow and its arc is
//! a little bigger than the round's, so that no face of it runs along a wall: each wall
//! meets the arc at a small angle instead of touching it, which the kernel's unions take
//! where they do not take a tangent contact.

use tp_design::volute::{CutwaterSlice, VoluteGeometry};
use tp_math::DVec3;

use crate::CadError;
use crate::occ::{self, Shape};
use crate::volute::MM;

/// How far the sliver reaches past the walls into the flow, mm.
const OVERLAP: f64 = 0.3;

/// How much bigger than the round's the radius of the sliver's arc is, mm. Less than
/// [`OVERLAP`].
const BULGE: f64 = 0.15;

/// Points of a wall closer than this, mm, count as one.
const MERGE: f64 = 0.02;

/// Gaps between faces the union closes, mm, from none to the most it may take.
const FUZZY: [f64; 4] = [0.0, 0.02, 0.05, 0.1];

/// The most the flow may grow by the round, as a share of its volume.
const MOST_GROWTH: f64 = 0.01;

/// `channel` with the sliver of the fillet added, if that is one valid solid that only
/// grew a little, by no more than the sliver.
pub(crate) fn round_tip(channel: &Shape, g: &VoluteGeometry) -> Option<Shape> {
    let wires = sliver_wires(g).ok()?;
    // The kernel's unions come out wrong with the sliver lofted through all the planes
    // in one piece, and right with a slab between each two.
    let slabs = wires
        .windows(2)
        .map(occ::loft)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let before = channel.volume_with_tolerance(0.0);
    let sliver: f64 = slabs.iter().map(|s| s.volume_with_tolerance(0.0)).sum();
    FUZZY.into_iter().find_map(|fuzzy| {
        let mut pieces = vec![channel.copied().ok()?];
        for slab in &slabs {
            pieces.push(slab.copied().ok()?);
        }
        let whole = occ::fuse_all_fuzzy(&pieces, fuzzy).ok()?;
        let after = whole.volume_with_tolerance(0.0);
        (whole.is_valid()
            && whole.solid_count() == 1
            && after > before
            && after < before * (1.0 + MOST_GROWTH)
            && after - before < sliver)
            .then_some(whole)
    })
}

/// The section of the sliver in each plane of the fillet, from −z to +z.
fn sliver_wires(g: &VoluteGeometry) -> Result<Vec<Shape>, CadError> {
    let wires = g
        .cutwater_slices
        .iter()
        .filter(|s| s.joins.is_some())
        .map(sliver_wire)
        .collect::<Result<Vec<_>, _>>()?;
    if wires.len() < 3 {
        return Err(CadError::Input(
            "the round fits in too few planes to loft its sliver".into(),
        ));
    }
    Ok(wires)
}

/// The closed outline of the sliver in one plane, mm: along the spiral wall from the
/// tip to where the arc touches it, round the arc to the diffuser wall and back along
/// it to the tip, the two walls pushed [`OVERLAP`] away from the nose and the arc
/// [`BULGE`] bigger.
fn sliver_wire(s: &CutwaterSlice) -> Result<Shape, CadError> {
    let (join_spiral, join_diffuser) = s
        .joins
        .ok_or_else(|| CadError::Input("a plane without a round".into()))?;
    let mm = |p: DVec3| p * MM;
    let (tip, centre) = (mm(s.tip), mm(s.nose_centre));
    let (join_spiral, join_diffuser) = (mm(join_spiral), mm(join_diffuser));
    let mut arc: Vec<DVec3> = s.nose_arc().iter().copied().map(mm).collect();
    if arc.len() < 3 {
        return Err(CadError::Input("the round has no arc".into()));
    }
    // From the spiral wall to the diffuser wall, on the very points the walls end at.
    if arc[0].distance(join_diffuser) < arc[0].distance(join_spiral) {
        arc.reverse();
    }
    arc[0] = join_spiral;
    let last = arc.len() - 1;
    arc[last] = join_diffuser;
    let spiral = walk(&s.spiral_wall, tip, join_spiral)?;
    let diffuser = walk(&s.diffuser_wall, tip, join_diffuser)?;
    let spiral_out = push_away(&spiral, centre);
    let diffuser_out = push_away(&diffuser, centre);
    let arc_out: Vec<DVec3> = arc
        .iter()
        .map(|&p| p + (p - centre).normalize_or_zero() * BULGE)
        .collect();
    let (spiral_end, diffuser_end) = (last_of(&spiral_out)?, last_of(&diffuser_out)?);
    let mut diffuser_back = diffuser_out.clone();
    diffuser_back.reverse();
    // A short jog joins each wall to the arc.
    occ::wire(&[
        occ::spline_edge(&spiral_out, false)?,
        occ::line_edge(spiral_end, arc_out[0])?,
        occ::spline_edge(&arc_out, false)?,
        occ::line_edge(last_of(&arc_out)?, diffuser_end)?,
        occ::spline_edge(&diffuser_back, false)?,
        occ::line_edge(diffuser_out[0], spiral_out[0])?,
    ])
}

fn last_of(points: &[DVec3]) -> Result<DVec3, CadError> {
    points
        .last()
        .copied()
        .ok_or_else(|| CadError::Input("an empty wall".into()))
}

/// The points of `wall` from `from` to `to`, mm, both on it: `from`, the points of the
/// wall between them, `to`.
fn walk(wall: &[DVec3], from: DVec3, to: DVec3) -> Result<Vec<DVec3>, CadError> {
    let wall: Vec<DVec3> = wall.iter().map(|&p| p * MM).collect();
    if wall.len() < 2 {
        return Err(CadError::Input("a wall of fewer than two points".into()));
    }
    let (first, last) = (segment_of(&wall, from), segment_of(&wall, to));
    if last < first {
        return Err(CadError::Input(
            "the round lies upstream of the tip on its wall".into(),
        ));
    }
    let mut points = vec![from];
    for &p in &wall[first + 1..=last] {
        if points.last().is_some_and(|q| q.distance(p) > MERGE) {
            points.push(p);
        }
    }
    while points.len() > 1 && points[points.len() - 1].distance(to) <= MERGE {
        points.pop();
    }
    points.push(to);
    if points.len() < 2 || points[0].distance(points[1]) <= MERGE {
        return Err(CadError::Input(
            "the round touches the wall at its tip".into(),
        ));
    }
    Ok(points)
}

/// Index of the segment of the polyline `wall` nearest to `p`.
fn segment_of(wall: &[DVec3], p: DVec3) -> usize {
    (0..wall.len() - 1)
        .min_by(|&i, &j| {
            distance_to_segment(wall[i], wall[i + 1], p).total_cmp(&distance_to_segment(
                wall[j],
                wall[j + 1],
                p,
            ))
        })
        .unwrap_or(0)
}

fn distance_to_segment(a: DVec3, b: DVec3, p: DVec3) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-30)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// `path` with every point pushed [`OVERLAP`] in the plane, perpendicular to the path
/// and away from `centre`.
fn push_away(path: &[DVec3], centre: DVec3) -> Vec<DVec3> {
    (0..path.len())
        .map(|i| {
            let before = path[i.saturating_sub(1)];
            let after = path[(i + 1).min(path.len() - 1)];
            let along = (after - before).normalize_or_zero();
            let mut normal = DVec3::new(-along.y, along.x, 0.0);
            if normal.dot(centre - path[i]) > 0.0 {
                normal = -normal;
            }
            path[i] + normal * OVERLAP
        })
        .collect()
}
