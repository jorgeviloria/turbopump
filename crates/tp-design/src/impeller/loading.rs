//! Relative velocities on the two faces of the blades, along each blade section.
//!
//! The flow is taken to follow the blades (no slip). On each stream surface the mean
//! relative velocity comes from the meridional velocity, the blade angle and the
//! blockage of the blades. The difference between the faces comes from the change of
//! angular momentum along the blade: a blade that raises r·c_u by d(r·c_u) over a
//! length ds carries the circulation 2π·d(r·c_u)/z, so
//!
//! Δw = w_SS − w_PS = (2π/z)·d(r·c_u)/ds
//!
//! and the faces see w ± Δw/2 (see `docs/theory/prestaciones.md`).

use std::f64::consts::{FRAC_PI_2, PI};

use serde::Serialize;
use tp_math::DVec2;

use super::ImpellerGeometry;
use super::meridian::{Channel, QuasiOrthogonal};
use crate::triangle::blockage_factor;

/// Velocities along one blade section, from the leading to the trailing edge.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SectionVelocities {
    /// Flow fraction of the span (0 hub, 1 shroud).
    pub span: f64,
    /// Meridional length from the leading edge over the whole length.
    pub position: Vec<f64>,
    /// Meridional velocity, m/s.
    pub cm: Vec<f64>,
    /// Circumferential absolute velocity with the flow following the camber, m/s.
    pub cu: Vec<f64>,
    /// Mean relative velocity, m/s.
    pub mean: Vec<f64>,
    /// Suction face, m/s.
    pub suction: Vec<f64>,
    /// Pressure face, m/s.
    pub pressure: Vec<f64>,
}

impl SectionVelocities {
    /// Δw = w_SS − w_PS at each station, m/s.
    pub fn loading(&self) -> Vec<f64> {
        self.suction
            .iter()
            .zip(&self.pressure)
            .map(|(s, p)| s - p)
            .collect()
    }
}

/// Velocities on every blade section.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BladeVelocities {
    pub sections: Vec<SectionVelocities>,
    /// Outlet circumferential speed u₂, to make the velocities dimensionless.
    pub outlet_speed: f64,
    pub warnings: Vec<String>,
}

/// Signed distance of `p` from the line of a quasi-orthogonal; it changes sign from
/// one side of the line to the other.
fn side(q: &QuasiOrthogonal, p: DVec2) -> f64 {
    let d = q.shroud - q.hub;
    (p - q.hub).perp_dot(d) / d.length().max(f64::EPSILON)
}

/// Meridional velocity at `p`: between the two quasi-orthogonals that bracket it,
/// weighted by the distances to their lines. Outside the channel, the nearest one.
pub fn meridional_velocity(channel: &Channel, p: DVec2) -> f64 {
    let qos = &channel.quasi_orthogonals;
    let sides: Vec<f64> = qos.iter().map(|q| side(q, p)).collect();
    for i in 0..qos.len().saturating_sub(1) {
        let (a, b) = (sides[i], sides[i + 1]);
        if a == 0.0 {
            return qos[i].cm;
        }
        if a * b <= 0.0 {
            let t = a / (a - b);
            return qos[i].cm + t * (qos[i + 1].cm - qos[i].cm);
        }
    }
    qos.iter()
        .zip(&sides)
        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map_or(0.0, |(q, _)| q.cm)
}

/// Derivative of `y` against `x` on an uneven grid: one-sided at the ends, the
/// three-point formula inside.
fn derivative(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = x.len();
    (0..n)
        .map(|i| {
            if n < 2 {
                0.0
            } else if i == 0 {
                (y[1] - y[0]) / (x[1] - x[0])
            } else if i == n - 1 {
                (y[n - 1] - y[n - 2]) / (x[n - 1] - x[n - 2])
            } else {
                let (h0, h1) = (x[i] - x[i - 1], x[i + 1] - x[i]);
                let (d0, d1) = ((y[i] - y[i - 1]) / h0, (y[i + 1] - y[i]) / h1);
                (h1 * d0 + h0 * d1) / (h0 + h1)
            }
        })
        .collect()
}

/// Velocities on the blades of `g`.
pub fn blade_velocities(g: &ImpellerGeometry) -> BladeVelocities {
    let basis = &g.basis;
    let z = basis.blade_count;
    let omega = basis.omega;
    let mut warnings = Vec::new();
    let sections: Vec<SectionVelocities> = g
        .blade
        .sections
        .iter()
        .map(|s| {
            let n = s.m.len();
            let total = s.m.last().copied().unwrap_or(1.0).max(f64::EPSILON);
            let mut cm = Vec::with_capacity(n);
            let mut cu = Vec::with_capacity(n);
            let mut mean = Vec::with_capacity(n);
            let mut momentum = Vec::with_capacity(n);
            for k in 0..n {
                let p = s.meridional[k];
                let r = p.y;
                let beta = s.beta[k];
                let tau = blockage_factor(z, s.thickness[k], 2.0 * r, beta, FRAC_PI_2);
                let tau = if tau.is_finite() { tau } else { 1.0 };
                let c_m = meridional_velocity(&g.channel, p);
                // The swirl follows the camber: the thickness, which grows from zero at
                // a rounded edge, speeds the flow up but does not turn it.
                let c_u = omega * r - c_m / beta.tan();
                cm.push(c_m);
                cu.push(c_u);
                mean.push(c_m * tau / beta.sin());
                momentum.push(r * c_u);
            }
            // Arc length along the mean line on the stream surface.
            let mut length = vec![0.0; n];
            for k in 1..n {
                let dm = s.m[k] - s.m[k - 1];
                let r = 0.5 * (s.meridional[k].y + s.meridional[k - 1].y);
                let dtheta = s.theta[k] - s.theta[k - 1];
                length[k] = length[k - 1] + dm.hypot(r * dtheta);
            }
            let loading: Vec<f64> = derivative(&length, &momentum)
                .into_iter()
                .map(|d| 2.0 * PI / f64::from(z) * d)
                .collect();
            let suction: Vec<f64> = mean
                .iter()
                .zip(&loading)
                .map(|(w, l)| w + l / 2.0)
                .collect();
            let pressure: Vec<f64> = mean
                .iter()
                .zip(&loading)
                .map(|(w, l)| w - l / 2.0)
                .collect();
            if let Some(k) = pressure.iter().position(|&w| w < 0.0) {
                warnings.push(format!(
                    "the relative velocity on the pressure face turns negative at m̄ = {:.2} \
                     on span {:.2}: the flow may separate there",
                    s.m[k] / total,
                    s.span
                ));
            }
            SectionVelocities {
                span: s.span,
                position: s.m.iter().map(|m| m / total).collect(),
                cm,
                cu,
                mean,
                suction,
                pressure,
            }
        })
        .collect();
    BladeVelocities {
        sections,
        outlet_speed: omega * basis.outlet_diameter / 2.0,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::tests::reference_geometry;
    use approx::assert_relative_eq;

    #[test]
    fn derivative_is_exact_for_a_parabola_inside() {
        let x = [0.0, 0.1, 0.3, 0.6, 1.0];
        let y: Vec<f64> = x.iter().map(|v| v * v).collect();
        let d = derivative(&x, &y);
        for i in 1..4 {
            assert_relative_eq!(d[i], 2.0 * x[i], epsilon = 1e-12);
        }
    }

    #[test]
    fn meridional_velocity_stays_within_the_channel_values() {
        let g = reference_geometry();
        let (lo, hi) = g
            .channel
            .quasi_orthogonals
            .iter()
            .fold((f64::INFINITY, 0.0f64), |(lo, hi), q| {
                (lo.min(q.cm), hi.max(q.cm))
            });
        for s in &g.blade.sections {
            for &p in &s.meridional {
                let cm = meridional_velocity(&g.channel, p);
                assert!(cm >= lo - 1e-9 && cm <= hi + 1e-9, "{cm}");
            }
        }
        // On a quasi-orthogonal itself the velocity is that line's.
        let q = g.channel.quasi_orthogonals[3];
        let mid = (q.hub + q.shroud) / 2.0;
        assert_relative_eq!(
            meridional_velocity(&g.channel, mid),
            q.cm,
            max_relative = 1e-9
        );
    }

    #[test]
    fn the_faces_straddle_the_mean_and_hold_the_work() {
        let g = reference_geometry();
        let v = blade_velocities(&g);
        assert_eq!(v.sections.len(), g.blade.sections.len());
        let z = f64::from(g.basis.blade_count);
        for (sv, s) in v.sections.iter().zip(&g.blade.sections) {
            let n = sv.mean.len();
            for k in 0..n {
                assert_relative_eq!(
                    0.5 * (sv.suction[k] + sv.pressure[k]),
                    sv.mean[k],
                    epsilon = 1e-9
                );
            }
            // Integrating the loading along the blade gives back the change of r·c_u.
            let mut integral = 0.0;
            let loading = sv.loading();
            for k in 1..n {
                let dm = s.m[k] - s.m[k - 1];
                let r = 0.5 * (s.meridional[k].y + s.meridional[k - 1].y);
                let ds = dm.hypot(r * (s.theta[k] - s.theta[k - 1]));
                integral += 0.5 * (loading[k] + loading[k - 1]) * ds;
            }
            let r_le = s.meridional[0].y;
            let r_te = s.meridional[n - 1].y;
            let work = r_te * sv.cu[n - 1] - r_le * sv.cu[0];
            assert_relative_eq!(integral, 2.0 * PI / z * work, max_relative = 0.03);
            // A pump blade adds swirl: the suction face is the faster one on average.
            assert!(work > 0.0);
        }
        assert!(v.outlet_speed > 20.0);
    }
}
