//! The axial inlet duct ahead of the impeller, the stationary part of the flow domain
//! upstream of the eye, for CFD:
//!
//! - an annulus between the hub radius at the inlet (a cylinder carrying the hub on, our
//!   simplification of the hub nose) and the duct radius, from a length L upstream of
//!   the eye down to the impeller inlet;
//! - when the side chambers are designed, the duct stops an axial gap Δz short of the
//!   tip of the shroud neck: the suction cover faces the neck across the gap, and the
//!   leakage leaving the annular seal at the tip turns into the eye through it, so the
//!   joined fluid has one domain from the duct inlet to the volute.
//!
//! The values are our choices: L = d₁, one eye diameter of run-up; the duct as wide as
//! the shroud at the inlet, so the wall carries straight on; Δz the axial gap of the
//! side chambers, s_ax.

use serde::{Deserialize, Serialize};
use tp_math::DVec2;

use super::ImpellerBasis;
use super::chambers::Chambers;
use super::wall::Wall;
use crate::Param;

/// Points of the shroud cover sampled to find its neck.
const COVER_SAMPLES: usize = 600;

/// Inputs of the inlet duct.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DuctSettings {
    /// Model the inlet duct: shown on the chambers page and exported with the CAD model.
    pub enabled: bool,
    /// Length L of the duct upstream of the eye, m; automatic: the eye diameter d₁.
    pub length: Param<f64>,
    /// Diameter of the duct, m; automatic: the shroud diameter at the inlet.
    pub diameter: Param<f64>,
    /// Axial gap Δz between the suction cover at the end of the duct and the tip of the
    /// shroud neck, m; automatic: the axial gap of the side chambers.
    pub axial_gap: Param<f64>,
}

/// The values the duct was built with, m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DuctValues {
    pub length: f64,
    pub diameter: f64,
    /// Diameter of the hub cylinder through the duct: the hub at the inlet.
    pub hub_diameter: f64,
    pub axial_gap: f64,
}

/// The inlet duct in the meridional plane.
#[derive(Debug, Clone, PartialEq)]
pub struct InletDuct {
    /// Closed outline of its fluid, (z, r) in m: up the hub from the duct inlet, across
    /// the impeller inlet, back along the inside of the neck, then (with the chambers)
    /// across the seal exit, the gap and the suction cover, and up the duct wall. Empty
    /// when it could not be built.
    pub outline: Vec<DVec2>,
    pub values: DuctValues,
    /// Whether it reaches the seal exit across the axial gap: the side chambers are
    /// designed.
    pub joins_seal: bool,
    /// Whether it is modelled: exported with the CAD model.
    pub enabled: bool,
    /// What could not be built as asked; only when the duct is modelled.
    pub warnings: Vec<String>,
}

impl InletDuct {
    /// The upstream end of the duct, where the flow comes in, m.
    pub fn inlet_z(&self) -> Option<f64> {
        self.outline.first().map(|p| p.x)
    }
}

/// The inside of the shroud cover from the tip of its neck (its most upstream point,
/// the outer radius first) round to the shroud start, where the channel begins.
fn neck_inside(cover: &Wall) -> Vec<DVec2> {
    let points = cover.sample(COVER_SAMPLES);
    let tip = points
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.x.total_cmp(&b.1.x).then(b.1.y.total_cmp(&a.1.y)))
        .map_or(0, |(i, _)| i);
    points[tip..].to_vec()
}

/// Builds the duct ahead of the channel walls `hub` and `shroud`, the shroud `cover`
/// with its neck, and the side `chambers`.
pub fn build(
    hub: &Wall,
    shroud: &Wall,
    cover: &Wall,
    chambers: &Chambers,
    basis: &ImpellerBasis,
    settings: &DuctSettings,
) -> InletDuct {
    let (hub_in, shroud_in) = (hub.start(), shroud.start());
    let values = DuctValues {
        length: settings.length.resolve(|| basis.inlet_diameter),
        diameter: settings.diameter.resolve(|| 2.0 * shroud_in.y),
        hub_diameter: 2.0 * hub_in.y,
        axial_gap: settings.axial_gap.resolve(|| chambers.values.axial_gap),
    };
    let joins_seal = chambers.enabled;
    let built = outline(hub_in, shroud_in, cover, chambers, &values, joins_seal);
    let (outline, warnings) = match built {
        Ok(points) => (points, Vec::new()),
        Err(reason) => (
            Vec::new(),
            vec![format!("the inlet duct was not built: {reason}")],
        ),
    };
    InletDuct {
        outline,
        values,
        joins_seal,
        enabled: settings.enabled,
        warnings: if settings.enabled {
            warnings
        } else {
            Vec::new()
        },
    }
}

fn outline(
    hub_in: DVec2,
    shroud_in: DVec2,
    cover: &Wall,
    chambers: &Chambers,
    v: &DuctValues,
    joins_seal: bool,
) -> Result<Vec<DVec2>, String> {
    let r_duct = v.diameter / 2.0;
    let r_hub = hub_in.y;
    if !(v.length > 0.0 && v.length.is_finite()) {
        return Err("its length must be positive".into());
    }
    if r_duct <= r_hub {
        return Err("its diameter must be larger than the hub diameter at the inlet".into());
    }
    let inside = neck_inside(cover);
    let tip = inside[0];
    if tip.x > shroud_in.x + 1e-9 {
        return Err("the shroud cover does not reach upstream of the inlet".into());
    }
    let upstream = tip.x.min(hub_in.x).min(shroud_in.x) - v.length;
    let mut points = vec![DVec2::new(upstream, r_hub), hub_in, shroud_in];
    // Back along the inside of the neck to the inner corner of its tip.
    let inner = inside
        .iter()
        .rposition(|p| (p.x - tip.x).abs() <= 1e-9)
        .unwrap_or(0);
    points.extend(inside[inner..].iter().rev().skip(1));
    let corner = inside[inner];
    if joins_seal {
        if !(v.axial_gap > 0.0 && v.axial_gap.is_finite()) {
            return Err("the axial gap must be positive".into());
        }
        // The seal exit, where the front chamber ends, and the gap ahead of it.
        let exit = chambers.front_casing.end();
        if (exit.x - tip.x).abs() > 1e-6 * exit.y.max(1e-3) {
            return Err("the front chamber does not end at the tip of the neck".into());
        }
        let cover_z = tip.x - v.axial_gap;
        points.extend([
            corner,
            tip,
            exit,
            DVec2::new(cover_z, exit.y),
            DVec2::new(cover_z, r_duct),
        ]);
    } else {
        // Straight on to the tip; the duct no wider than the neck.
        if r_duct < corner.y - 1e-9 || r_duct > tip.y + 1e-9 {
            return Err(
                "without the side chambers its diameter must lie between the inside and \
                 the outside of the neck"
                    .into(),
            );
        }
        points.extend([corner, DVec2::new(tip.x, r_duct)]);
    }
    points.push(DVec2::new(upstream, r_duct));
    points.dedup_by(|a, b| a.distance(*b) < 1e-12);
    Ok(without_straight_runs(points))
}

/// `points` less those on a straight line between their neighbours, so that the
/// samples of a straight neck do not leave slivers.
fn without_straight_runs(mut points: Vec<DVec2>) -> Vec<DVec2> {
    let mut i = 0;
    while points.len() > 3 && i < points.len() {
        let n = points.len();
        let (a, b, c) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let span = (c - a).length_squared();
        if (b - a).perp_dot(c - a).abs() <= 1e-9 * span && (b - a).dot(c - b) >= 0.0 {
            points.remove(i);
        } else {
            i += 1;
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use crate::Param;
    use crate::impeller::meridian::{MeridionalContour, MeridionalInput, tests::basis};

    use super::*;

    fn contour(chambers: bool, duct: DuctSettings) -> MeridionalContour {
        let mut input = MeridionalInput {
            inlet_duct: duct,
            ..MeridionalInput::default()
        };
        input.chambers.enabled = chambers;
        MeridionalContour::build(&basis(), &input).unwrap()
    }

    fn on(settings: DuctSettings) -> DuctSettings {
        DuctSettings {
            enabled: true,
            ..settings
        }
    }

    /// Twice the area of a closed outline, positive counter-clockwise in (z, r).
    fn area(points: &[DVec2]) -> f64 {
        (0..points.len())
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>()
    }

    #[test]
    fn the_duct_reaches_the_seal_exit_across_the_gap() {
        let b = basis();
        let c = contour(true, on(DuctSettings::default()));
        let d = &c.inlet_duct;
        let v = d.values;
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
        assert!(d.joins_seal);
        assert_eq!(v.length, b.inlet_diameter);
        assert_eq!(v.diameter, 2.0 * c.shroud.start().y);
        assert_eq!(v.axial_gap, c.chambers.values.axial_gap);
        // Across the impeller inlet and the seal exit: both are on the outline.
        let has = |p: DVec2| d.outline.iter().any(|q| q.distance(p) < 1e-12);
        assert!(has(c.hub.start()) && has(c.shroud.start()));
        assert!(has(c.chambers.front_casing.end()));
        // The suction cover lies Δz ahead of the neck tip.
        let tip = c.chambers.front_casing.end().x;
        assert!(has(DVec2::new(tip - v.axial_gap, v.diameter / 2.0)));
        // L upstream of the eye, from the hub to the duct wall.
        let upstream = d.inlet_z().unwrap();
        assert!((tip.min(c.shroud.start().x) - upstream - v.length).abs() < 1e-12);
        assert!(area(&d.outline).abs() > 0.0);
        // The straight runs keep only their ends: a handful of corners.
        assert!(d.outline.len() < 20, "{}", d.outline.len());
    }

    #[test]
    fn without_the_chambers_the_duct_runs_straight_to_the_neck() {
        let c = contour(false, on(DuctSettings::default()));
        let d = &c.inlet_duct;
        assert!(!d.joins_seal);
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
        let r = d.values.diameter / 2.0;
        assert!(d.outline.iter().all(|p| p.y <= r + 1e-12));
    }

    #[test]
    fn bad_values_leave_a_warning_not_an_error() {
        let c = contour(
            true,
            on(DuctSettings {
                axial_gap: Param::Manual(0.0),
                ..DuctSettings::default()
            }),
        );
        assert!(c.inlet_duct.outline.is_empty());
        assert!(c.inlet_duct.warnings[0].contains("gap"));
        // Not modelled: quiet.
        let quiet = contour(
            true,
            DuctSettings {
                axial_gap: Param::Manual(0.0),
                ..DuctSettings::default()
            },
        );
        assert!(quiet.inlet_duct.warnings.is_empty());
    }
}
