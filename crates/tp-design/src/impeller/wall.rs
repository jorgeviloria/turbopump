//! Hub and shroud walls drawn segment by segment, joined end to end, each joint smooth
//! (tangent) or a corner. A segment is a Bézier curve, a straight line, two lines
//! meeting at a corner rounded by a circular arc, or a clamped B-spline.
//!
//! The first segment starts at the inlet and the last ends at the outlet; those two
//! points come from the main dimensions and the axial length, so a drawn wall follows
//! them when they change. Every other point is stored as (z, r) in m.
//!
//! Every segment is evaluated as Bézier pieces: one for a Bézier or a line, one per knot
//! span for a B-spline (exact), and cubic pieces of at most 45° for an arc, within
//! 5·10⁻⁶ of its radius (Piegl & Tiller, *The NURBS Book*, §5.4 and §7.5).

use std::f64::consts::{FRAC_PI_4, PI};

use serde::{Deserialize, Serialize};
use tp_math::{BSpline, Bezier, DVec2};

use crate::DesignError;

/// Degrees a B-spline segment may have.
pub const SPLINE_DEGREES: std::ops::RangeInclusive<usize> = 2..=4;

/// Degree of a new B-spline segment.
pub const DEFAULT_SPLINE_DEGREE: usize = 3;

/// Degree a Bézier takes when its control points are coupled: the first for which
/// points 1 and n − 1 are distinct from the middle.
pub const COUPLED_DEGREE: usize = 4;

/// Shape of one segment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentKind {
    #[default]
    Bezier,
    Line,
    /// Two straight lines meeting at a corner point, rounded by a circular arc tangent
    /// to both.
    Arc,
    /// A clamped B-spline through its control polygon.
    BSpline,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// One segment of a drawn wall, as stored in the project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SegmentInput {
    pub kind: SegmentKind,
    /// Inner points, (z, r) in m: the control points of a Bézier or a B-spline, the
    /// corner of an arc segment; lines have none.
    #[serde(default)]
    pub controls: Vec<DVec2>,
    /// End point, (z, r) in m. The last segment ends at the outlet instead.
    pub end: DVec2,
    /// Whether the joint at `end` is tangent to the next segment.
    #[serde(default)]
    pub smooth: bool,
    /// Arc: radius of the rounded corner, m. It shrinks as built when the straight
    /// parts are too short to hold it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    /// B-spline: degree, 2 to 4; 3 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degree: Option<usize>,
    /// B-spline: interior knots in (0, 1), non-decreasing; empty for uniform knots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knots: Vec<f64>,
    /// Bézier of degree 4 or more: point 1 stays on the line from the start to point
    /// 2, and point n − 1 on the line from the end to point n − 2, so the curvature is
    /// zero at both ends.
    #[serde(default, skip_serializing_if = "is_false")]
    pub coupled: bool,
    /// Bézier: direction at the start, rad from the z axis towards r.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_angle: Option<f64>,
    /// Bézier: direction at the end, rad from the z axis towards r.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_angle: Option<f64>,
}

/// A point of a drawn wall that can be moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallPoint {
    /// Inner point `index` of segment `segment`: a control point, or an arc's corner.
    Control { segment: usize, index: usize },
    /// The joint at the end of segment `segment` (never the last one).
    Joint { segment: usize },
}

/// The rounded corner of an arc segment as built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fillet {
    /// Where the arc meets the line from the start and the line to the end.
    pub tangent_points: [DVec2; 2],
    pub center: DVec2,
    pub radius: f64,
}

/// The corner at `corner` between the lines to `a` and to `b`, rounded with `radius`,
/// or as much of it as fits on the shorter line. None when there is no corner to round:
/// the lines are in line, fold back on each other, or have no length.
pub fn fillet(a: DVec2, corner: DVec2, b: DVec2, radius: f64) -> Option<Fillet> {
    let (u, v) = (
        (a - corner).normalize_or_zero(),
        (b - corner).normalize_or_zero(),
    );
    if u == DVec2::ZERO || v == DVec2::ZERO || radius.is_nan() || radius <= 0.0 {
        return None;
    }
    // Angle between the two lines at the corner.
    let angle = u.dot(v).clamp(-1.0, 1.0).acos();
    if !(1e-6..PI - 1e-6).contains(&angle) {
        return None;
    }
    let half = angle / 2.0;
    let reach = (radius / half.tan()).min(a.distance(corner).min(b.distance(corner)));
    let radius = reach * half.tan();
    Some(Fillet {
        tangent_points: [corner + u * reach, corner + v * reach],
        center: corner + (u + v).normalize() * (radius / half.sin()),
        radius,
    })
}

/// Cubic pieces of the arc about `center` from `from` to `to`, the short way round.
fn arc_pieces(center: DVec2, from: DVec2, to: DVec2) -> Vec<Bezier<DVec2>> {
    let radius = from.distance(center);
    let start = (from - center).to_angle();
    let mut sweep = (to - center).to_angle() - start;
    if sweep > PI {
        sweep -= 2.0 * PI;
    } else if sweep < -PI {
        sweep += 2.0 * PI;
    }
    let n = (sweep.abs() / FRAC_PI_4 - 1e-9).ceil().max(1.0) as usize;
    let step = sweep / n as f64;
    // Control distance of a cubic arc of angle `step` (Piegl & Tiller eq. 7.35 limit).
    let k = 4.0 / 3.0 * (step / 4.0).tan() * radius;
    (0..n)
        .map(|i| {
            let (a0, a1) = (start + step * i as f64, start + step * (i + 1) as f64);
            let p0 = center + DVec2::from_angle(a0) * radius;
            let p3 = center + DVec2::from_angle(a1) * radius;
            let t0 = DVec2::from_angle(a0).perp();
            let t1 = DVec2::from_angle(a1).perp();
            Bezier::new(vec![p0, p0 + t0 * k, p3 - t1 * k, p3]).expect("four points")
        })
        .collect()
}

/// One segment of a built wall.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// The input, with the end and the inner points as built.
    input: SegmentInput,
    /// Start, inner points and end: the control polygon of a Bézier or a B-spline, the
    /// start, corner and end of an arc.
    points: Vec<DVec2>,
    /// The segment as Bézier pieces joined end to end.
    pieces: Vec<Bezier<DVec2>>,
    /// The segment parameter at the start of each piece and at the end, from 0 to 1:
    /// the knots of a B-spline, equal steps otherwise.
    breaks: Vec<f64>,
    fillet: Option<Fillet>,
}

impl Segment {
    pub fn kind(&self) -> SegmentKind {
        self.input.kind
    }

    /// Start, inner points and end.
    pub fn control_points(&self) -> &[DVec2] {
        &self.points
    }

    pub fn pieces(&self) -> &[Bezier<DVec2>] {
        &self.pieces
    }

    pub fn start(&self) -> DVec2 {
        self.points[0]
    }

    pub fn end(&self) -> DVec2 {
        *self
            .points
            .last()
            .expect("segments have two points or more")
    }

    /// Degree of the curve: 1 for a line, that of its polygon for a Bézier, the
    /// B-spline's own, 3 for the pieces of an arc.
    pub fn degree(&self) -> usize {
        match self.input.kind {
            SegmentKind::Line => 1,
            SegmentKind::Bezier => self.points.len() - 1,
            SegmentKind::BSpline => self.spline_degree(),
            SegmentKind::Arc => 3,
        }
    }

    fn spline_degree(&self) -> usize {
        self.input.degree.unwrap_or(DEFAULT_SPLINE_DEGREE)
    }

    /// The rounded corner of an arc segment, if it has one.
    pub fn fillet(&self) -> Option<Fillet> {
        self.fillet
    }

    /// Whether points 1 and n − 1 of a Bézier follow their lines to points 2 and n − 2.
    pub fn coupled(&self) -> bool {
        self.input.kind == SegmentKind::Bezier && self.input.coupled
    }

    /// The fixed start and end directions of a Bézier, rad from the z axis.
    pub fn angles(&self) -> (Option<f64>, Option<f64>) {
        (self.input.start_angle, self.input.end_angle)
    }

    /// Piece and its own parameter at segment parameter `t`.
    fn piece_at(&self, t: f64) -> (usize, f64) {
        let t = t.clamp(0.0, 1.0);
        let i = self
            .breaks
            .windows(2)
            .position(|w| t <= w[1])
            .unwrap_or(self.pieces.len() - 1);
        let (a, b) = (self.breaks[i], self.breaks[i + 1]);
        (i, if b > a { (t - a) / (b - a) } else { 0.0 })
    }

    /// Point at segment parameter `t` ∈ [0, 1].
    pub fn eval(&self, t: f64) -> DVec2 {
        let (i, u) = self.piece_at(t);
        self.pieces[i].eval(u)
    }

    /// Unit tangent at segment parameter `t`.
    pub fn direction(&self, t: f64) -> DVec2 {
        let (i, u) = self.piece_at(t);
        let d = self.pieces[i].derivative().eval(u);
        if d.length() > 1e-14 {
            d.normalize()
        } else {
            (self.pieces[i].end() - self.pieces[i].start()).normalize_or_zero()
        }
    }

    pub fn arc_length(&self) -> f64 {
        self.pieces.iter().map(Bezier::arc_length).sum()
    }

    fn spline(&self) -> Option<BSpline<DVec2>> {
        (self.input.kind == SegmentKind::BSpline)
            .then(|| spline(&self.points, self.spline_degree(), &self.input.knots))
            .flatten()
    }
}

/// The clamped B-spline on [0, 1] through `points` with interior `knots`, uniform when
/// they do not fit the number of points.
fn spline(points: &[DVec2], degree: usize, knots: &[f64]) -> Option<BSpline<DVec2>> {
    let fits = knots.len() + degree + 1 == points.len()
        && knots.iter().all(|k| *k > 0.0 && *k < 1.0)
        && knots.windows(2).all(|w| w[0] <= w[1]);
    if fits {
        let full = std::iter::repeat_n(0.0, degree + 1)
            .chain(knots.iter().copied())
            .chain(std::iter::repeat_n(1.0, degree + 1))
            .collect();
        BSpline::new(degree, full, points.to_vec()).ok()
    } else {
        BSpline::clamped_uniform(degree, points.to_vec()).ok()
    }
}

/// Interior knots of a clamped spline on [0, 1].
fn interior_knots(s: &BSpline<DVec2>) -> Vec<f64> {
    let p = s.degree();
    let k = s.knots();
    k[p + 1..k.len() - p - 1].to_vec()
}

/// A built hub or shroud wall.
#[derive(Debug, Clone, PartialEq)]
pub struct Wall {
    segments: Vec<Segment>,
    /// Joint after each segment but the last: tangent or not.
    smooth: Vec<bool>,
}

/// `p` moved onto the ray from `origin` along `direction`, at least a little way out.
fn onto_ray(p: DVec2, origin: DVec2, direction: DVec2) -> DVec2 {
    origin + direction * (p - origin).dot(direction).max(1e-6)
}

/// `p` moved onto the segment from `a` to `b`, between 5 % and 95 % of the way.
fn onto_segment(p: DVec2, a: DVec2, b: DVec2) -> DVec2 {
    let ab = b - a;
    if ab.length_squared() <= 0.0 {
        return p;
    }
    a + ab * ((p - a).dot(ab) / ab.length_squared()).clamp(0.05, 0.95)
}

/// Direction of travel at `angle` rad from the z axis towards r.
fn heading(angle: f64) -> DVec2 {
    DVec2::from_angle(angle)
}

/// Applies the coupling and the fixed end directions of a Bézier to its polygon.
fn constrain(s: &SegmentInput, p: &mut [DVec2]) {
    if s.kind != SegmentKind::Bezier {
        return;
    }
    let n = p.len();
    let coupled = s.coupled && n > COUPLED_DEGREE;
    let last = n - 1;
    if let Some(angle) = s.start_angle {
        let dir = heading(angle);
        let k = if coupled { 2 } else { 1 };
        p[k] = onto_ray(p[k], p[0], dir);
    }
    if let Some(angle) = s.end_angle {
        let dir = -heading(angle);
        let k = if coupled { last - 2 } else { last - 1 };
        p[k] = onto_ray(p[k], p[last], dir);
    }
    // At degree 4 both directions hold the same middle point: where they meet.
    if let (true, Some(a), Some(b)) = (
        coupled && last == COUPLED_DEGREE,
        s.start_angle,
        s.end_angle,
    ) {
        let (u, v) = (heading(a), -heading(b));
        let det = u.perp_dot(v);
        if det.abs() > 1e-12 {
            let along = (p[last] - p[0]).perp_dot(v) / det;
            let back = (p[last] - p[0]).perp_dot(u) / det;
            if along > 0.0 && back > 0.0 {
                p[2] = p[0] + u * along;
            }
        }
    }
    if coupled {
        p[1] = onto_segment(p[1], p[0], p[2]);
        p[last - 1] = onto_segment(p[last - 1], p[last], p[last - 2]);
    }
}

impl Wall {
    /// A wall of one Bézier curve.
    pub fn single(curve: Bezier<DVec2>) -> Self {
        let points = curve.control_points().to_vec();
        let input = SegmentInput {
            kind: if curve.degree() == 1 {
                SegmentKind::Line
            } else {
                SegmentKind::Bezier
            },
            controls: points[1..points.len() - 1].to_vec(),
            end: curve.end(),
            ..SegmentInput::default()
        };
        Self {
            segments: vec![Segment {
                input,
                points,
                pieces: vec![curve],
                breaks: vec![0.0, 1.0],
                fillet: None,
            }],
            smooth: Vec::new(),
        }
    }

    /// Builds the wall from `start` to `end` through the drawn segments. At a smooth
    /// joint the side with control points turns its nearest one (two for a coupled
    /// Bézier) to the tangent of the other side, keeping their distance to the joint;
    /// between two such sides the second follows the first. Two lines meet at a corner
    /// whatever the flag says.
    pub fn build(start: DVec2, end: DVec2, input: &[SegmentInput]) -> Result<Self, DesignError> {
        if input.is_empty() {
            return Err(DesignError::InvalidInput(
                "a drawn wall needs at least one segment".into(),
            ));
        }
        let n = input.len();
        let mut points: Vec<Vec<DVec2>> = Vec::with_capacity(n);
        let mut from = start;
        for (i, s) in input.iter().enumerate() {
            let to = if i + 1 == n { end } else { s.end };
            let invalid = |what: &str| {
                Err(DesignError::InvalidInput(format!(
                    "segment {} {what}",
                    i + 1
                )))
            };
            match s.kind {
                SegmentKind::Bezier if s.controls.is_empty() => {
                    return invalid("is a Bézier without control points");
                }
                SegmentKind::Arc if s.controls.len() != 1 => {
                    return invalid("is an arc without its corner point");
                }
                SegmentKind::BSpline => {
                    let degree = s.degree.unwrap_or(DEFAULT_SPLINE_DEGREE);
                    if !SPLINE_DEGREES.contains(&degree) {
                        return invalid("is a B-spline of degree other than 2, 3 or 4");
                    }
                    if s.controls.len() + 1 < degree {
                        return invalid("has too few control points for its degree");
                    }
                }
                _ => {}
            }
            let mut p = vec![from];
            if s.kind != SegmentKind::Line {
                p.extend(&s.controls);
            }
            p.push(to);
            if let Some(bad) = p.iter().find(|q| !(q.is_finite() && q.y >= 0.0)) {
                return Err(DesignError::InvalidInput(format!(
                    "segment {} has an invalid point ({:.4}, {:.4}) m: the radius must not \
                     be negative",
                    i + 1,
                    bad.x,
                    bad.y
                )));
            }
            if from.distance(to) <= 1e-9 {
                return invalid("has no length");
            }
            constrain(s, &mut p);
            points.push(p);
            from = to;
        }
        let smooth: Vec<bool> = input[..n - 1].iter().map(|s| s.smooth).collect();
        for (i, &tangent) in smooth.iter().enumerate() {
            if tangent {
                align(&mut points, input, i);
            }
        }
        let segments = input
            .iter()
            .zip(points)
            .map(|(s, p)| finish(s, p))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { segments, smooth })
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Every Bézier piece of the wall, from inlet to outlet.
    pub fn pieces(&self) -> impl Iterator<Item = &Bezier<DVec2>> {
        self.segments.iter().flat_map(|s| s.pieces.iter())
    }

    /// Tangent flag of the joint after each segment but the last.
    pub fn smooth(&self) -> &[bool] {
        &self.smooth
    }

    pub fn start(&self) -> DVec2 {
        self.segments[0].start()
    }

    pub fn end(&self) -> DVec2 {
        self.segments.last().expect("a wall has segments").end()
    }

    /// Unit direction in which the wall leaves the outlet.
    pub fn end_direction(&self) -> DVec2 {
        self.segments
            .last()
            .expect("a wall has segments")
            .direction(1.0)
    }

    /// `count` points from inlet to outlet, shared out among the pieces by their
    /// length, joints included once; at least one interval per piece. A single curve
    /// gives exactly its own parameter-uniform samples.
    pub fn sample(&self, count: usize) -> Vec<DVec2> {
        let pieces: Vec<&Bezier<DVec2>> = self.pieces().collect();
        let count = count.max(pieces.len() + 1);
        if pieces.len() == 1 {
            return pieces[0].sample(count);
        }
        let lengths: Vec<f64> = pieces.iter().map(|p| p.arc_length()).collect();
        let total: f64 = lengths.iter().sum();
        let intervals = count - 1;
        // Each piece gets at least one interval; the rest go by length.
        let mut shares: Vec<usize> = lengths
            .iter()
            .map(|l| ((l / total * intervals as f64).round() as usize).max(1))
            .collect();
        while shares.iter().sum::<usize>() > intervals {
            let i = (0..shares.len())
                .filter(|&i| shares[i] > 1)
                .max_by_key(|&i| shares[i])
                .unwrap_or(0);
            if shares[i] <= 1 {
                break;
            }
            shares[i] -= 1;
        }
        while shares.iter().sum::<usize>() < intervals {
            let i = (0..shares.len())
                .max_by(|&a, &b| {
                    (lengths[a] / shares[a] as f64).total_cmp(&(lengths[b] / shares[b] as f64))
                })
                .expect("a wall has pieces");
            shares[i] += 1;
        }
        let mut points = vec![self.start()];
        for (piece, &k) in pieces.iter().zip(&shares) {
            points.extend(piece.sample(k + 1).into_iter().skip(1));
        }
        points
    }

    /// The segments as stored inputs: built points, so a smooth joint keeps its
    /// aligned control points.
    pub fn to_input(&self) -> Vec<SegmentInput> {
        self.segments.iter().map(|s| s.input.clone()).collect()
    }

    /// Where a movable point sits.
    pub fn point(&self, at: WallPoint) -> Option<DVec2> {
        match at {
            WallPoint::Control { segment, index } => {
                let p = self.segments.get(segment)?.control_points();
                (index + 2 < p.len()).then(|| p[index + 1])
            }
            WallPoint::Joint { segment } => {
                (segment + 1 < self.segments.len()).then(|| self.segments[segment].end())
            }
        }
    }

    /// Every movable point, joints first so they win when points overlap.
    pub fn points(&self) -> Vec<(WallPoint, DVec2)> {
        let joints = (0..self.segments.len() - 1).map(|segment| {
            let at = WallPoint::Joint { segment };
            (at, self.segments[segment].end())
        });
        let controls = self.segments.iter().enumerate().flat_map(|(segment, s)| {
            let p = s.control_points();
            (1..p.len() - 1).map(move |i| {
                let at = WallPoint::Control {
                    segment,
                    index: i - 1,
                };
                (at, p[i])
            })
        });
        joints.chain(controls).collect()
    }

    /// Curvature along the wall: `per_piece` samples of each piece, as (arc length from
    /// the inlet in m, curvature κ in 1/m, point). Lines have none; a corner at a joint
    /// shows as two samples at the same length.
    pub fn curvature(&self, per_piece: usize) -> Vec<(f64, f64, DVec2)> {
        let n = per_piece.max(2);
        let mut out = Vec::new();
        let mut length = 0.0;
        for piece in self.pieces() {
            let d1 = piece.derivative();
            let d2 = (piece.degree() >= 2).then(|| d1.derivative());
            let mut previous = piece.start();
            for k in 0..n {
                let t = k as f64 / (n - 1) as f64;
                let p = piece.eval(t);
                length += p.distance(previous);
                previous = p;
                let kappa = d2.as_ref().map_or(0.0, |d2| {
                    let (v, a) = (d1.eval(t), d2.eval(t));
                    let speed = v.length();
                    if speed > 0.0 {
                        v.perp_dot(a).abs() / speed.powi(3)
                    } else {
                        0.0
                    }
                });
                out.push((length, kappa, p));
            }
        }
        out
    }

    /// Segment and segment parameter of the wall point nearest to `pointer`.
    pub fn nearest(&self, pointer: DVec2) -> (usize, f64) {
        const SAMPLES: usize = 200;
        let mut best = (0, 0.0, f64::INFINITY);
        for (i, s) in self.segments.iter().enumerate() {
            for k in 0..=SAMPLES {
                let t = k as f64 / SAMPLES as f64;
                let d = s.eval(t).distance_squared(pointer);
                if d < best.2 {
                    best = (i, t, d);
                }
            }
        }
        (best.0, best.1)
    }
}

/// The built segment from its input and its polygon as constrained and aligned.
fn finish(s: &SegmentInput, points: Vec<DVec2>) -> Result<Segment, DesignError> {
    let mut input = s.clone();
    input.end = *points.last().expect("two points or more");
    input.controls = if s.kind == SegmentKind::Line {
        Vec::new()
    } else {
        points[1..points.len() - 1].to_vec()
    };
    let (a, b) = (points[0], input.end);
    let even = |n: usize| (0..=n).map(|i| i as f64 / n as f64).collect::<Vec<_>>();
    let mut fillet_built = None;
    let (pieces, breaks) = match s.kind {
        SegmentKind::Line => (vec![Bezier::line(a, b)], even(1)),
        SegmentKind::Bezier => (vec![Bezier::new(points.clone()).expect("points")], even(1)),
        SegmentKind::Arc => {
            let corner = points[1];
            let mut pieces = Vec::new();
            let line = |from: DVec2, to: DVec2, pieces: &mut Vec<Bezier<DVec2>>| {
                if from.distance(to) > 1e-12 {
                    pieces.push(Bezier::line(from, to));
                }
            };
            match fillet(a, corner, b, s.radius.unwrap_or(0.0)) {
                Some(f) => {
                    let [t1, t2] = f.tangent_points;
                    line(a, t1, &mut pieces);
                    pieces.extend(arc_pieces(f.center, t1, t2));
                    line(t2, b, &mut pieces);
                    fillet_built = Some(f);
                }
                None => {
                    line(a, corner, &mut pieces);
                    line(corner, b, &mut pieces);
                }
            }
            let n = pieces.len();
            (pieces, even(n))
        }
        SegmentKind::BSpline => {
            let degree = s.degree.unwrap_or(DEFAULT_SPLINE_DEGREE);
            let curve = spline(&points, degree, &s.knots).ok_or_else(|| {
                DesignError::InvalidInput("a B-spline segment could not be built".into())
            })?;
            input.knots = interior_knots(&curve);
            let mut breaks = curve.knots().to_vec();
            breaks.dedup();
            (curve.to_beziers(), breaks)
        }
    };
    Ok(Segment {
        input,
        points,
        pieces,
        breaks,
        fillet: fillet_built,
    })
}

/// Turns the side with control points of the joint after segment `i` to the other
/// side's tangent.
fn align(points: &mut [Vec<DVec2>], input: &[SegmentInput], i: usize) {
    let (before, after) = points.split_at_mut(i + 1);
    let (a, b) = (&mut before[i], &mut after[0]);
    let joint = b[0];
    let (a_line, b_line) = (a.len() == 2, b.len() == 2);
    let coupled = |s: &SegmentInput, len: usize| {
        s.kind == SegmentKind::Bezier && s.coupled && len > COUPLED_DEGREE
    };
    if !b_line {
        let incoming = joint - a[a.len() - 2];
        if incoming.length() > 0.0 {
            let dir = incoming.normalize();
            let k = if coupled(&input[i + 1], b.len()) {
                2
            } else {
                1
            };
            for q in b.iter_mut().take(k + 1).skip(1) {
                *q = joint + dir * q.distance(joint);
            }
        }
    } else if !a_line {
        let outgoing = b[1] - joint;
        let dir = outgoing.normalize();
        let k = if coupled(&input[i], a.len()) { 2 } else { 1 };
        let n = a.len();
        for q in a.iter_mut().skip(n - 1 - k).take(k) {
            *q = joint - dir * q.distance(joint);
        }
    }
}

/// The inner points of curve `c`.
fn inner(c: &Bezier<DVec2>) -> Vec<DVec2> {
    let p = c.control_points();
    p[1..p.len() - 1].to_vec()
}

/// Splits segment `segment` at segment parameter `t` into two, joined smoothly; the
/// wall keeps its shape. A Bézier or a line splits into two of its kind, a B-spline into
/// two B-splines at a knot, an arc into the parts on each side of the point, each an arc
/// or a line.
pub fn split(wall: &Wall, segment: usize, t: f64) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(s) = wall.segments.get(segment) else {
        return input;
    };
    let t = t.clamp(0.05, 0.95);
    let original = input[segment].clone();
    let part = |kind, controls, end, smooth| SegmentInput {
        kind,
        controls,
        end,
        smooth,
        ..SegmentInput::default()
    };
    let (mut first, mut second) = match s.kind() {
        SegmentKind::Line | SegmentKind::Bezier => {
            let (left, right) = s.pieces[0].split(t);
            (
                part(s.kind(), inner(&left), left.end(), true),
                part(s.kind(), inner(&right), right.end(), original.smooth),
            )
        }
        SegmentKind::BSpline => match s.spline() {
            Some(curve) => split_spline(&curve, t, original.smooth),
            None => return input,
        },
        SegmentKind::Arc => split_arc(s, t, original.smooth),
    };
    if s.kind() == SegmentKind::Bezier {
        first.start_angle = original.start_angle;
        second.end_angle = original.end_angle;
    }
    if s.kind() == SegmentKind::BSpline {
        first.degree = original.degree;
        second.degree = original.degree;
    }
    input.splice(segment..=segment, [first, second]);
    input
}

/// The two B-splines of `curve` on each side of `u`, from the knot inserted there until
/// the curve passes through a control point.
fn split_spline(curve: &BSpline<DVec2>, u: f64, smooth: bool) -> (SegmentInput, SegmentInput) {
    let p = curve.degree();
    let mut full = curve.clone();
    while tp_math::multiplicity(full.knots(), u) < p {
        full = full.insert_knot(u);
    }
    let knots = full.knots();
    let points = full.control_points();
    let j = knots
        .iter()
        .position(|k| (k - u).abs() <= 1e-12)
        .expect("u was inserted");
    let left: Vec<DVec2> = points[..j].to_vec();
    let right: Vec<DVec2> = points[j - 1..].to_vec();
    let rescale = |k: &[f64], a: f64, b: f64| -> Vec<f64> {
        k.iter()
            .map(|x| (x - a) / (b - a))
            .filter(|x| *x > 1e-12 && *x < 1.0 - 1e-12)
            .collect()
    };
    let left_knots = rescale(&knots[p + 1..j], 0.0, u);
    let right_knots = rescale(&knots[j + p..knots.len() - p - 1], u, 1.0);
    let piece = |points: &[DVec2], knots: Vec<f64>, smooth: bool| SegmentInput {
        kind: SegmentKind::BSpline,
        controls: points[1..points.len() - 1].to_vec(),
        end: *points.last().expect("points"),
        smooth,
        degree: Some(p),
        knots,
        ..SegmentInput::default()
    };
    (
        piece(&left, left_knots, true),
        piece(&right, right_knots, smooth),
    )
}

/// The two parts of an arc segment on each side of the point at `t`. On a straight
/// part one side is a line; on the arc each side is an arc of the same radius whose
/// corner is where the tangent at the point meets the line.
fn split_arc(s: &Segment, t: f64, smooth: bool) -> (SegmentInput, SegmentInput) {
    let p = s.eval(t);
    let [a, corner, b] = [s.points[0], s.points[1], s.points[2]];
    let radius = s.input.radius;
    let line = |end, smooth| SegmentInput {
        kind: SegmentKind::Line,
        end,
        smooth,
        ..SegmentInput::default()
    };
    let arc = |corner, end, smooth| SegmentInput {
        kind: SegmentKind::Arc,
        controls: vec![corner],
        end,
        smooth,
        radius,
        ..SegmentInput::default()
    };
    // Without a rounding the corner itself divides the two straight parts.
    let (t1, t2) = s.fillet.map_or((corner, corner), |f| {
        (f.tangent_points[0], f.tangent_points[1])
    });
    let along = |from: DVec2, to: DVec2| (p - from).dot((to - from).normalize_or_zero());
    if along(a, corner) < a.distance(t1) - 1e-12 {
        return (line(p, true), arc(corner, b, smooth));
    }
    if along(b, corner) < b.distance(t2) - 1e-12 {
        return (arc(corner, p, true), line(b, smooth));
    }
    // On the arc: the tangent there meets both lines.
    let dir = s.direction(t);
    let meet = |from: DVec2, to: DVec2| {
        let d = to - from;
        let det = d.perp_dot(dir);
        if det.abs() < 1e-15 {
            corner
        } else {
            from + d * ((p - from).perp_dot(dir) / det)
        }
    };
    (
        arc(meet(a, corner), p, true),
        arc(meet(b, corner), b, smooth),
    )
}

/// Joins the segments on both sides of the joint after `segment` into one: a line if
/// both are lines, otherwise a cubic Bézier that keeps the outer tangents.
pub fn merge(wall: &Wall, segment: usize) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    if segment + 1 >= wall.segments.len() {
        return input;
    }
    let (a, b) = (
        wall.segments[segment].control_points(),
        wall.segments[segment + 1].control_points(),
    );
    let (start, end) = (a[0], b[b.len() - 1]);
    let smooth = input[segment + 1].smooth;
    let merged = if a.len() == 2 && b.len() == 2 {
        SegmentInput {
            kind: SegmentKind::Line,
            end,
            smooth,
            ..SegmentInput::default()
        }
    } else {
        let first = if a.len() > 2 {
            a[1]
        } else {
            start.lerp(end, 1.0 / 3.0)
        };
        let last = if b.len() > 2 {
            b[b.len() - 2]
        } else {
            start.lerp(end, 2.0 / 3.0)
        };
        SegmentInput {
            kind: SegmentKind::Bezier,
            controls: vec![first, last],
            end,
            smooth,
            ..SegmentInput::default()
        }
    };
    input.splice(segment..=segment + 1, [merged]);
    input
}

/// Where the tangents at both ends of `s` meet, if in front of both.
fn tangent_corner(s: &Segment) -> Option<DVec2> {
    let (a, b) = (s.start(), s.end());
    let (u, v) = (s.direction(0.0), -s.direction(1.0));
    let det = u.perp_dot(v);
    if det.abs() < 1e-12 {
        return None;
    }
    let along = (b - a).perp_dot(v) / det;
    let back = (b - a).perp_dot(u) / det;
    (along > 0.0 && back > 0.0).then(|| a + u * along)
}

/// Turns segment `segment` into `kind`, keeping its shape where the new kind can:
/// - a line drops its points;
/// - a Bézier from a line starts with its control points at the thirds, from an arc
///   or a B-spline it takes their polygon;
/// - an arc gets its corner where the end tangents meet (the middle of a line), with
///   half the shorter straight part rounded;
/// - a B-spline takes the polygon of a Bézier of degree 2 to 4 (the same curve),
///   otherwise a cubic through points of the segment.
pub fn set_kind(wall: &Wall, segment: usize, kind: SegmentKind) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(s) = wall.segments.get(segment) else {
        return input;
    };
    if s.kind() == kind {
        return input;
    }
    let (a, b) = (s.start(), s.end());
    let current = &input[segment];
    let mut next = SegmentInput {
        kind,
        end: current.end,
        smooth: current.smooth,
        ..SegmentInput::default()
    };
    match kind {
        SegmentKind::Line => {}
        SegmentKind::Bezier => {
            next.controls = match s.kind() {
                SegmentKind::Line => vec![a.lerp(b, 1.0 / 3.0), a.lerp(b, 2.0 / 3.0)],
                _ => s.points[1..s.points.len() - 1].to_vec(),
            };
        }
        SegmentKind::Arc => {
            let corner = tangent_corner(s).unwrap_or_else(|| a.lerp(b, 0.5));
            let reach = 0.5 * a.distance(corner).min(b.distance(corner));
            let angle = (a - corner)
                .normalize_or_zero()
                .dot((b - corner).normalize_or_zero())
                .clamp(-1.0, 1.0)
                .acos();
            let radius = if angle < PI - 1e-6 {
                reach * (angle / 2.0).tan()
            } else {
                0.25 * a.distance(b)
            };
            next.controls = vec![corner];
            next.radius = Some(radius);
        }
        SegmentKind::BSpline => {
            next.degree = Some(DEFAULT_SPLINE_DEGREE);
            let points = s.control_points();
            let degree = points.len() - 1;
            if s.kind() == SegmentKind::Bezier && SPLINE_DEGREES.contains(&degree) {
                next.degree = Some(degree);
                next.controls = points[1..points.len() - 1].to_vec();
            } else {
                let through: Vec<DVec2> = (0..=8).map(|k| s.eval(k as f64 / 8.0)).collect();
                match BSpline::interpolate(&through, DEFAULT_SPLINE_DEGREE) {
                    Ok(curve) => {
                        let p = curve.control_points();
                        next.controls = p[1..p.len() - 1].to_vec();
                        next.knots = interior_knots(&curve);
                    }
                    Err(_) => return input,
                }
            }
        }
    }
    input[segment] = next;
    input
}

/// Moves a point to `to`. Moving a joint carries its two neighbouring inner points
/// along, so the joint keeps its tangent; moving point 2 or n − 2 of a coupled Bézier
/// carries point 1 or n − 1 along its line.
pub fn move_point(wall: &Wall, at: WallPoint, to: DVec2) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    match at {
        WallPoint::Control { segment, index } => {
            let Some(s) = wall.segments.get(segment) else {
                return input;
            };
            let Some(c) = input[segment].controls.get_mut(index) else {
                return input;
            };
            *c = to;
            let p = s.control_points();
            let last = p.len() - 1;
            if s.coupled() && last >= COUPLED_DEGREE {
                // Polygon index of the moved point, and the one coupled to it.
                let moved = index + 1;
                let carry = |end: DVec2, follower: DVec2, from: DVec2| {
                    let f = follower.distance(end) / from.distance(end).max(1e-12);
                    end + (to - end) * f
                };
                let controls = &mut input[segment].controls;
                if moved == 2 {
                    controls[0] = carry(p[0], p[1], p[2]);
                }
                if moved == last - 2 {
                    controls[last - 2] = carry(p[last], p[last - 1], p[last - 2]);
                }
            }
        }
        WallPoint::Joint { segment } if segment + 1 < input.len() => {
            let shift = to - input[segment].end;
            input[segment].end = to;
            if let Some(c) = input[segment].controls.last_mut() {
                *c += shift;
            }
            if let Some(c) = input[segment + 1].controls.first_mut() {
                *c += shift;
            }
        }
        WallPoint::Joint { .. } => {}
    }
    input
}

/// Gives segment `segment` one more control point without changing its shape: degree
/// elevation of a Bézier, a knot in the middle of the longest span of a B-spline; a line
/// becomes a Bézier through the same points. Arcs have none to add.
pub fn add_control(wall: &Wall, segment: usize) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(s) = wall.segments.get(segment) else {
        return input;
    };
    match s.kind() {
        SegmentKind::Line | SegmentKind::Bezier => {
            input[segment].kind = SegmentKind::Bezier;
            input[segment].controls = inner(&s.pieces[0].elevate());
        }
        SegmentKind::BSpline => {
            let (i, _) = s
                .breaks
                .windows(2)
                .enumerate()
                .max_by(|a, b| (a.1[1] - a.1[0]).total_cmp(&(b.1[1] - b.1[0])))
                .expect("a spline has spans");
            let u = (s.breaks[i] + s.breaks[i + 1]) / 2.0;
            return insert_knot(wall, segment, u);
        }
        SegmentKind::Arc => {}
    }
    input
}

/// Inserts a knot at parameter `u` of B-spline segment `segment`: one control point
/// more, the same shape.
pub fn insert_knot(wall: &Wall, segment: usize, u: f64) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    if let Some(curve) = wall.segments.get(segment).and_then(Segment::spline) {
        let more = curve.insert_knot(u);
        let p = more.control_points();
        input[segment].controls = p[1..p.len() - 1].to_vec();
        input[segment].knots = interior_knots(&more);
    }
    input
}

/// Removes inner control point `index` of segment `segment`, keeping at least one for
/// a Bézier and degree + 1 points for a B-spline, whose knots turn uniform; the curve
/// takes the shape of the points left. The corner of an arc stays.
pub fn remove_control(wall: &Wall, segment: usize, index: usize) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(s) = input.get_mut(segment) else {
        return input;
    };
    let keep = match s.kind {
        SegmentKind::Bezier => 1,
        SegmentKind::BSpline => s.degree.unwrap_or(DEFAULT_SPLINE_DEGREE) - 1,
        SegmentKind::Line | SegmentKind::Arc => usize::MAX,
    };
    if s.controls.len() > keep && index < s.controls.len() {
        s.controls.remove(index);
        s.knots.clear();
        if s.kind == SegmentKind::Bezier && s.controls.len() + 1 < COUPLED_DEGREE {
            s.coupled = false;
        }
    }
    input
}

/// Sets whether the joint after `segment` is tangent.
pub fn set_smooth(wall: &Wall, segment: usize, smooth: bool) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    if segment + 1 < input.len() {
        input[segment].smooth = smooth;
    }
    input
}

/// Sets the corner radius of arc segment `segment`, m.
pub fn set_radius(wall: &Wall, segment: usize, radius: f64) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    if let Some(s) = input.get_mut(segment)
        && s.kind == SegmentKind::Arc
    {
        s.radius = Some(radius.max(0.0));
    }
    input
}

/// The radius of arc segment `segment` whose centre lies nearest to `center`: its
/// distance from the corner along the bisector, times the sine of half the corner.
pub fn radius_for_center(wall: &Wall, segment: usize, center: DVec2) -> Option<f64> {
    let s = wall.segments.get(segment)?;
    if s.kind() != SegmentKind::Arc {
        return None;
    }
    let [a, corner, b] = [s.points[0], s.points[1], s.points[2]];
    let (u, v) = (
        (a - corner).normalize_or_zero(),
        (b - corner).normalize_or_zero(),
    );
    let bisector = (u + v).normalize_or_zero();
    let half = u.dot(v).clamp(-1.0, 1.0).acos() / 2.0;
    Some(((center - corner).dot(bisector) * half.sin()).max(0.0))
}

/// Sets the degree of B-spline segment `segment`, adding control points (knots) first
/// when it has too few; the knots turn uniform.
pub fn set_degree(wall: &Wall, segment: usize, degree: usize) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(s) = wall.segments.get(segment) else {
        return input;
    };
    if s.kind() != SegmentKind::BSpline || !SPLINE_DEGREES.contains(&degree) {
        return input;
    }
    let mut controls = input[segment].controls.clone();
    if controls.len() + 1 < degree {
        // Enough points first: knots inserted, the same curve.
        let mut current = wall.clone();
        while current.segments[segment].points.len() < degree + 1 {
            let more = add_control(&current, segment);
            current = Wall::build(current.start(), current.end(), &more)
                .expect("inserting a knot keeps the wall valid");
        }
        controls = current.segments[segment].input.controls.clone();
    }
    let s = &mut input[segment];
    s.controls = controls;
    s.degree = Some(degree);
    s.knots.clear();
    input
}

/// Couples or uncouples the end control points of Bézier segment `segment`. Coupling
/// raises the curve to degree 4 first, without changing its shape.
pub fn set_coupled(wall: &Wall, segment: usize, coupled: bool) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(s) = wall.segments.get(segment) else {
        return input;
    };
    if s.kind() != SegmentKind::Bezier {
        return input;
    }
    if coupled {
        let mut curve = s.pieces[0].clone();
        while curve.degree() < COUPLED_DEGREE {
            curve = curve.elevate();
        }
        input[segment].controls = inner(&curve);
    }
    input[segment].coupled = coupled;
    input
}

/// Sets or frees the start and end directions of Bézier segment `segment`, rad from
/// the z axis.
pub fn set_angles(
    wall: &Wall,
    segment: usize,
    start: Option<f64>,
    end: Option<f64>,
) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    if let Some(s) = input.get_mut(segment)
        && s.kind == SegmentKind::Bezier
    {
        s.start_angle = start;
        s.end_angle = end;
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quarter() -> Bezier<DVec2> {
        Bezier::new(vec![
            DVec2::new(0.0, 0.01),
            DVec2::new(0.03, 0.01),
            DVec2::new(0.05, 0.03),
            DVec2::new(0.05, 0.08),
        ])
        .unwrap()
    }

    fn rebuild(wall: &Wall, input: &[SegmentInput]) -> Wall {
        Wall::build(wall.start(), wall.end(), input).unwrap()
    }

    /// Largest distance from 200 points of the first wall to the second, densely
    /// sampled as a polyline.
    fn gap(a: &Wall, b: &Wall) -> f64 {
        let dense = b.sample(4000);
        let to_chord = |p: DVec2, w: &[DVec2]| {
            let d = w[1] - w[0];
            let t = ((p - w[0]).dot(d) / d.length_squared().max(1e-30)).clamp(0.0, 1.0);
            p.distance(w[0] + d * t)
        };
        a.sample(200)
            .iter()
            .map(|p| {
                dense
                    .windows(2)
                    .map(|w| to_chord(*p, w))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_single_curve_round_trips_through_its_input() {
        let wall = Wall::single(quarter());
        assert_eq!(rebuild(&wall, &wall.to_input()), wall);
        assert_eq!(wall.sample(17), quarter().sample(17));
    }

    #[test]
    fn splitting_keeps_the_shape_and_merging_undoes_it() {
        let wall = Wall::single(quarter());
        let split = rebuild(&wall, &split(&wall, 0, 0.4));
        assert_eq!(split.segments().len(), 2);
        assert_eq!(split.smooth(), [true]);
        for (i, s) in split.segments().iter().enumerate() {
            for k in 0..=10 {
                let t = k as f64 / 10.0;
                let u = if i == 0 { 0.4 * t } else { 0.4 + 0.6 * t };
                assert!(s.eval(t).distance(quarter().eval(u)) < 1e-12);
            }
        }
        let merged = rebuild(&split, &merge(&split, 0));
        assert_eq!(merged.segments().len(), 1);
        let p = merged.segments()[0].control_points();
        let q = quarter();
        // The outer tangents survive the merge.
        let dir = |a: DVec2, b: DVec2| (b - a).normalize();
        assert!(dir(p[0], p[1]).distance(dir(q.eval(0.0), q.control_points()[1])) < 1e-12);
        assert!(dir(p[2], p[3]).distance(dir(q.control_points()[2], q.eval(1.0))) < 1e-12);
    }

    #[test]
    fn smooth_joints_align_the_bezier_side() {
        let start = DVec2::new(0.0, 0.01);
        let end = DVec2::new(0.05, 0.08);
        let joint = DVec2::new(0.03, 0.02);
        let line_then_curve = [
            SegmentInput {
                kind: SegmentKind::Line,
                end: joint,
                smooth: true,
                ..SegmentInput::default()
            },
            SegmentInput {
                kind: SegmentKind::Bezier,
                controls: vec![DVec2::new(0.03, 0.05), DVec2::new(0.05, 0.06)],
                end,
                ..SegmentInput::default()
            },
        ];
        let wall = Wall::build(start, end, &line_then_curve).unwrap();
        let c = wall.segments()[1].control_points();
        let along = (joint - start).normalize();
        assert!((c[1] - joint).normalize().distance(along) < 1e-12);
        // The control point keeps its distance to the joint.
        assert!((c[1].distance(joint) - 0.03).abs() < 1e-12);

        let mut curve_then_line = line_then_curve.clone();
        curve_then_line.swap(0, 1);
        curve_then_line[0].end = joint;
        curve_then_line[0].smooth = true;
        curve_then_line[1].smooth = false;
        let wall = Wall::build(start, end, &curve_then_line).unwrap();
        let c = wall.segments()[0].control_points();
        assert!(
            (joint - c[2])
                .normalize()
                .distance((end - joint).normalize())
                < 1e-12
        );
    }

    #[test]
    fn moving_a_joint_carries_its_tangent() {
        let wall = Wall::single(quarter());
        let split = rebuild(&wall, &split(&wall, 0, 0.5));
        let at = WallPoint::Joint { segment: 0 };
        let from = split.point(at).unwrap();
        let to = from + DVec2::new(0.002, -0.001);
        let moved = rebuild(&split, &move_point(&split, at, to));
        assert!(moved.point(at).unwrap().distance(to) < 1e-15);
        let (a, b) = (
            moved.segments()[0].control_points(),
            moved.segments()[1].control_points(),
        );
        let dir = |p: DVec2, q: DVec2| (q - p).normalize();
        assert!(dir(a[2], a[3]).distance(dir(b[0], b[1])) < 1e-12);
    }

    #[test]
    fn kinds_switch_and_samples_cover_every_segment() {
        let wall = Wall::single(quarter());
        let two = rebuild(&wall, &split(&wall, 0, 0.3));
        let lined = rebuild(&two, &set_kind(&two, 0, SegmentKind::Line));
        assert_eq!(lined.segments()[0].degree(), 1);
        let back = rebuild(&lined, &set_kind(&lined, 0, SegmentKind::Bezier));
        assert_eq!(back.segments()[0].degree(), 3);

        let points = two.sample(50);
        assert_eq!(points.len(), 50);
        assert!(points[0].distance(two.start()) < 1e-15);
        assert!(points.last().unwrap().distance(two.end()) < 1e-15);
        let joint = two.segments()[0].end();
        assert!(points.iter().any(|p| p.distance(joint) < 1e-15));
        assert_eq!(two.sample(2).len(), 3);
    }

    #[test]
    fn curvature_matches_a_circle_and_is_zero_on_lines() {
        // A cubic quarter circle of radius 0.05 m: the radius is within 0.03 %, but
        // its curvature swings by about 2 %.
        let k = 0.5523 * 0.05;
        let arc = Bezier::new(vec![
            DVec2::new(0.0, 0.05),
            DVec2::new(k, 0.05),
            DVec2::new(0.05, k),
            DVec2::new(0.05, 0.0),
        ])
        .unwrap();
        let profile = Wall::single(arc).curvature(50);
        for &(_, kappa, _) in &profile {
            assert!((kappa * 0.05 - 1.0).abs() < 0.03, "κR = {}", kappa * 0.05);
        }
        let length = profile.last().unwrap().0;
        assert!((length - 0.05 * std::f64::consts::FRAC_PI_2).abs() < 1e-4);
        let line = Wall::single(Bezier::line(DVec2::ZERO, DVec2::new(0.01, 0.02)));
        assert!(line.curvature(10).iter().all(|&(_, k, _)| k == 0.0));
    }

    #[test]
    fn control_points_are_added_without_changing_the_shape_and_removed() {
        let wall = Wall::single(quarter());
        let more = rebuild(&wall, &add_control(&wall, 0));
        assert_eq!(more.segments()[0].degree(), 4);
        for k in 0..=10 {
            let t = k as f64 / 10.0;
            assert!(more.segments()[0].eval(t).distance(quarter().eval(t)) < 1e-15);
        }
        let fewer = rebuild(&more, &remove_control(&more, 0, 1));
        assert_eq!(fewer.segments()[0].degree(), 3);
        // A Bézier keeps one control point at least, and a line has none to remove.
        let one = Wall::build(
            wall.start(),
            wall.end(),
            &[SegmentInput {
                kind: SegmentKind::Bezier,
                controls: vec![DVec2::new(0.04, 0.02)],
                end: wall.end(),
                ..SegmentInput::default()
            }],
        )
        .unwrap();
        assert_eq!(remove_control(&one, 0, 0), one.to_input());
        let line = Wall::single(Bezier::line(wall.start(), wall.end()));
        let curved = rebuild(&line, &add_control(&line, 0));
        assert_eq!(curved.segments()[0].degree(), 2);
        assert!(
            curved.segments()[0]
                .eval(0.5)
                .distance(line.segments()[0].eval(0.5))
                < 1e-15
        );
    }

    #[test]
    fn bad_walls_are_errors() {
        let (a, b) = (DVec2::new(0.0, 0.01), DVec2::new(0.05, 0.08));
        assert!(Wall::build(a, b, &[]).is_err());
        let negative = SegmentInput {
            kind: SegmentKind::Bezier,
            controls: vec![DVec2::new(0.02, -0.01)],
            end: b,
            ..SegmentInput::default()
        };
        assert!(Wall::build(a, b, &[negative]).is_err());
        let empty = SegmentInput {
            kind: SegmentKind::Bezier,
            end: b,
            ..SegmentInput::default()
        };
        assert!(Wall::build(a, b, std::slice::from_ref(&empty)).is_err());
        let cornerless = SegmentInput {
            kind: SegmentKind::Arc,
            ..empty.clone()
        };
        assert!(Wall::build(a, b, &[cornerless]).is_err());
        let quintic = SegmentInput {
            kind: SegmentKind::BSpline,
            degree: Some(5),
            controls: vec![DVec2::new(0.02, 0.02); 6],
            ..empty.clone()
        };
        assert!(Wall::build(a, b, &[quintic]).is_err());
        let short = SegmentInput {
            kind: SegmentKind::BSpline,
            degree: Some(4),
            controls: vec![DVec2::new(0.02, 0.02); 2],
            ..empty
        };
        assert!(Wall::build(a, b, &[short]).is_err());
    }

    /// A wall turning from axial to radial: an arc segment with its corner at (0.05,
    /// 0.01) and a 0.02 m radius.
    fn elbow() -> Wall {
        Wall::build(
            DVec2::new(0.0, 0.01),
            DVec2::new(0.05, 0.08),
            &[SegmentInput {
                kind: SegmentKind::Arc,
                controls: vec![DVec2::new(0.05, 0.01)],
                radius: Some(0.02),
                ..SegmentInput::default()
            }],
        )
        .unwrap()
    }

    #[test]
    fn an_arc_segment_rounds_its_corner_with_the_radius_that_fits() {
        let wall = elbow();
        let s = &wall.segments()[0];
        let f = s.fillet().unwrap();
        assert!((f.radius - 0.02).abs() < 1e-12);
        assert!(f.center.distance(DVec2::new(0.03, 0.03)) < 1e-12);
        assert!(f.tangent_points[0].distance(DVec2::new(0.03, 0.01)) < 1e-12);
        assert!(f.tangent_points[1].distance(DVec2::new(0.05, 0.03)) < 1e-12);
        // Line, two arc pieces of 45°, line; the arc is round within 5·10⁻⁶ R.
        assert_eq!(s.pieces().len(), 4);
        for piece in &s.pieces()[1..3] {
            for p in piece.sample(30) {
                let r = p.distance(f.center);
                assert!((r - 0.02).abs() < 1e-7, "r = {r}");
            }
        }
        let kappa: Vec<f64> = wall.curvature(20).iter().map(|c| c.1).collect();
        assert!(kappa.iter().cloned().fold(0.0, f64::max) < 1.0 / 0.02 * 1.005);
        assert!(s.direction(0.0).distance(DVec2::X) < 1e-12);
        assert!(s.direction(1.0).distance(DVec2::Y) < 1e-12);

        // Too large for the straight parts: the shorter one sets the radius.
        let mut input = wall.to_input();
        input[0].radius = Some(1.0);
        let fitted = rebuild(&wall, &input);
        let f = fitted.segments()[0].fillet().unwrap();
        assert!((f.radius - 0.05).abs() < 1e-12, "{}", f.radius);
        // The asked radius is kept in the input.
        assert_eq!(fitted.to_input()[0].radius, Some(1.0));

        // Dragging the centre along the bisector sets the radius.
        let r = radius_for_center(&wall, 0, DVec2::new(0.04, 0.02)).unwrap();
        assert!((r - 0.01).abs() < 1e-12, "{r}");
        let smaller = rebuild(&wall, &set_radius(&wall, 0, r));
        assert!((smaller.segments()[0].fillet().unwrap().radius - 0.01).abs() < 1e-12);
    }

    #[test]
    fn an_arc_splits_into_parts_that_keep_its_shape() {
        let wall = elbow();
        // On the first straight part, on the arc, and on the second straight part.
        for t in [0.1, 0.5, 0.9] {
            let parts = rebuild(&wall, &split(&wall, 0, t));
            assert_eq!(parts.segments().len(), 2, "t = {t}");
            assert!(gap(&parts, &wall) < 1e-6, "t = {t}: {}", gap(&parts, &wall));
            assert!(gap(&wall, &parts) < 1e-6, "t = {t}");
        }
        let on_arc = rebuild(&wall, &split(&wall, 0, 0.5));
        assert!(
            on_arc
                .segments()
                .iter()
                .all(|s| s.kind() == SegmentKind::Arc)
        );
    }

    #[test]
    fn b_spline_segments_split_insert_knots_and_change_degree() {
        let wall = Wall::single(quarter());
        let spline = rebuild(&wall, &set_kind(&wall, 0, SegmentKind::BSpline));
        let s = &spline.segments()[0];
        assert_eq!(s.kind(), SegmentKind::BSpline);
        assert_eq!(s.degree(), 3);
        // The cubic Bézier is the same curve as a B-spline.
        assert!(gap(&spline, &wall) < 1e-8);

        let more = rebuild(&spline, &insert_knot(&spline, 0, 0.3));
        assert_eq!(more.segments()[0].control_points().len(), 5);
        assert_eq!(more.segments()[0].pieces().len(), 2);
        assert!(gap(&more, &wall) < 1e-8);
        let again = rebuild(&more, &add_control(&more, 0));
        assert_eq!(again.segments()[0].control_points().len(), 6);
        assert!(gap(&again, &wall) < 1e-8);

        let parts = rebuild(&again, &split(&again, 0, 0.5));
        assert_eq!(parts.segments().len(), 2);
        assert!(
            parts
                .segments()
                .iter()
                .all(|s| s.kind() == SegmentKind::BSpline)
        );
        assert!(gap(&parts, &wall) < 1e-8);
        assert_eq!(parts.smooth(), [true]);

        let quartic = rebuild(&again, &set_degree(&again, 0, 4));
        assert_eq!(quartic.segments()[0].degree(), 4);
        // Too few points for degree 4: knots are inserted first.
        let quadratic = rebuild(
            &wall,
            &[SegmentInput {
                kind: SegmentKind::BSpline,
                degree: Some(2),
                controls: vec![DVec2::new(0.05, 0.01), DVec2::new(0.05, 0.03)],
                end: wall.end(),
                ..SegmentInput::default()
            }],
        );
        let raised = rebuild(&quadratic, &set_degree(&quadratic, 0, 4));
        assert_eq!(raised.segments()[0].degree(), 4);
        assert_eq!(raised.segments()[0].control_points().len(), 5);

        // An arc turns into a B-spline close to it.
        let from_arc = rebuild(&elbow(), &set_kind(&elbow(), 0, SegmentKind::BSpline));
        assert!(gap(&from_arc, &elbow()) < 2e-3);
    }

    #[test]
    fn a_coupled_bezier_has_no_curvature_at_its_ends() {
        let wall = Wall::single(quarter());
        let coupled = rebuild(&wall, &set_coupled(&wall, 0, true));
        let s = &coupled.segments()[0];
        assert!(s.coupled());
        assert_eq!(s.degree(), 4);
        let profile = coupled.curvature(200);
        assert!(profile[0].1 < 1e-9, "κ at the inlet {}", profile[0].1);
        assert!(profile.last().unwrap().1 < 1e-9);
        // Moving the middle point carries points 1 and 3 along their lines.
        let middle = WallPoint::Control {
            segment: 0,
            index: 1,
        };
        let to = coupled.point(middle).unwrap() + DVec2::new(-0.005, 0.004);
        let moved = rebuild(&coupled, &move_point(&coupled, middle, to));
        let p = moved.segments()[0].control_points();
        assert!(p[2].distance(to) < 1e-12);
        assert!((p[1] - p[0]).perp_dot(p[2] - p[0]).abs() < 1e-15);
        assert!((p[3] - p[4]).perp_dot(p[2] - p[4]).abs() < 1e-15);
        let profile = moved.curvature(200);
        assert!(profile[0].1 < 1e-9 && profile.last().unwrap().1 < 1e-9);
    }

    #[test]
    fn fixed_end_angles_turn_the_bezier_ends() {
        let wall = Wall::single(quarter());
        let tilted = rebuild(&wall, &set_angles(&wall, 0, Some(0.2), Some(1.3)));
        let s = &tilted.segments()[0];
        assert!(s.direction(0.0).distance(heading(0.2)) < 1e-12);
        assert!(s.direction(1.0).distance(heading(1.3)) < 1e-12);
        assert_eq!(s.angles(), (Some(0.2), Some(1.3)));
        // With coupled points the angle holds too, and the ends stay flat.
        let both = rebuild(&tilted, &set_coupled(&tilted, 0, true));
        let s = &both.segments()[0];
        assert!(s.direction(0.0).distance(heading(0.2)) < 1e-12);
        assert!(s.direction(1.0).distance(heading(1.3)) < 1e-12);
        assert!(both.curvature(100)[0].1 < 1e-9);
    }

    #[test]
    fn stored_walls_without_the_new_fields_still_load() {
        let old = r#"{"kind":"Bezier","controls":[[0.03,0.01]],"end":[0.05,0.08],"smooth":false}"#;
        let s: SegmentInput = serde_json::from_str(old).unwrap();
        assert_eq!(s.radius, None);
        assert!(!s.coupled && s.knots.is_empty());
        // Only the fields in use are written.
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("radius") && !json.contains("coupled"));
    }
}
