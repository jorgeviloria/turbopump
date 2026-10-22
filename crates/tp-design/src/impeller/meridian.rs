//! Meridional contour of a radial impeller and the quasi-one-dimensional flow through it.
//!
//! Coordinates in the meridional plane are `DVec2 { x: z, y: r }`: z along the axis in
//! the inflow direction, r the radius. The inlet plane is z = 0; at the outlet
//! (r = d₂/2) the shroud sits at z = L − b₂ and the hub at z = L, L being the axial
//! length of the impeller.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use tp_math::{Bezier, DVec2, Polyline};

use super::ImpellerBasis;
use crate::{DesignError, Param};

/// Points used to turn each Bézier contour into an arc-length parameterized polyline.
const CONTOUR_SAMPLES: usize = 401;

/// Inputs of the meridional-contour step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeridionalInput {
    /// Axial length L from the inlet plane to the hub at the outlet, m.
    pub axial_length: Param<f64>,
    /// Bézier tangent lengths as fractions (0–1) of the distance from each end point
    /// to the corner where the inlet and outlet tangents meet.
    pub hub_inlet_tangent: Param<f64>,
    pub hub_outlet_tangent: Param<f64>,
    pub shroud_inlet_tangent: Param<f64>,
    pub shroud_outlet_tangent: Param<f64>,
    /// Leading-edge position on the hub, as a fraction (0–1) of the hub length.
    pub leading_edge_hub: Param<f64>,
    /// Leading-edge position on the shroud, as a fraction (0–1) of the shroud length.
    pub leading_edge_shroud: Param<f64>,
}

/// Default tangent fraction: close to the 0.552 that makes a cubic Bézier match a
/// quarter ellipse inscribed in the corner.
pub const DEFAULT_TANGENT: f64 = 0.55;
pub const DEFAULT_LEADING_EDGE_HUB: f64 = 0.25;
pub const DEFAULT_LEADING_EDGE_SHROUD: f64 = 0.05;

/// Default axial length L = b₂ + (d₁ − d_n)/2: the outlet width plus the radial height
/// of the eye, which gives a channel of roughly constant curvature.
pub fn default_axial_length(basis: &ImpellerBasis) -> f64 {
    basis.outlet_width + (basis.inlet_diameter - basis.hub_diameter) / 2.0
}

/// Hub and shroud contours with the leading-edge position.
#[derive(Debug, Clone)]
pub struct MeridionalContour {
    pub hub: Bezier<DVec2>,
    pub shroud: Bezier<DVec2>,
    pub axial_length: f64,
    /// Leading edge on the hub and on the shroud, as fractions of each contour length.
    pub leading_edge: (f64, f64),
    hub_line: Polyline<DVec2>,
    shroud_line: Polyline<DVec2>,
}

fn fraction(name: &str, value: f64) -> Result<f64, DesignError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(DesignError::InvalidInput(format!(
            "{name} must lie between 0 and 1, got {value}"
        )))
    }
}

/// Cubic Bézier from `start` to `end` with tangents through `corner`.
fn corner_curve(start: DVec2, corner: DVec2, end: DVec2, t_in: f64, t_out: f64) -> Bezier<DVec2> {
    Bezier::new(vec![
        start,
        start + (corner - start) * t_in,
        end + (corner - end) * t_out,
        end,
    ])
    .expect("four control points")
}

impl MeridionalContour {
    pub fn build(basis: &ImpellerBasis, input: &MeridionalInput) -> Result<Self, DesignError> {
        let rn = basis.hub_diameter / 2.0;
        let r1 = basis.inlet_diameter / 2.0;
        let r2 = basis.outlet_diameter / 2.0;
        let b2 = basis.outlet_width;
        let length = input.axial_length.resolve(|| default_axial_length(basis));
        if !(length.is_finite() && length > b2) {
            return Err(DesignError::InvalidInput(format!(
                "axial length {length:.4} m must exceed the outlet width {b2:.4} m"
            )));
        }
        let tangent = |name, p: &Param<f64>| fraction(name, p.resolve(|| DEFAULT_TANGENT));
        let hub = corner_curve(
            DVec2::new(0.0, rn),
            DVec2::new(length, rn),
            DVec2::new(length, r2),
            tangent("hub inlet tangent", &input.hub_inlet_tangent)?,
            tangent("hub outlet tangent", &input.hub_outlet_tangent)?,
        );
        let shroud = corner_curve(
            DVec2::new(0.0, r1),
            DVec2::new(length - b2, r1),
            DVec2::new(length - b2, r2),
            tangent("shroud inlet tangent", &input.shroud_inlet_tangent)?,
            tangent("shroud outlet tangent", &input.shroud_outlet_tangent)?,
        );
        let leading_edge = (
            fraction(
                "leading edge on the hub",
                input.leading_edge_hub.resolve(|| DEFAULT_LEADING_EDGE_HUB),
            )?,
            fraction(
                "leading edge on the shroud",
                input
                    .leading_edge_shroud
                    .resolve(|| DEFAULT_LEADING_EDGE_SHROUD),
            )?,
        );
        if leading_edge.0 >= 1.0 || leading_edge.1 >= 1.0 {
            return Err(DesignError::InvalidInput(
                "the leading edge must lie upstream of the outlet".into(),
            ));
        }
        let line = |b: &Bezier<DVec2>| {
            Polyline::new(b.sample(CONTOUR_SAMPLES)).expect("sampled curve has points")
        };
        Ok(Self {
            hub_line: line(&hub),
            shroud_line: line(&shroud),
            hub,
            shroud,
            axial_length: length,
            leading_edge,
        })
    }

    /// Leading edge parallel to the axis, as cylindrical blades are usually laid out:
    /// at the radius where the shroud has turned 30° away from axial, and the hub point
    /// at the same radius. Fractions of the hub and the shroud length.
    pub fn axial_leading_edge(&self) -> (f64, f64) {
        const SAMPLES: usize = 400;
        let h = 0.5 / SAMPLES as f64;
        let at = |i: usize| i as f64 / SAMPLES as f64;
        let shroud = (1..SAMPLES)
            .map(at)
            .find(|&t| {
                let (a, b) = (self.shroud_at(t - h), self.shroud_at(t + h));
                (b.y - a.y) >= 0.5 * a.distance(b)
            })
            .unwrap_or(0.5);
        let r = self.shroud_at(shroud).y;
        let hub = (0..SAMPLES)
            .map(at)
            .find(|&t| self.hub_at(t).y >= r)
            .unwrap_or(0.5);
        (hub, shroud)
    }

    /// Hub point at fraction `t` of its arc length.
    pub fn hub_at(&self, t: f64) -> DVec2 {
        self.hub_line.point_at_fraction(t)
    }

    /// Shroud point at fraction `t` of its arc length.
    pub fn shroud_at(&self, t: f64) -> DVec2 {
        self.shroud_line.point_at_fraction(t)
    }

    pub fn hub_length(&self) -> f64 {
        self.hub_line.length()
    }

    pub fn shroud_length(&self) -> f64 {
        self.shroud_line.length()
    }
}

/// A straight line across the channel from hub to shroud.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct QuasiOrthogonal {
    /// Position along the channel: fraction of hub and shroud arc length.
    pub t: f64,
    pub hub: DVec2,
    pub shroud: DVec2,
    /// Flow area normal to the mean streamline, m².
    pub area: f64,
    /// Meridional velocity, uniform along the line, m/s.
    pub cm: f64,
}

/// A streamline in the meridional plane, one point per quasi-orthogonal.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpanLine {
    /// Fraction of the flow between the hub and this line (0 hub, 1 shroud).
    pub fraction: f64,
    pub points: Vec<DVec2>,
}

/// Quasi-one-dimensional meridional flow: uniform meridional velocity along each
/// quasi-orthogonal, and streamlines that split the flow into equal stream tubes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Channel {
    pub quasi_orthogonals: Vec<QuasiOrthogonal>,
    pub spans: Vec<SpanLine>,
}

/// Position λ ∈ [0, 1] along a hub→shroud line with radii `r_hub`, `r_shroud` that
/// encloses `fraction` of the annular area swept by revolving the line about the axis.
///
/// The swept area of the first λ of the line is π·(2r_h + λΔr)·λ·ℓ, so λ solves
/// Δr·λ² + 2r_h·λ − f·(2r_h + Δr) = 0, written without cancellation.
pub fn equal_area_position(r_hub: f64, r_shroud: f64, fraction: f64) -> f64 {
    let dr = r_shroud - r_hub;
    let rhs = fraction * (2.0 * r_hub + dr);
    let denominator = r_hub + (r_hub * r_hub + dr * rhs).max(0.0).sqrt();
    if denominator <= f64::EPSILON * r_shroud.abs().max(1.0) {
        fraction
    } else {
        rhs / denominator
    }
}

impl Channel {
    /// `quasi_orthogonals` lines along the channel and `spans` streamlines (≥ 2).
    pub fn build(
        contour: &MeridionalContour,
        flow: f64,
        quasi_orthogonals: usize,
        spans: usize,
    ) -> Self {
        let n = quasi_orthogonals.max(3);
        let dt = 0.5 / (n - 1) as f64;
        let direction = |at: &dyn Fn(f64) -> DVec2, t: f64| {
            (at((t + dt).min(1.0)) - at((t - dt).max(0.0))).normalize()
        };
        let qos: Vec<QuasiOrthogonal> = (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                let hub = contour.hub_at(t);
                let shroud = contour.shroud_at(t);
                let across = shroud - hub;
                let streamwise = (direction(&|t| contour.hub_at(t), t)
                    + direction(&|t| contour.shroud_at(t), t))
                .normalize();
                let sin = across.normalize().perp_dot(streamwise).abs();
                let area = PI * (hub.y + shroud.y) * across.length() * sin;
                QuasiOrthogonal {
                    t,
                    hub,
                    shroud,
                    area,
                    cm: flow / area,
                }
            })
            .collect();

        let m = spans.max(2);
        let span_lines = (0..m)
            .map(|j| {
                let fraction = j as f64 / (m - 1) as f64;
                let points = qos
                    .iter()
                    .map(|q| {
                        let lambda = equal_area_position(q.hub.y, q.shroud.y, fraction);
                        q.hub.lerp(q.shroud, lambda)
                    })
                    .collect();
                SpanLine { fraction, points }
            })
            .collect();
        Self {
            quasi_orthogonals: qos,
            spans: span_lines,
        }
    }

    /// Meridional velocity at channel position `t`, interpolated between
    /// quasi-orthogonals.
    pub fn cm_at(&self, t: f64) -> f64 {
        let n = self.quasi_orthogonals.len();
        let x = t.clamp(0.0, 1.0) * (n - 1) as f64;
        let i = (x.floor() as usize).min(n - 2);
        let w = x - i as f64;
        let (a, b) = (&self.quasi_orthogonals[i], &self.quasi_orthogonals[i + 1]);
        a.cm + w * (b.cm - a.cm)
    }

    /// Largest local rise of the meridional velocity along the channel, as a ratio to
    /// its value upstream (0 when cm never rises).
    pub fn max_cm_rise(&self) -> f64 {
        let mut min_so_far = f64::INFINITY;
        let mut rise = 0.0_f64;
        for q in &self.quasi_orthogonals {
            min_so_far = min_so_far.min(q.cm);
            rise = rise.max(q.cm / min_so_far - 1.0);
        }
        rise
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use approx::assert_relative_eq;

    pub(crate) fn basis() -> ImpellerBasis {
        ImpellerBasis {
            hub_diameter: 0.0191,
            inlet_diameter: 0.0723,
            outlet_diameter: 0.1722,
            outlet_width: 0.0174,
            flow: 0.02864,
            omega: 303.69,
            blade_count: 7,
            blade_thickness: 0.003,
            outlet_blade_angle: 22.0f64.to_radians(),
            incidence: 0.0,
            rotation_sign: 1.0,
            pressure_coefficient: 0.9176,
            hydraulic_efficiency: 0.9065,
            nq: 35.92,
            outlet_relative_velocity: 13.27,
        }
    }

    #[test]
    fn contour_end_points_and_tangents() {
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let l = default_axial_length(&b);
        assert_eq!(c.hub.start(), DVec2::new(0.0, b.hub_diameter / 2.0));
        assert_eq!(c.hub.end(), DVec2::new(l, b.outlet_diameter / 2.0));
        assert_eq!(c.shroud.start(), DVec2::new(0.0, b.inlet_diameter / 2.0));
        assert_eq!(
            c.shroud.end(),
            DVec2::new(l - b.outlet_width, b.outlet_diameter / 2.0)
        );
        // Axial inflow, radial outflow.
        let d_in = c.shroud.derivative().eval(0.0);
        let d_out = c.hub.derivative().eval(1.0);
        assert!(d_in.y.abs() < 1e-12 && d_in.x > 0.0);
        assert!(d_out.x.abs() < 1e-12 && d_out.y > 0.0);
    }

    #[test]
    fn inlet_and_outlet_areas_are_exact() {
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let ch = Channel::build(&c, b.flow, 41, 5);
        let inlet = ch.quasi_orthogonals.first().unwrap();
        let outlet = ch.quasi_orthogonals.last().unwrap();
        let (rn, r1, r2) = (
            b.hub_diameter / 2.0,
            b.inlet_diameter / 2.0,
            b.outlet_diameter / 2.0,
        );
        assert_relative_eq!(inlet.area, PI * (r1 * r1 - rn * rn), max_relative = 1e-3);
        assert_relative_eq!(
            outlet.area,
            2.0 * PI * r2 * b.outlet_width,
            max_relative = 1e-3
        );
        assert_relative_eq!(outlet.cm * outlet.area, b.flow, max_relative = 1e-12);
    }

    #[test]
    fn spans_split_flow_equally() {
        for (rh, rs) in [(0.01, 0.036), (0.086, 0.086), (0.0, 0.03), (0.05, 0.02)] {
            for f in [0.0, 0.25, 0.5, 1.0] {
                let lambda = equal_area_position(rh, rs, f);
                let swept = |l: f64| (2.0 * rh + l * (rs - rh)) * l;
                assert_relative_eq!(swept(lambda), f * swept(1.0), epsilon = 1e-12);
            }
        }
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let ch = Channel::build(&c, b.flow, 21, 5);
        assert_eq!(
            ch.spans.first().unwrap().points[10],
            ch.quasi_orthogonals[10].hub
        );
        assert_eq!(
            ch.spans.last().unwrap().points[10],
            ch.quasi_orthogonals[10].shroud
        );
    }

    #[test]
    fn reference_channel_decelerates_smoothly() {
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let ch = Channel::build(&c, b.flow, 61, 5);
        let first = ch.quasi_orthogonals.first().unwrap().cm;
        let last = ch.quasi_orthogonals.last().unwrap().cm;
        assert!(last < first, "cm1 = {first}, cm2 = {last}");
        assert!(ch.max_cm_rise() < 0.05, "rise = {}", ch.max_cm_rise());
        assert_relative_eq!(ch.cm_at(0.0), first);
    }

    #[test]
    fn rejects_bad_inputs() {
        let b = basis();
        let short = MeridionalInput {
            axial_length: Param::Manual(0.01),
            ..Default::default()
        };
        assert!(MeridionalContour::build(&b, &short).is_err());
        let bad_le = MeridionalInput {
            leading_edge_hub: Param::Manual(1.2),
            ..Default::default()
        };
        assert!(MeridionalContour::build(&b, &bad_le).is_err());
    }
}
