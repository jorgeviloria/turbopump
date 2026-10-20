//! Performance curves of a centrifugal pump: head, power and efficiency against flow,
//! predicted from the design point.
//!
//! Procedure (see `docs/theory/prestaciones.md`), following Gülich 2nd ed., Chap. 4.1
//! and 4.3:
//! 1. theoretical head from the outlet velocity triangle with a constant slip factor,
//!    a linear function of the flow through the impeller (eqs. 4.1 and T3.2.7);
//! 2. hydraulic efficiency against flow from the statistical eq. 4.14, so that
//!    H = η_h·H_th (eq. T3.3.7);
//! 3. power from Euler's equation plus secondary losses that do not depend on flow
//!    (Chap. 4.1.2, eq. T3.5.1);
//! 4. below the onset of recirculation the losses cannot be calculated: the head is
//!    interpolated toward the shut-off head of eqs. 3.27 and 3.28, and the power is
//!    extrapolated. These points are marked as estimates.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use tp_units::STANDARD_GRAVITY as G;

use crate::pump::{CentrifugalPumpDesign, PumpDuty};
use crate::{DesignError, Param};

/// What collects the flow after the impeller; it sets the shut-off head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Collector {
    #[default]
    Volute,
    VanedDiffuser,
    VanelessDiffuser,
}

/// Settings of the prediction. Every value is automatic by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceInput {
    pub collector: Collector,
    /// Q/Q_opt at which recirculation sets in at the impeller inlet.
    pub recirculation_onset: Param<f64>,
    /// ψ₀ = 2gH₀/u₂² at shut-off.
    pub shutoff_coefficient: Param<f64>,
    /// Shaft power at shut-off, W; automatic: the computed power curve continued
    /// along its tangent.
    pub shutoff_power: Param<f64>,
    /// Largest Q/Q_opt to compute.
    pub max_flow_ratio: Param<f64>,
    /// Points of the curve from shut-off to the largest flow.
    pub points: usize,
}

impl Default for PerformanceInput {
    fn default() -> Self {
        Self {
            collector: Collector::Volute,
            recirculation_onset: Param::Auto,
            shutoff_coefficient: Param::Auto,
            shutoff_power: Param::Auto,
            max_flow_ratio: Param::Auto,
            points: 61,
        }
    }
}

/// One point of the characteristic.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PerformancePoint {
    /// Flow through the pump, m³/s.
    pub flow: f64,
    /// m
    pub head: f64,
    /// Euler head without losses, m.
    pub theoretical_head: f64,
    /// Shaft power, W.
    pub power: f64,
    pub efficiency: f64,
    pub hydraulic_efficiency: f64,
    /// In the recirculation range, where the values are an estimate.
    pub estimated: bool,
}

/// The predicted characteristic of one stage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PumpPerformance {
    /// From shut-off to the largest flow.
    pub points: Vec<PerformancePoint>,
    /// The design point the prediction starts from.
    pub design: PerformancePoint,
    /// The point of highest efficiency.
    pub best: PerformancePoint,
    /// Head and power against a closed valve.
    pub shutoff_head: f64,
    pub shutoff_power: f64,
    /// ψ₀ used for the shut-off head.
    pub shutoff_coefficient: f64,
    /// Flow at which recirculation sets in, m³/s.
    pub recirculation_flow: f64,
    /// Largest flow computed, m³/s.
    pub max_flow: f64,
    /// NPSH required at the design point, m.
    pub npsh_required: f64,
    pub warnings: Vec<String>,
}

/// Hydraulic efficiency relative to its maximum at flow ratio q* = Q/Q_opt:
/// 1 − 0.6·(q* − 0.9)² − 0.25·(q* − 0.9)³ (Gülich eq. 4.14). The maximum lies at
/// q* = 0.9.
pub fn hydraulic_efficiency_ratio(flow_ratio: f64) -> f64 {
    let x = flow_ratio - 0.9;
    1.0 - 0.6 * x * x - 0.25 * x * x * x
}

/// Shut-off pressure coefficient ψ₀ = 2gH₀/u₂²: 1.31·e^(−0.3·nq/100) for diffuser pumps
/// (Gülich eq. 3.27) and 1.25·e^(−0.3·nq/100) for volute pumps (eq. 3.28). Vaneless
/// diffusers exchange less momentum with the impeller than vaned ones (Chap. 4.1.4):
/// they take the volute value.
pub fn shutoff_coefficient(nq: f64, collector: Collector) -> f64 {
    let factor = match collector {
        Collector::VanedDiffuser => 1.31,
        Collector::Volute | Collector::VanelessDiffuser => 1.25,
    };
    factor * (-0.3 * nq / 100.0).exp()
}

/// Flow ratio Q/Q_opt at which recirculation typically sets in: from 0.4 at nq = 10 to
/// 0.75 at nq = 300 (Gülich Chap. 5.2, p. 169), interpolated in ln nq.
pub fn recirculation_onset(nq: f64) -> f64 {
    let t = (nq.max(1.0) / 10.0).ln() / 30f64.ln();
    (0.4 + 0.35 * t).clamp(0.4, 0.75)
}

/// Default largest flow ratio: the range the statistics of eq. 4.14 cover.
pub const MAX_FLOW_RATIO: f64 = 1.5;

/// The computed part of the characteristic, where no recirculation disturbs it.
struct Model {
    flow: f64,
    head: f64,
    density: f64,
    u2: f64,
    area2: f64,
    slip: f64,
    blockage: f64,
    tan_beta2b: f64,
    leakage: f64,
    peak_hydraulic_efficiency: f64,
    secondary_power: f64,
}

impl Model {
    /// Euler head for flow `impeller_flow` through the impeller, swirl-free inflow:
    /// c₂u = u₂·γ − c₂m·τ₂/tan β₂B (eq. T3.2.7) and H_th = u₂·c₂u/g.
    fn theoretical_head(&self, impeller_flow: f64) -> f64 {
        let cm2 = impeller_flow / self.area2;
        let cu2 = self.u2 * self.slip - cm2 * self.blockage / self.tan_beta2b;
        self.u2 * cu2 / G
    }

    fn point(&self, flow_ratio: f64) -> PerformancePoint {
        let flow = flow_ratio * self.flow;
        let hydraulic = self.peak_hydraulic_efficiency * hydraulic_efficiency_ratio(flow_ratio);
        // Seal leakage follows the square root of the pressure across the seal.
        let mut leakage = self.leakage;
        let (mut theoretical, mut head) = (0.0, 0.0);
        for _ in 0..4 {
            theoretical = self.theoretical_head(flow + leakage);
            head = hydraulic * theoretical;
            leakage = self.leakage * (head.max(0.0) / self.head).sqrt();
        }
        let power = self.density * G * theoretical * (flow + leakage) + self.secondary_power;
        PerformancePoint {
            flow,
            head,
            theoretical_head: theoretical,
            power,
            efficiency: self.density * G * head * flow / power,
            hydraulic_efficiency: hydraulic,
            estimated: false,
        }
    }
}

/// Predicts the characteristic of one stage from its design point.
pub fn predict(
    duty: &PumpDuty,
    design: &CentrifugalPumpDesign,
    input: &PerformanceInput,
) -> Result<PumpPerformance, DesignError> {
    let outlet = &design.outlet;
    let tan_beta2b = outlet.blade_angle.tan();
    if !(duty.flow > 0.0 && duty.head > 0.0 && tan_beta2b > 0.0) {
        return Err(DesignError::Infeasible(
            "the performance prediction needs a positive flow, head and outlet blade angle".into(),
        ));
    }
    let eta = &design.efficiencies;
    let model = Model {
        flow: duty.flow,
        head: duty.head,
        density: duty.density,
        u2: outlet.triangle.u,
        area2: PI * design.outlet_diameter * design.outlet_width,
        slip: outlet.slip_factor,
        blockage: outlet.blockage,
        tan_beta2b,
        leakage: (design.impeller_flow - duty.flow).max(0.0),
        peak_hydraulic_efficiency: eta.hydraulic / hydraulic_efficiency_ratio(1.0),
        // Disk friction and mechanical losses: what the design power holds beyond
        // the Euler power (Chap. 4.1.2).
        secondary_power: (design.stage_power
            - duty.density * G * design.theoretical_head * design.impeller_flow)
            .max(0.0),
    };

    let onset = input
        .recirculation_onset
        .resolve(|| recirculation_onset(design.nq));
    let max_ratio = input.max_flow_ratio.resolve(|| MAX_FLOW_RATIO);
    if !(0.05..1.0).contains(&onset) {
        return Err(DesignError::Infeasible(format!(
            "the onset of recirculation must lie between 0.05 and 1 Q_opt, got {onset:.2}"
        )));
    }
    if !(1.05..=3.0).contains(&max_ratio) {
        return Err(DesignError::Infeasible(format!(
            "the largest flow must lie between 1.05 and 3 Q_opt, got {max_ratio:.2}"
        )));
    }
    let psi0 = input
        .shutoff_coefficient
        .resolve(|| shutoff_coefficient(design.nq, input.collector));
    let shutoff_head = psi0 * model.u2 * model.u2 / (2.0 * G);

    // The computed range ends where the head reaches zero, if before the largest flow.
    let end = if model.point(max_ratio).head > 0.0 {
        max_ratio
    } else {
        let mut lo = 1.0;
        let mut hi = max_ratio;
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if model.point(mid).head > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    };

    // Recirculation range: head toward the shut-off head with a cubic that leaves
    // shut-off flat and meets the computed curve; power continued along the tangent of
    // the computed curve, or toward a given shut-off power.
    let step = 1e-4;
    let at_onset = model.point(onset);
    let slope_head = (model.point(onset + step).head - at_onset.head) / step;
    let slope_power = (model.point(onset + step).power - at_onset.power) / step;
    // End slope in units of the interval, limited so the cubic adds no bump of its own
    // between the two heads (Fritsch and Carlson's monotonicity limit).
    let rise = at_onset.head - shutoff_head;
    let mut end_slope = onset * slope_head;
    if rise < 0.0 && end_slope < 3.0 * rise {
        end_slope = 3.0 * rise;
    }
    let power_at_zero = input
        .shutoff_power
        .resolve(|| (at_onset.power - slope_power * onset).max(model.secondary_power));
    let estimate = |ratio: f64| -> PerformancePoint {
        let t = ratio / onset;
        let (h00, h01, h11) = (
            2.0 * t.powi(3) - 3.0 * t * t + 1.0,
            -2.0 * t.powi(3) + 3.0 * t * t,
            t.powi(3) - t * t,
        );
        let head = h00 * shutoff_head + h01 * at_onset.head + h11 * end_slope;
        let power = power_at_zero + (at_onset.power - power_at_zero) * t;
        let flow = ratio * model.flow;
        PerformancePoint {
            flow,
            head,
            theoretical_head: f64::NAN,
            power,
            efficiency: duty.density * G * head * flow / power,
            hydraulic_efficiency: f64::NAN,
            estimated: true,
        }
    };

    let n = input.points.max(11);
    let mut points: Vec<PerformancePoint> = (0..n)
        .map(|i| end * i as f64 / (n - 1) as f64)
        .map(|r| {
            if r < onset {
                estimate(r)
            } else {
                model.point(r)
            }
        })
        .collect();
    // The onset itself, so the two parts meet in the plots.
    let index = points.partition_point(|p| p.flow < at_onset.flow);
    points.insert(index, at_onset);

    // Highest efficiency, by golden-section search in the computed range.
    let (mut a, mut b) = (onset.max(0.3), end);
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..80 {
        let c = b - ratio * (b - a);
        let d = a + ratio * (b - a);
        if model.point(c).efficiency > model.point(d).efficiency {
            b = d;
        } else {
            a = c;
        }
    }
    let best = model.point(0.5 * (a + b));
    let design_point = model.point(1.0);
    let shutoff = estimate(0.0);

    let mut warnings = Vec::new();
    let rises = points
        .windows(2)
        .any(|w| w[1].head > w[0].head + 1e-6 * duty.head);
    if rises {
        warnings.push(
            "the head rises with flow somewhere below the design point: the Q-H curve may be \
             unstable (Gülich Chap. 4 and 5)"
                .into(),
        );
    }
    if (best.flow / duty.flow - 1.0).abs() > 0.1 {
        warnings.push(format!(
            "the predicted best efficiency is at {:.0} % of the design flow",
            100.0 * best.flow / duty.flow
        ));
    }
    if end < max_ratio {
        warnings.push(format!(
            "the head reaches zero at {:.0} % of the design flow",
            100.0 * end
        ));
    }

    Ok(PumpPerformance {
        points,
        design: design_point,
        best,
        shutoff_head,
        shutoff_power: shutoff.power,
        shutoff_coefficient: psi0,
        recirculation_flow: onset * duty.flow,
        max_flow: end * duty.flow,
        npsh_required: design.npsh_required,
        warnings,
    })
}

/// Correlations behind the prediction.
pub const PERFORMANCE_CORRELATIONS: &[crate::pump::correlations::CorrelationInfo] = &[
    crate::pump::correlations::CorrelationInfo {
        id: "performance.hydraulic_efficiency",
        description: "η_h/η_h,max = 1 − 0.6·(q* − 0.9)² − 0.25·(q* − 0.9)³",
        reference: "Gülich 2nd ed., eq. 4.14",
        verification: crate::pump::correlations::Verification::Verified,
    },
    crate::pump::correlations::CorrelationInfo {
        id: "performance.shutoff_head",
        description: "ψ₀ = 1.31·e^(−0.3·nq/100) diffuser pumps, 1.25·e^(−0.3·nq/100) volutes",
        reference: "Gülich 2nd ed., eqs. 3.27 and 3.28",
        verification: crate::pump::correlations::Verification::Verified,
    },
    crate::pump::correlations::CorrelationInfo {
        id: "performance.recirculation_onset",
        description: "q*_RB from 0.4 (nq = 10) to 0.75 (nq = 300), interpolated in ln nq",
        reference: "range of Gülich Chap. 5.2; the interpolation is our choice",
        verification: crate::pump::correlations::Verification::EngineeringDefault,
    },
    crate::pump::correlations::CorrelationInfo {
        id: "performance.secondary_losses",
        description: "disk friction and mechanical losses constant with flow; seal leakage \
                      proportional to √H",
        reference: "Gülich 2nd ed., Chap. 4.1.2; the leakage law is the orifice law",
        verification: crate::pump::correlations::Verification::EngineeringDefault,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pump::{CentrifugalPumpInput, design};
    use approx::assert_relative_eq;

    fn duty() -> PumpDuty {
        PumpDuty {
            flow: 100.0 / 3600.0,
            head: 32.0,
            machine_head: 32.0,
            stages: 1,
            speed: 2900.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
        }
    }

    fn performance(input: &PerformanceInput) -> (CentrifugalPumpDesign, PumpPerformance) {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let p = predict(&duty(), &d, input).unwrap();
        (d, p)
    }

    #[test]
    fn hydraulic_efficiency_peaks_at_nine_tenths() {
        assert_relative_eq!(hydraulic_efficiency_ratio(0.9), 1.0);
        assert!(hydraulic_efficiency_ratio(0.8) < 1.0);
        assert!(hydraulic_efficiency_ratio(1.0) < 1.0);
        // The cubic term makes overload worse than part load.
        assert!(hydraulic_efficiency_ratio(1.3) < hydraulic_efficiency_ratio(0.5));
    }

    #[test]
    fn shutoff_coefficients_follow_the_collector() {
        let volute = shutoff_coefficient(36.0, Collector::Volute);
        assert_relative_eq!(volute, 1.25 * (-0.108f64).exp(), max_relative = 1e-12);
        assert!(shutoff_coefficient(36.0, Collector::VanedDiffuser) > volute);
        assert_eq!(
            shutoff_coefficient(36.0, Collector::VanelessDiffuser),
            volute
        );
    }

    #[test]
    fn recirculation_onset_rises_with_specific_speed() {
        assert_relative_eq!(recirculation_onset(10.0), 0.4);
        assert_relative_eq!(recirculation_onset(300.0), 0.75, max_relative = 1e-12);
        assert!(recirculation_onset(36.0) > 0.4 && recirculation_onset(36.0) < 0.75);
    }

    #[test]
    fn the_curve_passes_through_the_design_point() {
        let (d, p) = performance(&PerformanceInput::default());
        let q = duty().flow;
        assert_relative_eq!(p.design.flow, q, max_relative = 1e-12);
        assert_relative_eq!(p.design.head, 32.0, max_relative = 1e-9);
        assert_relative_eq!(p.design.power, d.stage_power, max_relative = 1e-9);
        assert_relative_eq!(
            p.design.efficiency,
            d.efficiencies.overall,
            max_relative = 1e-9
        );
        // The best efficiency lies near the design flow.
        assert!((p.best.flow / q - 1.0).abs() < 0.1, "{}", p.best.flow / q);
        assert!(p.best.efficiency >= p.design.efficiency);
    }

    #[test]
    fn a_backswept_low_nq_pump_has_a_falling_head_and_rising_power() {
        let (_, p) = performance(&PerformanceInput::default());
        for w in p.points.windows(2) {
            assert!(w[1].head <= w[0].head, "{:?}", (w[0].head, w[1].head));
        }
        // Euler power is a parabola in flow: it rises to past the design point.
        let up_to = |q: f64| p.points.iter().filter(move |p| p.flow <= q * duty().flow);
        let powers: Vec<f64> = up_to(1.2).map(|p| p.power).collect();
        assert!(powers.windows(2).all(|w| w[1] > w[0]));
        // Shut-off: the head of eq. 3.28, flat, and no flow.
        let first = p.points[0];
        assert_eq!(first.flow, 0.0);
        assert!(first.estimated && first.efficiency == 0.0);
        assert_relative_eq!(first.head, p.shutoff_head, max_relative = 1e-12);
        assert!(p.shutoff_head > 32.0);
        assert!(p.shutoff_power > 0.0 && p.shutoff_power < p.design.power);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    }

    #[test]
    fn manual_settings_are_used() {
        let input = PerformanceInput {
            collector: Collector::VanedDiffuser,
            recirculation_onset: Param::Manual(0.6),
            shutoff_coefficient: Param::Manual(1.2),
            shutoff_power: Param::Manual(4000.0),
            max_flow_ratio: Param::Manual(1.3),
            points: 21,
        };
        let (d, p) = performance(&input);
        assert_relative_eq!(
            p.recirculation_flow,
            0.6 * duty().flow,
            max_relative = 1e-12
        );
        assert_relative_eq!(p.shutoff_coefficient, 1.2);
        let u2 = d.outlet.triangle.u;
        assert_relative_eq!(
            p.shutoff_head,
            1.2 * u2 * u2 / (2.0 * G),
            max_relative = 1e-12
        );
        assert_relative_eq!(p.max_flow, 1.3 * duty().flow, max_relative = 1e-12);
        assert_relative_eq!(p.shutoff_power, 4000.0);
        // 21 points plus the onset of recirculation.
        assert_eq!(p.points.len(), 22);
        assert!(p.points.windows(2).all(|w| w[1].flow >= w[0].flow));
    }

    #[test]
    fn a_low_shutoff_head_is_flagged_as_unstable() {
        let input = PerformanceInput {
            shutoff_coefficient: Param::Manual(0.5),
            ..PerformanceInput::default()
        };
        let (_, p) = performance(&input);
        assert!(p.warnings.iter().any(|w| w.contains("unstable")));
    }
}
