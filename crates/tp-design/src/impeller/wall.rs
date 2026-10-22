//! Hub and shroud walls drawn segment by segment: Bézier curves and straight lines
//! joined end to end, each joint smooth (tangent) or a corner.
//!
//! The first segment starts at the inlet and the last ends at the outlet; those two
//! points come from the main dimensions and the axial length, so a drawn wall follows
//! them when they change. Every other point is stored as (z, r) in m.

use serde::{Deserialize, Serialize};
use tp_math::{Bezier, DVec2};

use crate::DesignError;

/// Shape of one segment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentKind {
    #[default]
    Bezier,
    Line,
}

/// One segment of a drawn wall, as stored in the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentInput {
    pub kind: SegmentKind,
    /// Inner control points of a Bézier, (z, r) in m; lines have none.
    #[serde(default)]
    pub controls: Vec<DVec2>,
    /// End point, (z, r) in m. The last segment ends at the outlet instead.
    pub end: DVec2,
    /// Whether the joint at `end` is tangent to the next segment.
    #[serde(default)]
    pub smooth: bool,
}

/// A point of a drawn wall that can be moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallPoint {
    /// Inner control point `index` of segment `segment`.
    Control { segment: usize, index: usize },
    /// The joint at the end of segment `segment` (never the last one).
    Joint { segment: usize },
}

/// A built hub or shroud wall.
#[derive(Debug, Clone, PartialEq)]
pub struct Wall {
    segments: Vec<Bezier<DVec2>>,
    /// Joint after each segment but the last: tangent or not.
    smooth: Vec<bool>,
}

impl Wall {
    /// A wall of one curve.
    pub fn single(curve: Bezier<DVec2>) -> Self {
        Self {
            segments: vec![curve],
            smooth: Vec::new(),
        }
    }

    /// Builds the wall from `start` to `end` through the drawn segments. At a smooth
    /// joint the Bézier side turns to the tangent of the other side, keeping the
    /// distance of its control point to the joint; between two Béziers the second one
    /// follows the first. Two lines meet at a corner whatever the flag says.
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
            let mut p = vec![from];
            if s.kind == SegmentKind::Bezier {
                if s.controls.is_empty() {
                    return Err(DesignError::InvalidInput(format!(
                        "Bézier segment {} has no control points",
                        i + 1
                    )));
                }
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
                return Err(DesignError::InvalidInput(format!(
                    "segment {} has no length",
                    i + 1
                )));
            }
            points.push(p);
            from = to;
        }
        let smooth: Vec<bool> = input[..n - 1].iter().map(|s| s.smooth).collect();
        for (i, &tangent) in smooth.iter().enumerate() {
            if tangent {
                align(&mut points, i);
            }
        }
        Ok(Self {
            segments: points
                .into_iter()
                .map(|p| Bezier::new(p).expect("segments have two points or more"))
                .collect(),
            smooth,
        })
    }

    pub fn segments(&self) -> &[Bezier<DVec2>] {
        &self.segments
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
        let p = self
            .segments
            .last()
            .expect("a wall has segments")
            .control_points();
        (p[p.len() - 1] - p[p.len() - 2]).normalize()
    }

    /// `count` points from inlet to outlet, shared out among the segments by their
    /// length, joints included once; at least one interval per segment. A single curve
    /// gives exactly its own parameter-uniform samples.
    pub fn sample(&self, count: usize) -> Vec<DVec2> {
        let count = count.max(self.segments.len() + 1);
        if self.segments.len() == 1 {
            return self.segments[0].sample(count);
        }
        let lengths: Vec<f64> = self.segments.iter().map(Bezier::arc_length).collect();
        let total: f64 = lengths.iter().sum();
        let intervals = count - 1;
        // Each segment gets at least one interval; the rest go by length.
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
                .expect("a wall has segments");
            shares[i] += 1;
        }
        let mut points = vec![self.start()];
        for (segment, &k) in self.segments.iter().zip(&shares) {
            points.extend(segment.sample(k + 1).into_iter().skip(1));
        }
        points
    }

    /// The segments as stored inputs: built points, so a smooth joint keeps its
    /// aligned control points.
    pub fn to_input(&self) -> Vec<SegmentInput> {
        let n = self.segments.len();
        self.segments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let p = s.control_points();
                SegmentInput {
                    kind: if s.degree() == 1 {
                        SegmentKind::Line
                    } else {
                        SegmentKind::Bezier
                    },
                    controls: p[1..p.len() - 1].to_vec(),
                    end: s.end(),
                    smooth: i + 1 < n && self.smooth[i],
                }
            })
            .collect()
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

    /// Curvature along the wall: `per_segment` samples of each segment, as (arc length
    /// from the inlet in m, curvature κ in 1/m, point). Lines have none; a corner at a
    /// joint shows as two samples at the same length.
    pub fn curvature(&self, per_segment: usize) -> Vec<(f64, f64, DVec2)> {
        let n = per_segment.max(2);
        let mut out = Vec::with_capacity(n * self.segments.len());
        let mut length = 0.0;
        for segment in &self.segments {
            let d1 = segment.derivative();
            let d2 = (segment.degree() >= 2).then(|| d1.derivative());
            let mut previous = segment.start();
            for k in 0..n {
                let t = k as f64 / (n - 1) as f64;
                let p = segment.eval(t);
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

    /// Segment and curve parameter of the wall point nearest to `pointer`.
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

/// Turns the Bézier side of the joint after segment `i` to the other side's tangent.
fn align(points: &mut [Vec<DVec2>], i: usize) {
    let (before, after) = points.split_at_mut(i + 1);
    let (a, b) = (&mut before[i], &mut after[0]);
    let joint = b[0];
    let (a_line, b_line) = (a.len() == 2, b.len() == 2);
    if !b_line {
        let incoming = joint - a[a.len() - 2];
        let reach = b[1].distance(joint);
        if incoming.length() > 0.0 {
            b[1] = joint + incoming.normalize() * reach;
        }
    } else if !a_line {
        let outgoing = b[1] - joint;
        let k = a.len() - 2;
        let reach = a[k].distance(joint);
        a[k] = joint - outgoing.normalize() * reach;
    }
}

/// Splits segment `segment` at curve parameter `t` into two of the same kind, joined
/// smoothly; the wall keeps its shape.
pub fn split(wall: &Wall, segment: usize, t: f64) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(curve) = wall.segments.get(segment) else {
        return input;
    };
    let t = t.clamp(0.05, 0.95);
    let (left, right) = curve.split(t);
    let kind = input[segment].kind;
    let inner = |c: &Bezier<DVec2>| {
        let p = c.control_points();
        p[1..p.len() - 1].to_vec()
    };
    let first = SegmentInput {
        kind,
        controls: inner(&left),
        end: left.end(),
        smooth: true,
    };
    let second = SegmentInput {
        kind,
        controls: inner(&right),
        end: right.end(),
        smooth: input[segment].smooth,
    };
    input.splice(segment..=segment, [first, second]);
    input
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
    let merged = if a.len() == 2 && b.len() == 2 {
        SegmentInput {
            kind: SegmentKind::Line,
            controls: Vec::new(),
            end,
            smooth: input[segment + 1].smooth,
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
            smooth: input[segment + 1].smooth,
        }
    };
    input.splice(segment..=segment + 1, [merged]);
    input
}

/// Turns segment `segment` into `kind`: a line drops its control points, a Bézier
/// starts as the line with its control points at the thirds.
pub fn set_kind(wall: &Wall, segment: usize, kind: SegmentKind) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(s) = input.get_mut(segment) else {
        return input;
    };
    let curve = &wall.segments[segment];
    s.kind = kind;
    s.controls = match kind {
        SegmentKind::Line => Vec::new(),
        SegmentKind::Bezier if curve.degree() == 1 => {
            let (a, b) = (curve.start(), curve.end());
            vec![a.lerp(b, 1.0 / 3.0), a.lerp(b, 2.0 / 3.0)]
        }
        SegmentKind::Bezier => s.controls.clone(),
    };
    input
}

/// Moves a point to `to`. Moving a joint carries its two neighbouring control points
/// along, so the joint keeps its tangent.
pub fn move_point(wall: &Wall, at: WallPoint, to: DVec2) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    match at {
        WallPoint::Control { segment, index } => {
            if let Some(c) = input
                .get_mut(segment)
                .and_then(|s| s.controls.get_mut(index))
            {
                *c = to;
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

/// Gives Bézier segment `segment` one more control point without changing its shape
/// (degree elevation); a line becomes a Bézier through the same points.
pub fn add_control(wall: &Wall, segment: usize) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    let Some(curve) = wall.segments.get(segment) else {
        return input;
    };
    let elevated = curve.elevate();
    let p = elevated.control_points();
    input[segment].kind = SegmentKind::Bezier;
    input[segment].controls = p[1..p.len() - 1].to_vec();
    input
}

/// Removes inner control point `index` of segment `segment`, keeping at least one; the
/// curve takes the shape of the points left.
pub fn remove_control(wall: &Wall, segment: usize, index: usize) -> Vec<SegmentInput> {
    let mut input = wall.to_input();
    if let Some(s) = input.get_mut(segment)
        && s.kind == SegmentKind::Bezier
        && s.controls.len() > 1
        && index < s.controls.len()
    {
        s.controls.remove(index);
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
                controls: Vec::new(),
                end: joint,
                smooth: true,
            },
            SegmentInput {
                kind: SegmentKind::Bezier,
                controls: vec![DVec2::new(0.03, 0.05), DVec2::new(0.05, 0.06)],
                end,
                smooth: false,
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
                smooth: false,
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
            smooth: false,
        };
        assert!(Wall::build(a, b, &[negative]).is_err());
        let empty = SegmentInput {
            kind: SegmentKind::Bezier,
            controls: Vec::new(),
            end: b,
            smooth: false,
        };
        assert!(Wall::build(a, b, &[empty]).is_err());
    }
}
