//! Thinning of the outlines a solid is lofted through, where splines through fewer of
//! them still pass within a tolerance of all.
//!
//! The kernel's boolean operations slow down steeply with the number of poles of the
//! B-spline surfaces they intersect: lofting the outer wall of a casing through every
//! outline the design samples made its two booleans take over a minute, through one
//! outline in two and one point in two a few seconds, for the same casing within 0.05 %
//! of its volume.

use tp_math::DVec3;

/// The outlines `pieces` (sequences of corresponding outlines, all with the same points,
/// whose smooth pieces start at `breaks`; one closed piece without breaks) with points
/// and outlines left out where splines through the rest pass within `tolerance` of
/// them, and the breaks of the thinned outlines. Every outline keeps the same points,
/// so they still correspond, and every sequence keeps its first and last outline, so
/// pieces that meet on an outline still meet on the same one.
pub(crate) fn thinned(
    pieces: &[Vec<Vec<DVec3>>],
    breaks: &[usize],
    tolerance: f64,
) -> (Vec<Vec<Vec<DVec3>>>, Vec<usize>) {
    let rings: Vec<&[DVec3]> = pieces.iter().flatten().map(Vec::as_slice).collect();
    let Some(n) = rings.first().map(|r| r.len()) else {
        return (pieces.to_vec(), breaks.to_vec());
    };
    // The points kept round each outline, the same for all of them; a closed outline is
    // a run from its first point round to it again.
    let starts: &[usize] = if breaks.len() < 2 { &[0] } else { breaks };
    let mut kept = Vec::new();
    let mut kept_breaks = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(n);
        let runs: Vec<Vec<DVec3>> = rings
            .iter()
            .map(|r| (start..=end).map(|i| r[i % n]).collect())
            .collect();
        kept_breaks.push(kept.len());
        let run = kept_run(&runs, tolerance);
        // The last point is the first of the next piece.
        kept.extend(run[..run.len() - 1].iter().map(|j| start + j));
    }
    if breaks.len() < 2 {
        kept_breaks.clear();
    }
    let thin = |r: &Vec<DVec3>| kept.iter().map(|&i| r[i]).collect::<Vec<_>>();
    // Then the outlines kept along each sequence.
    let thinned = pieces
        .iter()
        .map(|rings| {
            let rings: Vec<Vec<DVec3>> = rings.iter().map(thin).collect();
            kept_outlines(&rings, tolerance)
                .into_iter()
                .map(|j| rings[j].clone())
                .collect()
        })
        .collect();
    (thinned, kept_breaks)
}

/// The indices of the corresponding outlines `rings` a loft through them may keep, with
/// the first and the last, so that it passes within `tolerance` of the rest: judged by
/// the paths of their points from outline to outline.
fn kept_outlines(rings: &[Vec<DVec3>], tolerance: f64) -> Vec<usize> {
    let count = rings.first().map_or(0, Vec::len);
    let paths: Vec<Vec<DVec3>> = (0..count)
        .map(|i| rings.iter().map(|r| r[i]).collect())
        .collect();
    kept_run(&paths, tolerance)
}

/// The indices kept of the runs of corresponding points `runs`, the same for all, with
/// the first and the last: one after another, a point is left out while the splines
/// through the rest still pass within `tolerance` of every run's point.
fn kept_run(runs: &[Vec<DVec3>], tolerance: f64) -> Vec<usize> {
    let count = runs.first().map_or(0, Vec::len);
    let mut kept: Vec<usize> = (0..count).collect();
    // Passes until none leaves a point out; within one, a point that stays is the
    // neighbour of the next one tried, so that no pass leaves out two in a row.
    loop {
        let mut changed = false;
        let mut i = 1;
        while i + 1 < kept.len() {
            let mut trial = kept.clone();
            trial.remove(i);
            // The merged span and its neighbours, whose ends' tangents move.
            let spans = i.saturating_sub(2)..(i + 1).min(trial.len() - 1);
            let fits = spans
                .clone()
                .all(|k| runs.iter().all(|r| span_error(r, &trial, k) <= tolerance));
            if fits {
                kept = trial;
                changed = true;
            }
            i += 1;
        }
        if !changed {
            return kept;
        }
    }
}

/// Farthest that the points of `points` between `kept[k]` and `kept[k + 1]` lie from the
/// curve through the kept points: the cubic through the two kept points on each side
/// (the quadratic through three, the line through two, where the run ends), by chord
/// length, as close as an interpolating spline passes. Measured to the nearest of its
/// samples.
fn span_error(points: &[DVec3], kept: &[usize], k: usize) -> f64 {
    const SAMPLES: usize = 32;
    let (a, b) = (kept[k], kept[k + 1]);
    if b - a < 2 {
        return 0.0;
    }
    let first = k.saturating_sub(1);
    let last = (k + 2).min(kept.len() - 1);
    let nodes: Vec<DVec3> = (first..=last).map(|j| points[kept[j]]).collect();
    let mut knots = vec![0.0];
    for w in nodes.windows(2) {
        knots.push(knots[knots.len() - 1] + w[0].distance(w[1]));
    }
    let (s1, s2) = (knots[k - first], knots[k + 1 - first]);
    let curve: Vec<DVec3> = (0..=SAMPLES)
        .map(|i| lagrange(&knots, &nodes, s1 + (s2 - s1) * i as f64 / SAMPLES as f64))
        .collect();
    points[a + 1..b]
        .iter()
        .map(|q| {
            curve
                .windows(2)
                .map(|w| {
                    let (c, d) = (w[0], w[1]);
                    let t =
                        ((*q - c).dot(d - c) / (d - c).length_squared().max(1e-30)).clamp(0.0, 1.0);
                    q.distance(c + (d - c) * t)
                })
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

/// The polynomial through `nodes` at the parameters `knots`, at `s`.
fn lagrange(knots: &[f64], nodes: &[DVec3], s: f64) -> DVec3 {
    let mut sum = DVec3::ZERO;
    for (i, node) in nodes.iter().enumerate() {
        let mut weight = 1.0;
        for (j, knot) in knots.iter().enumerate() {
            if i != j {
                weight *= (s - knot) / (knots[i] - knot);
            }
        }
        sum += *node * weight;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn circle(radius: f64, n: usize, z: f64) -> Vec<DVec3> {
        (0..n)
            .map(|i| {
                let a = TAU * i as f64 / n as f64;
                DVec3::new(radius * a.cos(), radius * a.sin(), z)
            })
            .collect()
    }

    #[test]
    fn a_densely_sampled_circle_keeps_few_points() {
        let rings = vec![vec![circle(0.02, 64, 0.0), circle(0.02, 64, 0.01)]];
        let (thin, breaks) = thinned(&rings, &[], 2e-5);
        assert!(breaks.is_empty());
        let kept = thin[0][0].len();
        assert!((12..=32).contains(&kept), "{kept}");
        assert_eq!(thin[0][0][0], rings[0][0][0]);
    }

    #[test]
    fn a_coarse_outline_keeps_every_point() {
        let rings = vec![vec![circle(0.02, 8, 0.0), circle(0.02, 8, 0.01)]];
        let (thin, _) = thinned(&rings, &[], 1e-5);
        assert_eq!(thin[0][0].len(), 8);
    }

    #[test]
    fn outlines_left_out_lie_on_the_spline_through_the_rest() {
        // Outlines round a bend: their points run along smooth curves.
        let rings: Vec<Vec<DVec3>> = (0..=24)
            .map(|k| {
                let a = TAU * k as f64 / 48.0;
                circle(0.01, 16, 0.0)
                    .into_iter()
                    .map(|p| DVec3::new(p.x + 0.1 * a.cos(), p.y + 0.1 * a.sin(), p.z))
                    .collect()
            })
            .collect();
        let (thin, _) = thinned(std::slice::from_ref(&rings), &[], 5e-5);
        let kept = thin[0].len();
        assert!(kept < rings.len(), "{kept} of {}", rings.len());
        assert_eq!(thin[0][0], rings[0]);
        assert_eq!(thin[0].last(), rings.last());
    }

    #[test]
    fn a_kink_keeps_the_outlines_round_it() {
        // Straight on, then turning sharply: the outline at the turn stays.
        let rings: Vec<Vec<DVec3>> = (0..=20)
            .map(|k| {
                let shift = if k <= 10 {
                    DVec3::new(0.01 * k as f64, 0.0, 0.0)
                } else {
                    DVec3::new(0.1, 0.01 * (k - 10) as f64, 0.0)
                };
                circle(0.01, 16, 0.0)
                    .into_iter()
                    .map(|p| p + shift)
                    .collect()
            })
            .collect();
        let (thin, _) = thinned(std::slice::from_ref(&rings), &[], 5e-5);
        assert!(thin[0].contains(&rings[10]));
        assert!(thin[0].len() < rings.len());
    }

    #[test]
    fn the_breaks_follow_the_points_kept_and_corners_stay() {
        // A square of 4 straight sides, 8 points a side.
        let side = |a: DVec3, b: DVec3| (0..8).map(move |i| a + (b - a) * (i as f64 / 8.0));
        let c = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.01, 0.0, 0.0),
            DVec3::new(0.01, 0.01, 0.0),
            DVec3::new(0.0, 0.01, 0.0),
        ];
        let ring: Vec<DVec3> = (0..4).flat_map(|k| side(c[k], c[(k + 1) % 4])).collect();
        let rings = vec![vec![
            ring.clone(),
            ring.iter().map(|p| *p + DVec3::Z * 0.01).collect(),
        ]];
        let (thin, breaks) = thinned(&rings, &[0, 8, 16, 24], 1e-6);
        let out = &thin[0][0];
        assert_eq!(breaks.len(), 4);
        for (k, &b) in breaks.iter().enumerate() {
            assert_eq!(out[b], c[k], "corner {k}");
        }
        assert!(out.len() < ring.len());
    }
}
