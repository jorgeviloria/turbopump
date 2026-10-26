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

use tp_math::{Bezier, DVec2, DVec3};

use super::NozzleSection;
use super::{Continuity, CutwaterShape, VoluteDesign};
use crate::impeller::edge::EdgeProfile;

/// Planes across the inlet width, an odd number so that one is the midplane.
pub const SLICES: usize = 9;
/// The outer planes stay this fraction of the half width inside the side walls.
const EDGE_MARGIN: f64 = 0.05;
/// Points between consecutive spiral sections on the spline through them.
const SPIRAL_SUBDIVISIONS: usize = 4;
/// Points between consecutive diffuser sections on the spline through them.
const DIFFUSER_SUBDIVISIONS: usize = 24;
/// Points on each face of the body.
const FACE_POINTS: usize = 48;
/// Width of the smooth maximum that runs the faces into the walls, in nose thicknesses.
const SMOOTHING: f64 = 0.25;

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
}

/// The cutwater in planes across the inlet width, from −z to +z. `outlines` are the
/// fitted sections (r, z) of the spiral at their wrap angles, from its start, a degree
/// or less apart; `nozzle` the sections of the diffuser before the volute is turned. Planes
/// where the walls do not meet are left out. Also the centre of the nose circle, for a
/// rounded cutwater.
pub(super) fn slices(
    design: &VoluteDesign,
    outlines: &[(f64, Vec<DVec2>)],
    nozzle: &[NozzleSection],
    sign: f64,
    midplane: f64,
) -> (Vec<CutwaterSlice>, Option<DVec2>) {
    let half = 0.5 * design.inlet_width * (1.0 - EDGE_MARGIN);
    let levels: Vec<f64> = (0..SLICES)
        .map(|k| midplane - half + 2.0 * half * k as f64 / (SLICES - 1) as f64)
        .collect();
    let rounded = matches!(design.cutwater_shape, CutwaterShape::Rounded { .. });
    // The nose centre is set in the midplane and kept in the other planes.
    let mid = Walls::at(outlines, nozzle, sign, midplane);
    let centre = match (&mid, rounded) {
        (Some(w), true) => w.nose_centre(design, sign),
        _ => None,
    };
    let slices = levels
        .into_iter()
        .filter_map(|z| {
            let walls = Walls::at(outlines, nozzle, sign, z)?;
            let turned = centre.map(|c| oblique(c, design.cutwater_obliquity, sign, z - midplane));
            walls.slice(design, sign, z, turned)
        })
        .collect();
    (slices, centre)
}

/// How much thinner than its nose a rounded cutwater may get in a plane, as a share of
/// the nose thickness: an oblique cutwater squeezes between the cutwater circle and
/// the walls.
pub const THINNING: f64 = 0.05;

/// Whether a rounded cutwater reaches the walls in all `SLICES` planes, nowhere thinner
/// than its nose `e` (within [`THINNING`]).
pub fn sound(slices: &[CutwaterSlice], e: f64) -> bool {
    slices.len() == SLICES
        && slices
            .iter()
            .all(|s| !s.outline.is_empty() && s.min_thickness >= e * (1.0 - THINNING))
}

/// The point `point` of the midplane turned about the pump axis to the plane `dz` from
/// it, so that the line through the turned points makes the angle `obliquity` with the
/// axis unrolled onto their circle: downstream toward +z for a positive angle.
pub fn oblique(point: DVec2, obliquity: f64, sign: f64, dz: f64) -> DVec2 {
    let r = point.length();
    if r <= 0.0 {
        return point;
    }
    DVec2::from_angle(sign * dz * obliquity.tan() / r).rotate(point)
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

    /// The nose centre M of the midplane: on the stagnation radius on the x axis,
    /// moved radially until the innermost point of the cutwater lies on d_z.
    fn nose_centre(&self, design: &VoluteDesign, sign: f64) -> Option<DVec2> {
        let rz = design.cutwater_diameter / 2.0;
        let mut centre = DVec2::new(design.stagnation_radius, 0.0);
        for _ in 0..4 {
            let body = self.body(design, sign, centre)?;
            let nearest = body
                .outline
                .iter()
                .map(|p| p.length())
                .fold(f64::INFINITY, f64::min);
            if (nearest - rz).abs() < 1e-9 {
                break;
            }
            centre.x += rz - nearest;
        }
        Some(centre)
    }

    fn slice(
        &self,
        design: &VoluteDesign,
        sign: f64,
        z: f64,
        centre: Option<DVec2>,
    ) -> Option<CutwaterSlice> {
        let (s, d) = self.tip();
        let ts = direction(&self.spiral, s.segment);
        let td = direction(&self.diffuser, d.segment);
        let wedge = ts.dot(td).clamp(-1.0, 1.0).acos();
        let at = |p: DVec2| DVec3::new(p.x, p.y, z);
        let spiral_wall: Vec<DVec3> = self.spiral.iter().map(|p| at(*p)).collect();
        let diffuser_wall: Vec<DVec3> = self.diffuser.iter().map(|p| at(*p)).collect();
        let Some(centre) = centre else {
            return Some(CutwaterSlice {
                z,
                tip: at(s.point),
                wedge,
                outline: Vec::new(),
                min_thickness: 0.0,
                nose_distance: s.point.length(),
                join_length: 0.0,
                spiral_wall,
                diffuser_wall,
                nose_centre: at(s.point),
            });
        };
        let body = self.body(design, sign, centre)?;
        Some(CutwaterSlice {
            z,
            tip: at(s.point),
            wedge,
            nose_distance: body
                .outline
                .iter()
                .map(|p| p.length())
                .fold(f64::INFINITY, f64::min),
            outline: body.outline.into_iter().map(at).collect(),
            min_thickness: body.min_thickness,
            join_length: body.join_length,
            spiral_wall,
            diffuser_wall,
            nose_centre: at(centre),
        })
    }

    /// The rounded body with its nose centred on `centre`.
    fn body(&self, design: &VoluteDesign, sign: f64, centre: DVec2) -> Option<Body> {
        let h = design.nose_radius;
        if h <= 0.0 {
            return None;
        }
        // Frame at the nose: downstream along the circumference, outward, and the camber
        // line at α₃B between them.
        let radial = centre.normalize();
        let along = DVec2::new(-radial.y, radial.x) * sign;
        let (sa, ca) = design.cutwater_angle.sin_cos();
        let camber = along * ca + radial * sa;
        let normal = radial * ca - along * sa;
        let profile = EdgeProfile::new(design.cutwater_nose, 2.0 * h).ok()?;
        let lead = centre - camber * profile.length();
        let [outer_nose, inner_nose] = profile.faces().map(|face| -> Vec<DVec2> {
            face.iter()
                .map(|p| lead + camber * p.x + normal * (h * p.y))
                .collect()
        });
        let e = 2.0 * h;

        let (s, d) = self.tip();
        let spiral = Path::from(&self.spiral, s);
        let diffuser = Path::from(&self.diffuser, d);
        let longest = spiral.length.min(diffuser.length);
        // Past the tip, `l` along both walls: across them, pointing from the spiral wall
        // to the diffuser wall, and the gap between them, the wedge.
        let across = |l: f64| -> DVec2 {
            let t = spiral.point(l).1 + diffuser.point(l).1;
            let n = DVec2::new(-t.y, t.x).normalize();
            if n.dot(spiral.point(l).0) > 0.0 {
                n
            } else {
                -n
            }
        };
        let gap = |l: f64| (diffuser.point(l).0 - spiral.point(l).0).dot(across(l));
        // Each face is its wall moved into the flow by its share of what the wedge lacks
        // of the thickness e, smoothed (a quadratic smooth maximum with zero, C¹) so
        // that it runs into the wall tangentially where the wedge is e + k thick.
        // The faces for a body `target` thick at the walls: with an uneven share they
        // lean on the normal they are moved along, so the thinnest part is checked
        // and the target raised until it is e.
        let build = |target: f64| {
            let k = SMOOTHING * e;
            let lack = |l: f64| -> f64 {
                let g = target - gap(l);
                if g >= k {
                    g
                } else if g <= -k {
                    0.0
                } else {
                    match design.cutwater_join {
                        // Quadratic: C¹, the faces tangent to the walls.
                        Continuity::G1 => (g + k) * (g + k) / (4.0 * k),
                        // Quartic: C², the curvature of the walls carried on too.
                        Continuity::G2 => (g + k).powi(3) * (3.0 * k - g) / (16.0 * k.powi(3)),
                    }
                }
            };
            let join = if gap(longest) < target + k {
                longest
            } else {
                let (mut lo, mut hi) = (0.0, longest);
                for _ in 0..40 {
                    let mid = 0.5 * (lo + hi);
                    if gap(mid) < target + k {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                hi
            };
            let share = design.cutwater_share;
            let stations: Vec<f64> = (0..=FACE_POINTS)
                .map(|i| join * i as f64 / FACE_POINTS as f64)
                .collect();
            let inner_wall: Vec<DVec2> = stations
                .iter()
                .map(|&l| spiral.point(l).0 - across(l) * (share * lack(l)))
                .collect();
            let outer_wall: Vec<DVec2> = stations
                .iter()
                .map(|&l| diffuser.point(l).0 + across(l) * ((1.0 - share) * lack(l)))
                .collect();
            // From the nose to the tip, a camber line leaving the nose at α₃B and reaching
            // the middle of the faces at the tip along them; the faces e/2 to each side.
            let end = 0.5 * (inner_wall[0] + outer_wall[0]);
            let end_dir =
                (inner_wall[1] + outer_wall[1] - inner_wall[0] - outer_wall[0]).normalize();
            let reach = centre.distance(end) / 3.0;
            let camber_line = Bezier::new(vec![
                centre,
                centre + camber * reach,
                end - end_dir * reach,
                end,
            ])
            .expect("four points");
            let slope = camber_line.derivative();
            let (inner_lead, outer_lead): (Vec<DVec2>, Vec<DVec2>) = (0..=FACE_POINTS)
                .map(|i| {
                    let t = i as f64 / FACE_POINTS as f64;
                    let tangent = slope.eval(t).normalize();
                    let n = DVec2::new(-tangent.y, tangent.x);
                    let n = if n.dot(normal) > 0.0 { n } else { -n };
                    let p = camber_line.eval(t);
                    (p - n * (0.5 * e), p + n * (0.5 * e))
                })
                .unzip();
            // Nothing of the body comes nearer the axis than the cutwater circle: where
            // a turned nose (an oblique cutwater) leads the spiral face along a wall
            // close to it, the face follows the circle.
            let rz = design.cutwater_diameter / 2.0;
            let inner: Vec<DVec2> = inner_lead
                .iter()
                .chain(inner_wall.iter().skip(1))
                .map(|p| {
                    if p.length() < rz {
                        *p * (rz / p.length())
                    } else {
                        *p
                    }
                })
                .collect();
            let outer: Vec<DVec2> = outer_lead
                .iter()
                .chain(outer_wall.iter().skip(1))
                .copied()
                .collect();
            // Thickness behind the nose: from each face to the other side, the other face
            // or the wall past its join.
            // Only the wall near the join can be the nearest other side.
            let reach = join + 4.0 * e;
            let other_outer: Vec<DVec2> = outer
                .iter()
                .copied()
                .chain(diffuser.between(join, reach))
                .collect();
            let other_inner: Vec<DVec2> = inner
                .iter()
                .copied()
                .chain(spiral.between(join, reach))
                .collect();
            let thinnest = inner
                .iter()
                .map(|p| distance_to(&other_outer, *p))
                .chain(outer.iter().map(|p| distance_to(&other_inner, *p)))
                .fold(f64::INFINITY, f64::min);
            (inner, outer, thinnest, join)
        };
        let (mut inner, mut outer, mut thinnest, mut join) = build(e);
        let mut target = e;
        for _ in 0..3 {
            if thinnest >= e * (1.0 - 1e-3) {
                break;
            }
            target *= e / thinnest;
            (inner, outer, thinnest, join) = build(target);
        }
        // From the spiral wall back along the inner face, round the nose, and along the
        // outer face to the diffuser wall.
        let mut outline: Vec<DVec2> = inner.iter().rev().copied().collect();
        outline.extend(inner_nose.iter().rev().skip(1));
        outline.extend(outer_nose.iter().skip(1));
        outline.extend(outer.iter().skip(1));
        outline.dedup_by(|a, b| a.distance(*b) < 1e-12);
        Some(Body {
            outline,
            min_thickness: thinnest,
            join_length: join,
        })
    }
}

struct Body {
    outline: Vec<DVec2>,
    min_thickness: f64,
    join_length: f64,
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

    /// The polyline from `from` to `to` along.
    fn between(&self, from: f64, to: f64) -> impl Iterator<Item = DVec2> + '_ {
        let (first, last) = (self.point(from).0, self.point(to).0);
        std::iter::once(first)
            .chain(
                self.points
                    .iter()
                    .zip(&self.lengths)
                    .filter(move |(_, s)| **s > from && **s < to)
                    .map(|(p, _)| *p),
            )
            .chain(std::iter::once(last))
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
        BezierSide, CrossSectionInput, DischargeShape, ExternalRound, RadiusSide, SectionShape,
        SideShape, StraightWall, VoluteGeometry, VoluteInput, section,
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

    /// The angle the outline turns at each of its inner points.
    fn turning_angles(outline: &[DVec3]) -> impl Iterator<Item = f64> + '_ {
        outline.windows(3).map(|w| {
            let (a, b) = ((w[1] - w[0]).normalize(), (w[2] - w[1]).normalize());
            a.dot(b).clamp(-1.0, 1.0).acos()
        })
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
    fn the_nose_lies_on_the_cutwater_circle_in_every_plane() {
        for shape in discharges() {
            let g = build(|v| v.diffuser.shape = shape);
            let rz = g.design.cutwater_diameter / 2.0;
            for s in &g.cutwater_slices {
                assert!(
                    (s.nose_distance - rz).abs() < 1e-6,
                    "{shape:?}: {}",
                    s.nose_distance
                );
            }
            assert!((g.cutwater.nose_distance - rz).abs() < 1e-9);
            // The circle of the nose sits on r'_z but for the little its innermost point
            // needs to reach d_z.
            let centre = g.cutwater.nose_center.truncate().length();
            assert!((centre - g.design.stagnation_radius).abs() < 0.1 * g.design.nose_radius);
        }
    }

    #[test]
    fn the_nose_points_along_the_blade_angle() {
        for incidence in [-3f64, 0.0, 3.0] {
            let g = build(|v| v.cutwater.incidence = Param::Manual(incidence.to_radians()));
            let (m, h) = (g.cutwater.nose_center, g.design.nose_radius);
            // The ends of the half circle of the nose lie across the camber line.
            let ends: Vec<DVec3> = g
                .cutwater_outline()
                .iter()
                .filter(|p| ((**p - m).length() - h).abs() < 1e-9 * h.max(1.0))
                .copied()
                .collect();
            let (a, b) = ends
                .iter()
                .flat_map(|a| ends.iter().map(move |b| (*a, *b)))
                .max_by(|x, y| x.0.distance(x.1).total_cmp(&y.0.distance(y.1)))
                .unwrap();
            assert!((a.distance(b) - 2.0 * h).abs() < 1e-9);
            // Across the camber line, the radial direction turned by α₃B.
            let across = (b - a).truncate().normalize();
            let radial = m.truncate().normalize();
            let angle = across.dot(radial).abs().clamp(-1.0, 1.0).acos();
            assert!(
                (angle - g.design.cutwater_angle).abs() < 1e-6,
                "{incidence}: {} vs {}",
                angle.to_degrees(),
                g.design.cutwater_angle.to_degrees()
            );
        }
    }

    #[test]
    fn the_outline_turns_smoothly_into_the_walls() {
        for shape in discharges() {
            let g = build(|v| v.diffuser.shape = shape);
            for s in &g.cutwater_slices {
                // G1: no corner along the outline, nose included.
                let worst = turning_angles(&s.outline).fold(0.0, f64::max);
                assert!(
                    worst < 3f64.to_radians(),
                    "{shape:?}: {}°",
                    worst.to_degrees()
                );
            }
            // Where it starts, the outline leaves along the outer wall of the spiral.
            let outline = g.cutwater_outline();
            let start = outline[0];
            let k = (0..g.outer_wall.len() - 1)
                .min_by(|&i, &j| {
                    g.outer_wall[i]
                        .distance(start)
                        .total_cmp(&g.outer_wall[j].distance(start))
                })
                .unwrap();
            let wall = (g.outer_wall[k + 1] - g.outer_wall[k]).normalize();
            let face = (outline[0] - outline[1]).normalize();
            assert!(g.outer_wall[k].distance(start) < 1e-3, "{shape:?}");
            assert!(wall.dot(face) > 2f64.to_radians().cos(), "{shape:?}");
        }
    }

    #[test]
    fn the_outline_does_not_cross_itself() {
        let crosses = |p: &[DVec3]| {
            let n = p.len();
            (0..n).any(|i| {
                (i + 2..n).any(|j| {
                    if i == 0 && j == n - 1 {
                        return false;
                    }
                    let (a, b) = (p[i].truncate(), p[(i + 1) % n].truncate());
                    let (c, d) = (p[j].truncate(), p[(j + 1) % n].truncate());
                    segment_crossing(a, b, c, d).is_some()
                })
            })
        };
        for shape in discharges() {
            let g = build(|v| v.diffuser.shape = shape);
            for s in &g.cutwater_slices {
                assert!(!crosses(&s.outline), "{shape:?} at z = {}", s.z);
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
            let g = build(|v| v.cross_section = sections.clone());
            let e = 2.0 * g.design.nose_radius;
            let rz = g.design.cutwater_diameter / 2.0;
            assert_eq!(g.cutwater_slices.len(), SLICES, "{sections:?}");
            for s in &g.cutwater_slices {
                assert!(s.min_thickness > e * (1.0 - 1e-3), "{sections:?}");
                assert!((s.nose_distance - rz).abs() < 1e-6, "{sections:?}");
            }
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
    fn an_oblique_cutwater_turns_its_nose_across_the_width() {
        let obliquity = 35f64.to_radians();
        let g = build(|v| v.cutwater.obliquity = Param::Manual(obliquity));
        let e = 2.0 * g.design.nose_radius;
        let rz = g.design.cutwater_diameter / 2.0;
        let sign = g.basis.rotation_sign;
        assert_eq!(g.cutwater_slices.len(), SLICES);
        // The innermost point of the nose, on d_z in every plane, and its wrap angle.
        let nose = |s: &CutwaterSlice| {
            let p = s
                .outline
                .iter()
                .min_by(|a, b| a.truncate().length().total_cmp(&b.truncate().length()))
                .copied()
                .unwrap();
            sign * p.y.atan2(p.x)
        };
        for s in &g.cutwater_slices {
            assert!((s.nose_distance - rz).abs() < 1e-6, "{}", s.nose_distance);
            assert!(s.min_thickness > e * (1.0 - 1e-3), "{}", s.min_thickness);
        }
        // Unrolled on d_z, the edge leans by ε, downstream toward +z.
        let (first, last) = (&g.cutwater_slices[0], &g.cutwater_slices[SLICES - 1]);
        let arc = (nose(last) - nose(first)) * rz;
        let expected = (last.z - first.z) * obliquity.tan();
        assert!((arc / expected - 1.0).abs() < 0.02, "{arc} vs {expected}");
        // The midplane stays where it was.
        let straight = build(|_| {});
        let mid = SLICES / 2;
        assert!(
            (nose(&g.cutwater_slices[mid]) - nose(&straight.cutwater_slices[mid])).abs() < 1e-9
        );
    }

    #[test]
    fn a_cutwater_leans_only_as_far_as_the_walls_leave_it_room() {
        // Round sections squeeze an oblique cutwater between d_z and the diffuser wall
        // near the side walls.
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::round()),
            ..VoluteInput::default()
        };
        input.cutwater.obliquity = Param::Manual(35f64.to_radians());
        let err = VoluteGeometry::build(&reference_basis(), &input)
            .unwrap_err()
            .to_string();
        assert!(err.contains("past about"), "{err}");
        // Within that limit it builds, nowhere inside d_z.
        let g = build(|v| {
            v.cross_section = CrossSectionInput::single(SectionShape::round());
            v.cutwater.obliquity = Param::Manual(15f64.to_radians());
        });
        let rz = g.design.cutwater_diameter / 2.0;
        assert!(sound(&g.cutwater_slices, 2.0 * g.design.nose_radius));
        assert!(
            g.cutwater_slices
                .iter()
                .all(|s| s.nose_distance > rz - 1e-9)
        );
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
        assert!(sound(
            &moved.cutwater_slices,
            2.0 * moved.design.nose_radius
        ));
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
    fn the_body_rounds_into_the_side_walls() {
        let rectangles = |edit: &dyn Fn(&mut VoluteInput)| {
            build(|v| {
                v.cross_section = CrossSectionInput::single(SectionShape::rectangular());
                edit(v);
            })
        };
        let g = rectangles(&|_| {});
        let plain = rectangles(&|v| v.cutwater.side_radius = Param::Manual(0.0));
        let radius = g.design.cutwater_side_radius;
        assert!((radius - g.design.nose_radius).abs() < 1e-12);
        let (body, flat) = (g.cutwater_body(), plain.cutwater_body());
        let zm = g.basis.midplane;
        let wall = zm + 0.5 * g.design.inlet_width;
        // The planes keep one spacing, no more than half the radius, past the walls.
        let gaps: Vec<f64> = body.windows(2).map(|w| w[1][0].z - w[0][0].z).collect();
        assert!(
            gaps.iter()
                .all(|g| (g - gaps[0]).abs() < 1e-9 && *g <= 0.5 * radius + 1e-12)
        );
        assert!(body.last().unwrap()[0].z >= wall);
        // Away from the walls the body is the same; at a flat side wall it stands out by
        // the whole radius where it does not run along a wall already.
        let at = |outlines: &[Vec<DVec3>], z: f64| {
            outlines
                .iter()
                .min_by(|a, b| (a[0].z - z).abs().total_cmp(&(b[0].z - z).abs()))
                .unwrap()
                .clone()
        };
        let (mid, mid_flat) = (at(&body, zm), at(&flat, zm));
        assert!(
            mid.iter()
                .zip(&mid_flat)
                .all(|(p, q)| p.distance(*q) < 1e-12)
        );
        let top = at(&body, wall);
        let end = flat.last().unwrap();
        let moved: Vec<f64> = top
            .iter()
            .zip(end)
            .map(|(p, q)| p.truncate().distance(q.truncate()))
            .collect();
        let most = moved.iter().copied().fold(0.0, f64::max);
        assert!((most - radius).abs() < 0.05 * radius, "{most} vs {radius}");
        // Where the faces join the walls it does not stand out.
        assert!(moved[0] < 1e-9 && moved[moved.len() - 1] < 1e-9);
        // Nowhere nearer the axis than the cutwater circle.
        let rz = g.design.cutwater_diameter / 2.0;
        assert!(
            body.iter()
                .flatten()
                .all(|p| p.truncate().length() > rz - 1e-9)
        );
        // Automatic in a single volute of sections that take them, not in a double one.
        let round = build(|v| v.cross_section = CrossSectionInput::single(SectionShape::round()));
        assert_eq!(round.design.cutwater_side_radius, round.design.nose_radius);
        let external = build(|v| {
            v.cross_section = CrossSectionInput::single(SectionShape::RoundExternal(
                crate::volute::ExternalRound::default(),
            ))
        });
        assert_eq!(external.design.cutwater_side_radius, 0.0);
        let double = build(|v| v.setup.kind = crate::volute::VoluteKind::Double);
        assert_eq!(double.design.cutwater_side_radius, 0.0);
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
    fn the_nose_takes_any_edge_shape() {
        use crate::impeller::edge::EdgeShape;
        let round = build(|_| {});
        let e = 2.0 * round.design.nose_radius;
        let rz = round.design.cutwater_diameter / 2.0;
        // How far upstream the nose reaches, along the cutwater circle.
        let sign = round.basis.rotation_sign;
        let reach = |g: &VoluteGeometry| {
            let s = &g.cutwater_slices[SLICES / 2];
            let ahead = |p: &DVec3| -sign * p.y.atan2(p.x);
            rz * s
                .outline
                .iter()
                .map(ahead)
                .fold(f64::NEG_INFINITY, f64::max)
        };
        for shape in [
            EdgeShape::Ellipse { ratio: 3.0 },
            EdgeShape::TAPER,
            EdgeShape::BEZIER,
            EdgeShape::Cut,
        ] {
            let g = build(|v| v.cutwater.nose_shape = shape);
            assert_eq!(g.cutwater_slices.len(), SLICES, "{shape:?}");
            for s in &g.cutwater_slices {
                assert!(
                    (s.nose_distance - rz).abs() < 1e-6,
                    "{shape:?}: {}",
                    s.nose_distance
                );
                assert!(
                    s.min_thickness > e * (1.0 - 1e-3),
                    "{shape:?}: {}",
                    s.min_thickness
                );
            }
            if matches!(shape, EdgeShape::Ellipse { .. }) {
                // Two half thicknesses longer than the round one, at the flow angle.
                let longer = reach(&g) - reach(&round);
                let expected = 2.0 * round.design.nose_radius * g.design.cutwater_angle.cos();
                assert!(
                    (longer / expected - 1.0).abs() < 0.25,
                    "{longer} vs {expected}"
                );
            }
        }
        // A shape that cannot be built says so.
        let err = VoluteGeometry::build(&reference_basis(), &{
            let mut v = VoluteInput::default();
            v.cutwater.nose_shape = EdgeShape::Ellipse { ratio: -1.0 };
            v
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("nose of the cutwater"), "{err}");
    }

    #[test]
    fn a_g2_join_carries_the_curvature_of_the_wall() {
        // The gap between the spiral face and its wall along the stations of the face:
        // its second difference jumps at the join with a G1 face, fades with a G2 one.
        let jump = |join: crate::volute::Continuity| {
            let g = build(|v| v.cutwater.join = join);
            let s = &g.cutwater_slices[SLICES / 2];
            let wall: Vec<DVec2> = s.spiral_wall.iter().map(|p| p.truncate()).collect();
            // The spiral face runs from its join (the first point) toward the nose.
            let gaps: Vec<f64> = s.outline[..FACE_POINTS]
                .iter()
                .map(|p| distance_to(&wall, p.truncate()))
                .collect();
            let second: Vec<f64> = gaps.windows(3).map(|w| w[0] - 2.0 * w[1] + w[2]).collect();
            let largest = second.iter().map(|d| d.abs()).fold(0.0, f64::max);
            // At the join, against the largest bend along the face.
            second[0].abs() / largest
        };
        let (g1, g2) = (
            jump(crate::volute::Continuity::G1),
            jump(crate::volute::Continuity::G2),
        );
        assert!(g2 < 0.5 * g1, "G1 {g1}, G2 {g2}");
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
    fn the_share_puts_the_missing_thickness_on_one_side() {
        // Distance from the spiral wall a little past the tip to the body outline: with
        // share 0 the spiral side of the body is the wall itself.
        let off_the_wall = |share: f64| -> (f64, f64) {
            let g = build(|v| v.cutwater.share = Param::Manual(share));
            let mid = &g.cutwater_slices[SLICES / 2];
            let e = 2.0 * g.design.nose_radius;
            assert!(mid.min_thickness > e * (1.0 - 1e-3), "{share}");
            let tip = mid.tip;
            let wall = mid
                .spiral_wall
                .iter()
                .find(|p| {
                    p.distance(tip) > 0.5 * e && p.truncate().length() > tip.truncate().length()
                })
                .copied()
                .unwrap();
            let outline: Vec<DVec2> = mid.outline.iter().map(|p| p.truncate()).collect();
            (distance_to(&outline, wall.truncate()), e)
        };
        let (none, e) = off_the_wall(0.0);
        let (half, _) = off_the_wall(0.5);
        assert!(none < 0.05 * e, "{}", none / e);
        assert!(half > 0.2 * e, "{}", half / e);
        let mut input = VoluteInput::default();
        input.cutwater.share = Param::Manual(1.5);
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
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
