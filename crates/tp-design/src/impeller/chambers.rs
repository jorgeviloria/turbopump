//! The side chambers between the impeller and the casing, the secondary flow path:
//!
//! - **Front chamber**: between the outer face of the shroud cover and a casing wall an
//!   axial gap s_ax away from it, from r₂ inward, and the annular seal: a gap of radial
//!   clearance s between the impeller ring (radius r) and the casing (radius r + s),
//!   through which the leakage returns to the suction. The casing wall runs on the casing
//!   seat of the front seal along the ring; the outline of the fluid goes between r and
//!   r + s. Without a front seal the chamber closes on the face of the cover.
//! - **Rear chamber**: between the back of the hub disk and a flat casing wall s_ax
//!   behind it, from the disk rim down to the shaft seal d_D, where it is closed. With a
//!   rear seal the wall stands s_ax behind the tip of the collar that carries its
//!   impeller ring, and the chamber wraps the collar: down to the ring, along it, round
//!   the tip and inside the collar. The rear casing is a plain plate s_ax behind the
//!   collar tip: the rear seal gap (its diameter and clearance) is in the values only,
//!   not in the outline. With back vanes on the disk (our choice: still one plain
//!   plate) its gap to the disk is the vane height h plus their tip clearance s_v
//!   instead of s_ax: the clearance of the vanes governs, moving the plate forward or
//!   back as needed, because the thrust prediction uses that clearance.
//!
//! The casing walls are wires like the walls of the flow path: automatic from the
//! values here, or drawn segment by segment (see [`super::wall`]). The values follow the
//! loss analysis: s_ax = 0.05·r₂ (our choice; Gülich finds its effect small, Chap.
//! 3.6.1), the seals (diameter, length and clearance, Gülich eq. 3.12) those of
//! [`super::seals`], and d_D the hub diameter, as the axial thrust takes it. When the
//! chambers are designed, the performance prediction takes them wherever its own are
//! automatic.

use serde::{Deserialize, Serialize};
use tp_math::DVec2;

use super::ImpellerBasis;
use super::back_vanes::BackVanes;
use super::seals::{ResolvedSeals, Seal};
use super::wall::{SegmentInput, SegmentKind, Wall, offset_points};
use crate::losses::AXIAL_GAP_RATIO;
use crate::{DesignError, Param};

/// Points of a material wire used to find the faces the chambers lie on.
const FACE_SAMPLES: usize = 600;

/// Points of the offset face kept as control points of the casing B-spline.
const CASING_POINTS: usize = 9;

/// Inputs of the side chambers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChamberSettings {
    /// Design the side chambers: shown on their page and exported with the CAD model.
    pub enabled: bool,
    /// Axial gap s_ax between the impeller and the casing walls, m; automatic: 0.05·r₂.
    pub axial_gap: Param<f64>,
    /// Diameter of the shaft seal d_D, where the rear chamber ends, m; automatic: the
    /// hub diameter.
    pub shaft_seal_diameter: Param<f64>,
    /// Front casing wall drawn segment by segment, from the outlet to the seal exit;
    /// `None` keeps the automatic one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front: Option<Vec<SegmentInput>>,
    /// Rear casing wall drawn segment by segment, from the outlet to the shaft seal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rear: Option<Vec<SegmentInput>>,
}

/// The values the chambers were built with, m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChamberValues {
    pub axial_gap: f64,
    /// Diameter of the front annular seal, outside of its impeller ring; 0 without a
    /// front seal.
    pub seal_diameter: f64,
    /// Radial clearance of the front seal; 0 without it.
    pub seal_clearance: f64,
    /// Axial length of the front seal as built: the run of the casing wall on the casing
    /// seat, where the casing faces the impeller ring. It can be shorter than the nominal
    /// length of the seal: the wall must keep the axial gap from the shoulder of the
    /// cover and end at the tip of the seat, so it comes down to the seat that much
    /// short of the shoulder. 0 without a front seal, or when the wall never runs on it.
    pub seal_length: f64,
    pub shaft_seal_diameter: f64,
    /// Diameter of the rear annular seal, `None` without a rear seal.
    pub rear_seal_diameter: Option<f64>,
    /// Radial clearance of the rear seal, `None` without a rear seal.
    pub rear_seal_clearance: Option<f64>,
}

/// The two side chambers.
#[derive(Debug, Clone, PartialEq)]
pub struct Chambers {
    /// Casing wall of the front chamber, from the outlet to the seal exit.
    pub front_casing: Wall,
    /// Casing wall of the rear chamber, from the outlet to the shaft seal.
    pub rear_casing: Wall,
    /// Closed outlines of the chambers, (z, r) in m: the casing wall, the opening and
    /// the impeller face back to the outlet.
    pub front: Vec<DVec2>,
    pub rear: Vec<DVec2>,
    pub values: ChamberValues,
    /// Whether they are designed: exported with the CAD model.
    pub enabled: bool,
    /// What could not be built as asked; only when the chambers are designed.
    pub warnings: Vec<String>,
}

/// A straight segment of a wire to `end`.
fn line(end: DVec2) -> SegmentInput {
    SegmentInput {
        kind: SegmentKind::Line,
        end,
        ..SegmentInput::default()
    }
}

/// The part of `points` from the one nearest to `from` on.
fn from_nearest(points: &[DVec2], from: DVec2) -> &[DVec2] {
    let i = points
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.distance(from).total_cmp(&b.1.distance(from)))
        .map_or(0, |(i, _)| i);
    &points[i..]
}

/// The face of the shroud cover the front chamber lies on, from the rim at the outlet
/// to the tip of the seat, the most upstream point.
fn front_face(cover: &Wall) -> Vec<DVec2> {
    let points = cover.sample(FACE_SAMPLES);
    let rim = cover.segments()[0].end();
    let face = from_nearest(&points, rim);
    let tip = face
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.x.total_cmp(&b.1.x))
        .map_or(0, |(i, _)| i);
    face[..=tip].to_vec()
}

/// Index of the top of the shoulder of `face`, which ends in the seat of the impeller
/// ring: the radial step that leads to the run of points at the radius of the tip.
fn shoulder_top(face: &[DVec2]) -> usize {
    let tip = face.last().expect("a face has points");
    let on_seat = |p: &DVec2| (p.y - tip.y).abs() <= 1e-9 * tip.y;
    let seat_start =
        (face.len() - face.iter().rev().take_while(|p| on_seat(p)).count()).min(face.len() - 1);
    let mut top = seat_start;
    while top > 0 && (face[top - 1].x - face[seat_start].x).abs() <= 1e-9 * tip.y {
        top -= 1;
    }
    top
}

/// The back of the hub disk the rear chamber lies on, from the rim down to radius
/// `r_seal` (or the innermost point of the back). With a rear seal it follows the
/// collar: the outer face of the impeller ring, the end of the collar and its inside.
fn rear_face(disk: &Wall, r_seal: f64, seal: Option<&Seal>) -> Vec<DVec2> {
    let points = disk.sample(FACE_SAMPLES);
    let rim = disk.segments()[0].end();
    let back = from_nearest(&points, rim);
    let mut face = vec![back[0]];
    for w in back.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b.y <= r_seal {
            let f = if (a.y - b.y).abs() > 0.0 {
                (a.y - r_seal) / (a.y - b.y)
            } else {
                1.0
            };
            face.push(a.lerp(b, f.clamp(0.0, 1.0)));
            return wrap_collar(face, seal);
        }
        face.push(b);
    }
    wrap_collar(face, seal)
}

/// `face` with the run along the seat of the collar replaced by one along the outer
/// face of the impeller ring, which stands on the seat.
fn wrap_collar(face: Vec<DVec2>, seal: Option<&Seal>) -> Vec<DVec2> {
    let Some(seal) = seal.filter(|s| s.radius > s.impeller_seat + 1e-12) else {
        return face;
    };
    let on_seat = |p: &DVec2| (p.y - seal.impeller_seat).abs() <= 1e-9 * seal.radius;
    let Some(first) = face.iter().position(on_seat) else {
        return face;
    };
    let tip = face
        .iter()
        .enumerate()
        .filter(|(_, p)| on_seat(p))
        .max_by(|a, b| a.1.x.total_cmp(&b.1.x))
        .map_or(first, |(i, _)| i);
    // No collar on the seat (it was left out): the face is kept as it is.
    if face[tip].x - face[first].x < 0.5 * seal.length {
        return face;
    }
    let above = face
        .iter()
        .position(|p| p.y <= seal.radius)
        .filter(|&i| i > 0 && i <= first);
    let Some(above) = above else {
        return face;
    };
    let (p, q) = (face[above - 1], face[above]);
    let f = ((p.y - seal.radius) / (p.y - q.y)).clamp(0.0, 1.0);
    let mut wrapped = face[..above].to_vec();
    wrapped.push(p.lerp(q, f));
    wrapped.push(DVec2::new(face[tip].x, seal.radius));
    wrapped.extend_from_slice(&face[tip..]);
    wrapped
}

/// The cubic B-spline segment of a wire through a few of `points`.
fn curve(points: &[DVec2]) -> Option<SegmentInput> {
    let n = points.len();
    let kept: Vec<DVec2> = (0..CASING_POINTS)
        .map(|i| points[(i * (n - 1)) / (CASING_POINTS - 1)])
        .collect();
    let spline = tp_math::BSpline::interpolate(&kept, 3).ok()?;
    let p = spline.control_points();
    let k = spline.knots();
    Some(SegmentInput {
        kind: SegmentKind::BSpline,
        controls: p[1..p.len() - 1].to_vec(),
        end: *points.last()?,
        degree: Some(3),
        knots: k[4..k.len() - 4].to_vec(),
        ..SegmentInput::default()
    })
}

/// The wire of the casing along `points`: a curve, or a line when they are too few.
fn casing_piece(points: &[DVec2]) -> SegmentInput {
    let last = *points.last().expect("points");
    if points.len() >= CASING_POINTS
        && let Some(c) = curve(points)
    {
        c
    } else {
        line(last)
    }
}

/// `points` down to where they first reach radius `y`, which ends them, and the index
/// of the point it replaces; `None` when they never come down to it.
fn down_to(points: &[DVec2], y: f64) -> Option<Vec<DVec2>> {
    let i = points.iter().position(|p| p.y <= y).filter(|&i| i > 0)?;
    let (p, q) = (points[i - 1], points[i]);
    let f = ((p.y - y) / (p.y - q.y)).clamp(0.0, 1.0);
    let mut kept = points[..i].to_vec();
    kept.push(p.lerp(q, f));
    Some(kept)
}

/// Builds the chambers beside the material wires `cover` (shroud) and `disk` (hub).
/// `inside` is a point of the channel, to tell the impeller side from the casing side.
pub fn build(
    cover: &Wall,
    disk: &Wall,
    inside: DVec2,
    basis: &ImpellerBasis,
    settings: &ChamberSettings,
    seals: &ResolvedSeals,
    vanes: Option<&BackVanes>,
) -> Result<Chambers, DesignError> {
    let mut warnings = Vec::new();
    let axial_gap = settings
        .axial_gap
        .resolve(|| AXIAL_GAP_RATIO * basis.outlet_diameter / 2.0)
        .max(1e-5);

    // Front: the face of the cover, the seat of the impeller ring and the seal round it.
    let face = front_face(cover);
    let tip = *face.last().expect("a face has points");
    // The casing follows the face down to the top of the shoulder, not the radial step
    // of the shoulder itself, whose offset would fold on its corner; without a front
    // seal it follows the whole face.
    let top = match seals.front {
        Some(_) => shoulder_top(&face),
        None => face.len() - 1,
    };
    if seals.front.is_none() {
        warnings.push(
            "the front seal is off: the front chamber closes on the face of the shroud cover"
                .into(),
        );
    }
    let outer_face = &face[..=top];
    let casing_face = offset_points(outer_face, axial_gap, inside).unwrap_or_else(|_| {
        warnings.push(format!(
            "the front casing wall {:.1} mm from the shroud does not fit its curvature; a \
             straight one is used",
            axial_gap * 1e3
        ));
        vec![
            outer_face[0] - DVec2::new(axial_gap, 0.0),
            outer_face[outer_face.len() - 1] + DVec2::new(-axial_gap, axial_gap),
        ]
    });
    // At the radius of the rim, so that the chamber opens on one radius.
    let front_start = DVec2::new(casing_face[0].x, outer_face[0].y);
    let mut casing_face = casing_face;
    casing_face[0] = front_start;
    // With a seal the wall comes down to the casing seat and runs along it to the tip;
    // without one it goes on to the tip of the face.
    let front_end = match seals.front {
        Some(seal) => DVec2::new(tip.x, seal.casing_seat),
        None => tip,
    };
    let automatic_front = || {
        let mut wire = Vec::new();
        match seals.front {
            Some(seal) => {
                let at_seat = match down_to(&casing_face, seal.casing_seat) {
                    Some(to_seat) => {
                        wire.push(casing_piece(&to_seat));
                        *to_seat.last().expect("points")
                    }
                    None => {
                        // The offset face never comes down to the seat: it goes on to
                        // its end and across to the seat.
                        let last = *casing_face.last().expect("points");
                        wire.push(casing_piece(&casing_face));
                        let across = DVec2::new(last.x, seal.casing_seat);
                        wire.push(line(across));
                        across
                    }
                };
                if at_seat.distance(front_end) > 1e-12 {
                    wire.push(line(front_end));
                }
            }
            None => {
                wire.push(casing_piece(&casing_face));
                wire.push(line(front_end));
            }
        }
        wire
    };
    let front_casing = match &settings.front {
        Some(segments) => Wall::build(front_start, front_end, segments)?,
        None => Wall::build(front_start, front_end, &automatic_front())?,
    };
    // The run of the wall on the casing seat: the last segment, a line to the end, when
    // the one before it ends on the seat.
    let seal_length = seals.front.map_or(0.0, |seal| {
        let segments = front_casing.segments();
        let last = &segments[segments.len() - 1];
        let from = match segments.len() {
            1 => front_start,
            n => segments[n - 2].end(),
        };
        if last.kind() == SegmentKind::Line
            && (from.y - seal.casing_seat).abs() <= 1e-9 * seal.casing_seat
        {
            (from.x - front_end.x).max(0.0)
        } else {
            0.0
        }
    });
    let mut front = front_casing.sample(FACE_SAMPLES / 2);
    if let Some(seal) = seals.front {
        // The wall runs on the casing seat; the fluid is bounded by the casing ring, on
        // r + s, and by the impeller ring, on r. The run on the seat is replaced by the
        // downstream face of the casing ring and the gap to the exit.
        let on_seat = |p: &DVec2| (p.y - seal.casing_seat).abs() <= 1e-9 * seal.casing_seat;
        let run = front.iter().rev().take_while(|p| on_seat(p)).count();
        if run > 0 {
            let start = front[front.len() - run];
            front.truncate(front.len() - run);
            front.push(start);
            let exit = front_end.x;
            if seal.casing_seat > seal.radius + seal.clearance + 1e-12 {
                front.push(DVec2::new(start.x, seal.radius + seal.clearance));
            }
            front.push(DVec2::new(exit, seal.radius + seal.clearance));
        } else {
            front.push(DVec2::new(front_end.x, seal.radius + seal.clearance));
        }
        // Across the exit onto the impeller ring, along it to the shoulder.
        front.push(DVec2::new(front_end.x, seal.radius));
        let shoulder = face[top];
        if (shoulder.y - seal.radius).abs() > 1e-12 {
            front.push(DVec2::new(shoulder.x, seal.radius));
        }
    }
    front.extend(face[..=top].iter().rev());

    // Rear: the back of the disk down to the shaft seal, and a flat wall behind it.
    let shaft_seal_diameter = settings.shaft_seal_diameter.resolve(|| basis.hub_diameter);
    let back = rear_face(disk, shaft_seal_diameter / 2.0, seals.rear.as_ref());
    let mut rear_z = back.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max) + axial_gap;
    // Back vanes: the clearance of their tips governs, not s_ax. The plate stands
    // exactly the tip clearance behind the true maximum tip z (the tips are a height
    // from the face along its normal, so where the face leans their axial distance
    // from it is slightly under h), moving forward or back from the plain s_ax place.
    if let Some(tips) = vanes.and_then(|v| v.tip_z().map(|z| z + v.tip_clearance)) {
        rear_z = tips;
    }
    let seal_end = *back.last().expect("a face has points");
    let rear_start = DVec2::new(rear_z, back[0].y);
    let rear_end = DVec2::new(rear_z, seal_end.y);
    let rear_casing = match &settings.rear {
        Some(segments) => Wall::build(rear_start, rear_end, segments)?,
        None => Wall::build(rear_start, rear_end, &[line(rear_end)])?,
    };
    if settings.rear.is_some()
        && let Some(tips) = vanes.and_then(|v| v.tip_z().map(|z| z + v.tip_clearance))
        && rear_casing.start().x < tips - 1e-9
    {
        warnings.push(format!(
            "the rear casing wall drawn is closer to the back vanes than their tip clearance ({:.1} mm)",
            vanes.map_or(0.0, |v| v.tip_clearance) * 1e3
        ));
    }
    let mut rear = rear_casing.sample(FACE_SAMPLES / 2);
    rear.extend(back.iter().rev());

    Ok(Chambers {
        front_casing,
        rear_casing,
        front,
        rear,
        values: ChamberValues {
            axial_gap,
            seal_diameter: seals.front.map_or(0.0, |s| 2.0 * s.radius),
            seal_clearance: seals.front.map_or(0.0, |s| s.clearance),
            seal_length,
            shaft_seal_diameter: 2.0 * seal_end.y,
            rear_seal_diameter: seals.rear.map(|s| 2.0 * s.radius),
            rear_seal_clearance: seals.rear.map(|s| s.clearance),
        },
        enabled: settings.enabled,
        warnings: if settings.enabled {
            warnings
        } else {
            Vec::new()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::meridian::{MeridionalContour, MeridionalInput, tests::basis};
    use crate::losses::seal_clearance;

    fn contour(chambers: ChamberSettings) -> MeridionalContour {
        let input = MeridionalInput {
            chambers,
            ..MeridionalInput::default()
        };
        MeridionalContour::build(&basis(), &input, 1.0).unwrap()
    }

    #[test]
    fn the_front_chamber_keeps_the_gap_and_ends_in_the_seal() {
        let b = basis();
        let c = contour(ChamberSettings::default());
        let ch = &c.chambers;
        let v = ch.values;
        assert!((v.axial_gap - 0.05 * b.outlet_diameter / 2.0).abs() < 1e-12);
        // The seal values are those of the front seal; the casing wall ends on its
        // casing seat.
        let front = c.seals.front.unwrap();
        assert!(v.seal_length > 0.0 && v.seal_length <= front.length);
        assert_eq!(v.seal_diameter, 2.0 * front.radius);
        assert_eq!(v.seal_clearance, front.clearance);
        assert!((v.seal_clearance - seal_clearance(v.seal_diameter)).abs() < 1e-15);
        let end = ch.front_casing.end();
        assert!((end.y - front.casing_seat).abs() < 1e-12);
        // The casing wall keeps the axial gap from the shroud cover over the face.
        let cover = c.shroud_material.sample(2000);
        for k in 1..8 {
            let p = ch.front_casing.segments()[0].eval(k as f64 / 10.0);
            let d = cover
                .iter()
                .map(|q| q.distance(p))
                .fold(f64::INFINITY, f64::min);
            assert!((d - v.axial_gap).abs() < 0.05 * v.axial_gap, "{d}");
        }
        // The outline is closed by the openings: it starts and ends near r₂.
        assert!(ch.front[0].y > 0.9 * b.outlet_diameter / 2.0);
        assert!(ch.front.last().unwrap().y > 0.9 * b.outlet_diameter / 2.0);
    }

    #[test]
    fn the_rear_chamber_is_flat_behind_the_disk_down_to_the_shaft_seal() {
        let b = basis();
        let c = contour(ChamberSettings::default());
        let ch = &c.chambers;
        let v = ch.values;
        let rear = ch.rear_casing.start();
        assert_eq!(rear.x, ch.rear_casing.end().x);
        let back = c.hub_material.sample(2000);
        let farthest = back.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        assert!((rear.x - farthest - v.axial_gap).abs() < 1e-12);
        assert!((v.shaft_seal_diameter - b.hub_diameter).abs() < 1e-9);
        // A larger shaft seal ends the chamber higher.
        let higher = contour(ChamberSettings {
            shaft_seal_diameter: Param::Manual(1.5 * b.hub_diameter),
            ..ChamberSettings::default()
        });
        let end = higher.chambers.rear_casing.end();
        assert!((end.y - 0.75 * b.hub_diameter).abs() < 1e-6);
    }

    #[test]
    fn warnings_only_when_the_chambers_are_designed() {
        let b = basis();
        let mut input = MeridionalInput::default();
        input.seals.front.enabled = false;
        let quiet = MeridionalContour::build(&b, &input, 1.0).unwrap();
        assert!(quiet.chambers.warnings.is_empty());
        input.chambers.enabled = true;
        let loud = MeridionalContour::build(&b, &input, 1.0).unwrap();
        assert!(
            loud.chambers
                .warnings
                .iter()
                .any(|w| w.contains("front seal"))
        );
    }

    /// The extent in z of the points of `wall` on the horizontal line `y`.
    fn run_on(wall: &Wall, y: f64) -> Option<(f64, f64)> {
        let on: Vec<f64> = wall
            .sample(4000)
            .iter()
            .filter(|p| (p.y - y).abs() < 1e-9)
            .map(|p| p.x)
            .collect();
        let lo = on.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = on.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (on.len() > 1).then_some((lo, hi))
    }

    #[test]
    fn the_front_casing_runs_on_the_casing_seat() {
        let c = contour(ChamberSettings::default());
        let front = c.seals.front.unwrap();
        assert!(front.casing_ring.is_some());
        let wall = &c.chambers.front_casing;
        let (lo, hi) = run_on(wall, front.casing_seat).expect("a run on the casing seat");
        // It ends at the tip of the impeller seat; its length is the seal length as built,
        // at most the nominal one.
        assert!((lo - wall.end().x).abs() < 1e-12);
        let v = c.chambers.values;
        assert!(((hi - lo) - v.seal_length).abs() <= 1e-12 * v.seal_length);
        assert!(v.seal_length <= front.length);
        let gap = v.axial_gap;
        assert!(
            v.seal_length >= front.length - gap - 1e-9,
            "{}",
            v.seal_length
        );
        assert!((wall.end().y - front.casing_seat).abs() < 1e-12);
        // Nothing of the wall dips below the seat.
        assert!(
            wall.sample(400)
                .iter()
                .all(|p| p.y >= front.casing_seat - 1e-9)
        );
    }

    #[test]
    fn a_face_that_never_reaches_the_casing_seat_ends_across_to_it() {
        // A wall so thick that the seal radius lies far above the face: the offset face
        // never comes down to the casing seat, so the wall goes on to its end and across
        // to the seat; the run on the seat has no length.
        let input = MeridionalInput {
            material: crate::impeller::material::MaterialSettings {
                wall_thickness: Param::Manual(0.2),
                ..Default::default()
            },
            ..MeridionalInput::default()
        };
        let c = MeridionalContour::build(&basis(), &input, 1.0).unwrap();
        let front = c.seals.front.unwrap();
        let wall = &c.chambers.front_casing;
        assert!((wall.end().y - front.casing_seat).abs() < 1e-12);
        let segments = wall.segments();
        // The last two segments are the way across to the seat and onto the end.
        let n = segments.len();
        assert!(n >= 3);
        let across = segments[n - 2].end();
        assert!((across.y - front.casing_seat).abs() < 1e-12);
        assert!(
            segments[n - 3].end().y < front.casing_seat,
            "the face stays below the seat"
        );
        // The cover tip lies downstream of the face end, so the run on the seat is empty.
        assert_eq!(c.chambers.values.seal_length, 0.0);
    }

    #[test]
    fn the_seal_gap_is_the_clearance() {
        for casing_ring in [true, false] {
            let mut input = MeridionalInput::default();
            input.seals.front.casing_ring.enabled = casing_ring;
            let c = MeridionalContour::build(&basis(), &input, 1.0).unwrap();
            let front = c.seals.front.unwrap();
            let x = c.chambers.front_casing.end().x;
            // Across the exit the outline has the ring of the impeller and the inside of
            // the casing, a clearance apart.
            let has = |y: f64| {
                c.chambers
                    .front
                    .iter()
                    .any(|p| (p.x - x).abs() < 1e-12 && (p.y - y).abs() < 1e-12)
            };
            assert!(has(front.radius), "{casing_ring}");
            assert!(has(front.radius + front.clearance), "{casing_ring}");
            // Along the seal the outline keeps to those two radii.
            let along: Vec<_> = c
                .chambers
                .front
                .iter()
                .filter(|p| p.x >= x - 1e-12 && p.x < x + 0.5 * front.length)
                .filter(|p| p.y <= front.radius + front.clearance + 1e-9)
                .collect();
            assert!(along.len() >= 2);
            for p in along {
                assert!(
                    (p.y - front.radius).abs() < 1e-12
                        || (p.y - (front.radius + front.clearance)).abs() < 1e-12,
                    "{p:?}"
                );
            }
            assert_eq!(c.chambers.values.seal_clearance, front.clearance);
        }
    }

    #[test]
    fn the_rear_chamber_wraps_the_collar() {
        use crate::impeller::material::collar_inner_radius;
        let b = basis();
        let c = contour(ChamberSettings::default());
        let ch = &c.chambers;
        let rear = c.seals.rear.unwrap();
        let t = MeridionalInput::default()
            .material
            .resolve(&b)
            .wall_thickness;
        // The collar tip: the farthest point of the hub material on the seat radius.
        let samples = c.hub_material.sample(4000);
        let tip_x = samples
            .iter()
            .filter(|p| (p.y - rear.impeller_seat).abs() < 1e-9)
            .map(|p| p.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let farthest = samples
            .iter()
            .map(|p| p.x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((tip_x - farthest).abs() < 1e-9);
        let has = |x: f64, y: f64| {
            ch.rear
                .iter()
                .any(|p| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9)
        };
        // Round the ring on the collar, across its end and inside the collar.
        assert!(has(tip_x, rear.radius), "outer face of the ring at the tip");
        assert!(has(tip_x, rear.impeller_seat));
        assert!(has(tip_x, collar_inner_radius(&rear, t)));
        // Down to the shaft seal, behind a plate at the axial gap from the tip.
        let low = ch.rear.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        assert!((low - ch.values.shaft_seal_diameter / 2.0).abs() < 1e-9);
        assert!((ch.rear_casing.start().x - tip_x - ch.values.axial_gap).abs() < 1e-9);
        assert_eq!(ch.values.rear_seal_diameter, Some(2.0 * rear.radius));
        assert_eq!(ch.values.rear_seal_clearance, Some(rear.clearance));
    }

    #[test]
    fn without_seals_the_chambers_close_on_the_faces() {
        let b = basis();
        let mut input = MeridionalInput::default();
        input.seals.front.enabled = false;
        input.seals.rear.enabled = false;
        let c = MeridionalContour::build(&b, &input, 1.0).unwrap();
        let ch = &c.chambers;
        let v = ch.values;
        assert_eq!(
            (v.seal_diameter, v.seal_clearance, v.seal_length),
            (0.0, 0.0, 0.0)
        );
        assert_eq!((v.rear_seal_diameter, v.rear_seal_clearance), (None, None));
        // The front wall ends on the cover; the rear plate runs down to the shaft seal.
        let near = |wall: &Wall, p: DVec2| {
            wall.sample(4000)
                .iter()
                .map(|q| q.distance(p))
                .fold(f64::INFINITY, f64::min)
        };
        assert!(near(&c.shroud_material, ch.front_casing.end()) < 1e-4 * b.outlet_diameter);
        assert!((ch.rear_casing.end().y - b.hub_diameter / 2.0).abs() < 1e-9);
        assert!(near(&c.hub_material, *ch.rear.last().unwrap()) < 1e-9 * b.outlet_diameter);
        assert!(c.seals.front.is_none() && c.seals.rear.is_none());
    }
}
