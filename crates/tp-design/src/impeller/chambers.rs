//! The side chambers between the impeller and the casing, the secondary flow path:
//!
//! - **Front chamber**: between the outer face of the shroud cover and a casing wall an
//!   axial gap s_ax away from it, from r₂ inward, and the annular seal: a gap of radial
//!   clearance s round the neck of the shroud, through which the leakage returns to the
//!   suction.
//! - **Rear chamber**: between the back of the hub disk and a flat casing wall s_ax
//!   behind it, from the disk rim down to the shaft seal d_D, where it is closed.
//!
//! The casing walls are wires like the walls of the flow path: automatic from the
//! values here, or drawn segment by segment (see [`super::wall`]). The values follow the
//! loss analysis: s_ax = 0.05·r₂ (our choice; Gülich finds its effect small, Chap.
//! 3.6.1), s from Gülich eq. 3.12 on the seal diameter, which is that of the neck, and
//! d_D the hub diameter, as the axial thrust takes it. When the chambers are designed,
//! the performance prediction takes them wherever its own are automatic.

use serde::{Deserialize, Serialize};
use tp_math::DVec2;

use super::ImpellerBasis;
use super::wall::{SegmentInput, SegmentKind, Wall, offset_points};
use crate::losses::{AXIAL_GAP_RATIO, seal_clearance};
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
    /// Radial clearance s of the annular seal on the neck, m; automatic: Gülich eq. 3.12.
    pub seal_clearance: Param<f64>,
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
    /// Diameter of the annular seal: the outer diameter of the neck.
    pub seal_diameter: f64,
    pub seal_clearance: f64,
    /// Axial length of the seal: that of the neck.
    pub seal_length: f64,
    pub shaft_seal_diameter: f64,
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
/// to the tip of the neck, the most upstream point.
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

/// The back of the hub disk the rear chamber lies on, from the rim down to radius
/// `r_seal` (or the innermost point of the back).
fn rear_face(disk: &Wall, r_seal: f64) -> Vec<DVec2> {
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
            return face;
        }
        face.push(b);
    }
    face
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

/// Builds the chambers beside the material wires `cover` (shroud) and `disk` (hub).
/// `inside` is a point of the channel, to tell the impeller side from the casing side.
pub fn build(
    cover: &Wall,
    disk: &Wall,
    inside: DVec2,
    basis: &ImpellerBasis,
    settings: &ChamberSettings,
) -> Result<Chambers, DesignError> {
    let mut warnings = Vec::new();
    let axial_gap = settings
        .axial_gap
        .resolve(|| AXIAL_GAP_RATIO * basis.outlet_diameter / 2.0)
        .max(1e-5);

    // Front: the face of the cover, its neck and the seal round it.
    let face = front_face(cover);
    let tip = *face.last().expect("a face has points");
    let neck_radius = tip.y;
    // The neck: the run of points at the tip radius that ends the face.
    let on_neck = |p: &DVec2| (p.y - neck_radius).abs() <= 1e-9 * neck_radius;
    let neck_start = face.len() - face.iter().rev().take_while(|p| on_neck(p)).count();
    let neck_start = neck_start.min(face.len() - 1);
    let seal_length = (face[neck_start].x - tip.x).max(0.0);
    if seal_length < 0.01 * 2.0 * neck_radius {
        warnings.push(
            "the shroud has no neck for the annular seal: give the material a neck length".into(),
        );
    }
    let seal_diameter = 2.0 * neck_radius;
    let clearance = settings
        .seal_clearance
        .resolve(|| seal_clearance(seal_diameter))
        .max(1e-6);
    let outer_face = &face[..=neck_start];
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
    let front_end = DVec2::new(tip.x, neck_radius + clearance);
    let automatic_front = || {
        let mut wire = Vec::new();
        let last = *casing_face.last().expect("points");
        if casing_face.len() >= CASING_POINTS
            && let Some(c) = curve(&casing_face)
        {
            wire.push(c);
        } else {
            wire.push(line(last));
        }
        // Down to the seal ring, then along it to the exit.
        let ring = DVec2::new(last.x, front_end.y);
        if last.x > front_end.x + 1e-9 && ring.distance(last) > 1e-9 {
            wire.push(line(ring));
        }
        wire.push(line(front_end));
        wire
    };
    let front_casing = match &settings.front {
        Some(segments) => Wall::build(front_start, front_end, segments)?,
        None => Wall::build(front_start, front_end, &automatic_front())?,
    };
    let mut front = front_casing.sample(FACE_SAMPLES / 2);
    front.extend(face.iter().rev());
    front.insert(front.len() - face.len(), DVec2::new(tip.x, neck_radius));

    // Rear: the back of the disk down to the shaft seal, and a flat wall behind it.
    let shaft_seal_diameter = settings.shaft_seal_diameter.resolve(|| basis.hub_diameter);
    let back = rear_face(disk, shaft_seal_diameter / 2.0);
    let rear_z = back.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max) + axial_gap;
    let seal_end = *back.last().expect("a face has points");
    let rear_start = DVec2::new(rear_z, back[0].y);
    let rear_end = DVec2::new(rear_z, seal_end.y);
    let rear_casing = match &settings.rear {
        Some(segments) => Wall::build(rear_start, rear_end, segments)?,
        None => Wall::build(rear_start, rear_end, &[line(rear_end)])?,
    };
    let mut rear = rear_casing.sample(FACE_SAMPLES / 2);
    rear.extend(back.iter().rev());

    Ok(Chambers {
        front_casing,
        rear_casing,
        front,
        rear,
        values: ChamberValues {
            axial_gap,
            seal_diameter,
            seal_clearance: clearance,
            seal_length,
            shaft_seal_diameter: 2.0 * seal_end.y,
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

    fn contour(chambers: ChamberSettings) -> MeridionalContour {
        let input = MeridionalInput {
            chambers,
            ..MeridionalInput::default()
        };
        MeridionalContour::build(&basis(), &input).unwrap()
    }

    #[test]
    fn the_front_chamber_keeps_the_gap_and_ends_in_the_seal() {
        let b = basis();
        let c = contour(ChamberSettings::default());
        let ch = &c.chambers;
        let v = ch.values;
        assert!((v.axial_gap - 0.05 * b.outlet_diameter / 2.0).abs() < 1e-12);
        // The seal sits on the neck, of the material's neck length, with the clearance
        // of eq. 3.12 on its diameter.
        assert!((v.seal_length - c.material.neck_length).abs() < 1e-3 * v.seal_length);
        assert!((v.seal_clearance - seal_clearance(v.seal_diameter)).abs() < 1e-15);
        let end = ch.front_casing.end();
        assert!((end.y - (v.seal_diameter / 2.0 + v.seal_clearance)).abs() < 1e-12);
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
        input.material.neck_length = Param::Manual(0.0);
        let quiet = MeridionalContour::build(&b, &input).unwrap();
        assert!(quiet.chambers.warnings.is_empty());
        input.chambers.enabled = true;
        let loud = MeridionalContour::build(&b, &input).unwrap();
        assert!(loud.chambers.warnings.iter().any(|w| w.contains("neck")));
    }
}
