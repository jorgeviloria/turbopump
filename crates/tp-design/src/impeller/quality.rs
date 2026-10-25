//! Indicators of blade quality from the velocities on the blade faces: how much the
//! relative flow decelerates, how high the suction face peaks over the outlet, where
//! the loading peaks, whether the pressure face stalls and how strongly the suction
//! face drives a secondary flow from hub to shroud.
//!
//! They come from the same estimate as [`super::loading`], the flow following the
//! blades, so they compare designs rather than predict losses.

use serde::Serialize;

use super::ImpellerGeometry;
use super::loading::{BladeVelocities, TRAILING_EDGE_SLIP};

/// Lowest ratio w₂/w₁ before the relative flow stalls early and loses efficiency: the
/// de Haller criterion, which Gülich (2nd ed., Chap. 7.2.1, p. 346) applies to the
/// outer streamline.
pub const DE_HALLER: f64 = 0.7;

/// Part of the blade from the leading edge over which the leading-edge loading is
/// averaged (our choice).
pub const LEADING_PART: f64 = 0.1;

/// Indicators of one blade section.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SectionQuality {
    /// Flow fraction of the span (0 hub, 1 shroud).
    pub span: f64,
    /// w₂/w₁ with the flow following the blade at both edges (no slip, which only
    /// raises w₂).
    pub deceleration: f64,
    /// Highest suction-face velocity over the mean outlet velocity: how far the
    /// suction face decelerates to the trailing edge.
    pub diffusion: f64,
    /// Highest Δw/w̄ = (w_SS − w_PS)/w̄ ahead of the trailing-edge slip zone.
    pub peak_loading: f64,
    /// m̄ of the highest loading.
    pub peak_at: f64,
    /// Mean Δw/w̄ over the first [`LEADING_PART`] of the blade: a loaded leading edge
    /// lowers the suction-face pressure where cavitation starts.
    pub leading_loading: f64,
    /// Lowest pressure-face velocity ahead of the trailing-edge slip zone, over u₂;
    /// below zero the flow there may separate.
    pub pressure_minimum: f64,
}

/// Indicators of a whole blade.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BladeQuality {
    /// From hub to shroud.
    pub sections: Vec<SectionQuality>,
    /// Highest difference of reduced static pressure p − ½ρu² on the suction face
    /// between hub and shroud, over ½ρu₂², ahead of the trailing-edge slip zone. With
    /// uniform rothalpy p_red = const − ½ρw², so it is (w_SS,shroud² − w_SS,hub²)/u₂².
    /// Positive: the shroud side is lower and draws the low-energy fluid of the suction
    /// face toward the shroud corner, where the wake forms. Gülich (2nd ed., Chap.
    /// 8.9.4, item 3) quantifies secondary flow by such pressure gradients across the
    /// main flow; Zangeneh, Goto & Harada (1998) design to keep this difference small.
    pub secondary_flow: f64,
    /// m̄ of the highest difference.
    pub secondary_at: f64,
}

impl BladeQuality {
    /// The outer streamline (the shroud section), which the de Haller criterion
    /// checks: near the hub w₁ is small and the flow may even accelerate.
    pub fn outer(&self) -> Option<&SectionQuality> {
        self.sections.last()
    }

    /// The section with the highest value of `value`.
    pub fn highest(&self, value: impl Fn(&SectionQuality) -> f64) -> Option<&SectionQuality> {
        self.sections
            .iter()
            .max_by(|a, b| value(a).total_cmp(&value(b)))
    }
}

/// Linear interpolation of `y` over the increasing `x` at `at`, held at the ends.
fn interpolate(x: &[f64], y: &[f64], at: f64) -> f64 {
    let n = x.len();
    if at <= x[0] {
        return y[0];
    }
    if at >= x[n - 1] {
        return y[n - 1];
    }
    let i = x.partition_point(|&v| v < at).clamp(1, n - 1);
    let w = (at - x[i - 1]) / (x[i] - x[i - 1]);
    y[i - 1] + w * (y[i] - y[i - 1])
}

/// Indicators of the blade of `g`, on its face velocities `v`.
pub fn blade_quality(g: &ImpellerGeometry, v: &BladeVelocities) -> BladeQuality {
    let u2 = v.outlet_speed.max(f64::EPSILON);
    let sections = v
        .sections
        .iter()
        .zip(&g.blade.sections)
        .map(|(sv, s)| {
            let n = sv.position.len();
            let along = |k: usize| sv.position[k] < TRAILING_EDGE_SLIP;
            let relative = |k: usize| s.cm[k] / s.beta[k].sin();
            let outlet = sv.mean[n - 1].max(f64::EPSILON);
            let loading =
                |k: usize| (sv.suction[k] - sv.pressure[k]) / sv.mean[k].max(f64::EPSILON);
            let (peak_loading, peak_at) = (0..n)
                .filter(|&k| along(k))
                .map(|k| (loading(k), sv.position[k]))
                .fold(
                    (f64::NEG_INFINITY, 0.0),
                    |a, b| if b.0 > a.0 { b } else { a },
                );
            // Trapezoids over the leading part.
            let mut area = 0.0;
            for k in 1..n {
                let (x0, x1) = (sv.position[k - 1], sv.position[k].min(LEADING_PART));
                if x1 <= x0 {
                    break;
                }
                let l1 = interpolate(&sv.position, &sv.suction, x1)
                    - interpolate(&sv.position, &sv.pressure, x1);
                let w1 = interpolate(&sv.position, &sv.mean, x1).max(f64::EPSILON);
                area += 0.5 * (loading(k - 1) + l1 / w1) * (x1 - x0);
            }
            SectionQuality {
                span: sv.span,
                deceleration: relative(n - 1) / relative(0).max(f64::EPSILON),
                diffusion: sv.suction.iter().copied().fold(f64::NEG_INFINITY, f64::max) / outlet,
                peak_loading,
                peak_at,
                leading_loading: area / LEADING_PART,
                pressure_minimum: (0..n)
                    .filter(|&k| along(k))
                    .map(|k| sv.pressure[k] / u2)
                    .fold(f64::INFINITY, f64::min),
            }
        })
        .collect();
    let (secondary_flow, secondary_at) = match (v.sections.first(), v.sections.last()) {
        (Some(hub), Some(shroud)) if v.sections.len() > 1 => hub
            .position
            .iter()
            .zip(&hub.suction)
            .filter(|(m, _)| **m < TRAILING_EDGE_SLIP)
            .map(|(&m, &w_hub)| {
                let w_shroud = interpolate(&shroud.position, &shroud.suction, m);
                ((w_shroud * w_shroud - w_hub * w_hub) / (u2 * u2), m)
            })
            .fold(
                (f64::NEG_INFINITY, 0.0),
                |a, b| if b.0 > a.0 { b } else { a },
            ),
        _ => (0.0, 0.0),
    };
    BladeQuality {
        sections,
        secondary_flow,
        secondary_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::loading::blade_velocities;
    use crate::impeller::tests::reference_geometry;
    use approx::assert_relative_eq;

    #[test]
    fn interpolation_holds_at_the_ends() {
        let (x, y) = ([0.0, 0.5, 1.0], [1.0, 3.0, 2.0]);
        assert_eq!(interpolate(&x, &y, -1.0), 1.0);
        assert_eq!(interpolate(&x, &y, 2.0), 2.0);
        assert_relative_eq!(interpolate(&x, &y, 0.25), 2.0);
        assert_relative_eq!(interpolate(&x, &y, 0.75), 2.5);
    }

    #[test]
    fn the_reference_blade_is_plausible() {
        let g = reference_geometry();
        let v = blade_velocities(&g);
        let q = blade_quality(&g, &v);
        assert_eq!(q.sections.len(), g.blade.sections.len());
        for s in &q.sections {
            assert!((0.3..2.0).contains(&s.deceleration), "{s:?}");
            assert!(s.diffusion > 1.0, "{s:?}");
            assert!(
                s.peak_loading > 0.0 && (0.0..1.0).contains(&s.peak_at),
                "{s:?}"
            );
            assert!(s.leading_loading.is_finite(), "{s:?}");
            assert!(s.pressure_minimum.is_finite(), "{s:?}");
        }
        assert!(q.secondary_flow.is_finite());
        assert!((0.0..TRAILING_EDGE_SLIP).contains(&q.secondary_at));
        // The outer streamline decelerates, and more than the hub.
        let outer = q.outer().unwrap();
        assert!(outer.deceleration < 1.0);
        assert!(outer.deceleration < q.sections[0].deceleration);
    }

    #[test]
    fn the_deceleration_is_that_of_the_edge_triangles() {
        let g = reference_geometry();
        let q = blade_quality(&g, &blade_velocities(&g));
        let s = &g.blade.sections[0];
        let n = s.m.len();
        let w = |k: usize| s.cm[k] / s.beta[k].sin();
        assert_relative_eq!(
            q.sections[0].deceleration,
            w(n - 1) / w(0),
            max_relative = 1e-12
        );
    }
}
