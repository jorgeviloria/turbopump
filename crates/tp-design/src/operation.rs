//! A pump in its system: the operating point, other speeds, a trimmed impeller and the
//! allowable range of continuous operation, after Gülich 2nd ed., Chaps. 3.4, 4.5 and 11.
//!
//! - System curve H_A = H_stat + R·Q² (eqs. 11.1 to 11.3). It passes through a given
//!   point, by default the design point, with no static head unless one is given.
//! - Another speed by the similarity laws Q ∝ n, H ∝ n², NPSH ∝ n² with equal
//!   efficiencies, the first approximation of Table 3.4.
//! - A trimmed impeller along rays from the origin: Q'/Q = H'/H = (d₂'/d₂)^m (eq. 4.15),
//!   with m = 2 by default as Gülich quotes from [B.5]. The best efficiency drops by
//!   Δη = ε·(1 − d₂'/d₂) (eq. 4.19), ε = 0.2 for volute pumps and 0.45 for diffuser
//!   pumps (the middle of 0.15–0.25 and 0.4–0.5); we take the same drop over the whole
//!   curve. The inlet is unchanged, so NPSH₃ keeps its value at each flow.
//! - The trim that brings the curve through the system point follows the ray through
//!   it (eq. 4.17); the speed, the parabola H ∝ Q² through it.
//! - Trimming limits: d₂'/d₂ ≥ 0.8 for volute pumps up to n_q = 40, plus
//!   0.0025·(n_q − 40) up to n_q = 100 (eq. 4.23, the lower end of 0.8 to 0.85); 5 %
//!   for diffuser pumps without tests (Chap. 4.5).
//! - Continuous operation where η ≥ 0.8·η_max (Chap. 11.6: 80 to 85 % of the best),
//!   and below the flow at which NPSH₃ reaches the NPSH available.

use serde::{Deserialize, Serialize};

use crate::npsh::NpshCurve;
use crate::performance::{Collector, PerformancePoint};
use crate::pump::{CentrifugalPumpDesign, PumpDuty};
use crate::{DesignError, Param};

/// Default exponent m of the trimming law (Gülich eq. 4.15 with [B.5]).
pub const TRIM_EXPONENT: f64 = 2.0;
/// Default fraction of the best efficiency that bounds continuous operation.
pub const CONTINUOUS_EFFICIENCY: f64 = 0.8;

/// Settings of the operation in a system. Every value is automatic by default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OperationInput {
    /// Static head of the system H_stat, m; automatic: 0.
    pub static_head: Param<f64>,
    /// A point of the system curve: flow (m³/s) and head (m); automatic: the design
    /// point.
    pub system_flow: Param<f64>,
    pub system_head: Param<f64>,
    /// Speed, rad/s; automatic: the design speed.
    pub speed: Param<f64>,
    /// Trimmed impeller diameter d₂', m; automatic: d₂.
    pub trimmed_diameter: Param<f64>,
    /// Exponent m of the trimming law; automatic: 2.
    pub trim_exponent: Param<f64>,
    /// η/η_max that bounds continuous operation; automatic: 0.8.
    pub continuous_efficiency: Param<f64>,
}

/// H_A = H_stat + R·Q².
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SystemCurve {
    /// m
    pub static_head: f64,
    /// R, s²/m⁵.
    pub resistance: f64,
}

impl SystemCurve {
    /// The curve with static head `static_head` (m) through flow `flow` (m³/s) and head
    /// `head` (m).
    pub fn through(static_head: f64, flow: f64, head: f64) -> Result<Self, DesignError> {
        if !(flow > 0.0 && head > static_head) {
            return Err(DesignError::InvalidInput(format!(
                "the system point needs a positive flow and a head above the static head of \
                 {static_head:.2} m"
            )));
        }
        Ok(Self {
            static_head,
            resistance: (head - static_head) / (flow * flow),
        })
    }

    pub fn head(&self, flow: f64) -> f64 {
        self.static_head + self.resistance * flow * flow
    }
}

/// One point of the pump curve at the speed and diameter of operation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CurvePoint {
    /// m³/s
    pub flow: f64,
    /// m
    pub head: f64,
    pub efficiency: f64,
    /// W
    pub power: f64,
    /// m
    pub npsh_required: f64,
    /// In the recirculation range of the prediction.
    pub estimated: bool,
}

/// The pump in its system.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Operation {
    pub system: SystemCurve,
    /// The point the system curve goes through, m³/s and m.
    pub system_flow: f64,
    pub system_head: f64,
    /// Speed, rad/s, and its ratio to the design speed.
    pub speed: f64,
    pub speed_ratio: f64,
    /// d₂', m, and d₂'/d₂.
    pub diameter: f64,
    pub trim_ratio: f64,
    pub trim_exponent: f64,
    /// Drop of the best efficiency by the trim (eq. 4.19).
    pub efficiency_drop: f64,
    /// Smallest d₂'/d₂ advised (eq. 4.23).
    pub trim_limit: f64,
    /// The pump curve at this speed and diameter.
    pub curve: Vec<CurvePoint>,
    /// Its best efficiency point.
    pub best: CurvePoint,
    /// Where the pump and system curves cross.
    pub point: Option<CurvePoint>,
    /// Flows of continuous operation, m³/s.
    pub continuous: (f64, f64),
    /// The d₂'/d₂ at this speed and the speed ratio at full diameter that bring the
    /// pump curve through the system point, when they exist.
    pub required_trim: Option<f64>,
    pub required_speed: Option<f64>,
    pub warnings: Vec<String>,
}

/// Head of a curve at `flow` by linear interpolation; `None` outside it.
fn interpolate(curve: &[CurvePoint], flow: f64) -> Option<CurvePoint> {
    let i = curve
        .windows(2)
        .position(|w| w[0].flow <= flow && flow <= w[1].flow)?;
    let (a, b) = (curve[i], curve[i + 1]);
    let t = if b.flow > a.flow {
        (flow - a.flow) / (b.flow - a.flow)
    } else {
        0.0
    };
    let mix = |x: f64, y: f64| x + (y - x) * t;
    Some(CurvePoint {
        flow,
        head: mix(a.head, b.head),
        efficiency: mix(a.efficiency, b.efficiency),
        power: mix(a.power, b.power),
        npsh_required: mix(a.npsh_required, b.npsh_required),
        estimated: a.estimated || b.estimated,
    })
}

/// Largest flow at which `f` turns from positive to negative along the curve, found by
/// bisection between the points around it.
fn last_crossing(curve: &[CurvePoint], f: impl Fn(&CurvePoint) -> f64) -> Option<f64> {
    let i = curve
        .windows(2)
        .rposition(|w| f(&w[0]) >= 0.0 && f(&w[1]) < 0.0)?;
    let (mut lo, mut hi) = (curve[i].flow, curve[i + 1].flow);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        match interpolate(curve, mid) {
            Some(p) if f(&p) >= 0.0 => lo = mid,
            _ => hi = mid,
        }
    }
    Some(0.5 * (lo + hi))
}

/// Smallest flow at which `f` turns from negative to positive.
fn first_rise(curve: &[CurvePoint], f: impl Fn(&CurvePoint) -> f64) -> Option<f64> {
    let i = curve
        .windows(2)
        .position(|w| f(&w[0]) < 0.0 && f(&w[1]) >= 0.0)?;
    let (mut lo, mut hi) = (curve[i].flow, curve[i + 1].flow);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        match interpolate(curve, mid) {
            Some(p) if f(&p) < 0.0 => lo = mid,
            _ => hi = mid,
        }
    }
    Some(0.5 * (lo + hi))
}

impl Operation {
    /// The pump of `design` at `duty`, whose predicted points are `points` with best
    /// point `best` and NPSH₃ curve `npsh`, in the system and at the speed and diameter
    /// of `input`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        points: &[PerformancePoint],
        best: &PerformancePoint,
        npsh: &NpshCurve,
        npsh_available: f64,
        duty: &PumpDuty,
        design: &CentrifugalPumpDesign,
        collector: Collector,
        input: &OperationInput,
    ) -> Result<Self, DesignError> {
        let invalid = |message: String| Err(DesignError::InvalidInput(message));
        let speed = input.speed.resolve(|| duty.speed);
        if speed <= 0.0 {
            return invalid("the speed must be positive".into());
        }
        let d2 = design.outlet_diameter;
        let diameter = input.trimmed_diameter.resolve(|| d2);
        if !(diameter > 0.0 && diameter <= d2 * (1.0 + 1e-9)) {
            return invalid(format!(
                "the trimmed diameter must be positive and at most d₂ = {:.1} mm",
                d2 * 1e3
            ));
        }
        let m = input.trim_exponent.resolve(|| TRIM_EXPONENT);
        if !(1.0..=3.5).contains(&m) {
            return invalid(format!(
                "the trimming exponent must lie between 1 and 3.5, got {m}"
            ));
        }
        let fraction = input
            .continuous_efficiency
            .resolve(|| CONTINUOUS_EFFICIENCY);
        if !(0.3..1.0).contains(&fraction) {
            return invalid(format!(
                "the efficiency fraction of continuous operation must lie between 0.3 and 1, \
                 got {fraction}"
            ));
        }
        let static_head = input.static_head.resolve(|| 0.0);
        let system_flow = input.system_flow.resolve(|| duty.flow);
        let system_head = input.system_head.resolve(|| duty.head);
        let system = SystemCurve::through(static_head, system_flow, system_head)?;

        let s = speed / duty.speed;
        let t = (diameter / d2).min(1.0);
        let diffuser = matches!(collector, Collector::VanedDiffuser);
        let epsilon = if diffuser { 0.45 } else { 0.2 };
        let drop = epsilon * (1.0 - t);
        let along = t.powf(m);
        // Trim along rays, then change the speed.
        let scale = |p: &PerformancePoint, along: f64, drop: f64| -> CurvePoint {
            let flow = p.flow * along * s;
            let head = p.head * along * s * s;
            let efficiency = (p.efficiency - drop).max(0.0);
            let power = if p.efficiency > 0.0 && efficiency > 0.0 {
                p.power * along * along * s.powi(3) * p.efficiency / efficiency
            } else {
                p.power * along * along * s.powi(3)
            };
            CurvePoint {
                flow,
                head,
                efficiency,
                power,
                npsh_required: s * s * npsh.at(flow / s),
                estimated: p.estimated,
            }
        };
        let curve: Vec<CurvePoint> = points.iter().map(|p| scale(p, along, drop)).collect();
        let best_point = scale(best, along, drop);
        let point = last_crossing(&curve, |p| p.head - system.head(p.flow))
            .and_then(|q| interpolate(&curve, q));

        // Continuous operation: η ≥ fraction·η_max, below the cavitation flow.
        let threshold = fraction * best_point.efficiency;
        let low = first_rise(&curve, |p| p.efficiency - threshold).unwrap_or(0.0);
        let mut high = last_crossing(&curve, |p| p.efficiency - threshold)
            .unwrap_or_else(|| curve.last().map_or(0.0, |p| p.flow));
        if let Some(q) = last_crossing(&curve, |p| npsh_available - p.npsh_required) {
            high = high.min(q);
        }

        // Trim and speed that reach the system point from the full-diameter curve at
        // this speed.
        let full: Vec<CurvePoint> = points.iter().map(|p| scale(p, 1.0, 0.0)).collect();
        let ray = system_head / system_flow;
        let required_trim = last_crossing(&full, |p| p.head - ray * p.flow)
            .and_then(|q| interpolate(&full, q))
            .map(|p| (system_head / p.head).powf(1.0 / m))
            .filter(|&t| t <= 1.0);
        let parabola = system_head / (system_flow * system_flow);
        let required_speed = last_crossing(&full, |p| p.head - parabola * p.flow * p.flow)
            .map(|q| s * system_flow / q);

        let trim_limit = if diffuser {
            0.95
        } else {
            (0.8 + 0.0025 * (design.nq.clamp(40.0, 100.0) - 40.0)).min(0.95)
        };
        let mut warnings = Vec::new();
        if t < trim_limit - 1e-9 {
            warnings.push(format!(
                "d₂'/d₂ = {t:.3} is below the {trim_limit:.3} advised for this pump (Gülich eq. \
                 4.23): expect noise, vibration and a poor efficiency"
            ));
        }
        match &point {
            None => warnings.push(
                "the pump and system curves do not cross: there is no operating point".into(),
            ),
            Some(p) => {
                if p.flow < low || p.flow > high {
                    warnings.push(format!(
                        "the operating point at {:.0} % of the best efficiency flow lies outside \
                         continuous operation ({:.0} to {:.0} %, Gülich Chap. 11.6)",
                        100.0 * p.flow / best_point.flow,
                        100.0 * low / best_point.flow,
                        100.0 * high / best_point.flow
                    ));
                }
                if p.npsh_required > npsh_available {
                    warnings.push(format!(
                        "NPSH₃ at the operating point is {:.2} m, above the {:.2} m available",
                        p.npsh_required, npsh_available
                    ));
                }
            }
        }
        Ok(Self {
            system,
            system_flow,
            system_head,
            speed,
            speed_ratio: s,
            diameter,
            trim_ratio: t,
            trim_exponent: m,
            efficiency_drop: drop,
            trim_limit,
            curve,
            best: best_point,
            point,
            continuous: (low, high),
            required_trim,
            required_speed,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn the_system_curve_is_a_parabola_from_the_static_head() {
        let c = SystemCurve::through(10.0, 0.02, 30.0).unwrap();
        assert_relative_eq!(c.head(0.0), 10.0);
        assert_relative_eq!(c.head(0.02), 30.0, max_relative = 1e-12);
        assert_relative_eq!(c.resistance, 20.0 / 4e-4, max_relative = 1e-12);
        assert!(SystemCurve::through(40.0, 0.02, 30.0).is_err());
        assert!(SystemCurve::through(0.0, 0.0, 30.0).is_err());
    }

    #[test]
    fn crossings_are_found_between_points() {
        let curve: Vec<CurvePoint> = (0..=10)
            .map(|i| {
                let q = f64::from(i) / 10.0;
                CurvePoint {
                    flow: q,
                    head: 1.0 - q * q,
                    efficiency: q * (2.0 - q),
                    power: 1.0,
                    npsh_required: q,
                    estimated: false,
                }
            })
            .collect();
        // 1 − q² = 0.5 at q = √0.5.
        let q = last_crossing(&curve, |p| p.head - 0.5).unwrap();
        assert_relative_eq!(q, 0.5f64.sqrt(), epsilon = 2e-3);
        let q = first_rise(&curve, |p| p.efficiency - 0.75).unwrap();
        assert_relative_eq!(q, 0.5, epsilon = 1e-9);
        assert!(interpolate(&curve, 1.5).is_none());
    }
}
