//! Performance curves of a centrifugal pump: head, power and efficiency against flow,
//! predicted from the design point.
//!
//! Procedure (see `docs/theory/prestaciones.md`), following Gülich 2nd ed., Chap. 4.1
//! and 4.3:
//! 1. theoretical head from the outlet velocity triangle (eqs. 4.1 and T3.2.7), with
//!    the slip factor of the design point times the measured trend γ/γ_opt of Fig. 4.15:
//!    the slip falls at part load, so H_th rises faster than the linear law;
//! 2. hydraulic efficiency against flow from the statistical eq. 4.14, so that
//!    H = η_h·H_th (eq. T3.3.7);
//! 3. power from Euler's equation plus secondary losses that do not depend on flow
//!    (Chap. 4.1.2, eq. T3.5.1);
//! 4. below the onset of recirculation the losses cannot be calculated: the head is
//!    interpolated toward the shut-off head of eqs. 3.27 and 3.28, and the power is
//!    extrapolated. These points are marked as estimates;
//! 5. with a volute after the impeller, its losses against flow come from the model of
//!    [`crate::volute::loss`]. The statistics of eq. 4.14 describe pumps with well
//!    sized volutes, losses of the volute included, so eq. 4.14 shapes all the
//!    hydraulic losses and the volute only adds what it loses beyond an equivalent
//!    matched volute (λ₆·AR = 1 at the design point) of the same shape (our split). A
//!    mismatched volute then loses head at the design point too and moves the best
//!    point. With the loss analysis the hydraulic losses of the design point are summed
//!    instead (Gülich Table 3.8): friction and inlet shock in the impeller, the shock
//!    where the flow widens into the volute and the volute itself; eq. 4.14 shapes them
//!    with the matched volute in place of the actual one. Without a volute the
//!    statistical η_h stays;
//! 6. the required NPSH₃ against flow after Gülich Chap. 6.3.3 ([`crate::npsh`]);
//! 7. the steady radial force on the impeller after Gülich Table 9.4
//!    ([`crate::radial_thrust`]).

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use tp_units::STANDARD_GRAVITY as G;

use crate::axial_thrust::{AxialForces, AxialThrust, AxialThrustInput, BalanceHoleSize};
use crate::losses::{ImpellerChannels, ImpellerLosses, LossInput, SecondaryLosses, outlet_shock};
use crate::npsh::{CollectorThroat, NpshCurve, NpshSettings};
use crate::operation::{Operation, OperationInput};
use crate::pump::{CentrifugalPumpDesign, PumpDuty};
use crate::radial_thrust::{RadialCollector, RadialThrust, default_shroud_thickness};
use crate::viscous::{
    CHANNEL_ROUGHNESS, VISCOSITY_TOLERANCE, ViscousCorrection, WATER_VISCOSITY, WaterBaseline,
};
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
    /// Shaft power at shut-off, W; automatic: the power coefficient of
    /// [`shutoff_power_coefficient`].
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
    /// System curve, speed and trimmed diameter of operation.
    pub operation: OperationInput,
    /// Seals and gaps of the loss analysis.
    pub losses: LossInput,
    /// Kinematic viscosity of the liquid, m²/s; automatic: that of the project.
    pub viscosity: Param<f64>,
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
    /// J_sp = Σ ∬ dA/r over the throats of the volute, m (Gülich Table 4.1).
    pub throat_integral: Option<f64>,
    /// Flow angle α₃′ at the cutwater at the design point and camber angle α₃B of the
    /// cutwater, rad.
    pub cutwater: Option<(f64, f64)>,
}

/// The flow at which the volute characteristic meets the impeller characteristic,
/// Q_opt,th = η_v·A₂·u₂·γ / (τ₂/tan β₂B + 2π·b₂·Q_Le/(J_sp·Q_La)) (Gülich 2nd ed.,
/// Table 4.1, eq. T4.1.3), with A₂ and b₂ of the whole impeller (f_q included), no
/// pre-swirl and the volute sized for the pump flow (Q_Le = Q, so Q_Le/Q_La = η_v). The
/// throat sets the best-efficiency point: a larger throat moves it to a larger flow.
pub fn throat_best_efficiency_flow(design: &CentrifugalPumpDesign, throat_integral: f64) -> f64 {
    let outlet = &design.outlet;
    let eta_v = design.efficiencies.volumetric;
    let area2 = PI * design.outlet_mean_diameter * design.outlet_width;
    let impeller = outlet.blockage / outlet.blade_angle.tan();
    let volute = 2.0 * PI * design.outlet_width * eta_v / throat_integral;
    eta_v * area2 * outlet.triangle.u * outlet.slip_factor / (impeller + volute)
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
            operation: OperationInput::default(),
            losses: LossInput::default(),
            viscosity: Param::Auto,
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
    /// The balance holes, when the impeller has them.
    pub balance_holes: Option<BalanceHoleSize>,
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
    /// Flows between which the incidence on the cutwater stays within ±3°
    /// ([`INCIDENCE_LIMIT`]), m³/s; the ends of the characteristic where it does not
    /// leave that band.
    pub incidence_range: Option<(f64, f64)>,
}

/// Incidence on the cutwater Gülich admits, ±3° (Chap. 7.8.1, item 7), rad.
pub const INCIDENCE_LIMIT: f64 = 3.0 * std::f64::consts::PI / 180.0;

/// Incidence i₃ = α₃B − α₃′ on a cutwater of camber angle `blade_angle` at Q/Q_design =
/// `flow_ratio` and H_th/H_th,design = `head_ratio`, where the flow meets it at
/// `flow_angle` (rad): c₃m grows with the flow and c₃u with the theoretical head
/// (eqs. T7.7.3 to T7.7.6, no prerotation). At part load the flow comes flatter, i₃ > 0
/// (Gülich Fig. 9.20).
pub fn cutwater_incidence(
    flow_angle: f64,
    blade_angle: f64,
    flow_ratio: f64,
    head_ratio: f64,
) -> f64 {
    blade_angle - (flow_angle.tan() * flow_ratio).atan2(head_ratio)
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
    /// Where ψ₀ came from.
    pub shutoff_source: ShutoffSource,
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
    /// The pump in its system.
    pub operation: Operation,
    /// Best-efficiency flow the throat of the volute gives by Gülich eq. T4.1.3, m³/s;
    /// `None` without a volute.
    pub throat_best_flow: Option<f64>,
    /// Leakage, disk friction and mechanical losses of the loss analysis for water;
    /// `None` when the statistics of the design stand for them.
    pub losses: Option<SecondaryLosses>,
    /// Hydraulic losses of the loss analysis for water, when a volute follows the
    /// impeller; `None` when the statistical η_h stands for them.
    pub hydraulic: Option<HydraulicLosses>,
    /// The overall efficiency of the design point for water as a product of its parts;
    /// `None` when the statistics of the design stand for the losses.
    pub breakdown: Option<EfficiencyBreakdown>,
    /// The correction from the water characteristic to the liquid of the project
    /// (Gülich Chap. 13.1); all factors 1 for cold water.
    pub viscous: ViscousCorrection,
    /// Overall efficiency of the design statistics (Gülich Table 3.9) and its expected
    /// scatter, for comparison.
    pub statistical_efficiency: f64,
    pub efficiency_tolerance: f64,
    pub warnings: Vec<String>,
}

/// The hydraulic losses of the design point as a sum (Gülich Table 3.8), m.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HydraulicLosses {
    /// Euler head H_th of the design point, m.
    pub theoretical_head: f64,
    /// Friction and inlet shock in the impeller channels.
    pub impeller: ImpellerLosses,
    /// Shock where the flow leaves the impeller into the volute (eq. T3.8.24).
    pub outlet_shock: f64,
    /// Friction on the walls of the spiral (eq. T3.8.21).
    pub volute_friction: f64,
    /// Mixing in the spiral and the discharge cone, from the model of
    /// [`crate::volute::loss`].
    pub volute: f64,
    /// η_h = 1 − ΣZ/H_th and the η_h of the design statistics.
    pub efficiency: f64,
    pub statistical_efficiency: f64,
}

impl HydraulicLosses {
    pub fn total(&self) -> f64 {
        self.impeller.total() + self.outlet_shock + self.volute_friction + self.volute
    }
}

/// The overall efficiency of one operating point as a product,
/// η = η_h·η_v·η_RR·η_m.
///
/// Gülich (2nd ed., Table 3.5) gives the power balance P = ρgHQ/(η_v·η_h) + ΣP_RR + P_m
/// (eq. T3.5.1), the mechanical efficiency η_m = 1 − P_m/P (eq. T3.5.6) and the inner
/// efficiency η_i = η/η_m on the inner power P_i = P − P_m (eqs. T3.5.4 and T3.5.5). The
/// book has no separate disk-friction efficiency. We define η_RR = 1 − ΣP_RR/P_i. It is
/// the share of the inner power that reaches the fluid in the channels, so the product
/// is exact. The interstage-seal and balance-device terms of eq. T3.5.1 are not
/// modelled.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EfficiencyBreakdown {
    /// η_h = H/H_th.
    pub hydraulic: f64,
    /// η_v = Q/(Q + Q_sp) (eq. T3.5.9), from the power balance.
    pub volumetric: f64,
    /// η_RR = 1 − ΣP_RR/P_i, disk friction (our definition).
    pub disk_friction: f64,
    /// η_m = 1 − P_m/P (eq. T3.5.6).
    pub mechanical: f64,
    /// η_i = η/η_m (eq. T3.5.5).
    pub inner: f64,
    /// η = ρgHQ/P (eq. T3.5.3).
    pub overall: f64,
}

impl EfficiencyBreakdown {
    /// The parts of the efficiency of `point`, whose power holds the disk friction
    /// `disk_friction` and the mechanical losses `mechanical` (W).
    pub fn new(point: &PerformancePoint, disk_friction: f64, mechanical: f64) -> Self {
        let mechanical_efficiency = 1.0 - mechanical / point.power;
        let inner_power = point.power - mechanical;
        let disk = 1.0 - disk_friction / inner_power;
        // The rest of eq. T3.5.1, ρ·g·H_th·(Q + Q_sp) = η_RR·P_i, gives η_v.
        let volumetric =
            point.efficiency / (point.hydraulic_efficiency * disk * mechanical_efficiency);
        Self {
            hydraulic: point.hydraulic_efficiency,
            volumetric,
            disk_friction: disk,
            mechanical: mechanical_efficiency,
            inner: point.efficiency / mechanical_efficiency,
            overall: point.efficiency,
        }
    }
}

/// Hydraulic efficiency relative to its maximum at flow ratio q* = Q/Q_opt:
/// 1 − 0.6·(q* − 0.9)² − 0.25·(q* − 0.9)³ (Gülich eq. 4.14). The maximum lies at
/// q* = 0.9.
pub fn hydraulic_efficiency_ratio(flow_ratio: f64) -> f64 {
    let x = flow_ratio - 0.9;
    1.0 - 0.6 * x * x - 0.25 * x * x * x
}

/// Slip factor relative to the one of the best point at flow ratio q* = Q/Q_opt, our
/// fit of the mean line of Gülich Fig. 4.15 (measured γ/γ_opt): 1 + 0.138·(1 − q*)² +
/// 0.233·(1 − q*)³ below the best point, 1 up to q* = 1.3 and 1 − 0.21·(q* − 1.3)²
/// above. It gives 1.10 at q* = 0.4, 1.04 at 0.6 and 0.98 at 1.6. The data cover
/// 0.4 ≤ q* ≤ 1.6; outside that range the end values hold.
pub fn slip_factor_ratio(flow_ratio: f64) -> f64 {
    let q = flow_ratio.clamp(0.4, 1.6);
    if q < 1.0 {
        let x = 1.0 - q;
        1.0 + 0.138 * x * x + 0.233 * x * x * x
    } else if q <= 1.3 {
        1.0
    } else {
        1.0 - 0.21 * (q - 1.3) * (q - 1.3)
    }
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

/// Power coefficient at shut-off λ₀ = 2P₀/(ρ·π·d₂·b₂·u₂³) (Gülich eq. T3.4.9; d₂ the
/// mean outlet diameter, u₂ at the shroud): our power-law fit
/// 0.0276·(nq/10)^0.552 to the volute pumps of Fig. 4.10, within ±16 % of them for
/// 10 ≤ nq ≤ 92. Outside that range the end values hold. Diffuser pumps above nq ≈ 20
/// lie higher in the figure; the fit stays for them as the lower bound.
pub fn shutoff_power_coefficient(nq: f64) -> f64 {
    0.0276 * (nq.clamp(10.0, 92.0) / 10.0).powf(0.552)
}

/// Where the automatic shut-off head comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutoffSource {
    /// The head rise of the blade number and outlet angle ([`blade_shutoff_coefficient`]).
    Blades,
    /// Gülich eqs. 3.27–3.28 from nq alone, outside the range of the head-rise chart.
    SpecificSpeed,
    /// Raised to the least head that keeps the estimated range falling from the
    /// computed curve: the correlation gave less.
    Curve,
    Manual,
}

/// Shut-off pressure coefficient from the blades: H₀ = H·(1 + Δh), Δh the head rise of
/// the blade number and outlet angle ([`crate::pump::correlations::head_rise_for_blades`],
/// after Lobanoff & Ross Fig. 3-2) over the head `design_head` of the curve at the design
/// flow. The chart does not tell volutes from diffusers; vaned diffusers take the ratio
/// 1.31/1.25 of Gülich eqs. 3.27 and 3.28 on top (our combination).
pub fn blade_shutoff_coefficient(
    design: &CentrifugalPumpDesign,
    collector: Collector,
    design_head: f64,
) -> f64 {
    let rise = crate::pump::correlations::head_rise_for_blades(
        design.nq,
        design.blade_count,
        design.outlet.blade_angle,
    );
    let collector_factor = shutoff_coefficient(design.nq, collector)
        / shutoff_coefficient(design.nq, Collector::Volute);
    let u2 = design.outlet.triangle.u;
    2.0 * G * design_head * (1.0 + rise) * collector_factor / (u2 * u2)
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
    /// The seals of the loss analysis, which give the leakage at each head.
    losses: Option<SecondaryLosses>,
}

/// The volute and its matched equivalent, whose losses eq. 4.14 already holds.
struct VoluteSplit {
    geometry: VoluteLossGeometry,
    /// The same volute with the throat that makes λ₆·AR = 1 at the design point.
    matched: VoluteLossGeometry,
    /// Hydraulic losses of the design point, (1 − η_h)·H_th, m.
    design_losses: f64,
}

impl Model {
    /// Euler head for pump flow `flow` and seal leakage `leakage` through the impeller,
    /// swirl-free inflow: c₂u = u₂·γ − c₂m·τ₂/tan β₂B (eq. T3.2.7) and H_th = u₂·c₂u/g,
    /// γ following Fig. 4.15 ([`slip_factor_ratio`]).
    fn theoretical_head(&self, flow: f64, leakage: f64) -> f64 {
        let cm2 = (flow + leakage) / self.area2;
        let slip = self.slip * slip_factor_ratio(flow / self.flow);
        let cu2 = self.u2 * slip - cm2 * self.blockage / self.tan_beta2b;
        self.u2 * cu2 / G
    }

    /// Swirl at the volute inlet for Euler head `theoretical`: c₂u as a free vortex.
    fn volute_swirl(&self, theoretical: f64, inlet_radius: f64) -> f64 {
        G * theoretical / self.u2 * self.impeller_radius / inlet_radius
    }

    /// Losses of the volute for pump flow `flow` and Euler head `theoretical`.
    fn volute_loss(&self, flow: f64, theoretical: f64) -> Option<VoluteLoss> {
        let v = self.volute.as_ref()?;
        let swirl = self.volute_swirl(theoretical, v.geometry.inlet_radius);
        Some(volute_loss(&v.geometry, flow, swirl))
    }

    /// What the volute loses beyond its matched equivalent, m; it may be negative.
    fn volute_mismatch(&self, flow: f64, theoretical: f64) -> Option<(VoluteLoss, f64)> {
        let v = self.volute.as_ref()?;
        let actual = self.volute_loss(flow, theoretical)?;
        let swirl = self.volute_swirl(theoretical, v.matched.inlet_radius);
        let matched = volute_loss(&v.matched, flow, swirl);
        Some((actual, actual.total() - matched.total()))
    }

    fn point(&self, flow_ratio: f64) -> PerformancePoint {
        let flow = flow_ratio * self.flow;
        let hydraulic = self.peak_hydraulic_efficiency * hydraulic_efficiency_ratio(flow_ratio);
        // Seal leakage follows the square root of the pressure across the seal.
        let mut leakage = self.leakage;
        let (mut theoretical, mut head, mut volute) = (0.0, 0.0, f64::NAN);
        for _ in 0..4 {
            theoretical = self.theoretical_head(flow, leakage);
            head = hydraulic * theoretical;
            if let Some((loss, mismatch)) = self.volute_mismatch(flow, theoretical) {
                volute = loss.total();
                head -= mismatch;
            }
            leakage = match &self.losses {
                Some(l) => l.leakage_at(head, theoretical),
                None => self.leakage * (head.max(0.0) / self.head).sqrt(),
            };
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
        area2: PI * design.outlet_mean_diameter * design.outlet_width,
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
        losses: None,
    };
    // The characteristic is predicted for cold water and corrected for the liquid at
    // the end (Gülich Chap. 13.1).
    let water = PumpDuty {
        kinematic_viscosity: WATER_VISCOSITY,
        ..*duty
    };
    // The loss analysis replaces the leakage and secondary power of the statistics.
    let axial = AxialThrust::new(duty, design, &input.axial, input.wide_seal_clearances)?;
    warnings.extend(axial.back_vane_warnings());
    let water_losses = SecondaryLosses::new(
        &water,
        design,
        &axial,
        &input.losses,
        input.wide_seal_clearances,
        design.theoretical_head,
    )?;
    let losses = (!input.losses.statistics).then_some(water_losses);
    if let Some(l) = &losses {
        model.leakage = l.leakage;
        model.secondary_power = l.secondary_power();
        model.losses = Some(*l);
    }
    let mut hydraulic = None;
    if let Some(geometry) = collector.volute {
        let impeller_flow = duty.flow + model.leakage;
        let theoretical = model.theoretical_head(duty.flow, model.leakage);
        let design_losses = (1.0 - eta.hydraulic) * theoretical;
        let swirl = model.volute_swirl(theoretical, geometry.inlet_radius);
        let actual = volute_loss(&geometry, duty.flow, swirl);
        // The matched volute: the same one with the throat that makes λ₆·AR = 1 at
        // the design point (the matching grows with the throat area).
        let matched = VoluteLossGeometry {
            throat_area: geometry.throat_area / actual.matching,
            ..geometry
        };
        model.volute = Some(VoluteSplit {
            geometry,
            matched,
            design_losses,
        });
        let reference = volute_loss(&matched, duty.flow, swirl).total();
        if losses.is_some() {
            // The loss analysis sums the losses of the design point instead (Gülich
            // Table 3.8). Its estimates hold near the best point only (Chap. 3.8), so
            // eq. 4.14 gives their shape against flow, with the matched volute in place
            // of the actual one; wall friction of the volute is part of the rest.
            let omega = 2.0 * model.u2 / design.outlet_diameter;
            let impeller = ImpellerChannels::new(design, omega, CHANNEL_ROUGHNESS)
                .losses(impeller_flow, WATER_VISCOSITY);
            let shock = outlet_shock(
                impeller_flow / model.area2 / model.u2,
                outlet.blockage,
                design.outlet_width / geometry.inlet_width,
                model.u2,
            );
            let walls = geometry.wall_friction_loss(duty.flow);
            let rest = impeller.total() + shock + walls;
            let total = rest + actual.total();
            let efficiency = 1.0 - total / theoretical;
            if efficiency.is_nan() || efficiency <= 0.3 {
                return Err(DesignError::Infeasible(format!(
                    "the hydraulic losses add up to {total:.1} m of the {theoretical:.1} m of \
                     Euler head: check the impeller and the volute"
                )));
            }
            let matched_efficiency = 1.0 - (rest + reference) / theoretical;
            model.peak_hydraulic_efficiency = matched_efficiency / hydraulic_efficiency_ratio(1.0);
            if let Some(split) = &mut model.volute {
                split.design_losses = total;
            }
            hydraulic = Some(HydraulicLosses {
                theoretical_head: theoretical,
                impeller,
                outlet_shock: shock,
                volute_friction: walls,
                volute: actual.total(),
                efficiency,
                statistical_efficiency: eta.hydraulic,
            });
        } else if reference > design_losses {
            warnings.push(format!(
                "the volute loses {reference:.2} m at the design point, more than the \
                 {design_losses:.2} m of all the hydraulic losses: check the volute"
            ));
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
    let (mut psi0, mut shutoff_source) = match input.shutoff_coefficient {
        Param::Manual(psi0) => (psi0, ShutoffSource::Manual),
        Param::Auto => {
            let (lo, hi) = crate::pump::correlations::HEAD_RISE_NQ_RANGE;
            if (lo..=hi).contains(&design.nq) {
                (
                    blade_shutoff_coefficient(design, input.collector, model.point(1.0).head),
                    ShutoffSource::Blades,
                )
            } else {
                (
                    shutoff_coefficient(design.nq, input.collector),
                    ShutoffSource::SpecificSpeed,
                )
            }
        }
    };
    let mut shutoff_head = psi0 * model.u2 * model.u2 / (2.0 * G);

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
    // shut-off flat and meets the computed curve; power along a straight line from the
    // shut-off power.
    let step = 1e-4;
    let at_onset = model.point(onset);
    let slope_head = (model.point(onset + step).head - at_onset.head) / step;
    // End slope in units of the interval, limited so the cubic adds no bump of its own
    // between the two heads (Fritsch and Carlson's monotonicity limit).
    let mut end_slope = onset * slope_head;
    // Measured characteristics of volute pumps fall from shut-off to the best point
    // (Gülich Fig. 4.13) and the statistics cannot tell an unstable one (Chap. 4.3):
    // an automatic shut-off head is at least the one the limit lets fall into the
    // computed curve.
    let least = at_onset.head - end_slope.min(0.0) / 3.0;
    if shutoff_source != ShutoffSource::Manual && shutoff_head < least {
        shutoff_head = least;
        psi0 = 2.0 * G * shutoff_head / (model.u2 * model.u2);
        shutoff_source = ShutoffSource::Curve;
    }
    let rise = at_onset.head - shutoff_head;
    if rise < 0.0 && end_slope < 3.0 * rise {
        end_slope = 3.0 * rise;
    }
    // Power toward shut-off: what Fig. 4.10 gives, never less than the secondary losses
    // (Gülich 4.3.5).
    let power_at_zero = input.shutoff_power.resolve(|| {
        let u2 = model.u2;
        let lambda = shutoff_power_coefficient(design.nq);
        (lambda
            * water.density
            * PI
            * design.outlet_mean_diameter
            * design.outlet_width
            * u2.powi(3)
            / 2.0)
            .max(model.secondary_power)
    });
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
                .volute_loss(flow, model.theoretical_head(flow, model.leakage))
                .map_or(f64::NAN, |l| l.total()),
            radial_force: f64::NAN,
            axial_force: f64::NAN,
        }
    };

    // Highest efficiency of the water characteristic, by golden-section search in the
    // computed range.
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
    let best_ratio = 0.5 * (a + b);

    // From water to the liquid of the project: Q, H and η of every point (Gülich Table
    // 13.1), with the factors of the loss analysis of Table 3.10 at the design point.
    let water_design = model.point(1.0);
    let useful = duty.density * G * water_design.head * duty.flow;
    let baseline = WaterBaseline {
        nq: design.nq,
        eyes: design.eyes,
        omega: duty.speed,
        impeller_radius: design.outlet_diameter / 2.0,
        axial_gap: input
            .losses
            .axial_gap
            .resolve(|| crate::losses::AXIAL_GAP_RATIO * design.outlet_diameter / 2.0),
        hydraulic_efficiency: water_design.hydraulic_efficiency,
        volumetric_efficiency: water_losses.volumetric_efficiency,
        disk_share: (water_losses.front_friction + water_losses.rear_friction) / useful,
        mechanical_share: water_losses.mechanical / useful,
        inlet_coefficient: input
            .npsh_inlet_coefficient
            .resolve(|| crate::npsh::AXIAL_INLET_COEFFICIENT),
    };
    let viscosity = input.viscosity.resolve(|| duty.kinematic_viscosity);
    if viscosity.is_nan() || viscosity <= 0.0 {
        return Err(DesignError::InvalidInput(
            "the viscosity must be positive".into(),
        ));
    }
    // Within 5 % of cold water the liquid counts as water.
    let effective = if (viscosity / WATER_VISCOSITY - 1.0).abs() < VISCOSITY_TOLERANCE {
        WATER_VISCOSITY
    } else {
        viscosity
    };
    let viscous = ViscousCorrection::new(effective, &baseline);
    let best_water_flow = best_ratio * duty.flow;
    let viscid = |p: PerformancePoint| -> PerformancePoint {
        let f_q = viscous.flow_factor();
        let f_h = viscous.head_factor_at(p.flow / best_water_flow);
        let f_eta = viscous.efficiency_factor;
        let head = p.head * f_h;
        PerformancePoint {
            flow: p.flow * f_q,
            head,
            power: p.power * f_q * f_h / f_eta,
            efficiency: p.efficiency * f_eta,
            hydraulic_efficiency: if p.theoretical_head.is_finite() {
                head / p.theoretical_head
            } else {
                p.hydraulic_efficiency
            },
            ..p
        }
    };
    // NPSH₃ at constant flow (eq. 13.12).
    let npsh_at = |flow: f64| {
        let water = npsh.at(flow);
        water * viscous.npsh_factor(flow / axial.eye_area, water)
    };

    let n = input.points.max(11);
    let water_points: Vec<PerformancePoint> = (0..n)
        .map(|i| end * i as f64 / (n - 1) as f64)
        .map(|r| {
            if r < onset {
                estimate(r)
            } else {
                model.point(r)
            }
        })
        .collect();
    let mut points: Vec<PerformancePoint> = water_points.iter().map(|p| viscid(*p)).collect();
    // The onset itself, so the two parts meet in the plots.
    let at_onset = viscid(at_onset);
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
    // In the recirculation range the Euler head comes from the model without it.
    let axial_forces = |p: &PerformancePoint| {
        let theoretical = if p.theoretical_head.is_finite() {
            p.theoretical_head
        } else {
            model.theoretical_head(p.flow, model.leakage)
        };
        axial.forces(p.flow, p.head, theoretical)
    };
    let with_npsh = |mut p: PerformancePoint| {
        p.npsh_required = npsh_at(p.flow);
        if let Some(law) = &radial {
            p.radial_force = law.force(p.flow / duty.flow, p.head, duty.head);
        }
        p.axial_force = axial_forces(&p).total;
        p
    };
    for p in &mut points {
        *p = with_npsh(*p);
    }

    let best = with_npsh(viscid(model.point(best_ratio)));
    let design_point = with_npsh(viscid(model.point(1.0)));
    let shutoff = with_npsh(viscid(estimate(0.0)));
    if viscous.outside_tests && (viscosity / WATER_VISCOSITY - 1.0).abs() > 0.5 {
        warnings.push(format!(
            "the correction for a viscosity of {:.0} mm²/s goes beyond the tests behind it \
             (Gülich Table 13.1: 7 < nq < 50, up to 4000 mm²/s)",
            viscosity * 1e6
        ));
    }
    // On the water curve: the viscous correction of a liquid thinner than the reference
    // water lifts the head a little with flow from shut-off.
    let rises = water_points
        .windows(2)
        .any(|w| w[1].head > w[0].head + 1e-6 * duty.head);
    if rises {
        warnings.push(
            "the head rises with flow somewhere below the design point: the Q-H curve may be \
             unstable, which a prediction from the design point cannot really tell (Gülich \
             Chap. 4.3 and 5)"
                .into(),
        );
    }
    if (best.flow / design_point.flow - 1.0).abs() > 0.1 {
        warnings.push(format!(
            "the predicted best efficiency is at {:.0} % of the design flow",
            100.0 * best.flow / design_point.flow
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
        // The incidence falls with the flow: where it crosses +3° and −3°.
        let incidence_range = collector.cutwater.map(|(flow_angle, blade_angle)| {
            let incidence = |ratio: f64| {
                let p = model.point(ratio);
                cutwater_incidence(
                    flow_angle,
                    blade_angle,
                    ratio,
                    p.theoretical_head / design_point.theoretical_head,
                )
            };
            let crossing = |limit: f64| {
                let (mut lo, mut hi) = (0.05, end);
                if incidence(lo) <= limit {
                    return lo;
                }
                if incidence(hi) >= limit {
                    return hi;
                }
                for _ in 0..50 {
                    let mid = 0.5 * (lo + hi);
                    if incidence(mid) > limit {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                0.5 * (lo + hi)
            };
            (
                crossing(INCIDENCE_LIMIT) * duty.flow,
                crossing(-INCIDENCE_LIMIT) * duty.flow,
            )
        });
        VoluteSummary {
            design,
            share: design.total() / split.design_losses,
            matched_flow,
            incidence_range,
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
        balance_holes: axial.balance_holes(design.blade_count),
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
    let operation = Operation::new(
        &points,
        &best,
        &npsh_at,
        design.npsh_available,
        duty,
        design,
        input.collector,
        &input.operation,
    )?;
    warnings.extend(operation.warnings.iter().cloned());
    // Where NPSH₃ of the liquid rises past the NPSH available.
    let cavitation_flow = points
        .windows(2)
        .rposition(|w| {
            w[0].npsh_required <= design.npsh_available
                && w[1].npsh_required > design.npsh_available
        })
        .map(|i| {
            let (mut lo, mut hi) = (points[i].flow, points[i + 1].flow);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if npsh_at(mid) > design.npsh_available {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            0.5 * (lo + hi)
        });
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
        shutoff_source,
        recirculation_flow: onset * duty.flow * viscous.flow_factor(),
        max_flow: end * duty.flow * viscous.flow_factor(),
        npsh_required: npsh.design_npsh,
        npsh_available: design.npsh_available,
        cavitation_flow,
        npsh,
        volute,
        radial,
        axial,
        operation,
        throat_best_flow: collector
            .throat_integral
            .map(|j| throat_best_efficiency_flow(design, j)),
        breakdown: losses.map(|l| {
            EfficiencyBreakdown::new(
                &water_design,
                l.front_friction + l.rear_friction,
                l.mechanical,
            )
        }),
        losses,
        hydraulic,
        viscous,
        statistical_efficiency: design.efficiencies.overall,
        efficiency_tolerance: design.efficiency_tolerance,
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
        description: "ψ₀ = 1.31·e^(−0.3·nq/100) diffuser pumps, 1.25·e^(−0.3·nq/100) volutes; \
                      used outside nq 4–68",
        reference: "Gülich 2nd ed., eqs. 3.27 and 3.28",
        verification: crate::pump::correlations::Verification::Verified,
    },
    crate::pump::correlations::CorrelationInfo {
        id: "performance.shutoff_head_blades",
        description: "H₀ = H·(1 + Δh) with the head rise Δh of the blade number and outlet \
                      angle for nq 4–68; vaned diffusers × 1.31/1.25 (our combination)",
        reference: "Lobanoff & Ross, Centrifugal Pumps: Design and Application, 2nd ed., \
                    Fig. 3-2 (our fit); Gülich 2nd ed., eqs. 3.27 and 3.28 for the collector",
        verification: crate::pump::correlations::Verification::EngineeringDefault,
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
                      from the annular seal at each head (or ∝ √H with the statistics)",
        reference: "Gülich 2nd ed., Chap. 4.1.2; the leakage law is the orifice law",
        verification: crate::pump::correlations::Verification::EngineeringDefault,
    },
    crate::pump::correlations::CorrelationInfo {
        id: "performance.loss_analysis",
        description: "seal leakage of Table 3.7 (1) with s from eq. 3.12; disk friction \
                      k_RR of eq. T3.6.3 with f_L; P_m/P = 0.0045·(Q_Ref/Q)^0.4·(n_Ref/n)^0.3",
        reference: "Gülich 2nd ed., Chap. 3.6, eqs. 3.12, T3.5.6a, T3.6.2 to T3.6.7, \
                    T3.7.1 to T3.7.14",
        verification: crate::pump::correlations::Verification::Verified,
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
        id: "performance.back_vanes",
        description: "k_Rs = 1/[1 + 0.13·s/(s + h)·√(r₂/(h·z))], k_av for trimmed vanes, on \
                      the rear shroud of the axial force; P_Rs = ζ_Rs·ρ·ω³·r₂⁵ in place of the \
                      rear disk friction (our reading)",
        reference: "Gülich 2nd ed., Table 9.3 (after Möhring), eqs. T9.3.2 to T9.3.4",
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
    use crate::losses::LossInput;
    use crate::operation::OperationInput;
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
            kinematic_viscosity: 1.0e-6,
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
        // The rear seal leaks too, so the head shifts a little.
        assert_relative_eq!(
            holes.axial.design.hydraulic,
            0.15 * plain.hydraulic,
            max_relative = 0.02
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

    fn operating(edit: &dyn Fn(&mut OperationInput)) -> PumpPerformance {
        let mut input = PerformanceInput::default();
        edit(&mut input.operation);
        performance(&input).1
    }

    #[test]
    fn the_default_system_runs_the_pump_at_its_design_point() {
        let p = operating(&|_| {});
        let o = &p.operation;
        let point = o.point.unwrap();
        assert_relative_eq!(point.flow, duty().flow, max_relative = 0.01);
        assert_relative_eq!(point.head, o.system.head(point.flow), max_relative = 1e-6);
        assert_relative_eq!(o.speed_ratio, 1.0);
        assert_relative_eq!(o.trim_ratio, 1.0);
        assert_relative_eq!(o.required_speed.unwrap(), 1.0, max_relative = 0.01);
        // Continuous operation brackets the best point, at 80 % of its efficiency.
        let (low, high) = o.continuous;
        assert!(low < o.best.flow && o.best.flow < high);
        let eff = |q: f64| {
            o.curve
                .windows(2)
                .find(|w| w[0].flow <= q && q <= w[1].flow)
                .map(|w| {
                    let t = (q - w[0].flow) / (w[1].flow - w[0].flow);
                    w[0].efficiency + t * (w[1].efficiency - w[0].efficiency)
                })
                .unwrap()
        };
        assert_relative_eq!(eff(low), 0.8 * o.best.efficiency, max_relative = 1e-3);
        assert!(high <= o.curve.last().unwrap().flow);
    }

    #[test]
    fn speed_follows_the_similarity_laws() {
        let full = operating(&|_| {});
        let slow = operating(&|o| o.speed = Param::Manual(0.8 * duty().speed));
        let o = &slow.operation;
        assert_relative_eq!(o.speed_ratio, 0.8, max_relative = 1e-12);
        assert_relative_eq!(
            o.best.flow,
            0.8 * full.operation.best.flow,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            o.best.head,
            0.64 * full.operation.best.head,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            o.best.power,
            0.512 * full.operation.best.power,
            max_relative = 1e-9
        );
        // A system without static head is a similarity parabola: the operating point
        // slides along it.
        let (a, b) = (full.operation.point.unwrap(), o.point.unwrap());
        assert_relative_eq!(b.flow, 0.8 * a.flow, max_relative = 2e-3);
        // The speed that brings the full curve through a lower system point.
        let target = |o: &mut OperationInput| {
            o.static_head = Param::Manual(10.0);
            o.system_flow = Param::Manual(0.9 * duty().flow);
            o.system_head = Param::Manual(26.0);
        };
        let ratio = operating(&target).operation.required_speed.unwrap();
        assert!(ratio < 1.0);
        let matched = operating(&|o| {
            target(o);
            o.speed = Param::Manual(ratio * duty().speed);
        });
        let point = matched.operation.point.unwrap();
        assert_relative_eq!(point.flow, 0.9 * duty().flow, max_relative = 5e-3);
        assert_relative_eq!(point.head, 26.0, max_relative = 5e-3);
    }

    #[test]
    fn trimming_follows_eqs_4_15_to_4_23() {
        let full = operating(&|_| {});
        let d2 = full.operation.diameter;
        let trimmed = operating(&|o| o.trimmed_diameter = Param::Manual(0.9 * d2));
        let o = &trimmed.operation;
        // Rays from the origin with m = 2, and Δη = 0.2·(1 − 0.9) for a volute pump.
        assert_relative_eq!(
            o.best.flow,
            0.81 * full.operation.best.flow,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            o.best.head,
            0.81 * full.operation.best.head,
            max_relative = 1e-9
        );
        assert_relative_eq!(o.efficiency_drop, 0.02, max_relative = 1e-9);
        assert_relative_eq!(
            o.best.efficiency,
            full.operation.best.efficiency - 0.02,
            max_relative = 1e-12
        );
        // The trim that brings the curve through a lower system point (eq. 4.17).
        let target = |o: &mut OperationInput| {
            o.static_head = Param::Manual(10.0);
            o.system_flow = Param::Manual(0.9 * duty().flow);
            o.system_head = Param::Manual(26.0);
        };
        let ratio = operating(&target).operation.required_trim.unwrap();
        assert!(ratio > 0.85 && ratio < 1.0, "{ratio}");
        let matched = operating(&|o| {
            target(o);
            o.trimmed_diameter = Param::Manual(ratio * d2);
        });
        let point = matched.operation.point.unwrap();
        assert_relative_eq!(point.flow, 0.9 * duty().flow, max_relative = 5e-3);
        assert_relative_eq!(point.head, 26.0, max_relative = 5e-3);
        // Too deep a trim for a volute pump with nq = 36 (eq. 4.23).
        let deep = operating(&|o| o.trimmed_diameter = Param::Manual(0.75 * d2));
        assert_relative_eq!(deep.operation.trim_limit, 0.8);
        assert!(deep.warnings.iter().any(|w| w.contains("eq. 4.23")));
    }

    #[test]
    fn operating_limits_and_impossible_systems() {
        // Far to the left of the best point: a system through the pump curve at 25 %.
        let q = 0.25 * duty().flow;
        let curve = operating(&|_| {}).operation.curve;
        let w = curve
            .windows(2)
            .find(|w| w[0].flow <= q && q <= w[1].flow)
            .unwrap();
        let h = w[0].head + (q - w[0].flow) / (w[1].flow - w[0].flow) * (w[1].head - w[0].head);
        let partload = operating(&|o| {
            o.static_head = Param::Manual(0.9 * h);
            o.system_flow = Param::Manual(q);
            o.system_head = Param::Manual(h);
        });
        let point = partload.operation.point.unwrap();
        assert_relative_eq!(point.flow, q, max_relative = 0.02);
        assert!(
            partload
                .warnings
                .iter()
                .any(|w| w.contains("continuous operation"))
        );
        // A static head above the shut-off head: no crossing.
        let blocked = operating(&|o| {
            o.static_head = Param::Manual(60.0);
            o.system_head = Param::Manual(70.0);
        });
        assert!(blocked.operation.point.is_none());
        assert!(blocked.warnings.iter().any(|w| w.contains("do not cross")));
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        for edit in [
            &(|o: &mut OperationInput| o.trimmed_diameter = Param::Manual(1.0))
                as &dyn Fn(&mut OperationInput),
            &|o: &mut OperationInput| o.static_head = Param::Manual(40.0),
            &|o: &mut OperationInput| o.speed = Param::Manual(-1.0),
        ] {
            let mut input = PerformanceInput::default();
            edit(&mut input.operation);
            assert!(predict(&duty(), &d, &input, &CollectorData::default()).is_err());
        }
    }

    #[test]
    fn a_viscous_liquid_shifts_the_water_curve_by_table_13_1() {
        let (_, w) = performance(&PerformanceInput::default());
        assert_relative_eq!(w.viscous.head_factor, 1.0, max_relative = 1e-12);
        let mut oil = duty();
        oil.kinematic_viscosity = 300e-6;
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let v = predict(
            &oil,
            &d,
            &PerformanceInput::default(),
            &CollectorData::default(),
        )
        .unwrap();
        let c = v.viscous;
        assert!(c.head_factor < 0.95 && c.efficiency_factor < c.head_factor);
        // Q_v = f_Q·Q_w, η_v = f_η·η_w and H_v = f_H(q*)·H_w at every point.
        assert_relative_eq!(
            v.design.flow,
            c.flow_factor() * w.design.flow,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            v.design.efficiency,
            c.efficiency_factor * w.design.efficiency,
            max_relative = 1e-9
        );
        let q = w.design.flow / w.best.flow;
        assert_relative_eq!(
            v.design.head,
            c.head_factor_at(q) * w.design.head,
            max_relative = 1e-6
        );
        // Shut-off keeps its head; the power rises.
        assert_relative_eq!(v.points[0].head, w.points[0].head, max_relative = 1e-9);
        assert!(v.points[0].power > w.points[0].power);
        // More NPSH₃ at the same flow.
        assert!(v.design.npsh_required > w.npsh.at(v.design.flow));
        // The two methods of Gülich agree within the scatter of the tests.
        assert!((c.efficiency_factor / c.correlation_efficiency_factor - 1.0).abs() < 0.3);
        // Too viscous for the tests behind the method.
        oil.kinematic_viscosity = 6000e-6;
        let thick = predict(
            &oil,
            &d,
            &PerformanceInput::default(),
            &CollectorData::default(),
        )
        .unwrap();
        assert!(
            thick
                .warnings
                .iter()
                .any(|w| w.contains("beyond the tests"))
        );
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
        // With the efficiencies of the design statistics the curve meets the design
        // point exactly.
        let statistics = PerformanceInput {
            losses: LossInput {
                statistics: true,
                ..LossInput::default()
            },
            ..PerformanceInput::default()
        };
        let (d, p) = performance(&statistics);
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
            operation: OperationInput::default(),
            losses: LossInput::default(),
            viscosity: Param::Auto,
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
    fn the_blades_set_the_shutoff_head() {
        let duty = low_nq_duty();
        let d = design(&duty, &CentrifugalPumpInput::default()).unwrap();
        let p = predict(
            &duty,
            &d,
            &PerformanceInput::default(),
            &CollectorData::default(),
        )
        .unwrap();
        assert_eq!(p.shutoff_source, ShutoffSource::Blades);
        let rise = crate::pump::correlations::head_rise_for_blades(
            d.nq,
            d.blade_count,
            d.outlet.blade_angle,
        );
        assert_relative_eq!(
            p.shutoff_head,
            p.design.head * (1.0 + rise),
            max_relative = 1e-6
        );
        // Fewer blades at a flatter angle give a steeper curve.
        let steep = CentrifugalPumpInput {
            head_rise: Param::Manual(0.45),
            ..Default::default()
        };
        let ds = design(&duty, &steep).unwrap();
        let ps = predict(
            &duty,
            &ds,
            &PerformanceInput::default(),
            &CollectorData::default(),
        )
        .unwrap();
        assert!(ds.blade_count < d.blade_count);
        let relative = |p: &PumpPerformance| p.shutoff_head / p.design.head;
        assert!(
            relative(&ps) > relative(&p) + 0.1,
            "{} vs {}",
            relative(&ps),
            relative(&p)
        );
    }

    /// nq ≈ 16, the casing of Gülich Fig. 4.13.
    fn low_nq_duty() -> PumpDuty {
        PumpDuty {
            head: 94.0,
            machine_head: 94.0,
            ..duty()
        }
    }

    #[test]
    fn a_low_nq_volute_pump_follows_the_measured_curve_of_fig_4_13() {
        let duty = low_nq_duty();
        let d = design(&duty, &CentrifugalPumpInput::default()).unwrap();
        assert!((15.0..17.0).contains(&d.nq), "{}", d.nq);
        let input = PerformanceInput {
            points: 61,
            ..PerformanceInput::default()
        };
        let p = predict(&duty, &d, &input, &volute_collector_for(&d, &duty, 1.0)).unwrap();
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert!(
            p.points
                .windows(2)
                .all(|w| w[1].head <= w[0].head + 1e-9 * duty.head)
        );
        let at = |ratio: f64| {
            let q = ratio * duty.flow;
            let i = p
                .points
                .partition_point(|x| x.flow < q)
                .clamp(1, p.points.len() - 1);
            let (a, b) = (p.points[i - 1], p.points[i]);
            (a.head + (b.head - a.head) * (q - a.flow) / (b.flow - a.flow)) / p.design.head
        };
        // H/H_opt of the nq = 16 impeller in Fig. 4.13, read off the figure.
        for (ratio, measured) in [
            (0.0, 1.21),
            (0.35, 1.175),
            (0.5, 1.15),
            (0.7, 1.10),
            (0.8, 1.075),
            (1.2, 0.89),
            (1.4, 0.79),
        ] {
            let predicted = at(ratio);
            assert!(
                (predicted - measured).abs() < 0.06,
                "q* {ratio}: {predicted:.3} against {measured}"
            );
        }
    }

    #[test]
    fn the_slip_factor_follows_fig_4_15() {
        // The mean line of Gülich Fig. 4.15, read off the figure.
        for (ratio, read) in [
            (0.4, 1.10),
            (0.5, 1.065),
            (0.6, 1.04),
            (0.8, 1.01),
            (1.0, 1.0),
            (1.3, 1.0),
            (1.6, 0.98),
        ] {
            assert!(
                (slip_factor_ratio(ratio) - read).abs() < 0.006,
                "q* {ratio}: {}",
                slip_factor_ratio(ratio)
            );
        }
        assert_relative_eq!(slip_factor_ratio(0.0), slip_factor_ratio(0.4));
        assert_relative_eq!(slip_factor_ratio(2.0), slip_factor_ratio(1.6));
    }

    #[test]
    fn the_shutoff_power_follows_fig_4_10() {
        // Volute pumps of Gülich Fig. 4.10, read off the figure.
        for (nq, read) in [(10.0, 0.028), (20.4, 0.040), (30.2, 0.050), (66.4, 0.077)] {
            let fit = shutoff_power_coefficient(nq);
            assert!((fit / read - 1.0).abs() < 0.16, "nq {nq}: {fit:.4}");
        }
        let duty = low_nq_duty();
        let d = design(&duty, &CentrifugalPumpInput::default()).unwrap();
        let p = predict(
            &duty,
            &d,
            &PerformanceInput::default(),
            &CollectorData::default(),
        )
        .unwrap();
        let u2 = d.outlet.triangle.u;
        let lambda = 2.0 * p.shutoff_power
            / (duty.density * PI * d.outlet_mean_diameter * d.outlet_width * u2.powi(3));
        assert_relative_eq!(lambda, shutoff_power_coefficient(d.nq), max_relative = 1e-9);
    }

    #[test]
    fn a_shutoff_head_below_the_curve_is_raised() {
        // In the reference pump the head rise of the blades stays below what the
        // computed curve reaches at the onset of recirculation.
        let (d, p) = performance(&PerformanceInput::default());
        assert_eq!(p.shutoff_source, ShutoffSource::Curve);
        let rise = crate::pump::correlations::head_rise_for_blades(
            d.nq,
            d.blade_count,
            d.outlet.blade_angle,
        );
        assert!(p.shutoff_head > p.design.head * (1.0 + rise));
        assert_relative_eq!(
            p.shutoff_coefficient,
            2.0 * G * p.shutoff_head / d.outlet.triangle.u.powi(2),
            max_relative = 1e-12
        );
        assert!(!p.warnings.iter().any(|w| w.contains("unstable")));
        assert!(
            p.points
                .windows(2)
                .all(|w| w[1].head <= w[0].head + 1e-9 * p.design.head)
        );
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
        volute_collector_for(d, &duty(), throat_scale)
    }

    fn volute_collector_for(
        d: &CentrifugalPumpDesign,
        duty: &PumpDuty,
        throat_scale: f64,
    ) -> CollectorData {
        use crate::outlet::ImpellerOutlet;
        use crate::volute::{VoluteDesign, VoluteInput};
        let outlet = ImpellerOutlet::from_pump(d, duty, 0.0, 1.0);
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
            throat_integral: Some(volute.throat_integral().unwrap() * throat_scale),
            cutwater: Some((volute.cutwater_flow_angle, volute.cutwater_angle)),
        }
    }

    #[test]
    fn the_throat_sets_the_best_efficiency_flow() {
        // Gülich eq. T4.1.3 by hand for the reference pump and its angular-momentum
        // volute.
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let collector = volute_collector(&d, 1.0);
        let j = collector.throat_integral.unwrap();
        let o = &d.outlet;
        let area2 = PI * d.outlet_mean_diameter * d.outlet_width;
        let eta_v = d.efficiencies.volumetric;
        let by_hand = eta_v * area2 * o.triangle.u * o.slip_factor
            / (o.blockage / o.blade_angle.tan() + 2.0 * PI * d.outlet_width * eta_v / j);
        assert_relative_eq!(
            throat_best_efficiency_flow(&d, j),
            by_hand,
            max_relative = 1e-12
        );
        // A volute sized by angular momentum for the design flow puts it near there.
        let p = with_collector(&collector);
        let ratio = p.throat_best_flow.unwrap() / duty().flow;
        assert!((0.85..1.15).contains(&ratio), "{ratio}");
        // A larger throat moves it to a larger flow.
        let wide = with_collector(&volute_collector(&d, 1.4));
        assert!(wide.throat_best_flow.unwrap() > p.throat_best_flow.unwrap());
        assert!(
            with_collector(&CollectorData::default())
                .throat_best_flow
                .is_none()
        );
    }

    fn with_collector(collector: &CollectorData) -> PumpPerformance {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        predict(&duty(), &d, &PerformanceInput::default(), collector).unwrap()
    }

    #[test]
    fn the_hydraulic_losses_add_up_to_the_hydraulic_efficiency() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let collector = volute_collector(&d, 1.0);
        let p = with_collector(&collector);
        let h = p.hydraulic.unwrap();
        // Impeller, outlet shock and volute leave the design head (Gülich Table 3.8).
        assert_relative_eq!(
            p.design.head,
            h.theoretical_head - h.total(),
            max_relative = 0.005
        );
        assert_relative_eq!(h.efficiency, 1.0 - h.total() / h.theoretical_head);
        assert!(h.impeller.friction > 0.0 && h.outlet_shock > 0.0 && h.volute_friction > 0.0);
        assert_relative_eq!(h.volute, p.volute.unwrap().design.total());
        // The sum lands within the scatter of the statistics for the reference pump.
        assert!((h.efficiency - d.efficiencies.hydraulic).abs() < 0.03);
        assert_relative_eq!(h.statistical_efficiency, d.efficiencies.hydraulic);
        // A volute with too small a throat loses more, and so does the stage.
        let narrow = with_collector(&volute_collector(&d, 0.6))
            .hydraulic
            .unwrap();
        assert!(narrow.volute > h.volute && narrow.efficiency < h.efficiency);
        // The statistics, or no volute to sum, keep the statistical η_h.
        let statistics = PerformanceInput {
            losses: LossInput {
                statistics: true,
                ..LossInput::default()
            },
            ..PerformanceInput::default()
        };
        assert!(
            predict(&duty(), &d, &statistics, &collector)
                .unwrap()
                .hydraulic
                .is_none()
        );
        assert!(
            with_collector(&CollectorData::default())
                .hydraulic
                .is_none()
        );
    }

    #[test]
    fn the_loss_analysis_sums_leakage_disk_friction_and_mechanical_losses() {
        let (d, p) = performance(&PerformanceInput::default());
        let l = p.losses.unwrap();
        // Close to the design head: the impeller passes less leakage than the
        // statistics assume.
        assert_relative_eq!(p.design.head, 32.0, max_relative = 0.015);
        assert!(l.volumetric_efficiency > 0.95 && l.volumetric_efficiency < 0.995);
        // The power balance of eq. T3.5.1 at the design point.
        let leakage = l.leakage_at(p.design.head, p.design.theoretical_head);
        let balance =
            998.2 * G * p.design.theoretical_head * (duty().flow + leakage) + l.secondary_power();
        assert_relative_eq!(p.design.power, balance, max_relative = 1e-9);
        // Both estimates of the efficiency within a few points.
        assert!((p.design.efficiency - d.efficiencies.overall).abs() < 0.06);
        assert_relative_eq!(p.statistical_efficiency, d.efficiencies.overall);
        // Wider clearances leak much more; a viscous liquid rubs more on the disks.
        let wide = performance(&PerformanceInput {
            wide_seal_clearances: true,
            ..PerformanceInput::default()
        })
        .1
        .losses
        .unwrap();
        assert!(wide.leakage > 2.0 * l.leakage);
        // The losses are those of water; an oil goes through the viscous correction.
        let mut oil = duty();
        oil.kinematic_viscosity = 5e-5;
        let p_oil = predict(
            &oil,
            &d,
            &PerformanceInput::default(),
            &CollectorData::default(),
        )
        .unwrap();
        assert_relative_eq!(p_oil.losses.unwrap().leakage, l.leakage);
        assert!(p_oil.viscous.efficiency_factor < 1.0);
        // The leakage falls toward shut-off as the head across the seal changes little
        // and grows with it.
        assert!(l.leakage_at(1.2 * p.design.head, p.design.theoretical_head) > l.leakage);
    }

    #[test]
    fn the_efficiency_is_the_product_of_its_parts() {
        let (_, p) = performance(&PerformanceInput::default());
        let b = p.breakdown.unwrap();
        let l = p.losses.unwrap();
        assert_relative_eq!(
            b.hydraulic * b.volumetric * b.disk_friction * b.mechanical,
            p.design.efficiency,
            max_relative = 1e-12
        );
        assert_relative_eq!(b.overall, p.design.efficiency);
        assert_relative_eq!(b.inner * b.mechanical, b.overall, max_relative = 1e-12);
        // η_v is Q/(Q + Q_sp) with the leakage at the head of the design point.
        let leakage = l.leakage_at(p.design.head, p.design.theoretical_head);
        assert_relative_eq!(
            b.volumetric,
            duty().flow / (duty().flow + leakage),
            max_relative = 1e-9
        );
        assert_relative_eq!(b.mechanical, 1.0 - l.mechanical / p.design.power);
        let inner_power = p.design.power - l.mechanical;
        let disk = l.front_friction + l.rear_friction;
        assert_relative_eq!(b.disk_friction, 1.0 - disk / inner_power);
        // With the statistics there are no separate losses to split.
        let statistics = PerformanceInput {
            losses: LossInput {
                statistics: true,
                ..LossInput::default()
            },
            ..PerformanceInput::default()
        };
        assert!(performance(&statistics).1.breakdown.is_none());
    }

    #[test]
    fn a_matched_volute_takes_a_share_of_the_losses_at_the_design_head() {
        let (_, plain) = performance(&PerformanceInput::default());
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let p = with_collector(&volute_collector(&d, 1.0));
        // Sized by angular momentum, the volute is nearly the matched one of the split.
        assert_relative_eq!(p.design.head, 32.0, max_relative = 0.015);
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
    fn the_flow_meets_the_cutwater_within_3_degrees_about_the_design_point() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let collector = volute_collector(&d, 1.0);
        let p = with_collector(&collector);
        let (lo, hi) = p.volute.unwrap().incidence_range.unwrap();
        let q = duty().flow;
        assert!(lo < q && hi > q, "{} to {}", lo / q, hi / q);
        // At the ends of the band the incidence is ±3°, positive at part load.
        let (flow_angle, blade_angle) = collector.cutwater.unwrap();
        let at = |flow: f64| {
            let point = p
                .points
                .windows(2)
                .find(|w| w[0].flow <= flow && w[1].flow >= flow)
                .map(|w| {
                    let t = (flow - w[0].flow) / (w[1].flow - w[0].flow);
                    w[0].theoretical_head + t * (w[1].theoretical_head - w[0].theoretical_head)
                })
                .unwrap();
            cutwater_incidence(
                flow_angle,
                blade_angle,
                flow / q,
                point / p.design.theoretical_head,
            )
        };
        assert_relative_eq!(at(lo), INCIDENCE_LIMIT, max_relative = 0.05);
        assert_relative_eq!(at(hi), -INCIDENCE_LIMIT, max_relative = 0.05);
        // A cutwater turned by +2° meets the design flow at +2°: the band moves to
        // larger flows.
        let mut turned = collector;
        turned.cutwater = Some((flow_angle, blade_angle + 2f64.to_radians()));
        let (lo2, hi2) = with_collector(&turned)
            .volute
            .unwrap()
            .incidence_range
            .unwrap();
        assert!(lo2 > lo && hi2 > hi);
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
