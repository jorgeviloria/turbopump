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
//!    extrapolated. These points are marked as estimates;
//! 5. with a volute after the impeller, its losses against flow come from the model of
//!    [`crate::volute::loss`]. The statistics of eq. 4.14 describe pumps with well
//!    sized volutes, so at the design point the hydraulic losses are split between an
//!    equivalent matched volute (λ₆·AR = 1) and the rest of the stage; the rest keeps
//!    the statistical shape and the actual volute adds its own losses (our split). A
//!    mismatched volute then loses head at the design point too;
//! 6. the required NPSH₃ against flow after Gülich Chap. 6.3.3 ([`crate::npsh`]);
//! 7. the steady radial force on the impeller after Gülich Table 9.4
//!    ([`crate::radial_thrust`]).

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use tp_units::STANDARD_GRAVITY as G;

use crate::axial_thrust::{AxialForces, AxialThrust, AxialThrustInput};
use crate::npsh::{CollectorThroat, NpshCurve, NpshSettings};
use crate::pump::{CentrifugalPumpDesign, PumpDuty};
use crate::radial_thrust::{RadialCollector, RadialThrust, default_shroud_thickness};
use crate::volute::loss::{VoluteLoss, VoluteLossGeometry, volute_loss};
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
    /// λ_c of the NPSH₃ (Gülich eq. 6.10); automatic: 1.1 for axial inflow.
    pub npsh_inlet_coefficient: Param<f64>,
    /// λ_w at the design point; automatic: Gülich Fig. 6.20.
    pub npsh_blade_coefficient: Param<f64>,
    /// Exponent of the steep rise of NPSH₃ (eq. 6.22); automatic: 2.5.
    pub npsh_exponent: Param<f64>,
    /// Seal clearances twice those of Gülich eq. 3.12, which raise the radial force
    /// (curve 2 of Fig. 9.27) and the leakage that sets the axial force.
    pub wide_seal_clearances: bool,
    /// Seals and balance holes for the axial force.
    pub axial: AxialThrustInput,
}

/// What the prediction knows about the collector after the impeller, beyond its kind.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CollectorData {
    /// Its throat, which sets the knee of the NPSH₃ curve.
    pub throat: Option<CollectorThroat>,
    /// A volute after the impeller, directly or behind a vaneless diffuser.
    pub volute: Option<VoluteLossGeometry>,
    /// Wrap angle of the inner volute when the volute is a double one, rad.
    pub double_volute_wrap: Option<f64>,
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
            npsh_inlet_coefficient: Param::Auto,
            npsh_blade_coefficient: Param::Auto,
            npsh_exponent: Param::Auto,
            wide_seal_clearances: false,
            axial: AxialThrustInput::default(),
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
    /// Required NPSH₃, m.
    pub npsh_required: f64,
    /// Head lost in the volute, m; NaN without a volute.
    pub volute_loss: f64,
    /// Steady radial force on the impeller, N; NaN without data for the collector.
    pub radial_force: f64,
    /// Axial force on the impeller, N, positive toward the suction side.
    pub axial_force: f64,
}

/// Axial force on the impeller.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AxialSummary {
    pub law: AxialThrust,
    /// Its parts at the design point and at shut-off.
    pub design: AxialForces,
    pub shutoff: AxialForces,
    /// Largest magnitude over the curve, N.
    pub largest: f64,
    /// The rough estimate of Gülich eq. T9.2.12 at the design point, N.
    pub estimate: (f64, f64),
}

/// Radial force on the impeller.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RadialSummary {
    pub law: RadialThrust,
    /// Steady force at the design point and at shut-off, N.
    pub design_force: f64,
    pub shutoff_force: f64,
    /// Largest steady force over the curve, N.
    pub largest_force: f64,
    /// Largest steady plus unsteady force for the mechanical design, with
    /// k_R,tot ≥ 0.15 (Gülich Table 9.4, notes 12 and 13), N.
    pub design_load: f64,
}

/// The volute after the impeller at the design point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VoluteSummary {
    pub design: VoluteLoss,
    /// Fraction of the hydraulic losses of the design point lost in the volute.
    pub share: f64,
    /// Flow at which the spiral turns from accelerating to decelerating
    /// (λ₆·AR = 1), where its losses are least, m³/s.
    pub matched_flow: Option<f64>,
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
    /// NPSH₃ at the design point, m.
    pub npsh_required: f64,
    /// NPSH available at the inlet, m.
    pub npsh_available: f64,
    /// Smallest flow at which NPSH₃ reaches the NPSH available, m³/s.
    pub cavitation_flow: Option<f64>,
    /// The NPSH₃ curve and its characteristic flows.
    pub npsh: NpshCurve,
    /// Losses of the volute after the impeller, if one follows it.
    pub volute: Option<VoluteSummary>,
    /// Radial force, when Table 9.4 covers the collector.
    pub radial: Option<RadialSummary>,
    pub axial: AxialSummary,
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
    impeller_radius: f64,
    volute: Option<VoluteSplit>,
}

/// The hydraulic losses split between the volute and the rest of the stage.
struct VoluteSplit {
    geometry: VoluteLossGeometry,
    /// Hydraulic losses of the design point, (1 − η_h)·H_th, m.
    design_losses: f64,
    /// The part of them not lost in a matched volute, m.
    design_rest: f64,
}

impl Model {
    /// Euler head for flow `impeller_flow` through the impeller, swirl-free inflow:
    /// c₂u = u₂·γ − c₂m·τ₂/tan β₂B (eq. T3.2.7) and H_th = u₂·c₂u/g.
    fn theoretical_head(&self, impeller_flow: f64) -> f64 {
        let cm2 = impeller_flow / self.area2;
        let cu2 = self.u2 * self.slip - cm2 * self.blockage / self.tan_beta2b;
        self.u2 * cu2 / G
    }

    /// Losses of the volute for pump flow `flow` and Euler head `theoretical`: the
    /// swirl c₂u reaches the volute inlet as a free vortex.
    fn volute_loss(&self, flow: f64, theoretical: f64) -> Option<VoluteLoss> {
        let v = self.volute.as_ref()?;
        let cu2 = G * theoretical / self.u2;
        let swirl = cu2 * self.impeller_radius / v.geometry.inlet_radius;
        Some(volute_loss(&v.geometry, flow, swirl))
    }

    fn point(&self, flow_ratio: f64) -> PerformancePoint {
        let flow = flow_ratio * self.flow;
        let hydraulic = self.peak_hydraulic_efficiency * hydraulic_efficiency_ratio(flow_ratio);
        // Seal leakage follows the square root of the pressure across the seal.
        let mut leakage = self.leakage;
        let (mut theoretical, mut head, mut volute) = (0.0, 0.0, f64::NAN);
        for _ in 0..4 {
            theoretical = self.theoretical_head(flow + leakage);
            head = match (&self.volute, self.volute_loss(flow, theoretical)) {
                (Some(split), Some(loss)) => {
                    volute = loss.total();
                    let rest =
                        split.design_rest * (1.0 - hydraulic) * theoretical / split.design_losses;
                    theoretical - volute - rest
                }
                _ => hydraulic * theoretical,
            };
            leakage = self.leakage * (head.max(0.0) / self.head).sqrt();
        }
        let power = self.density * G * theoretical * (flow + leakage) + self.secondary_power;
        PerformancePoint {
            flow,
            head,
            theoretical_head: theoretical,
            power,
            efficiency: self.density * G * head * flow / power,
            hydraulic_efficiency: head / theoretical,
            estimated: false,
            npsh_required: f64::NAN,
            volute_loss: volute,
            radial_force: f64::NAN,
            axial_force: f64::NAN,
        }
    }
}

/// Predicts the characteristic of one stage from its design point.
pub fn predict(
    duty: &PumpDuty,
    design: &CentrifugalPumpDesign,
    input: &PerformanceInput,
    collector: &CollectorData,
) -> Result<PumpPerformance, DesignError> {
    let outlet = &design.outlet;
    let tan_beta2b = outlet.blade_angle.tan();
    if !(duty.flow > 0.0 && duty.head > 0.0 && tan_beta2b > 0.0) {
        return Err(DesignError::Infeasible(
            "the performance prediction needs a positive flow, head and outlet blade angle".into(),
        ));
    }
    let eta = &design.efficiencies;
    let mut warnings = Vec::new();
    let mut model = Model {
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
        impeller_radius: design.outlet_diameter / 2.0,
        volute: None,
    };
    if let Some(geometry) = collector.volute {
        let theoretical = model.theoretical_head(design.impeller_flow);
        let design_losses = (1.0 - eta.hydraulic) * theoretical;
        model.volute = Some(VoluteSplit {
            geometry,
            design_losses,
            design_rest: design_losses,
        });
        // The matched volute: the same one with the throat that makes λ₆·AR = 1 at
        // the design point (the matching grows with the throat area).
        let actual = model
            .volute_loss(duty.flow, theoretical)
            .expect("the split has a volute");
        let matched = VoluteLossGeometry {
            throat_area: geometry.throat_area / actual.matching,
            ..geometry
        };
        let swirl = G * theoretical / model.u2 * model.impeller_radius / geometry.inlet_radius;
        let reference = volute_loss(&matched, duty.flow, swirl).total();
        if reference > design_losses {
            warnings.push(format!(
                "the volute loses {reference:.2} m at the design point, more than the \
                 {design_losses:.2} m of all the hydraulic losses: check the volute"
            ));
        }
        if let Some(split) = &mut model.volute {
            split.design_rest = (design_losses - reference).max(0.0);
        }
    }
    let npsh = NpshCurve::new(
        design,
        duty.flow,
        collector.throat,
        &NpshSettings {
            inlet_coefficient: input
                .npsh_inlet_coefficient
                .resolve(|| crate::npsh::AXIAL_INLET_COEFFICIENT),
            blade_coefficient: input.npsh_blade_coefficient.manual_value(),
            exponent: input
                .npsh_exponent
                .resolve(|| crate::npsh::DEFAULT_EXPONENT),
        },
    );

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
    for (name, value, range) in [
        (
            "λ_c of the NPSH₃",
            input.npsh_inlet_coefficient.manual_value(),
            0.5..=3.0,
        ),
        (
            "λ_w of the NPSH₃",
            input.npsh_blade_coefficient.manual_value(),
            0.0..=3.0,
        ),
        (
            "NPSH₃ exponent",
            input.npsh_exponent.manual_value(),
            1.0..=6.0,
        ),
    ] {
        if let Some(v) = value
            && !range.contains(&v)
        {
            return Err(DesignError::Infeasible(format!(
                "the {name} must lie between {} and {}, got {v}",
                range.start(),
                range.end()
            )));
        }
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
            npsh_required: f64::NAN,
            volute_loss: model
                .volute_loss(flow, model.theoretical_head(flow + model.leakage))
                .map_or(f64::NAN, |l| l.total()),
            radial_force: f64::NAN,
            axial_force: f64::NAN,
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
    // Radial force: the volute decides it, also behind a vaneless diffuser.
    let volute_kind = match collector.double_volute_wrap {
        Some(wrap) => RadialCollector::DoubleVolute { wrap },
        None => RadialCollector::SingleVolute,
    };
    let radial_collector = match input.collector {
        Collector::Volute => Some(volute_kind),
        Collector::VanedDiffuser => Some(RadialCollector::VanedDiffuser),
        Collector::VanelessDiffuser => collector.volute.map(|_| volute_kind),
    };
    let radial = radial_collector.map(|kind| {
        let d2 = design.outlet_diameter;
        let b2tot = design.outlet_width + 2.0 * default_shroud_thickness(d2);
        RadialThrust::new(
            kind,
            design.nq * design.eyes.sqrt(),
            input.wide_seal_clearances,
            duty.density * G * duty.head * d2 * b2tot,
            b2tot,
        )
    });
    let axial = AxialThrust::new(duty, design, &input.axial, input.wide_seal_clearances)?;
    // In the recirculation range the Euler head comes from the model without it.
    let axial_forces = |p: &PerformancePoint| {
        let theoretical = if p.theoretical_head.is_finite() {
            p.theoretical_head
        } else {
            model.theoretical_head(p.flow + model.leakage)
        };
        axial.forces(p.flow, p.head, theoretical)
    };
    let with_npsh = |mut p: PerformancePoint| {
        p.npsh_required = npsh.at(p.flow);
        if let Some(law) = &radial {
            p.radial_force = law.force(p.flow / duty.flow, p.head, duty.head);
        }
        p.axial_force = axial_forces(&p).total;
        p
    };
    for p in &mut points {
        *p = with_npsh(*p);
    }

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
    let best = with_npsh(model.point(0.5 * (a + b)));
    let design_point = with_npsh(model.point(1.0));
    let shutoff = with_npsh(estimate(0.0));
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

    let volute = model.volute.as_ref().map(|split| {
        let design = model
            .volute_loss(duty.flow, design_point.theoretical_head)
            .expect("the split has a volute");
        // The spiral decelerates below the matched flow and accelerates above it.
        let matching = |ratio: f64| {
            let p = model.point(ratio);
            model
                .volute_loss(p.flow, p.theoretical_head)
                .map_or(f64::NAN, |l| l.matching)
        };
        let matched_flow = (matching(0.2) > 1.0 && matching(end) < 1.0).then(|| {
            let (mut lo, mut hi) = (0.2, end);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if matching(mid) > 1.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            0.5 * (lo + hi) * duty.flow
        });
        VoluteSummary {
            design,
            share: design.total() / split.design_losses,
            matched_flow,
        }
    });
    if let Some(q) = volute.and_then(|v| v.matched_flow)
        && (q / duty.flow - 1.0).abs() > 0.15
    {
        warnings.push(format!(
            "the volute has its least loss at {:.0} % of the design flow",
            100.0 * q / duty.flow
        ));
    }
    let radial = radial.map(|law| {
        let largest_force = points.iter().map(|p| p.radial_force).fold(0.0, f64::max);
        let design_load = points
            .iter()
            .map(|p| {
                law.design_coefficient(p.flow / duty.flow) * law.reference_force * p.head
                    / duty.head
            })
            .fold(0.0, f64::max);
        RadialSummary {
            law,
            design_force: design_point.radial_force,
            shutoff_force: shutoff.radial_force,
            largest_force,
            design_load,
        }
    });
    let axial = AxialSummary {
        law: axial,
        design: axial_forces(&design_point),
        shutoff: axial_forces(&shutoff),
        largest: points
            .iter()
            .map(|p| p.axial_force.abs())
            .fold(0.0, f64::max),
        estimate: axial.estimate(design_point.head),
    };
    // Without balance holes the shrouds push toward the suction side; a reversed force
    // comes from a high suction pressure.
    if axial.design.total < 0.0 && axial.law.eyes < 1.5 && axial.law.rear_seal_diameter.is_none() {
        warnings.push(format!(
            "the axial force points to the pressure side ({:.0} N at design): the suction \
             pressure on the shaft outweighs the shrouds (Gülich Chap. 9.2.2)",
            axial.design.total
        ));
    }
    let cavitation_flow = npsh.cavitation_flow(design.npsh_available, end * duty.flow);
    if let Some(q) = cavitation_flow {
        warnings.push(format!(
            "NPSH₃ exceeds the NPSH available of {:.2} m above {:.0} % of the design flow",
            design.npsh_available,
            100.0 * q / duty.flow
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
        npsh_required: npsh.design_npsh,
        npsh_available: design.npsh_available,
        cavitation_flow,
        npsh,
        volute,
        radial,
        axial,
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
    crate::pump::correlations::CorrelationInfo {
        id: "performance.axial_force",
        description: "F = π·r₂²·{(1 − x²)·Δp_La − ρ/4·u₂²·k²·(1 − x²)²} on each shroud, \
                      Δp_La = ρ·g·H·(1 − g·H_th/(2u₂²)), F_ax = F_Hy − F_I + F_w",
        reference: "Gülich 2nd ed., Table 9.2, eqs. T9.2.2, T9.2.3, T9.2.8 to T9.2.11",
        verification: crate::pump::correlations::Verification::Verified,
    },
    crate::pump::correlations::CorrelationInfo {
        id: "performance.sidewall_rotation",
        description: "k_o = 0.5 for open sidewall gaps; k = k_o·exp{300·φ_sp·(r₂/r_sp − 1)} \
                      with φ_sp = 5.5·10⁻⁴·ψ^1.5·(nq/20)^0.4",
        reference: "Gülich 2nd ed., Table 9.1, eqs. T9.1.2a to T9.1.4",
        verification: crate::pump::correlations::Verification::Verified,
    },
    crate::pump::correlations::CorrelationInfo {
        id: "performance.axial_balance",
        description: "balance holes leave 0.15 of the force; double entry f_ax = 0.015",
        reference: "middle of the ranges of Gülich eqs. T9.2.14 and T9.2.15",
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
        let p = predict(&duty(), &d, input, &CollectorData::default()).unwrap();
        (d, p)
    }

    #[test]
    fn axial_force_of_an_overhung_impeller_follows_table_9_2() {
        let (d, p) = performance(&PerformanceInput::default());
        let a = &p.axial;
        let f = &a.design;
        // Default seals: the eye plus the shroud on both sides, the hub at the rear.
        assert_relative_eq!(
            a.law.seal_diameter,
            d.inlet_diameter + 2.0 * default_shroud_thickness(d.outlet_diameter)
        );
        assert_relative_eq!(a.law.shaft_seal_diameter, d.hub_diameter);
        // Δp_La = ρ·g·H·(1 − g·H_th/(2u₂²)) (eq. T9.2.2).
        let u2 = a.law.tip_speed;
        assert_relative_eq!(
            f.pressure_rise,
            998.2 * G * p.design.head * (1.0 - G * p.design.theoretical_head / (2.0 * u2 * u2)),
            max_relative = 1e-12
        );
        // The leakage speeds up the liquid on the front shroud only.
        assert!(f.front_rotation > f.rear_rotation);
        assert_relative_eq!(f.rear_rotation, 0.5);
        assert_relative_eq!(f.hydraulic, f.rear - f.front, max_relative = 1e-12);
        assert_relative_eq!(
            f.total,
            f.hydraulic - f.momentum + f.shaft,
            max_relative = 1e-12
        );
        // Toward the suction side, a little below the rough estimate of eq. T9.2.12.
        assert!(f.total > 0.0);
        assert!(f.hydraulic > 0.8 * a.estimate.0 && f.hydraulic < a.estimate.1);
        assert_relative_eq!(p.design.axial_force, f.total, max_relative = 1e-12);
        assert!(a.largest >= f.total && a.largest >= a.shutoff.total.abs());
        assert!(p.points.iter().all(|q| q.axial_force.is_finite()));
    }

    #[test]
    fn balance_holes_seals_and_suction_pressure_change_the_axial_force() {
        let plain = performance(&PerformanceInput::default()).1.axial.design;
        let force = |edit: &dyn Fn(&mut PerformanceInput)| {
            let mut input = PerformanceInput::default();
            edit(&mut input);
            performance(&input).1
        };
        // Balance holes leave 15 % of the force on the shrouds (eq. T9.2.14).
        let holes = force(&|i| i.axial.balance_holes = true);
        assert_relative_eq!(
            holes.axial.design.hydraulic,
            0.15 * plain.hydraulic,
            max_relative = 1e-9
        );
        assert!(!holes.warnings.iter().any(|w| w.contains("pressure side")));
        // A larger rear seal pushes toward the pressure side (eq. T9.2.13).
        let wide_rear = force(&|i| {
            i.axial.balance_holes = true;
            i.axial.rear_seal_diameter = Param::Manual(0.1);
        });
        assert!(wide_rear.axial.design.hydraulic < holes.axial.design.hydraulic);
        // Wider clearances leak more and lower the force on the front shroud.
        let wide = force(&|i| i.wide_seal_clearances = true);
        assert!(wide.axial.design.front_rotation > plain.front_rotation);
        assert!(wide.axial.design.total > plain.total);
        // A high suction pressure on the shaft can reverse the force.
        let mut duty = duty();
        duty.inlet_total_pressure = 40e5;
        let d = design(&duty, &CentrifugalPumpInput::default()).unwrap();
        let p = predict(
            &duty,
            &d,
            &PerformanceInput::default(),
            &CollectorData::default(),
        )
        .unwrap();
        assert!(p.axial.design.shaft < -1000.0);
        assert!(p.axial.design.total < 0.0);
        assert!(p.warnings.iter().any(|w| w.contains("pressure side")));
        // Impossible seals.
        let mut input = PerformanceInput::default();
        input.axial.seal_diameter = Param::Manual(0.5);
        let d = design(&self::duty(), &CentrifugalPumpInput::default()).unwrap();
        assert!(predict(&self::duty(), &d, &input, &CollectorData::default()).is_err());
    }

    #[test]
    fn a_double_entry_impeller_keeps_a_small_residual() {
        let pump = CentrifugalPumpInput {
            suction: crate::pump::Suction::Double,
            ..CentrifugalPumpInput::default()
        };
        let d = design(&duty(), &pump).unwrap();
        let p = predict(
            &duty(),
            &d,
            &PerformanceInput::default(),
            &CollectorData::default(),
        )
        .unwrap();
        let a = &p.axial;
        let u2 = a.law.tip_speed;
        // Eq. T9.2.15 with f_ax = 0.015.
        assert_relative_eq!(
            a.design.total,
            0.015 * 998.2 / 2.0
                * u2
                * u2
                * (d.outlet_diameter.powi(2) - a.law.seal_diameter.powi(2)),
            max_relative = 1e-12
        );
        let single = performance(&PerformanceInput::default())
            .1
            .axial
            .design
            .total;
        assert!(a.design.total < 0.5 * single);
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
        // Only the NPSH₃ at the end of the curve passes the 10.1 m available.
        assert!(
            p.warnings.iter().all(|w| w.starts_with("NPSH₃ exceeds")),
            "{:?}",
            p.warnings
        );
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
            npsh_inlet_coefficient: Param::Manual(1.3),
            npsh_blade_coefficient: Param::Manual(0.2),
            npsh_exponent: Param::Manual(3.0),
            wide_seal_clearances: false,
            axial: AxialThrustInput::default(),
        };
        let (d, p) = performance(&input);
        assert_relative_eq!(p.npsh.inlet_coefficient, 1.3);
        assert_relative_eq!(p.npsh.blade_coefficient, 0.2);
        assert_relative_eq!(p.npsh.exponent, 3.0);
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

    /// The reference volute, sized by the automatic steps, scaled in throat area.
    fn volute_collector(d: &CentrifugalPumpDesign, throat_scale: f64) -> CollectorData {
        use crate::outlet::ImpellerOutlet;
        use crate::volute::{VoluteDesign, VoluteInput};
        let outlet = ImpellerOutlet::from_pump(d, &duty(), 0.0, 1.0);
        let volute = VoluteDesign::new(&outlet, &VoluteInput::default()).unwrap();
        let mut geometry = VoluteLossGeometry::from_design(&volute);
        geometry.throat_area *= throat_scale;
        CollectorData {
            throat: Some(CollectorThroat {
                count: 1,
                area: geometry.throat_area,
            }),
            volute: Some(geometry),
            double_volute_wrap: None,
        }
    }

    fn with_collector(collector: &CollectorData) -> PumpPerformance {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        predict(&duty(), &d, &PerformanceInput::default(), collector).unwrap()
    }

    #[test]
    fn a_matched_volute_takes_a_share_of_the_losses_at_the_design_head() {
        let (_, plain) = performance(&PerformanceInput::default());
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let p = with_collector(&volute_collector(&d, 1.0));
        // Sized by angular momentum, the volute is nearly the matched one of the split.
        assert_relative_eq!(p.design.head, 32.0, max_relative = 5e-3);
        let v = p.volute.unwrap();
        assert!((0.1..0.9).contains(&v.share), "{}", v.share);
        assert_relative_eq!(p.design.volute_loss, v.design.total(), max_relative = 1e-9);
        // Sized by angular momentum, the spiral is matched near the design flow.
        let matched = v.matched_flow.unwrap() / duty().flow;
        assert!((0.85..1.15).contains(&matched), "{matched}");
        // Away from the design point the curve stays close to the statistical one.
        for (a, b) in p.points.iter().zip(&plain.points) {
            assert_relative_eq!(a.head, b.head, max_relative = 0.08);
        }
        // Volute losses grow toward overload.
        let last = p.points.last().unwrap();
        assert!(last.volute_loss > p.design.volute_loss);
    }

    #[test]
    fn a_small_throat_moves_the_best_point_to_lower_flow() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let matched = with_collector(&volute_collector(&d, 1.0));
        let small = with_collector(&volute_collector(&d, 0.7));
        assert!(small.best.flow < matched.best.flow);
        // It no longer reaches the design head.
        assert!(small.design.head < matched.design.head - 0.5);
        assert!(
            small.volute.unwrap().matched_flow.unwrap()
                < matched.volute.unwrap().matched_flow.unwrap()
        );
        // At overload the narrow throat loses more head.
        let last = |p: &PumpPerformance| *p.points.last().unwrap();
        assert!(last(&small).head < last(&matched).head);
    }

    #[test]
    fn every_point_carries_its_npsh() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let p = with_collector(&volute_collector(&d, 1.0));
        assert_relative_eq!(p.design.npsh_required, p.npsh.design_npsh);
        assert_relative_eq!(p.npsh_required, p.npsh.design_npsh);
        assert!(p.points.iter().all(|q| q.npsh_required.is_finite()));
        assert!(p.npsh.collector_flow.is_some());
        // With 10.1 m available the reference pump cavitates past the steep rise, a
        // little before the end of the curve.
        let q = p.cavitation_flow.unwrap();
        assert!(
            (1.3..1.5).contains(&(q / duty().flow)),
            "{}",
            q / duty().flow
        );
        assert_relative_eq!(p.npsh.at(q), p.npsh_available, max_relative = 1e-6);
        assert!(p.warnings.iter().any(|w| w.starts_with("NPSH₃ exceeds")));
    }

    #[test]
    fn the_radial_force_follows_table_9_4() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let single = with_collector(&volute_collector(&d, 1.0));
        let r = single.radial.unwrap();
        assert_eq!(r.law.collector, RadialCollector::SingleVolute);
        // Least near the design point, largest against a closed valve.
        assert!(r.design_force > 0.0 && r.shutoff_force > 5.0 * r.design_force);
        let first = single.points.first().unwrap();
        assert_relative_eq!(first.radial_force, r.shutoff_force);
        assert!(r.design_load >= 0.15 * r.law.reference_force * 0.99);
        // F_R = k_R·ρ·g·H·d₂·b₂tot at the design point.
        let b2tot = d.outlet_width + 2.0 * default_shroud_thickness(d.outlet_diameter);
        let expected = r.law.best_coefficient
            * duty().density
            * G
            * single.design.head
            * d.outlet_diameter
            * b2tot;
        assert_relative_eq!(r.design_force, expected, max_relative = 1e-9);
        // A double volute of 180° cuts the force at shut-off to under a third.
        let mut double = volute_collector(&d, 1.0);
        double.double_volute_wrap = Some(PI);
        let double = with_collector(&double).radial.unwrap();
        assert!(double.shutoff_force < r.shutoff_force / 3.0);
        // Wider seal clearances raise it.
        let wide = predict(
            &duty(),
            &d,
            &PerformanceInput {
                wide_seal_clearances: true,
                ..PerformanceInput::default()
            },
            &volute_collector(&d, 1.0),
        )
        .unwrap();
        assert!(wide.radial.unwrap().shutoff_force > r.shutoff_force);
        // A vaneless diffuser with nothing after it has no data.
        let input = PerformanceInput {
            collector: Collector::VanelessDiffuser,
            ..PerformanceInput::default()
        };
        let p = predict(&duty(), &d, &input, &CollectorData::default()).unwrap();
        assert!(p.radial.is_none() && p.design.radial_force.is_nan());
    }

    #[test]
    fn manual_npsh_settings_are_checked() {
        let input = PerformanceInput {
            npsh_exponent: Param::Manual(0.2),
            ..PerformanceInput::default()
        };
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        assert!(predict(&duty(), &d, &input, &CollectorData::default()).is_err());
    }
}
