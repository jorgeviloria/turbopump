//! The cutwater cut by planes z = const across the width of the volute.
//!
//! In each plane the outer wall of the spiral and the inner wall of the discharge
//! diffuser cross at the tip of a thin wedge of casing. A rounded cutwater adds a
//! profiled body ahead of the tip (Gülich 2nd ed., Chap. 7.8.1, item 7, and Fig. 7.41b):
//!
//! - its camber line leaves the nose at the blade angle α₃B = α₃′ + i₃ to the
//!   circumferential direction (eq. T7.7.6);
//! - the nose is shaped as a blade edge on a body e = 2ρ thick (round by default, or
//!   elliptic, tapered, Bézier or blunt), with the end of its shape M on the stagnation
//!   radius r′_z on the x axis, moved radially so that the innermost point of the
//!   cutwater lies on the cutwater circle d_z;
//! - from the nose to the tip of the wedge the body keeps the nose thickness e = 2ρ
//!   about a cubic Bézier camber line, tangent to α₃B at the nose and to the middle of
//!   the walls at the tip;
//! - past the tip each face is its wall moved into the flow by its share (½ by default)
//!   of what the wedge lacks of e, through a smooth maximum, so that it runs into the
//!   wall where the wedge has grown to e + k: quadratic, tangent to it (G1), or quartic,
//!   with its curvature too (G2).
//!
//! So the cutwater is nowhere thinner than e, and takes no more of the flow than that
//! needs (our construction: Gülich asks for a short, thick profile, except the nose
//! radius, against cracks).
//!
//! The faces follow the walls of each plane, which differ when the sections are not
//! straight sided. The nose stays on the same line parallel to the pump axis in every
//! plane, or, for an oblique cutwater, turns about the axis from plane to plane so that
//! its edge makes the angle ε with the axis unrolled onto its circle (Gülich Table 10.2:
//! ε > 35° against pressure pulsations).

use tp_math::{DVec2, DVec3};

use super::NozzleSection;
use super::{CutMeasure, VoluteDesign};

/// Planes across the inlet width, an odd number so that one is the midplane.
pub const SLICES: usize = 9;
/// The outer planes stay this fraction of the half width inside the side walls.
const EDGE_MARGIN: f64 = 0.05;
/// Points between consecutive spiral sections on the spline through them.
const SPIRAL_SUBDIVISIONS: usize = 4;
/// Points between consecutive diffuser sections on the spline through them.
const DIFFUSER_SUBDIVISIONS: usize = 24;
/// Points on each wall of the head of a fillet, and on its arc.
const FILLET_WALL_POINTS: usize = 23;
const FILLET_ARC_POINTS: usize = 17;
/// Points the cubic of a simple cutwater is sampled at, before its spacing.
const FINE_POINTS: usize = 400;
/// How far the head of a fillet runs back along the walls, in radii.
const FILLET_HEAD: f64 = 8.0;
/// The cutwater in one plane z = const, in the plane of the design (the cutwater on
/// the +x axis) until the volute is turned.
#[derive(Debug, Clone, PartialEq)]
pub struct CutwaterSlice {
    /// m
    pub z: f64,
    /// Where the outer wall of the spiral crosses the inner wall of the diffuser, m.
    pub tip: DVec3,
    /// Angle between the two walls at the tip, rad.
    pub wedge: f64,
    /// Angles of the outer wall of the spiral and of the inner wall of the diffuser at
    /// the tip from the circumferential direction, positive away from the pump axis,
    /// rad. The wedge is their difference.
    pub spiral_angle: f64,
    pub diffuser_angle: f64,
    /// Smooth outline of the material a rounded cutwater adds, from where it joins
    /// the spiral wall, round the nose, to where it joins the diffuser wall; the chord
    /// back to the first point closes it inside the wedge. Empty unless rounded.
    pub outline: Vec<DVec3>,
    /// Thinnest material of the cutwater behind the nose, m: 0 for a sharp one.
    pub min_thickness: f64,
    /// Distance from the pump axis to the nearest point of the cutwater, m.
    pub nose_distance: f64,
    /// Lengths of wall from the tip to where the faces join the spiral wall and the
    /// diffuser wall, m; 0 for a sharp cutwater.
    pub join_length: f64,
    /// The outer wall of the spiral in this plane, from the start of the spiral
    /// downstream past the cutwater.
    pub spiral_wall: Vec<DVec3>,
    /// The inner wall of the diffuser in this plane, from the throat downstream.
    pub diffuser_wall: Vec<DVec3>,
    /// Centre of the nose circle in this plane; the tip for a sharp cutwater.
    pub nose_centre: DVec3,
    /// For a fillet: the points where its arc touches the spiral wall and the diffuser
    /// wall.
    pub joins: Option<(DVec3, DVec3)>,
    /// For a fillet: the sliver of the sharp wedge the round cuts off, closed (the tip,
    /// along the spiral wall to the arc, round it, and back along the diffuser wall).
    /// Empty otherwise.
    pub removed: Vec<DVec3>,
}

/// The arc of the nose in the outline of a fillet (see [`Walls::fillet_slice`]): the part
/// that runs round the circle, between the walls.
pub(super) fn arc_of(outline: &[DVec3]) -> &[DVec3] {
    outline
        .get(FILLET_WALL_POINTS..FILLET_WALL_POINTS + FILLET_ARC_POINTS)
        .unwrap_or(&[])
}

impl CutwaterSlice {
    /// The arc that rounds the tip of a fillet in this plane, from the spiral wall to
    /// the diffuser wall; empty for any other cutwater.
    pub fn nose_arc(&self) -> &[DVec3] {
        if self.joins.is_some() {
            arc_of(&self.outline)
        } else {
            &[]
        }
    }

    /// The outer wall of the spiral from the tip downstream: the wall trimmed where it
    /// crosses the inner wall of the diffuser, so that the part before the crossing,
    /// which lies inside the diffuser, is left out (a sharp cutwater).
    pub fn trimmed_spiral_wall(&self) -> Vec<DVec3> {
        trimmed(&self.spiral_wall, self.joins.map_or(self.tip, |j| j.0))
    }

    /// The inner wall of the diffuser from the tip downstream: the wall trimmed where it
    /// crosses the outer wall of the spiral.
    pub fn trimmed_diffuser_wall(&self) -> Vec<DVec3> {
        trimmed(&self.diffuser_wall, self.joins.map_or(self.tip, |j| j.1))
    }
}

/// The polyline `wall` from `tip` on: the tip, then the points past the segment nearest
/// to it.
fn trimmed(wall: &[DVec3], tip: DVec3) -> Vec<DVec3> {
    if wall.len() < 2 {
        return wall.to_vec();
    }
    let flat: Vec<DVec2> = wall.iter().map(|p| p.truncate()).collect();
    let (k, _) = nearest_on(&flat, tip.truncate());
    let mut out = vec![tip];
    out.extend_from_slice(&wall[k + 1..]);
    out
}

/// The cutwater in planes across the inlet width, from −z to +z. `outlines` are the
/// fitted sections (r, z) of the spiral at their wrap angles, from its start, a degree
/// or less apart; `nozzle` the sections of the diffuser before the volute is turned. Planes
/// where the walls do not meet are left out.
pub(super) fn slices(
    design: &VoluteDesign,
    outlines: &[(f64, Vec<DVec2>)],
    nozzle: &[NozzleSection],
    sign: f64,
    midplane: f64,
) -> Vec<CutwaterSlice> {
    let half = 0.5 * design.inlet_width * (1.0 - EDGE_MARGIN);
    let levels = (0..SLICES).map(|k| midplane - half + 2.0 * half * k as f64 / (SLICES - 1) as f64);
    // Sharp or rounded, the cutwater stands where the walls cross; a fillet rounds that
    // edge.
    let fillet = design.cutwater_shape.is_fillet();
    levels
        .filter_map(|z| {
            let walls = Walls::at(outlines, nozzle, sign, z)?;
            if fillet {
                walls.fillet_slice(design, sign, z)
            } else {
                walls.slice(sign, z)
            }
        })
        .collect()
}

/// Where the outer wall of the spiral crosses the inner wall of the diffuser in each of
/// the planes across the inlet width: the wrap angle of the tip from the cutwater
/// (rad) and the wedge between the walls there; `None` in a plane where they do not
/// cross. `outlines` are the fitted sections of the spiral from an early start, which do
/// not depend on where the spiral really starts.
pub(super) fn crossings(
    design: &VoluteDesign,
    outlines: &[(f64, Vec<DVec2>)],
    nozzle: &[NozzleSection],
    sign: f64,
    midplane: f64,
) -> Vec<Option<(f64, f64)>> {
    let half = 0.5 * design.inlet_width * (1.0 - EDGE_MARGIN);
    (0..SLICES)
        .map(|k| {
            let z = midplane - half + 2.0 * half * k as f64 / (SLICES - 1) as f64;
            let walls = Walls::at(outlines, nozzle, sign, z)?;
            let (s, d) = walls.crossing()?;
            let ts = direction(&walls.spiral, s.segment);
            let td = direction(&walls.diffuser, d.segment);
            let wedge = ts.dot(td).clamp(-1.0, 1.0).acos();
            let angle = (sign * s.point.y.atan2(s.point.x)).rem_euclid(std::f64::consts::TAU);
            Some((angle, wedge))
        })
        .collect()
}

/// How many times the outer wall of the spiral crosses the inner wall of the diffuser
/// in each of the planes across the inlet width, as for [`crossings`]. A cutwater needs
/// one intersection curve between the spiral and the diffuser: more than one crossing in
/// a plane means several.
pub(super) fn crossing_counts(
    design: &VoluteDesign,
    outlines: &[(f64, Vec<DVec2>)],
    nozzle: &[NozzleSection],
    sign: f64,
    midplane: f64,
) -> Vec<usize> {
    let half = 0.5 * design.inlet_width * (1.0 - EDGE_MARGIN);
    (0..SLICES)
        .map(|k| {
            let z = midplane - half + 2.0 * half * k as f64 / (SLICES - 1) as f64;
            Walls::at(outlines, nozzle, sign, z).map_or(0, |w| w.count_crossings())
        })
        .collect()
}

/// The walls near the cutwater in one plane z = const, as the model has them (turned):
/// the outer wall of the spiral from where its sections first reach the plane, and the
/// inner wall of the diffuser, both downstream; how many times they cross and the first
/// crossing, the tip of the cutwater in that plane.
#[derive(Debug, Clone, PartialEq)]
pub struct TongueCut {
    /// m
    pub z: f64,
    pub spiral: Vec<DVec3>,
    pub diffuser: Vec<DVec3>,
    pub crossings: usize,
    pub tip: Option<DVec3>,
}

/// The walls of the spiral `outlines` (r, z by wrap angle) and of the diffuser `nozzle`
/// (turned) in the plane z = `z`, the spiral turned by `rotation` for an impeller
/// turning with `sign`.
pub(super) fn tongue_cut(
    outlines: &[(f64, Vec<DVec2>)],
    nozzle: &[NozzleSection],
    sign: f64,
    rotation: f64,
    z: f64,
) -> Option<TongueCut> {
    let spiral: Vec<DVec2> = outlines
        .iter()
        .filter_map(|(angle, outline)| {
            let r = outermost_at(outline, z)?;
            let theta = rotation + sign * angle;
            Some(DVec2::new(r * theta.cos(), r * theta.sin()))
        })
        .collect();
    let spiral = catmull_rom(&spiral, SPIRAL_SUBDIVISIONS);
    let inner: Vec<DVec2> = nozzle.iter().filter_map(|n| innermost_at(n, z)).collect();
    let diffuser = catmull_rom(&inner, DIFFUSER_SUBDIVISIONS);
    if spiral.len() < 2 || diffuser.len() < 2 {
        return None;
    }
    let walls = Walls { spiral, diffuser };
    let lift = |p: &DVec2| DVec3::new(p.x, p.y, z);
    Some(TongueCut {
        z,
        crossings: walls.count_crossings(),
        tip: walls.crossing().map(|(s, _)| lift(&s.point)),
        spiral: walls.spiral.iter().map(lift).collect(),
        diffuser: walls.diffuser.iter().map(lift).collect(),
    })
}

/// The two walls in one plane, as polylines (x, y) downstream.
struct Walls {
    spiral: Vec<DVec2>,
    diffuser: Vec<DVec2>,
}

/// A point on a polyline: the segment it lies on and the point.
#[derive(Debug, Clone, Copy)]
struct Station {
    segment: usize,
    point: DVec2,
}

impl Walls {
    fn at(
        outlines: &[(f64, Vec<DVec2>)],
        nozzle: &[NozzleSection],
        sign: f64,
        z: f64,
    ) -> Option<Self> {
        let spiral: Vec<DVec2> = outlines
            .iter()
            .filter_map(|(angle, outline)| {
                let r = outermost_at(outline, z)?;
                let theta = sign * angle;
                Some(DVec2::new(r * theta.cos(), r * theta.sin()))
            })
            .collect();
        // A spline between the sections too, for the direction of the wall at the tip.
        let spiral = catmull_rom(&spiral, SPIRAL_SUBDIVISIONS);
        // The diffuser is lofted smoothly through its sections, which lie far apart: a
        // spline through their inner points follows its wall between them.
        let inner: Vec<DVec2> = nozzle.iter().filter_map(|n| innermost_at(n, z)).collect();
        let diffuser = catmull_rom(&inner, DIFFUSER_SUBDIVISIONS);
        (spiral.len() >= 2 && diffuser.len() >= 2).then_some(Self { spiral, diffuser })
    }

    /// Where the spiral wall first crosses the diffuser wall, on each polyline; when
    /// they never cross (the diffuser wall turns away at once), the start of the spiral
    /// and the nearest point of the diffuser wall to it.
    fn tip(&self) -> (Station, Station) {
        self.crossing().unwrap_or_else(|| {
            let start = self.spiral[0];
            let (segment, point) = nearest_on(&self.diffuser, start);
            (
                Station {
                    segment: 0,
                    point: start,
                },
                Station { segment, point },
            )
        })
    }

    /// The number of distinct points where the two walls cross.
    fn count_crossings(&self) -> usize {
        let mut points: Vec<DVec2> = Vec::new();
        for i in 0..self.spiral.len() - 1 {
            for j in 0..self.diffuser.len() - 1 {
                if let Some(p) = segment_crossing(
                    self.spiral[i],
                    self.spiral[i + 1],
                    self.diffuser[j],
                    self.diffuser[j + 1],
                ) && points.iter().all(|q| q.distance(p) > 1e-5)
                {
                    points.push(p);
                }
            }
        }
        points.len()
    }

    /// Where the spiral wall first crosses the diffuser wall, on each polyline.
    fn crossing(&self) -> Option<(Station, Station)> {
        for i in 0..self.spiral.len() - 1 {
            let (a, b) = (self.spiral[i], self.spiral[i + 1]);
            for j in 0..self.diffuser.len() - 1 {
                let (c, d) = (self.diffuser[j], self.diffuser[j + 1]);
                if let Some(p) = segment_crossing(a, b, c, d) {
                    return Some((
                        Station {
                            segment: i,
                            point: p,
                        },
                        Station {
                            segment: j,
                            point: p,
                        },
                    ));
                }
            }
        }
        None
    }

    /// The cutwater of a fillet in this plane: the sharp wedge with its tip rounded by the
    /// circle of radius ρ tangent to both walls. The slice has the sharp tip and wedge;
    /// its outline is the head of the tongue, from `FILLET_HEAD` radii back along each
    /// wall to the round, and `removed` the sliver of the sharp wedge the round cuts
    /// off. `None` where the walls do not cross or the round does not fit between them
    /// (the radius is too large for the length of wall).
    fn fillet_slice(&self, design: &VoluteDesign, sign: f64, z: f64) -> Option<CutwaterSlice> {
        if design.cutwater_shape.is_simple() {
            return self.simple_slice(design, sign, z);
        }
        let mut slice = self.slice(sign, z)?;
        let r = design.nose_radius;
        if r <= 0.0 {
            return Some(slice);
        }
        let (s, d) = self.tip();
        let (ts, td) = (
            direction(&self.spiral, s.segment),
            direction(&self.diffuser, d.segment),
        );
        let wedge = ts.dot(td).clamp(-1.0, 1.0).acos();
        let bisector = (ts + td).normalize_or_zero();
        if wedge < 1e-3 || bisector == DVec2::ZERO {
            return None;
        }
        let (spiral, diffuser) = (Path::from(&self.spiral, s), Path::from(&self.diffuser, d));
        // The centre is r from both walls: Newton from the centre of the circle between
        // straight walls.
        let gaps = |c: DVec2| {
            DVec2::new(
                distance_to(&spiral.points, c) - r,
                distance_to(&diffuser.points, c) - r,
            )
        };
        let mut c = s.point + bisector * (r / (0.5 * wedge).sin());
        for _ in 0..40 {
            let v = gaps(c);
            if v.length() < 1e-12 {
                break;
            }
            let h = 1e-7;
            let (jx, jy) = (
                (gaps(c + DVec2::X * h) - v) / h,
                (gaps(c + DVec2::Y * h) - v) / h,
            );
            let det = jx.x * jy.y - jy.x * jx.y;
            if det.abs() < 1e-12 {
                return None;
            }
            c += DVec2::new(
                (-v.x * jy.y + jy.x * v.y) / det,
                (-jx.x * v.y + jx.y * v.x) / det,
            );
        }
        if gaps(c).length() > 1e-9 {
            return None;
        }
        // Where the circle touches each wall, and how far from the tip along it.
        let touch = |path: &Path| -> Option<(f64, DVec2)> {
            let (k, p) = nearest_on(&path.points, c);
            let l = path.lengths[k] + path.points[k].distance(p);
            // Short of the end of the wall, and past the tip.
            (l > 1e-9 && l < path.length - FILLET_HEAD * r).then_some((l, p))
        };
        let ((l_s, t_s), (l_d, t_d)) = (touch(&spiral)?, touch(&diffuser)?);
        // The arc through the direction of the tip, from one wall to the other.
        let angle = |p: DVec2| (p - c).to_angle();
        let wrap = |a: f64| {
            (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
        };
        let (a0, to_d) = (angle(t_s), wrap(angle(t_d) - angle(t_s)));
        let toward_tip = wrap(angle(s.point) - a0);
        let sweep = if toward_tip.signum() == to_d.signum() && toward_tip.abs() <= to_d.abs() {
            to_d
        } else {
            to_d - std::f64::consts::TAU * to_d.signum()
        };
        let arc: Vec<DVec2> = (0..FILLET_ARC_POINTS)
            .map(|i| {
                c + DVec2::from_angle(a0 + sweep * i as f64 / (FILLET_ARC_POINTS - 1) as f64) * r
            })
            .collect();
        // The head: back along the spiral wall to the arc, round it, and out along the
        // diffuser wall.
        let reach = FILLET_HEAD * r;
        let wall = |path: &Path, from: f64, outward: bool| -> Vec<DVec2> {
            (0..FILLET_WALL_POINTS)
                .map(|i| {
                    let t = (i + 1) as f64 / FILLET_WALL_POINTS as f64;
                    let l = if outward {
                        from + reach * t
                    } else {
                        from + reach * (1.0 - t)
                    };
                    path.point(l).0
                })
                .collect()
        };
        let at = |p: &DVec2| DVec3::new(p.x, p.y, z);
        slice.outline = wall(&spiral, l_s, false)
            .iter()
            .chain(&arc)
            .chain(&wall(&diffuser, l_d, true))
            .map(at)
            .collect();
        // The sliver the round removes from the sharp wedge.
        let along = |path: &Path, to: f64| -> Vec<DVec2> {
            let n = 4;
            (0..=n)
                .map(|i| path.point(to * i as f64 / n as f64).0)
                .collect()
        };
        slice.removed = along(&spiral, l_s)
            .into_iter()
            .chain(arc.iter().copied().skip(1))
            .chain(along(&diffuser, l_d).into_iter().rev().skip(1))
            .map(|p| at(&p))
            .collect();
        slice.joins = Some((at(&t_s), at(&t_d)));
        slice.nose_centre = at(&c);
        slice.min_thickness = 2.0 * r;
        slice.join_length = 0.5 * (l_s + l_d);
        slice.nose_distance = slice
            .outline
            .iter()
            .map(|p| p.truncate().length())
            .fold(f64::INFINITY, f64::min);
        Some(slice)
    }

    /// The cutwater of a simple edge in this plane: the walls cut back from the tip, and
    /// a cubic from one cut to the other, tangent to both walls there, its handles
    /// pointing back toward the tip. Cut along, both walls end `cutwater_cut` back from
    /// the tip; cut across, each ends where the other lies `cutwater_cut` away. The slice
    /// has the structure of a fillet's: `joins` at the cuts, the cubic where the arc of a
    /// fillet would be, and `removed`, the sliver of the sharp wedge the cubic cuts off.
    /// `None` where the walls are too short for the cut.
    fn simple_slice(&self, design: &VoluteDesign, sign: f64, z: f64) -> Option<CutwaterSlice> {
        let mut slice = self.slice(sign, z)?;
        let (s, d) = self.tip();
        let (spiral, diffuser) = (Path::from(&self.spiral, s), Path::from(&self.diffuser, d));
        let cut = design.cutwater_cut;
        let (l_s, l_d) = match design.cutwater_shape.cut_measure() {
            Some(CutMeasure::Across) => (
                spiral.reach_away(&diffuser.points, cut)?,
                diffuser.reach_away(&spiral.points, cut)?,
            ),
            _ => (cut, cut),
        };
        let reach = (0.5 * l_s.min(l_d))
            .min(spiral.length - l_s)
            .min(diffuser.length - l_d);
        if l_s.min(l_d) <= 1e-9 || reach <= 1e-9 {
            return None;
        }
        let ((p0, ts), (p3, td)) = (spiral.point(l_s), diffuser.point(l_d));
        let (p1, p2) = (
            p0 - ts * (design.cutwater_handle * l_s),
            p3 - td * (design.cutwater_handle * l_d),
        );
        let cubic = |u: f64| {
            let v = 1.0 - u;
            p0 * (v * v * v) + p1 * (3.0 * v * v * u) + p2 * (3.0 * v * u * u) + p3 * (u * u * u)
        };
        // Spaced by half its length and half its turning, so that the points are closer
        // together round the nose, where the curve turns most.
        let fine: Vec<DVec2> = (0..=FINE_POINTS)
            .map(|i| cubic(i as f64 / FINE_POINTS as f64))
            .collect();
        let steps: Vec<(f64, f64)> = fine
            .windows(3)
            .map(|w| {
                let (a, b) = (
                    (w[1] - w[0]).normalize_or_zero(),
                    (w[2] - w[1]).normalize_or_zero(),
                );
                (w[1].distance(w[0]), a.dot(b).clamp(-1.0, 1.0).acos())
            })
            .collect();
        let (length, turn) = steps
            .iter()
            .fold((0.0, 0.0), |(l, t), (dl, dt)| (l + dl, t + dt));
        let mut cumulative = vec![0.0];
        for (dl, dt) in &steps {
            let last = cumulative[cumulative.len() - 1];
            cumulative.push(last + 0.5 * dl / length.max(1e-30) + 0.5 * dt / turn.max(1e-30));
        }
        let total = cumulative[cumulative.len() - 1].max(1e-30);
        let arc: Vec<DVec2> = (0..FILLET_ARC_POINTS)
            .map(|i| {
                let target = total * i as f64 / (FILLET_ARC_POINTS - 1) as f64;
                let k = cumulative
                    .partition_point(|&c| c < target)
                    .min(cumulative.len() - 1);
                fine[k + 1]
            })
            .collect();
        let (mut arc, last) = (arc, FILLET_ARC_POINTS - 1);
        arc[0] = p0;
        arc[last] = p3;
        let wall = |path: &Path, l: f64, outward: bool| -> Vec<DVec2> {
            (0..FILLET_WALL_POINTS)
                .map(|i| {
                    let t = (i + 1) as f64 / FILLET_WALL_POINTS as f64;
                    let along = if outward {
                        l + reach * t
                    } else {
                        l + reach * (1.0 - t)
                    };
                    path.point(along).0
                })
                .collect()
        };
        let at = |p: &DVec2| DVec3::new(p.x, p.y, z);
        slice.outline = wall(&spiral, l_s, false)
            .iter()
            .chain(&arc)
            .chain(&wall(&diffuser, l_d, true))
            .map(at)
            .collect();
        let along = |path: &Path, to: f64| -> Vec<DVec2> {
            let n = 4;
            (0..=n)
                .map(|i| path.point(to * i as f64 / n as f64).0)
                .collect()
        };
        slice.removed = along(&spiral, l_s)
            .into_iter()
            .chain(arc.iter().copied().skip(1))
            .chain(along(&diffuser, l_d).into_iter().rev().skip(1))
            .map(|p| at(&p))
            .collect();
        slice.joins = Some((at(&p0), at(&p3)));
        slice.nose_centre = at(&(0.5 * (p0 + p3)));
        slice.join_length = 0.5 * (l_s + l_d);
        slice.nose_distance = slice
            .outline
            .iter()
            .map(|p| p.truncate().length())
            .fold(f64::INFINITY, f64::min);
        Some(slice)
    }

    /// The cutwater of a sharp edge in this plane: the tip and the wedge where the walls
    /// cross, with no outline.
    fn slice(&self, sign: f64, z: f64) -> Option<CutwaterSlice> {
        let (s, d) = self.tip();
        let ts = direction(&self.spiral, s.segment);
        let td = direction(&self.diffuser, d.segment);
        let wedge = ts.dot(td).clamp(-1.0, 1.0).acos();
        let radial = s.point.normalize();
        let circumferential = DVec2::new(-radial.y, radial.x) * sign;
        let from_circle = |d: DVec2| d.dot(radial).atan2(d.dot(circumferential));
        let (spiral_angle, diffuser_angle) = (from_circle(ts), from_circle(td));
        let at = |p: DVec2| DVec3::new(p.x, p.y, z);
        let spiral_wall: Vec<DVec3> = self.spiral.iter().map(|p| at(*p)).collect();
        let diffuser_wall: Vec<DVec3> = self.diffuser.iter().map(|p| at(*p)).collect();
        Some(CutwaterSlice {
            z,
            tip: at(s.point),
            wedge,
            spiral_angle,
            diffuser_angle,
            outline: Vec::new(),
            min_thickness: 0.0,
            nose_distance: s.point.length(),
            join_length: 0.0,
            spiral_wall,
            diffuser_wall,
            nose_centre: at(s.point),
            joins: None,
            removed: Vec::new(),
        })
    }
}

/// A polyline from a station onward, by arc length.
struct Path {
    points: Vec<DVec2>,
    /// Arc length at each point, from the station.
    lengths: Vec<f64>,
    length: f64,
}

impl Path {
    fn from(polyline: &[DVec2], start: Station) -> Self {
        let mut points = vec![start.point];
        points.extend(&polyline[start.segment + 1..]);
        let mut lengths = vec![0.0];
        for w in points.windows(2) {
            lengths.push(lengths.last().copied().unwrap_or(0.0) + w[0].distance(w[1]));
        }
        let length = lengths.last().copied().unwrap_or(0.0);
        Self {
            points,
            lengths,
            length,
        }
    }

    /// The point `l` along and the unit tangent there.
    fn point(&self, l: f64) -> (DVec2, DVec2) {
        let l = l.clamp(0.0, self.length);
        let k = self
            .lengths
            .windows(2)
            .position(|w| l <= w[1])
            .unwrap_or(self.lengths.len() - 2);
        let (a, b) = (self.points[k], self.points[k + 1]);
        let span = self.lengths[k + 1] - self.lengths[k];
        let t = if span > 0.0 {
            (l - self.lengths[k]) / span
        } else {
            0.0
        };
        (a.lerp(b, t), (b - a).normalize())
    }

    /// The first length along where the polyline `other` lies `distance` away, found by
    /// steps from the station and then by bisection; `None` if it never does.
    fn reach_away(&self, other: &[DVec2], distance: f64) -> Option<f64> {
        let gap = |l: f64| distance_to(other, self.point(l).0) - distance;
        let steps = 200;
        let step = self.length / steps as f64;
        let mut lo = 0.0;
        for i in 1..=steps {
            let hi = step * i as f64;
            if gap(hi) >= 0.0 {
                let (mut a, mut b) = (lo, hi);
                for _ in 0..60 {
                    let m = 0.5 * (a + b);
                    if gap(m) >= 0.0 { b = m } else { a = m }
                }
                return Some(b);
            }
            lo = hi;
        }
        None
    }
}

/// Outlines of the body between the planes too, from −z to +z: `steps` per gap between
/// two planes, on a Catmull–Rom spline through the outlines of the planes point by point
/// (uniform: the planes are evenly spaced). Lofted straight from one to the next they
/// follow the smooth surface the CAD lofts through the planes, where the ends of the
/// faces slide along the walls from plane to plane (round sections). Empty unless
/// rounded.
pub fn fine_outlines(slices: &[CutwaterSlice], steps: usize) -> Vec<Vec<DVec3>> {
    let rings: Vec<&[DVec3]> = slices
        .iter()
        .map(|s| s.outline.as_slice())
        .filter(|o| o.len() > 2)
        .collect();
    let Some(n) = rings.first().map(|r| r.len()) else {
        return Vec::new();
    };
    if rings.len() < 3 || rings.iter().any(|r| r.len() != n) {
        return rings.iter().map(|r| r.to_vec()).collect();
    }
    let m = rings.len();
    // The planes beyond the ends carry on the first and last gaps straight.
    let at = |k: isize, i: usize| -> DVec3 {
        if k < 0 {
            rings[0][i] * 2.0 - rings[1][i]
        } else if k as usize >= m {
            rings[m - 1][i] * 2.0 - rings[m - 2][i]
        } else {
            rings[k as usize][i]
        }
    };
    let mut out = Vec::with_capacity((m - 1) * steps + 1);
    for k in 0..m as isize - 1 {
        for j in 0..steps {
            let t = j as f64 / steps as f64;
            out.push(
                (0..n)
                    .map(|i| {
                        let (p0, p1, p2, p3) = (at(k - 1, i), at(k, i), at(k + 1, i), at(k + 2, i));
                        0.5 * (p1 * 2.0
                            + (p2 - p0) * t
                            + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * (t * t)
                            + (p3 - p0 + (p1 - p2) * 3.0) * (t * t * t))
                    })
                    .collect(),
            );
        }
    }
    out.push(rings[m - 1].to_vec());
    out
}

/// The planes of a body with side endings lie no farther apart than its radius over this.
const ENDING_PLANES: f64 = 2.0;

/// What a point of the outline of the body faces.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Facing {
    /// The spiral, along the inner face.
    Spiral,
    /// The flow ahead of the cutwater, round the nose.
    Nose,
    /// The diffuser, along the outer face.
    Diffuser,
}

/// The side walls of the channels around the cutwater, before the volute is turned.
struct SideWalls<'a> {
    /// The fitted sections (r, z) of the spiral at their wrap angles.
    outlines: &'a [(f64, Vec<DVec2>)],
    nozzle: &'a [NozzleSection],
    sign: f64,
    /// The side walls of the inlet: the heights of the bottom and the top, m.
    inlet: (f64, f64),
    /// How far out the inlet ring reaches, m.
    ring: f64,
}

impl SideWalls<'_> {
    /// Heights of the bottom and the top side walls straight below and above `p` (m):
    /// those of the flow there, the inlet ring, the spiral and the diffuser together;
    /// `None` where none of them reaches.
    fn at(&self, p: DVec2) -> Option<(f64, f64)> {
        let inlet = (p.length() <= self.ring).then_some(self.inlet);
        [inlet, self.spiral(p), self.diffuser(p)]
            .into_iter()
            .flatten()
            .reduce(|(lo, hi), (a, b)| (lo.min(a), hi.max(b)))
    }

    /// In the spiral, from its sections either side of the point.
    fn spiral(&self, p: DVec2) -> Option<(f64, f64)> {
        let theta = (self.sign * p.y.atan2(p.x)).rem_euclid(std::f64::consts::TAU);
        let k = self.outlines.partition_point(|(angle, _)| *angle < theta);
        if k == 0 || k >= self.outlines.len() {
            return None;
        }
        let ((a0, o0), (a1, o1)) = (&self.outlines[k - 1], &self.outlines[k]);
        let r = p.length();
        let (lo0, hi0) = span_at(o0, r)?;
        let (lo1, hi1) = span_at(o1, r)?;
        let t = (theta - a0) / (a1 - a0);
        Some((lo0 + (lo1 - lo0) * t, hi0 + (hi1 - hi0) * t))
    }

    /// In the diffuser, from its sections either side of the point, each seen in its
    /// own plane (across the diffuser, z).
    fn diffuser(&self, p: DVec2) -> Option<(f64, f64)> {
        let ahead = |n: &NozzleSection| {
            (DVec3::new(p.x, p.y, n.center.z) - n.center).dot(n.tangent.normalize())
        };
        let k = (0..self.nozzle.len().saturating_sub(1))
            .find(|&k| ahead(&self.nozzle[k]) >= 0.0 && ahead(&self.nozzle[k + 1]) < 0.0)?;
        let span = |n: &NozzleSection| {
            let u = n.tangent.cross(DVec3::Z).normalize();
            let at = DVec3::new(p.x, p.y, n.center.z);
            let flat: Vec<DVec2> = n
                .points
                .iter()
                .map(|q| DVec2::new((*q - n.center).dot(u), q.z))
                .collect();
            span_at(&flat, (at - n.center).dot(u))
        };
        let (a, b) = (&self.nozzle[k], &self.nozzle[k + 1]);
        let ((lo0, hi0), (lo1, hi1)) = (span(a)?, span(b)?);
        let t = ahead(a) / (ahead(a) - ahead(b));
        Some((lo0 + (lo1 - lo0) * t, hi0 + (hi1 - hi0) * t))
    }
}

/// The lowest and the highest point where the closed polygon `outline` (x, y) crosses
/// the line x = `x`.
fn span_at(outline: &[DVec2], x: f64) -> Option<(f64, f64)> {
    let n = outline.len();
    let crossings = (0..n).filter_map(|i| {
        let (p, q) = (outline[i], outline[(i + 1) % n]);
        if (p.x - x) * (q.x - x) > 0.0 || (q.x - p.x).abs() < 1e-15 {
            return None;
        }
        Some(p.y + (q.y - p.y) * (x - p.x) / (q.x - p.x))
    });
    crossings.fold(None, |span, y| match span {
        None => Some((y, y)),
        Some((lo, hi)) => Some((f64::min(lo, y), f64::max(hi, y))),
    })
}

/// How far the body stands out from its face at the height `v` so that it rounds into
/// a side wall at the height `wall` with the radius `radius`; the wall rises by the
/// angle `slope` away from the face, into the flow. All measured toward the wall: the
/// face meets the round where the circle of the round touches it, and past the top of
/// the circle the body stands out by the radius.
fn flare(v: f64, wall: f64, slope: f64, radius: f64) -> f64 {
    let touch = wall + radius * slope.tan() - radius / slope.cos();
    let w = (v - touch).clamp(0.0, radius);
    radius - (radius * radius - w * w).max(0.0).sqrt()
}

/// The outlines of the body of a rounded cutwater from −z to +z, with the side endings:
/// toward each side wall of the channel the body widens with the round of `radius`
/// between its face and the wall, the height and the slope of the wall taken in the
/// channel each point of the outline faces (the spiral, the diffuser, or the inlet
/// round the nose). Where a face runs into its wall the body has no corner with the
/// side wall, so the widening fades out there. `steps` outlines lie between two planes,
/// on a Catmull–Rom spline through theirs ([`fine_outlines`]), and all of them no
/// farther apart than `radius` / [`ENDING_PLANES`], on to where the body has widened by
/// the whole radius. The channels are the fitted sections of the spiral at their wrap
/// angles and the sections of the diffuser. Before the volute is turned.
pub(super) fn body_outlines(
    design: &VoluteDesign,
    slices: &[CutwaterSlice],
    (outlines, nozzle): (&[(f64, Vec<DVec2>)], &[NozzleSection]),
    sign: f64,
    midplane: f64,
    radius: f64,
    steps: usize,
) -> Vec<Vec<DVec3>> {
    let fine = fine_outlines(slices, steps);
    let rounded: Vec<&CutwaterSlice> = slices.iter().filter(|s| s.outline.len() > 2).collect();
    let (Some(first), Some(last)) = (rounded.first(), rounded.last()) else {
        return fine;
    };
    if radius <= 0.0 || fine.len() < 2 {
        return fine;
    }
    let b3 = design.inlet_width;
    let rz = design.cutwater_diameter / 2.0;
    let walls = SideWalls {
        outlines,
        nozzle,
        sign,
        inlet: (midplane - 0.5 * b3, midplane + 0.5 * b3),
        // As the ring of the flow domain (VoluteGeometry::inlet_profile).
        ring: design.stagnation_radius + 0.02 * b3,
    };
    // What each point faces and where it stands from its wall: in the outer planes,
    // the bottom one for the bottom wall and the top one for the top wall.
    let nose = design.nose_radius * (1.0 + 1e-6);
    struct Point {
        normal: DVec2,
        /// Height of the wall and its slope away from the face.
        wall: f64,
        slope: f64,
        /// How much of the round the point takes, 0 where the face runs into its wall.
        share: f64,
    }
    let points = |s: &CutwaterSlice, top: bool| -> Vec<Point> {
        let o: Vec<DVec2> = s.outline.iter().map(|p| p.truncate()).collect();
        let n = o.len();
        let centre = s.nose_centre.truncate();
        let (Some(i0), Some(i1)) = (
            o.iter().position(|p| p.distance(centre) <= nose),
            o.iter().rposition(|p| p.distance(centre) <= nose),
        ) else {
            return Vec::new();
        };
        let ccw = (0..n).map(|i| o[i].perp_dot(o[(i + 1) % n])).sum::<f64>() > 0.0;
        let spiral: Vec<DVec2> = s.spiral_wall.iter().map(|p| p.truncate()).collect();
        let diffuser: Vec<DVec2> = s.diffuser_wall.iter().map(|p| p.truncate()).collect();
        (0..n)
            .map(|i| {
                let facing = if i < i0 {
                    Facing::Spiral
                } else if i <= i1 {
                    Facing::Nose
                } else {
                    Facing::Diffuser
                };
                let t = (o[(i + 1).min(n - 1)] - o[i.saturating_sub(1)]).normalize_or_zero();
                let normal = if ccw {
                    DVec2::new(t.y, -t.x)
                } else {
                    DVec2::new(-t.y, t.x)
                };
                let height = |q: DVec2| walls.at(q).map(|(lo, hi)| if top { hi } else { -lo });
                // The slope over the width of the round, which steps of the wall from one
                // channel to the next do not tilt much.
                let (wall, slope) = match (height(o[i]), height(o[i] + normal * radius)) {
                    (Some(h), Some(further)) => (h, ((further - h) / radius).atan()),
                    (Some(h), None) => (h, 0.0),
                    _ => {
                        let (lo, hi) = walls.inlet;
                        (if top { hi } else { -lo }, 0.0)
                    }
                };
                let off = match facing {
                    Facing::Spiral => distance_to(&spiral, o[i]),
                    Facing::Diffuser => distance_to(&diffuser, o[i]),
                    Facing::Nose => radius,
                };
                let x = (off / radius).clamp(0.0, 1.0);
                Point {
                    normal,
                    wall,
                    slope: slope.clamp(-1.0, 1.0),
                    share: x * x * (3.0 - 2.0 * x),
                }
            })
            .collect()
    };
    let (below, above) = (points(first, false), points(last, true));
    let n = first.outline.len();
    if below.len() != n || above.len() != n || fine.iter().any(|o| o.len() != n) {
        return fine;
    }
    // Where the round starts on the face, toward each wall, and where it reaches its
    // radius. The planes keep one spacing over the whole height, no more than half the
    // radius: a smooth loft through unevenly spaced sections swings between them.
    let touch = |p: &Point| p.wall + radius * p.slope.tan() - radius / p.slope.cos();
    let reach = |ps: &[Point]| {
        ps.iter()
            .map(|p| touch(p) + radius)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let (z0, z1) = (fine[0][0].z, fine[fine.len() - 1][0].z);
    let gap = fine[1][0].z - z0;
    let step = gap / (gap / (radius / ENDING_PLANES)).ceil();
    let (below_to, above_to) = (-reach(&below), reach(&above));
    let before = ((z0 - below_to) / step).ceil().max(0.0) as usize;
    let after = ((above_to - z1) / step).ceil().max(0.0) as usize;
    let count = ((z1 - z0) / step).round() as usize;
    let levels: Vec<f64> = (0..=before + count + after)
        .map(|k| z0 + (k as f64 - before as f64) * step)
        .collect();
    // The plain outline at a height: between the planes on the spline, beyond them the
    // outer one.
    let plain = |z: f64| -> Vec<DVec3> {
        let k = fine.partition_point(|o| o[0].z < z);
        if k == 0 {
            return fine[0].iter().map(|p| DVec3::new(p.x, p.y, z)).collect();
        }
        if k >= fine.len() {
            return fine[fine.len() - 1]
                .iter()
                .map(|p| DVec3::new(p.x, p.y, z))
                .collect();
        }
        let (a, b) = (&fine[k - 1], &fine[k]);
        let t = (z - a[0].z) / (b[0].z - a[0].z);
        a.iter().zip(b).map(|(p, q)| p.lerp(*q, t)).collect()
    };
    levels
        .into_iter()
        .map(|z| {
            plain(z)
                .into_iter()
                .enumerate()
                .map(|(i, p)| {
                    let (b, a) = (&below[i], &above[i]);
                    let out = a.share * flare(z, a.wall, a.slope, radius) * a.normal
                        + b.share * flare(-z, b.wall, b.slope, radius) * b.normal;
                    // Nothing nearer the axis than the cutwater circle: round the nose the
                    // ending rides on it.
                    let q = p.truncate() + out;
                    let q = if q.length() < rz {
                        q * (rz / q.length())
                    } else {
                        q
                    };
                    DVec3::new(q.x, q.y, z)
                })
                .collect()
        })
        .collect()
}

/// A centripetal Catmull–Rom spline through `points`, `n` points per span.
fn catmull_rom(points: &[DVec2], n: usize) -> Vec<DVec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let m = points.len();
    let at = |i: isize| -> DVec2 {
        if i < 0 {
            points[0] * 2.0 - points[1]
        } else if i as usize >= m {
            points[m - 1] * 2.0 - points[m - 2]
        } else {
            points[i as usize]
        }
    };
    let mut out = Vec::with_capacity((m - 1) * n + 1);
    for k in 0..m - 1 {
        let k = k as isize;
        let (p0, p1, p2, p3) = (at(k - 1), at(k), at(k + 1), at(k + 2));
        // Knots spaced by the square root of the chord lengths.
        let step = |a: DVec2, b: DVec2| a.distance(b).sqrt().max(1e-12);
        let (t1, t2, t3) = {
            let t1 = step(p0, p1);
            let t2 = t1 + step(p1, p2);
            (t1, t2, t2 + step(p2, p3))
        };
        for j in 0..n {
            let t = t1 + (t2 - t1) * j as f64 / n as f64;
            let a1 = p0 * ((t1 - t) / t1) + p1 * (t / t1);
            let a2 = p1 * ((t2 - t) / (t2 - t1)) + p2 * ((t - t1) / (t2 - t1));
            let a3 = p2 * ((t3 - t) / (t3 - t2)) + p3 * ((t - t2) / (t3 - t2));
            let b1 = a1 * ((t2 - t) / t2) + a2 * (t / t2);
            let b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1));
            out.push(b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1)));
        }
    }
    out.push(points[m - 1]);
    out
}

/// Unit direction of segment `k` of a polyline.
fn direction(polyline: &[DVec2], k: usize) -> DVec2 {
    (polyline[k + 1] - polyline[k]).normalize()
}

/// Crossing of the segments ab and cd, ends included.
fn segment_crossing(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<DVec2> {
    let (r, s) = (b - a, d - c);
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-18 {
        return None;
    }
    let t = (c - a).perp_dot(s) / denom;
    let u = (c - a).perp_dot(r) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| a + r * t)
}

/// The segment of a polyline nearest to `p` and the nearest point on it.
fn nearest_on(polyline: &[DVec2], p: DVec2) -> (usize, DVec2) {
    (0..polyline.len() - 1)
        .map(|k| (k, closest_on_segment(polyline[k], polyline[k + 1], p)))
        .min_by(|x, y| x.1.distance(p).total_cmp(&y.1.distance(p)))
        .expect("at least one segment")
}

fn closest_on_segment(a: DVec2, b: DVec2, p: DVec2) -> DVec2 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    a + ab * t
}

fn distance_to(polyline: &[DVec2], p: DVec2) -> f64 {
    polyline
        .windows(2)
        .map(|w| closest_on_segment(w[0], w[1], p).distance(p))
        .fold(f64::INFINITY, f64::min)
}

/// The largest radius where a closed outline (r, z) crosses the level `z`.
fn outermost_at(outline: &[DVec2], z: f64) -> Option<f64> {
    let n = outline.len();
    (0..n)
        .filter_map(|i| {
            let (p, q) = (outline[i], outline[(i + 1) % n]);
            if (p.y - z) * (q.y - z) > 0.0 || (q.y - p.y).abs() < 1e-15 {
                return None;
            }
            let t = (z - p.y) / (q.y - p.y);
            Some(p.x + (q.x - p.x) * t)
        })
        .reduce(f64::max)
}

/// The point of a diffuser section at the level `z` nearest the pump axis across the
/// centerline, in (x, y).
fn innermost_at(section: &NozzleSection, z: f64) -> Option<DVec2> {
    let u = section.tangent.cross(DVec3::Z);
    let u = if u.dot(section.center) > 0.0 { u } else { -u };
    let n = section.points.len();
    (0..n)
        .filter_map(|i| {
            let (p, q) = (section.points[i], section.points[(i + 1) % n]);
            if (p.z - z) * (q.z - z) > 0.0 || (q.z - p.z).abs() < 1e-15 {
                return None;
            }
            Some(p.lerp(q, (z - p.z) / (q.z - p.z)))
        })
        .min_by(|a, b| {
            (*a - section.center)
                .dot(u)
                .total_cmp(&(*b - section.center).dot(u))
        })
        .map(|p| p.truncate())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Param;
    use crate::volute::tests::reference_basis;
    use crate::volute::{
        BezierSide, CrossSectionInput, CutMeasure, CutwaterShape, DischargeShape, ExternalRound,
        RadiusSide, SectionShape, SideShape, StraightWall, VoluteGeometry, VoluteInput, section,
    };

    fn build(edit: impl Fn(&mut VoluteInput)) -> VoluteGeometry {
        let mut input = VoluteInput::default();
        edit(&mut input);
        VoluteGeometry::build(&reference_basis(), &input).unwrap()
    }

    /// Every discharge: radial (the default) and tangential with each straight wall.
    fn discharges() -> Vec<DischargeShape> {
        let mut shapes = vec![DischargeShape::default()];
        for straight_wall in [
            StraightWall::Inner,
            StraightWall::Centered,
            StraightWall::Outer,
        ] {
            shapes.push(DischargeShape::Tangential { straight_wall });
        }
        shapes
    }

    #[test]
    fn a_simple_cutwater_closes_the_tip_with_a_cubic_tangent_to_the_walls() {
        for shape in discharges() {
            let g = build(|v| {
                v.diffuser.shape = shape;
                v.cutwater.shape = CutwaterShape::Simple {
                    cut: Param::Manual(0.012),
                    handle: Param::Manual(0.5),
                    measure: CutMeasure::Along,
                };
            });
            assert_eq!(g.cutwater_slices.len(), SLICES, "{shape:?}");
            let sharp = build(|v| {
                v.diffuser.shape = shape;
                v.cutwater.shape = CutwaterShape::Sharp;
            });
            // The same tip and the same start of the spiral as a sharp cutwater.
            assert_eq!(g.cutwater.tip, sharp.cutwater.tip, "{shape:?}");
            assert_eq!(g.start_angle, sharp.start_angle, "{shape:?}");
            for s in &g.cutwater_slices {
                let (j_spiral, j_diffuser) = s.joins.expect("the cuts");
                let arc = s.nose_arc();
                assert_eq!(arc.len(), FILLET_ARC_POINTS);
                assert!(
                    arc[0].distance(j_spiral) < 1e-12
                        && arc[arc.len() - 1].distance(j_diffuser) < 1e-12
                );
                // Cut back from the tip along each wall.
                for join in [j_spiral, j_diffuser] {
                    let back = join.distance(s.tip);
                    assert!(back > 0.9 * 0.012 * 0.9 && back <= 0.012 * 1.001, "{back}");
                }
                // Tangent to each wall at its cut, and smooth between: the curve turns
                // gently from point to point.
                let direction = |a: DVec3, b: DVec3| (b - a).truncate().normalize();
                let (w_spiral, w_diffuser) = (s.trimmed_spiral_wall(), s.trimmed_diffuser_wall());
                let wall_dir = |wall: &[DVec3], join: DVec3| {
                    let k = wall
                        .iter()
                        .enumerate()
                        .min_by(|a, b| a.1.distance(join).total_cmp(&b.1.distance(join)))
                        .map_or(0, |(k, _)| k);
                    direction(wall[k.saturating_sub(1)], wall[(k + 1).min(wall.len() - 1)])
                };
                let at_spiral = direction(arc[0], arc[1]);
                let at_diffuser = direction(arc[arc.len() - 2], arc[arc.len() - 1]);
                assert!(
                    at_spiral.dot(wall_dir(&w_spiral, j_spiral)).abs() > 0.995,
                    "{shape:?} z {}: spiral {:?}",
                    s.z,
                    at_spiral.dot(wall_dir(&w_spiral, j_spiral))
                );
                assert!(at_diffuser.dot(wall_dir(&w_diffuser, j_diffuser)).abs() > 0.995);
                let worst = arc
                    .windows(3)
                    .map(|w| {
                        direction(w[0], w[1])
                            .dot(direction(w[1], w[2]))
                            .clamp(-1.0, 1.0)
                            .acos()
                    })
                    .fold(0.0, f64::max);
                assert!(worst < 35f64.to_radians(), "{worst}");
            }
        }
    }

    #[test]
    fn a_simple_cutwater_cut_across_leaves_the_walls_that_far_apart() {
        let width = 0.003;
        for shape in discharges() {
            let g = build(|v| {
                v.diffuser.shape = shape;
                v.cutwater.shape = CutwaterShape::Simple {
                    cut: Param::Manual(width),
                    handle: Param::Manual(0.5),
                    measure: CutMeasure::Across,
                };
            });
            assert_eq!(g.design.cutwater_cut, width);
            assert_eq!(g.cutwater_slices.len(), SLICES, "{shape:?}");
            for s in &g.cutwater_slices {
                let (j_spiral, j_diffuser) = s.joins.expect("the cuts");
                let flat = |wall: &[DVec3]| wall.iter().map(|p| p.truncate()).collect::<Vec<_>>();
                let (spiral, diffuser) = (flat(&s.spiral_wall), flat(&s.diffuser_wall));
                // Each wall ends where the other lies the width away.
                for (join, other) in [(j_spiral, &diffuser), (j_diffuser, &spiral)] {
                    let gap = distance_to(other, join.truncate());
                    assert!(
                        (gap - width).abs() < 1e-3 * width,
                        "{shape:?} z {}: {gap}",
                        s.z
                    );
                }
                // Still a smooth curve between the cuts.
                let arc = s.nose_arc();
                assert!(arc[0].distance(j_spiral) < 1e-12);
                assert!(arc[arc.len() - 1].distance(j_diffuser) < 1e-12);
            }
        }
    }

    #[test]
    fn a_simple_cutwater_cut_across_is_e3_wide_by_default() {
        let g = build(|v| {
            v.cutwater.shape = CutwaterShape::Simple {
                cut: Param::Auto,
                handle: Param::Auto,
                measure: CutMeasure::Across,
            };
        });
        assert_eq!(g.design.cutwater_cut, g.design.cutwater_thickness);
    }

    #[test]
    fn a_simple_cutwater_saved_without_a_measure_is_cut_along_the_walls() {
        let shape: CutwaterShape = serde_json::from_str(
            r#"{"type":"simple","cut":{"mode":"auto"},"handle":{"mode":"auto"}}"#,
        )
        .unwrap();
        assert!(matches!(
            shape,
            CutwaterShape::Simple {
                measure: CutMeasure::Along,
                ..
            }
        ));
    }

    #[test]
    fn a_fillet_rounds_the_tip_with_an_arc_tangent_to_both_walls() {
        for shape in discharges() {
            let g = build(|v| {
                v.diffuser.shape = shape;
                v.cutwater.shape = CutwaterShape::Fillet {
                    radius: Param::Manual(0.0015),
                };
            });
            assert_eq!(g.cutwater_slices.len(), SLICES);
            for s in &g.cutwater_slices {
                let (c, r) = (s.nose_centre.truncate(), 0.0015);
                let (t_s, t_d) = s.joins.expect("a fillet touches both walls");
                // The centre is r from both walls, which the arc touches.
                let flat = |w: &[DVec3]| w.iter().map(|p| p.truncate()).collect::<Vec<_>>();
                let spiral = flat(&s.trimmed_spiral_wall());
                let diffuser = flat(&s.trimmed_diffuser_wall());
                assert!((distance_to(&spiral, c) - r).abs() < 1e-8, "z = {}", s.z);
                assert!((distance_to(&diffuser, c) - r).abs() < 1e-8, "z = {}", s.z);
                assert!((t_s.truncate().distance(c) - r).abs() < 1e-8);
                assert!((t_d.truncate().distance(c) - r).abs() < 1e-8);
                // The walls start at the arc, and the round cuts the tip off: the sliver
                // is closed and the nearest point of the nose is behind the tip.
                assert_eq!(spiral[0], t_s.truncate());
                assert_eq!(diffuser[0], t_d.truncate());
                assert!(s.removed.len() > 4);
                assert!(s.nose_centre.distance(s.tip) > r);
                assert_eq!(s.outline.len(), 2 * FILLET_WALL_POINTS + FILLET_ARC_POINTS);
            }
            // The effective diameter follows the round, not d_z.
            let values = g.cutwater_values().unwrap();
            let nearest = g
                .cutwater_slices
                .iter()
                .map(|s| s.nose_distance)
                .fold(f64::INFINITY, f64::min);
            assert!((values.least_diameter - 2.0 * nearest).abs() < 1e-12);
            assert!(values.least_diameter <= 2.0 * g.cutwater.nose_distance + 1e-12);
        }
    }

    #[test]
    fn a_fillet_is_not_warned_of_while_the_cutwater_stands_above_the_minimum() {
        let g = build(|v| {
            v.cutwater.shape = CutwaterShape::Fillet {
                radius: Param::Manual(0.0015),
            }
        });
        let effective = g.cutwater_values().unwrap().least_diameter;
        assert!(effective >= g.design.min_cutwater_diameter);
        assert!(g.warnings.iter().all(|w| !w.contains("fillet leaves")));
    }

    #[test]
    fn a_fillet_that_does_not_fit_between_the_walls_is_refused() {
        let mut input = VoluteInput::default();
        input.cutwater.shape = CutwaterShape::Fillet {
            radius: Param::Manual(0.5),
        };
        let err = VoluteGeometry::build(&reference_basis(), &input);
        assert!(err.is_err(), "a 500 mm fillet cannot fit");
    }

    #[test]
    fn the_cut_at_any_height_finds_the_tip_until_the_wedge_closes() {
        let g = build(|v| v.cutwater.shape = CutwaterShape::Sharp);
        let zm = g.basis.midplane;
        let half = g.design.inlet_width / 2.0;
        // In the midplane the tip is the cutwater's.
        let mid = g.tongue_cut(zm).expect("the walls in the midplane");
        assert_eq!(mid.crossings, 1);
        let tip = mid.tip.expect("a tip");
        assert!(tip.truncate().distance(g.cutwater.tip.truncate()) < 1e-6);
        // Across the inlet width the tip stays put; well past it the wedge has closed:
        // the spiral starts there only where it already lies inside the diffuser wall.
        for dz in [-0.9 * half, 0.5 * half, 0.9 * half] {
            let cut = g.tongue_cut(zm + dz).expect("the walls across the width");
            assert_eq!(cut.crossings, 1, "{dz}");
            assert!(cut.tip.unwrap().truncate().distance(tip.truncate()) < 1e-3);
        }
        for dz in [-(half + 3e-3), half + 3e-3] {
            if let Some(cut) = g.tongue_cut(zm + dz) {
                assert_eq!(cut.crossings, 0, "{dz}");
            }
        }
    }

    #[test]
    fn the_trimmed_walls_meet_only_at_the_tip() {
        for shape in discharges() {
            let g = build(|v| {
                v.diffuser.shape = shape;
                v.cutwater.shape = CutwaterShape::Sharp;
            });
            for s in &g.cutwater_slices {
                let flat = |w: Vec<DVec3>| w.iter().map(|p| p.truncate()).collect::<Vec<_>>();
                let (spiral, diffuser) = (
                    flat(s.trimmed_spiral_wall()),
                    flat(s.trimmed_diffuser_wall()),
                );
                assert_eq!(spiral[0], s.tip.truncate());
                assert_eq!(diffuser[0], s.tip.truncate());
                assert!(
                    spiral.len() < s.spiral_wall.len() && diffuser.len() <= s.diffuser_wall.len()
                );
                let mut apart = 0;
                for i in 0..spiral.len() - 1 {
                    for j in 0..diffuser.len() - 1 {
                        if let Some(p) =
                            segment_crossing(spiral[i], spiral[i + 1], diffuser[j], diffuser[j + 1])
                            && p.distance(s.tip.truncate()) > 1e-5
                        {
                            apart += 1;
                        }
                    }
                }
                assert_eq!(apart, 0, "{shape:?} at z = {:.4}", s.z);
            }
        }
    }

    #[test]
    fn the_rounded_cutwater_is_nowhere_thinner_than_its_nose() {
        for shape in discharges() {
            let g = build(|v| v.diffuser.shape = shape);
            let e = 2.0 * g.design.nose_radius;
            assert_eq!(g.cutwater_slices.len(), SLICES, "{shape:?}");
            for s in &g.cutwater_slices {
                assert!(!s.outline.is_empty(), "{shape:?}");
                assert!(
                    s.min_thickness > e * (1.0 - 1e-3),
                    "{shape:?}: {}",
                    s.min_thickness
                );
                // The faces run into the walls past the tip, within a few centimetres.
                assert!(s.join_length > 0.0 && s.join_length < 0.05, "{shape:?}");
            }
        }
    }

    #[test]
    fn every_kind_of_cross_section_gets_its_cutwater_across_the_width() {
        let single = |shape| CrossSectionInput::single(shape);
        for sections in [
            CrossSectionInput::default(),
            single(SectionShape::rectangular()),
            single(SectionShape::round()),
            single(SectionShape::symmetric(SideShape::RadiusBased(
                RadiusSide::default(),
            ))),
            single(SectionShape::symmetric(SideShape::Bezier(
                BezierSide::default(),
            ))),
            single(SectionShape::symmetric(SideShape::LineSegments {
                points: section::default_segments(),
            })),
            single(SectionShape::RoundExternal(ExternalRound::default())),
        ] {
            let g = build(|v| {
                v.cross_section = sections.clone();
                v.cutwater.shape = CutwaterShape::Sharp;
            });
            assert_eq!(g.cutwater_slices.len(), SLICES, "{sections:?}");
        }
    }

    #[test]
    fn round_sections_move_the_tip_across_the_width() {
        // Straight-sided sections cut the diffuser along a line parallel to the axis;
        // round ones along a curve that bows outward in the midplane.
        let radius = |g: &VoluteGeometry, k: usize| g.cutwater_slices[k].tip.truncate().length();
        let straight =
            build(|v| v.cross_section = CrossSectionInput::single(SectionShape::rectangular()));
        let spread = (0..SLICES).map(|k| radius(&straight, k));
        let (lo, hi) = spread.fold((f64::INFINITY, 0.0f64), |(lo, hi), r| {
            (lo.min(r), hi.max(r))
        });
        assert!(hi - lo < 1e-4, "{}", (hi - lo) * 1e3);
        let round = build(|v| v.cross_section = CrossSectionInput::single(SectionShape::round()));
        let middle = radius(&round, SLICES / 2);
        assert!(middle > radius(&round, 0) + 2e-3);
        assert!(middle > radius(&round, SLICES - 1) + 2e-3);
        // Symmetric sections, a symmetric cutwater.
        for k in 0..SLICES {
            let mirror = SLICES - 1 - k;
            assert!((radius(&round, k) - radius(&round, mirror)).abs() < 1e-9);
        }
    }

    #[test]
    fn the_outlines_between_the_planes_pass_through_theirs_smoothly() {
        let g = build(|v| v.cross_section = CrossSectionInput::single(SectionShape::round()));
        let steps = 4;
        let fine = fine_outlines(&g.cutwater_slices, steps);
        assert_eq!(fine.len(), (SLICES - 1) * steps + 1);
        for (k, slice) in g.cutwater_slices.iter().enumerate() {
            assert_eq!(fine[k * steps], slice.outline);
        }
        // Each outline in its own plane, and every point on a smooth curve across them:
        // the second differences along z stay small next to the first ones.
        for outline in &fine {
            assert!(outline.iter().all(|p| (p.z - outline[0].z).abs() < 1e-12));
        }
        for i in 0..fine[0].len() {
            let along: Vec<DVec3> = fine.iter().map(|o| o[i]).collect();
            let step = along
                .windows(2)
                .map(|w| w[0].distance(w[1]))
                .fold(0.0, f64::max);
            let bend = along
                .windows(3)
                .map(|w| (w[0] + w[2] - w[1] * 2.0).length())
                .fold(0.0, f64::max);
            assert!(bend < 0.25 * step, "{i}: {bend} vs {step}");
        }
    }

    #[test]
    fn the_spiral_starts_before_its_wall_crosses_the_diffuser_in_every_plane() {
        let g = build(|_| {});
        let (lo, hi) = g.start_range.unwrap();
        // Automatic: 3°, within the range, and before the tip in every plane.
        assert!((g.start_angle - 3f64.to_radians()).abs() < 1e-12);
        assert!(lo <= g.start_angle && g.start_angle <= hi);
        let sign = g.basis.rotation_sign;
        let unturned = |p: &DVec3| {
            let a = sign * p.y.atan2(p.x) - sign * g.cutwater.tip.y.atan2(g.cutwater.tip.x);
            a.rem_euclid(std::f64::consts::TAU)
        };
        let tip = g.cutwater.angle;
        for s in &g.cutwater_slices {
            assert!(
                tip + unturned(&s.tip) > hi,
                "{}",
                (tip + unturned(&s.tip)).to_degrees()
            );
        }
        // By hand within the range: the spiral starts there and the cutwater is built.
        let late = 0.5 * (g.start_angle + hi);
        let moved = build(|v| v.cutwater.spiral_start = Param::Manual(late));
        assert!((moved.start_angle - late).abs() < 1e-12);
        assert_eq!(moved.start_range, g.start_range);
        assert!(!moved.cutwater_slices.is_empty());
        // Past the range it says where the range is.
        let err = VoluteGeometry::build(&reference_basis(), &{
            let mut v = VoluteInput::default();
            v.cutwater.spiral_start = Param::Manual(hi + 1f64.to_radians());
            v
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("must start between"), "{err}");
    }

    #[test]
    fn the_walls_are_counted_where_they_cross() {
        let line = |points: &[(f64, f64)]| -> Vec<DVec2> {
            points.iter().map(|&(x, y)| DVec2::new(x, y)).collect()
        };
        // A spiral wall running up through a straight diffuser wall: one crossing.
        let walls = Walls {
            spiral: line(&[(0.0, -1.0), (1.0, 1.0)]),
            diffuser: line(&[(-1.0, 0.0), (2.0, 0.0)]),
        };
        assert_eq!(walls.count_crossings(), 1);
        // It comes back down through it again: two. A crossing at a vertex counts once.
        let walls = Walls {
            spiral: line(&[(0.0, -1.0), (1.0, 1.0), (2.0, -1.0)]),
            diffuser: line(&[(-1.0, 0.0), (1.0, 0.0), (3.0, 0.0)]),
        };
        assert_eq!(walls.count_crossings(), 2);
        // Apart: none.
        let walls = Walls {
            spiral: line(&[(0.0, 1.0), (1.0, 2.0)]),
            diffuser: line(&[(-1.0, 0.0), (2.0, 0.0)]),
        };
        assert_eq!(walls.count_crossings(), 0);
    }

    #[test]
    fn a_sharp_cutwater_needs_a_true_wedge_in_every_plane() {
        // In round sections with the inner wall of the diffuser straight the walls only
        // graze near the side walls: a sharp cutwater would be a knife there, so the
        // spiral starts where it clears the diffuser. A rounded one fills them.
        let round = |shape: crate::volute::CutwaterShape| {
            build(|v| {
                v.cross_section = CrossSectionInput::single(SectionShape::round());
                v.diffuser.shape = DischargeShape::Tangential {
                    straight_wall: StraightWall::Inner,
                };
                v.cutwater.shape = shape;
            })
        };
        let sharp = round(crate::volute::CutwaterShape::Sharp);
        assert!(sharp.start_range.is_none());
        assert!(sharp.start_angle > 10f64.to_radians());
        let rounded = round(crate::volute::CutwaterShape::default());
        assert!(rounded.start_range.is_some());
        assert!((rounded.start_angle - 3f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn the_cutwater_values_follow_the_planes_and_the_sections() {
        let g = build(|_| {});
        let v = g.cutwater_values().unwrap();
        // The least nose diameter is the effective one, and the planes bracket the mean.
        let least = g
            .cutwater_slices
            .iter()
            .map(|s| 2.0 * s.nose_distance)
            .fold(f64::INFINITY, f64::min);
        assert!((v.least_diameter - least).abs() < 1e-15);
        assert!(v.least_diameter <= v.diameter.mean + 1e-15);
        assert!(v.diameter.lower.min(v.diameter.upper) >= v.least_diameter - 1e-15);
        // Equivalent diameters are the circles of the sections' areas.
        for s in [v.start, v.throat, v.outlet] {
            let circle = std::f64::consts::PI * s.equivalent_diameter * s.equivalent_diameter / 4.0;
            assert!((circle - s.area).abs() < 1e-12 * s.area.max(1.0));
        }
        // The cutwater section is the start of the spiral.
        assert!((v.start.area - g.sections[0].size.area).abs() < 1e-15);
        assert!((v.position - g.cutwater.angle).abs() < 1e-15);
    }

    #[test]
    fn a_sharp_cutwater_is_the_bare_wedge_in_every_plane() {
        let g = build(|v| v.cutwater.shape = CutwaterShape::Sharp);
        assert_eq!(g.cutwater_slices.len(), SLICES);
        for s in &g.cutwater_slices {
            assert!(s.outline.is_empty());
            assert_eq!(s.min_thickness, 0.0);
            assert!((s.nose_distance - s.tip.truncate().length()).abs() < 1e-12);
            assert!(s.wedge > 0.0 && s.wedge < 30f64.to_radians());
        }
        // A tangent cutwater does not cut the diffuser wall.
        let tangent = build(|v| v.cutwater.shape = CutwaterShape::Tangent);
        assert!(tangent.cutwater_slices.is_empty());
        assert!(tangent.cutwater_outline().is_empty());
    }

    #[test]
    fn the_flow_meets_the_cutwater_flatter_at_part_load() {
        let g = build(|_| {});
        let d = &g.design;
        assert!((d.cutwater_flow_angle_at(1.0, 1.0) - d.cutwater_flow_angle).abs() < 1e-12);
        // Less flow and more theoretical head: a flatter flow, positive incidence on the
        // cutwater (Gülich Fig. 9.20).
        let part = d.cutwater_flow_angle_at(0.5, 1.15);
        assert!(part < d.cutwater_flow_angle && part > 0.0);
        assert!(d.cutwater_flow_angle_at(1.3, 0.8) > d.cutwater_flow_angle);
    }

    #[test]
    fn every_point_of_every_plane_is_finite() {
        for shape in discharges() {
            let g = build(|v| v.diffuser.shape = shape);
            for s in &g.cutwater_slices {
                for p in s
                    .outline
                    .iter()
                    .chain(&s.spiral_wall)
                    .chain(&s.diffuser_wall)
                {
                    assert!(p.is_finite(), "{shape:?} at z = {}: {p}", s.z);
                }
            }
        }
    }

    #[test]
    fn the_spline_through_the_diffuser_sections_passes_through_them() {
        let points = [
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 0.2),
            DVec2::new(2.0, 1.0),
            DVec2::new(3.0, 2.5),
        ];
        let curve = catmull_rom(&points, 8);
        assert_eq!(curve.len(), 3 * 8 + 1);
        for (k, p) in points.iter().enumerate() {
            assert!(curve[k * 8].distance(*p) < 1e-12);
        }
        // Straight points stay on their line.
        let line = catmull_rom(&[DVec2::ZERO, DVec2::X, DVec2::X * 2.0], 5);
        assert!(line.iter().all(|p| p.y.abs() < 1e-12));
    }
}
