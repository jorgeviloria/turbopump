//! Where walls of the meridional section cross: hub and shroud, the material wires and
//! the walls they close on, the casing walls and the material, and each wire with
//! itself. A drawn wire can be dragged across another; the geometry is then wrong but
//! still built, so these are warnings with the points to mark.

use tp_math::DVec2;

use super::wall::Wall;

/// Points each wall is sampled at to look for crossings.
const SAMPLES: usize = 240;

/// Two walls crossing at a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// The names of the walls, as written in warnings.
    pub walls: (&'static str, &'static str),
    /// (z, r) in m.
    pub at: DVec2,
}

/// The point where segments `a0`–`a1` and `b0`–`b1` cross, strictly inside both.
fn segment_crossing(a0: DVec2, a1: DVec2, b0: DVec2, b1: DVec2) -> Option<DVec2> {
    let (da, db) = (a1 - a0, b1 - b0);
    let det = da.perp_dot(db);
    if det.abs() < 1e-18 {
        return None;
    }
    let t = (b0 - a0).perp_dot(db) / det;
    let u = (b0 - a0).perp_dot(da) / det;
    let inside = |x: f64| x > 1e-9 && x < 1.0 - 1e-9;
    (inside(t) && inside(u)).then(|| a0 + da * t)
}

/// Where the polylines `a` and `b` cross, leaving out points within `reach` of any of
/// `shared` (ends the walls have in common). With `same`, `a` and `b` are one polyline
/// and neighbouring segments are not compared.
fn polyline_crossings(
    a: &[DVec2],
    b: &[DVec2],
    shared: &[DVec2],
    reach: f64,
    same: bool,
) -> Vec<DVec2> {
    let mut found = Vec::new();
    for i in 0..a.len().saturating_sub(1) {
        let from = if same { i + 2 } else { 0 };
        for j in from..b.len().saturating_sub(1) {
            if let Some(p) = segment_crossing(a[i], a[i + 1], b[j], b[j + 1])
                && shared.iter().all(|s| s.distance(p) > reach)
            {
                // One mark per crossing: neighbours of a found point are the same one.
                if found.last().is_none_or(|q: &DVec2| q.distance(p) > reach) {
                    found.push(p);
                }
            }
        }
    }
    found
}

/// Every crossing of the pairs of `walls` and of each wall with itself. Walls are
/// (name, wall); ends they share are not crossings.
pub fn crossings(walls: &[(&'static str, &Wall)], pairs: &[(usize, usize)]) -> Vec<Crossing> {
    let sampled: Vec<Vec<DVec2>> = walls.iter().map(|(_, w)| w.sample(SAMPLES)).collect();
    let size = sampled.iter().flatten().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let reach = 1e-3 * (size.1 - size.0).length().max(1e-9);
    let mut out = Vec::new();
    for &(i, j) in pairs {
        let (a, b) = (walls[i].1, walls[j].1);
        let shared = [a.start(), a.end(), b.start(), b.end()];
        for at in polyline_crossings(&sampled[i], &sampled[j], &shared, reach, false) {
            out.push(Crossing {
                walls: (walls[i].0, walls[j].0),
                at,
            });
        }
    }
    for (k, (name, _)) in walls.iter().enumerate() {
        for at in polyline_crossings(&sampled[k], &sampled[k], &[], reach, true) {
            out.push(Crossing {
                walls: (name, name),
                at,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_math::Bezier;

    fn line(a: (f64, f64), b: (f64, f64)) -> Wall {
        Wall::single(Bezier::line(DVec2::new(a.0, a.1), DVec2::new(b.0, b.1)))
    }

    #[test]
    fn crossing_walls_are_found_and_shared_ends_are_not() {
        let a = line((0.0, 0.0), (1.0, 1.0));
        let b = line((0.0, 1.0), (1.0, 0.0));
        let c = line((1.0, 1.0), (2.0, 0.0));
        let found = crossings(&[("a", &a), ("b", &b), ("c", &c)], &[(0, 1), (0, 2)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].walls, ("a", "b"));
        assert!(found[0].at.distance(DVec2::new(0.5, 0.5)) < 1e-9);
    }

    #[test]
    fn a_wire_that_loops_crosses_itself() {
        let loop_ = Wall::single(
            Bezier::new(vec![
                DVec2::new(0.0, 0.0),
                DVec2::new(2.0, 1.0),
                DVec2::new(-1.0, 1.0),
                DVec2::new(1.0, 0.0),
            ])
            .unwrap(),
        );
        let found = crossings(&[("loop", &loop_)], &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].walls, ("loop", "loop"));
    }

    #[test]
    fn the_automatic_contours_have_no_crossings_and_a_dragged_wall_has_one() {
        use crate::Param;
        use crate::impeller::meridian::{
            DesignMode, MeridionalContour, MeridionalInput, tests::basis,
        };
        let radial = basis();
        let mut semi = basis();
        semi.outlet_inclination = 30f64.to_radians();
        semi.outlet_hub_diameter = 0.8 * semi.outlet_diameter;
        let build = |b: &crate::impeller::ImpellerBasis, mode| {
            let mut input = MeridionalInput {
                design_mode: mode,
                ..MeridionalInput::default()
            };
            input.chambers.enabled = true;
            MeridionalContour::build(b, &input).unwrap()
        };
        for b in [&radial, &semi] {
            let c = build(b, DesignMode::Walls);
            assert!(c.crossings.is_empty(), "{:?}", c.crossings);
            // On its center line the hub dips a little under d_n near the eye and
            // crosses the bore of the disk: a true conflict, and the only one.
            let dip = build(b, DesignMode::CenterLine);
            assert!(dip.crossings.iter().all(|x| x.walls == ("hub disk", "hub")));
        }
        // The front casing pulled through the shroud cover.
        let b = basis();
        let mut input = MeridionalInput::default();
        input.chambers.enabled = true;
        let c = MeridionalContour::build(&b, &input).unwrap();
        let mut front = c.chambers.front_casing.to_input();
        let inside = c.shroud.sample(9)[4] + DVec2::new(0.002, 0.0);
        front[0].controls[3] = inside;
        input.chambers.front = Some(front);
        input.chambers.axial_gap = Param::Manual(c.chambers.values.axial_gap);
        let crossed = MeridionalContour::build(&b, &input).unwrap();
        assert!(
            crossed
                .crossings
                .iter()
                .any(|x| x.walls.0 == "front casing wall"),
            "{:?}",
            crossed.crossings
        );
    }
}
