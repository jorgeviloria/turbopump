//! The cutwater cut by planes z = const across the width of the volute.
//!
//! In each plane the outer wall of the spiral and the inner wall of the discharge
//! diffuser cross at the tip of a thin wedge of casing. A rounded cutwater adds a
//! profiled body ahead of the tip (Gülich 2nd ed., Chap. 7.8.1, item 7, and Fig. 7.41b):
//!
//! - its camber line leaves the nose at the blade angle α₃B = α₃′ + i₃ to the
//!   circumferential direction (eq. T7.7.6);
//! - the nose is a circle of radius ρ whose centre M lies on the stagnation radius
//!   r′_z on the x axis, moved radially so that the innermost point of the cutwater lies
//!   on the cutwater circle d_z;
//! - from the nose to the tip of the wedge the body keeps the nose thickness e = 2ρ
//!   about a cubic Bézier camber line, tangent to α₃B at the nose and to the middle of
//!   the walls at the tip;
//! - past the tip each face is its wall moved into the flow by its share (½ by default)
//!   of what the wedge lacks of e, through a quadratic smooth maximum, so that it runs tangentially into
//!   the wall where the wedge has grown to e + k.
//!
//! So the cutwater is nowhere thinner than e, and takes no more of the flow than that
//! needs (our construction: Gülich asks for a short, thick profile, except the nose
//! radius, against cracks).
//!
//! The nose stays on the same line parallel to the pump axis in every plane; the faces
//! follow the walls of each plane, which differ when the sections are not straight
//! sided.

use tp_math::{Bezier, DVec2, DVec3};

use super::NozzleSection;
use super::{CutwaterShape, VoluteDesign};
use crate::impeller::edge::{EdgeProfile, EdgeShape};

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
            walls.slice(design, sign, z, centre)
        })
        .collect();
    (slices, centre)
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
        for i in 0..self.spiral.len() - 1 {
            let (a, b) = (self.spiral[i], self.spiral[i + 1]);
            for j in 0..self.diffuser.len() - 1 {
                let (c, d) = (self.diffuser[j], self.diffuser[j + 1]);
                if let Some(p) = segment_crossing(a, b, c, d) {
                    return (
                        Station {
                            segment: i,
                            point: p,
                        },
                        Station {
                            segment: j,
                            point: p,
                        },
                    );
                }
            }
        }
        let start = self.spiral[0];
        let (segment, point) = nearest_on(&self.diffuser, start);
        (
            Station {
                segment: 0,
                point: start,
            },
            Station { segment, point },
        )
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
        let profile = EdgeProfile::new(EdgeShape::Ellipse { ratio: 1.0 }, 2.0 * h).ok()?;
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
                    (g + k) * (g + k) / (4.0 * k)
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
            let inner: Vec<DVec2> = inner_lead
                .iter()
                .chain(inner_wall.iter().skip(1))
                .copied()
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
