//! Hub and shroud from the center line of the channel and a distribution of its cross
//! section, instead of drawing both walls (the second way of designing the meridian,
//! suited to mixed-flow impellers whose channel turns gently).
//!
//! The center line runs from the middle of the inlet line to the middle of the outlet
//! line. At a fraction x of its length the cross section is
//! A = A₁ + A_rel(x)·(A₂ − A₁), with A₁ = 2π·r·b at the inlet and A₂ = 2π·r·b₂ at the
//! outlet, and A_rel from a table of (x, A_rel) pairs, linear when there is none (our
//! reading of the method). The channel is as wide normal to the center line as that
//! section asks, b = A/(2π·r), half on each side: the hub at −b/2, the shroud at +b/2.
//! Both walls end exactly at their ends from the main dimensions.

use serde::{Deserialize, Serialize};
use tp_math::{BSpline, DVec2, Polyline};

use super::wall::{SegmentInput, SegmentKind, Wall};
use crate::DesignError;

/// Points of the center line at which hub and shroud are placed.
const STATIONS: usize = 41;

/// How the meridian is designed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DesignMode {
    /// Hub and shroud drawn directly.
    #[default]
    Walls,
    /// The center line and a cross-section distribution; the walls follow.
    CenterLine,
}

/// A_rel at fraction `x` of the center line: linear between the pairs of `table`, flat
/// beyond them, and x itself without a table.
pub fn relative_area(table: &[[f64; 2]], x: f64) -> f64 {
    match table {
        [] => x,
        [only] => only[1],
        _ => {
            if x <= table[0][0] {
                return table[0][1];
            }
            for w in table.windows(2) {
                let ([x0, a0], [x1, a1]) = (w[0], w[1]);
                if x <= x1 {
                    let f = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
                    return a0 + (a1 - a0) * f;
                }
            }
            table[table.len() - 1][1]
        }
    }
}

/// A wall through `points`, its ends being the first and the last: one cubic B-spline.
fn wall_through(points: &[DVec2]) -> Result<Wall, DesignError> {
    let spline = BSpline::interpolate(points, 3)
        .map_err(|e| DesignError::InvalidInput(format!("center-line walls: {e}")))?;
    let p = spline.control_points();
    let k = spline.knots();
    let segment = SegmentInput {
        kind: SegmentKind::BSpline,
        controls: p[1..p.len() - 1].to_vec(),
        end: points[points.len() - 1],
        degree: Some(3),
        knots: k[4..k.len() - 4].to_vec(),
        ..SegmentInput::default()
    };
    Wall::build(points[0], points[points.len() - 1], &[segment])
}

/// Hub and shroud on both sides of `center`: from `hub` and `shroud` (start, end) of the
/// main dimensions, `outlet_width` b₂ normal to the outflow, and the distribution
/// `table`.
pub fn walls(
    center: &Wall,
    hub: (DVec2, DVec2),
    shroud: (DVec2, DVec2),
    outlet_width: f64,
    table: &[[f64; 2]],
) -> Result<(Wall, Wall), DesignError> {
    let line = Polyline::new(center.sample(400))
        .map_err(|e| DesignError::InvalidInput(format!("center line: {e}")))?;
    let (start, end) = (center.start(), center.end());
    let area = |r: f64, width: f64| 2.0 * std::f64::consts::PI * r * width;
    let inlet = area(start.y, hub.0.distance(shroud.0));
    let outlet = area(end.y, outlet_width);
    let h = 1e-4;
    let mut hub_points = Vec::with_capacity(STATIONS);
    let mut shroud_points = Vec::with_capacity(STATIONS);
    for k in 0..STATIONS {
        let x = k as f64 / (STATIONS - 1) as f64;
        let c = line.point_at_fraction(x);
        let along = (line.point_at_fraction((x + h).min(1.0))
            - line.point_at_fraction((x - h).max(0.0)))
        .normalize_or_zero();
        // Toward the shroud: +r at an axial inlet, −z at a radial outlet.
        let normal = along.perp();
        let a = inlet + relative_area(table, x) * (outlet - inlet);
        if !(c.y > 0.0 && a > 0.0) {
            return Err(DesignError::InvalidInput(format!(
                "the center line leaves no cross section at {:.0} % of its length",
                x * 100.0
            )));
        }
        let half = a / (2.0 * std::f64::consts::PI * c.y) / 2.0;
        hub_points.push(c - normal * half);
        shroud_points.push(c + normal * half);
    }
    // The ends of the main dimensions, exactly: what the offset misses there (the ends
    // of an inclined outlet lie apart along the flow too) is spread along the wall.
    for (points, (first, last)) in [(&mut hub_points, hub), (&mut shroud_points, shroud)] {
        let (d0, d1) = (first - points[0], last - points[STATIONS - 1]);
        for (k, p) in points.iter_mut().enumerate() {
            let x = k as f64 / (STATIONS - 1) as f64;
            *p += d0 * (1.0 - x) + d1 * x;
        }
        points[0] = first;
        points[STATIONS - 1] = last;
    }
    for (name, points) in [("hub", &hub_points), ("shroud", &shroud_points)] {
        let folds = points
            .windows(3)
            .any(|w| (w[1] - w[0]).dot(w[2] - w[1]) <= 0.0);
        if folds || points.iter().any(|p| p.y < 0.0) {
            return Err(DesignError::InvalidInput(format!(
                "the center line bends tighter than half the channel is wide: the {name} \
                 folds back"
            )));
        }
    }
    Ok((wall_through(&hub_points)?, wall_through(&shroud_points)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::meridian::{MeridionalContour, MeridionalInput, tests::basis};

    #[test]
    fn the_relative_area_interpolates_its_table() {
        assert_eq!(relative_area(&[], 0.3), 0.3);
        let table = [[0.0, 0.0], [0.5, 0.8], [1.0, 1.0]];
        assert!((relative_area(&table, 0.25) - 0.4).abs() < 1e-12);
        assert!((relative_area(&table, 0.75) - 0.9).abs() < 1e-12);
        assert_eq!(relative_area(&table, 2.0), 1.0);
    }

    #[test]
    fn walls_from_the_center_line_keep_the_ends_and_the_area() {
        let b = basis();
        let input = MeridionalInput {
            design_mode: DesignMode::CenterLine,
            ..MeridionalInput::default()
        };
        let c = MeridionalContour::build(&b, &input, 1.0).unwrap();
        let walls = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        // The same ends as the walls of the main dimensions.
        assert_eq!(c.hub.start(), walls.hub.start());
        assert_eq!(c.hub.end(), walls.hub.end());
        assert_eq!(c.shroud.start(), walls.shroud.start());
        assert_eq!(c.shroud.end(), walls.shroud.end());
        // Halfway along the center line the section is halfway between inlet and
        // outlet: 2π·r·b with b across the channel.
        let line = Polyline::new(c.center_line.sample(400)).unwrap();
        let middle = line.point_at_fraction(0.5);
        let (r1, rn) = (b.inlet_diameter / 2.0, b.hub_diameter / 2.0);
        let inlet = 2.0 * std::f64::consts::PI * (r1 + rn) / 2.0 * (r1 - rn);
        let outlet = 2.0 * std::f64::consts::PI * b.outlet_diameter / 2.0 * b.outlet_width;
        let hub = c.hub.sample(2000);
        let shroud = c.shroud.sample(2000);
        let nearest = |points: &[DVec2]| {
            points
                .iter()
                .map(|p| p.distance(middle))
                .fold(f64::INFINITY, f64::min)
        };
        let width = nearest(&hub) + nearest(&shroud);
        let section = 2.0 * std::f64::consts::PI * middle.y * width;
        let expected = (inlet + outlet) / 2.0;
        assert!(
            (section / expected - 1.0).abs() < 0.03,
            "{section} vs {expected}"
        );
    }

    #[test]
    fn a_semi_axial_impeller_follows_its_center_line_too() {
        let mut b = basis();
        b.outlet_inclination = 30f64.to_radians();
        b.outlet_hub_diameter = 0.8 * b.outlet_diameter;
        let input = MeridionalInput {
            design_mode: DesignMode::CenterLine,
            area_progression: vec![[0.0, 0.0], [0.4, 0.55], [1.0, 1.0]],
            ..MeridionalInput::default()
        };
        let c = MeridionalContour::build(&b, &input, 1.0).unwrap();
        let walls = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        assert_eq!(c.hub.end(), walls.hub.end());
        assert_eq!(c.shroud.end(), walls.shroud.end());
    }

    #[test]
    fn a_tight_center_line_folds_and_is_an_error() {
        let b = basis();
        // A section a hundred times the outlet: far wider than the bend allows.
        let input = MeridionalInput {
            design_mode: DesignMode::CenterLine,
            area_progression: vec![[0.0, 0.0], [0.5, 100.0], [1.0, 1.0]],
            ..MeridionalInput::default()
        };
        assert!(MeridionalContour::build(&b, &input, 1.0).is_err());
    }
}
