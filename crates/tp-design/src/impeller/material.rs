//! The impeller material in the meridional plane: the hub disk behind the hub and the
//! shroud cover outside the shroud. Each is a closed contour: its wall of the flow path
//! and a wire from the outlet end of that wall round the material back to its inlet end.
//!
//! An automatic wire follows a wall thickness, the shaft bore and a boss around it (our
//! layout for a cast impeller, the one the CAD model always had):
//! - hub: across the wall at the outlet, the hub contour offset by the thickness down
//!   to the boss, the rear face of the boss, the bore, and up to the hub at the inlet;
//! - shroud: across the wall at the outlet, the shroud contour offset outwards by the
//!   thickness, a cylindrical neck reaching upstream of the inlet for the annular seal,
//!   and down to the shroud at the inlet.
//!
//! A drawn wire is edited segment by segment like the walls (see [`super::wall`]).

use serde::{Deserialize, Serialize};
use tp_math::{BSpline, DVec2};

use super::ImpellerBasis;
use super::seals::{ResolvedSeals, Seal};
pub use super::wall::offset_points;
use super::wall::{SegmentInput, SegmentKind, Wall};
use crate::{DesignError, Param};

/// Points of a wall used to offset it.
const OFFSET_SAMPLES: usize = 41;

/// Points of an offset curve kept as control points of its B-spline: enough to follow
/// it within a small fraction of the thickness, few enough to drag by hand.
const OFFSET_POINTS: usize = 9;

/// Inputs of the automatic material.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialSettings {
    /// Wall thickness of the hub disk and the shroud, m; automatic:
    /// [`default_wall_thickness`].
    pub wall_thickness: Param<f64>,
    /// Diameter of the shaft bore, m; automatic: the hub diameter at the eye d_n.
    pub bore_diameter: Param<f64>,
    /// Outer diameter of the boss around the bore, m; automatic: the bore plus a wall
    /// of a quarter of the bore, and no less than the wall thickness.
    pub boss_diameter: Param<f64>,
}

/// Thickness of the hub disk and the shroud unless given: 0.02·d₂, at least 3 mm (our
/// choice for a cast impeller).
pub fn default_wall_thickness(outlet_diameter: f64) -> f64 {
    (0.02 * outlet_diameter).max(0.003)
}

/// The material settings as used, m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub wall_thickness: f64,
    pub bore_diameter: f64,
    pub boss_diameter: f64,
}

impl MaterialSettings {
    /// The values used for `basis`, the automatic ones where not given.
    pub fn resolve(&self, basis: &ImpellerBasis) -> Material {
        let wall_thickness = self
            .wall_thickness
            .resolve(|| default_wall_thickness(basis.outlet_diameter));
        let bore_diameter = self.bore_diameter.resolve(|| basis.hub_diameter);
        let boss_diameter = self.boss_diameter.resolve(|| {
            let rb = bore_diameter / 2.0;
            2.0 * (rb + (0.5 * rb).max(wall_thickness))
        });
        Material {
            wall_thickness,
            bore_diameter,
            boss_diameter,
        }
    }
}

/// A straight segment of a wire to `end`.
fn line(end: DVec2) -> SegmentInput {
    SegmentInput {
        kind: SegmentKind::Line,
        end,
        ..SegmentInput::default()
    }
}

/// A cubic B-spline segment of a wire through `points`, kept to a few of them.
fn curve(points: &[DVec2]) -> Result<SegmentInput, DesignError> {
    let n = points.len();
    let kept: Vec<DVec2> = (0..OFFSET_POINTS)
        .map(|i| points[(i * (n - 1)) / (OFFSET_POINTS - 1)])
        .collect();
    let spline = BSpline::interpolate(&kept, 3)
        .map_err(|e| DesignError::InvalidInput(format!("material contour: {e}")))?;
    let p = spline.control_points();
    let k = spline.knots();
    Ok(SegmentInput {
        kind: SegmentKind::BSpline,
        controls: p[1..p.len() - 1].to_vec(),
        end: *points.last().expect("points"),
        degree: Some(3),
        knots: k[4..k.len() - 4].to_vec(),
        ..SegmentInput::default()
    })
}

/// The automatic wire of the hub disk, from the hub end at the outlet to the hub start
/// at the inlet.
pub fn automatic_hub(
    hub: &Wall,
    shroud: &Wall,
    basis: &ImpellerBasis,
    m: &Material,
    _rear: Option<&Seal>,
) -> Result<Vec<SegmentInput>, DesignError> {
    let t = m.wall_thickness;
    let (rb, rboss) = (m.bore_diameter / 2.0, m.boss_diameter / 2.0);
    let rn = hub.start().y;
    if !(t > 0.0 && t.is_finite()) {
        return Err(DesignError::InvalidInput(
            "the wall thickness must be positive".into(),
        ));
    }
    if !(rb > 0.0 && rb <= rn) {
        return Err(DesignError::InvalidInput(
            "the bore diameter must be positive and not larger than the hub diameter".into(),
        ));
    }
    // The disk ends at the hub end of the outlet edge (d₂i of a semi-axial impeller).
    let r2 = (basis.outlet_diameter / 2.0).min(basis.outlet_hub_diameter / 2.0);
    if !(rboss > rb && rboss < r2 - t) {
        return Err(DesignError::InvalidInput(format!(
            "the boss diameter must lie between the bore ({:.1} mm) and the hub outlet \
             diameter less the wall",
            2.0 * rb * 1e3
        )));
    }
    let points = hub.sample(OFFSET_SAMPLES);
    let mut behind = offset_points(
        &points,
        t,
        shroud.sample(OFFSET_SAMPLES)[OFFSET_SAMPLES / 2],
    )?;
    // The rim on the outlet radius: across the wall at the outlet, whatever the normal.
    behind[OFFSET_SAMPLES - 1].y = hub.end().y;
    // The back of the wall, from the outlet inward, until it meets the boss.
    let mut back: Vec<DVec2> = Vec::new();
    for p in behind.iter().rev().copied() {
        if p.y <= rboss {
            let q = back.last().copied().unwrap_or(p);
            let f = (q.y - rboss) / (q.y - p.y);
            back.push(q + (p - q) * f);
            break;
        }
        back.push(p);
    }
    if back.last().is_some_and(|p| p.y > rboss) {
        let end = *back.last().expect("not empty");
        back.push(DVec2::new(end.x, rboss));
    }
    if back.len() < 4 {
        return Err(DesignError::InvalidInput(
            "the hub disk is too short to reach the boss".into(),
        ));
    }
    let meet = *back.last().expect("not empty");
    let rear = hub.end().x + t;
    let start = hub.start();
    let mut wire = vec![line(back[0]), curve(&back)?];
    let corners = [
        DVec2::new(rear, rboss),
        DVec2::new(rear, rb),
        DVec2::new(start.x, rb),
        start,
    ];
    let mut from = meet;
    for corner in corners {
        if corner.distance(from) > 1e-9 {
            wire.push(line(corner));
            from = corner;
        }
    }
    Ok(wire)
}

/// The automatic wire of the shroud, from the shroud end at the outlet to the shroud
/// start at the inlet.
pub fn automatic_shroud(
    hub: &Wall,
    shroud: &Wall,
    m: &Material,
    front: Option<&Seal>,
) -> Result<Vec<SegmentInput>, DesignError> {
    let points = shroud.sample(OFFSET_SAMPLES);
    let outer = offset_points(
        &points,
        m.wall_thickness,
        hub.sample(OFFSET_SAMPLES)[OFFSET_SAMPLES / 2],
    )?;
    let mut back: Vec<DVec2> = outer.iter().rev().copied().collect();
    // The rim on the outlet radius, as on the hub disk.
    back[0].y = shroud.end().y;
    let mut wire = vec![line(back[0]), curve(&back)?];
    let eye = *back.last().expect("points");
    if let Some(seal) = front {
        // The neck: upstream along its outer radius, then in to the eye radius.
        let tip = eye - DVec2::new(seal.length, 0.0);
        wire.push(line(tip));
        wire.push(line(DVec2::new(tip.x, shroud.start().y)));
    }
    wire.push(line(shroud.start()));
    Ok(wire)
}

/// A plain wire when the automatic one cannot be built: `thickness` along the axis
/// from both ends of the wall, on the side away from `away_from`, so no radius changes.
pub fn fallback(wall: &Wall, thickness: f64, away_from: DVec2) -> Vec<SegmentInput> {
    let (start, end) = (wall.start(), wall.end());
    let middle = wall.sample(OFFSET_SAMPLES)[OFFSET_SAMPLES / 2];
    let shift = DVec2::new(thickness.copysign(middle.x - away_from.x), 0.0);
    vec![line(end + shift), line(start + shift), line(start)]
}

/// The two material wires of a contour and what could not be built as asked.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialWires {
    pub hub: Wall,
    pub shroud: Wall,
    pub warnings: Vec<String>,
}

/// The material wires of `hub` and `shroud`: the drawn ones, else the automatic ones,
/// else a plain one with a warning.
pub fn wires(
    hub: &Wall,
    shroud: &Wall,
    basis: &ImpellerBasis,
    m: &Material,
    seals: &ResolvedSeals,
    drawn: (Option<&[SegmentInput]>, Option<&[SegmentInput]>),
) -> Result<MaterialWires, DesignError> {
    let mut warnings = Vec::new();
    let mut pick = |name: &str,
                    wall: &Wall,
                    away_from: DVec2,
                    drawn: Option<&[SegmentInput]>,
                    automatic: Result<Vec<SegmentInput>, DesignError>|
     -> Result<Wall, DesignError> {
        // The wire runs from the outlet end of the wall back to its inlet end.
        let (from, to) = (wall.end(), wall.start());
        if let Some(segments) = drawn {
            return Wall::build(from, to, segments);
        }
        let segments = automatic.unwrap_or_else(|e| {
            warnings.push(format!(
                "the automatic {name} material could not be built ({e}); a plain one is used"
            ));
            fallback(wall, m.wall_thickness, away_from)
        });
        Wall::build(from, to, &segments)
    };
    let middle = |w: &Wall| w.sample(OFFSET_SAMPLES)[OFFSET_SAMPLES / 2];
    let hub_wire = pick(
        "hub",
        hub,
        middle(shroud),
        drawn.0,
        automatic_hub(hub, shroud, basis, m, seals.rear.as_ref()),
    )?;
    let shroud_wire = pick(
        "shroud",
        shroud,
        middle(hub),
        drawn.1,
        automatic_shroud(hub, shroud, m, seals.front.as_ref()),
    )?;
    Ok(MaterialWires {
        hub: hub_wire,
        shroud: shroud_wire,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::meridian::{MeridionalContour, MeridionalInput, tests::basis};

    fn contour() -> (ImpellerBasis, MeridionalContour) {
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        (b, c)
    }

    #[test]
    fn the_automatic_hub_disk_follows_the_hub_and_closes_at_the_bore() {
        let (b, c) = contour();
        let m = MaterialSettings::default().resolve(&b);
        assert_eq!(m.wall_thickness, default_wall_thickness(b.outlet_diameter));
        assert_eq!(m.bore_diameter, b.hub_diameter);
        let hub = &c.hub_material;
        assert_eq!(hub.start(), c.hub.end());
        assert_eq!(hub.end(), c.hub.start());
        let t = m.wall_thickness;
        // The offset part stays a wall thickness behind the hub.
        let wall = c.hub.sample(400);
        let curve = &hub.segments()[1];
        assert_eq!(curve.kind(), SegmentKind::BSpline);
        for k in 1..10 {
            let p = curve.eval(k as f64 / 10.0);
            let d = wall
                .iter()
                .map(|q| q.distance(p))
                .fold(f64::INFINITY, f64::min);
            assert!((d - t).abs() < 0.03 * t, "{d} vs {t}");
        }
        // The rear face of the boss sits a wall behind the outlet, down to the bore.
        let rear = c.hub.end().x + t;
        let rb = m.bore_diameter / 2.0;
        assert!(
            hub.points()
                .iter()
                .any(|(_, p)| p.distance(DVec2::new(rear, rb)) < 1e-12)
        );
    }

    #[test]
    fn the_automatic_shroud_is_the_shroud_moved_outwards() {
        let (b, c) = contour();
        let t = MaterialSettings::default().resolve(&b).wall_thickness;
        let s = &c.shroud_material;
        assert_eq!(s.start(), c.shroud.end());
        assert_eq!(s.end(), c.shroud.start());
        let first = s.segments()[0].end();
        // Across the wall at the outlet, a thickness away from the shroud end.
        assert!((first.distance(c.shroud.end()) - t).abs() < 1e-3 * t);
        // Outwards: towards the inlet side, away from the hub.
        assert!(first.x < c.shroud.end().x);
        // The neck runs upstream of the inlet as long as the seal.
        let seal = c.seals.front.expect("the default has a front seal");
        let neck = &s.segments()[2];
        assert_eq!(neck.kind(), SegmentKind::Line);
        assert!((neck.start().x - neck.end().x - seal.length).abs() < 1e-12);
        assert!(
            (neck.start().y - neck.end().y).abs() < 1e-12,
            "along the axis"
        );
        assert!((seal.length - 0.12 * 2.0 * seal.radius).abs() < 1e-12);
    }

    #[test]
    fn a_wall_too_thick_for_the_contour_falls_back_with_a_warning() {
        let b = basis();
        let input = MeridionalInput {
            material: MaterialSettings {
                wall_thickness: Param::Manual(0.2),
                ..MaterialSettings::default()
            },
            ..MeridionalInput::default()
        };
        let c = MeridionalContour::build(&b, &input, 1.0).unwrap();
        assert!(!c.material_warnings.is_empty());
        assert_eq!(c.hub_material.segments().len(), 3);
    }

    #[test]
    fn a_drawn_material_wire_follows_the_wall_ends() {
        let (b, c) = contour();
        let mut input = MeridionalInput {
            hub_material: Some(c.hub_material.to_input()),
            ..MeridionalInput::default()
        };
        input.axial_length = Param::Manual(c.axial_length * 1.1);
        let longer = MeridionalContour::build(&b, &input, 1.0).unwrap();
        assert_eq!(longer.hub_material.start(), longer.hub.end());
        assert_eq!(longer.hub_material.end(), longer.hub.start());
    }
}
