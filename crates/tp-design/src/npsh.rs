//! Required NPSH of a radial impeller against flow, after Gülich 2nd ed., Chap. 6.3.3.
//!
//! 1. NPSH₃ = λ_c·c₁m²/2g + λ_w·w₁²/2g (eq. 6.10), with c₁m at the eye and w₁ at the
//!    outer streamline. λ_c = 1.1 for axial inflow. At the design point the blade
//!    coefficient comes from the flow angle at the outer streamline,
//!    λ_w,3 = 0.3·(tan β₁a)^0.57 (Fig. 6.20).
//! 2. Below the flow Q_sa the curve follows eq. 6.10 with
//!    λ_w = λ_w,opt·(tan β₁/tan β₁,opt)^0.57 (eq. 6.21).
//! 3. Q_sa marks the steep rise: the smaller of the flow Q_c at which the collector
//!    throat velocity equals c₂u (eq. 6.19) and Q_PS = (Q_SF + Q_W)/2 (eq. 6.20), with
//!    Q_SF the flow of shockless entry (eq. 6.17) and Q_W the flow at which w₁ equals
//!    the velocity w₁q in the inlet throat of the blades (eq. 6.18).
//! 4. Above Q_sa, NPSH₃ = NPSH_sa·(Q/Q_sa)^x with x = 2 to 3 (eq. 6.22).
//!
//! The inflow is taken swirl-free (α₁ = 90°). The inlet throat of the blades is
//! estimated from the main dimensions as the pitch times the sine of the blade angle
//! less the blade thickness at the mean diameter, over the eye width (our estimate).

use std::f64::consts::PI;

use serde::Serialize;

use tp_units::STANDARD_GRAVITY as G;

use crate::pump::{CentrifugalPumpDesign, InletDesign, PumpLayout};

/// λ_c for axial inflow (Gülich Chap. 6.3.2); radial inlet casings take 1.2 to 1.35.
pub const AXIAL_INLET_COEFFICIENT: f64 = 1.1;
/// Exponent x of eq. 6.22, in the middle of Gülich's 2 to 3.
pub const DEFAULT_EXPONENT: f64 = 2.5;
/// Range of flow angles at the outer streamline covered by the data of Fig. 6.20, rad.
pub const BLADE_COEFFICIENT_RANGE: (f64, f64) = (10.0 * PI / 180.0, 25.0 * PI / 180.0);

/// Blade coefficient for a 3 % head drop at q* = 1, from the flow angle β₁a at the
/// outer streamline: λ_w,3 = 0.3·(tan β₁a)^0.57 (Gülich Fig. 6.20, ±25 %).
pub fn blade_coefficient(flow_angle: f64) -> f64 {
    0.3 * flow_angle.tan().max(0.0).powf(0.57)
}

/// NPSH = λ_c·c₁m²/2g + λ_w·w₁²/2g (Gülich eq. 6.10), m.
pub fn npsh(inlet_coefficient: f64, blade_coefficient: f64, cm1: f64, w1: f64) -> f64 {
    (inlet_coefficient * cm1 * cm1 + blade_coefficient * w1 * w1) / (2.0 * G)
}

/// Throat of the collector after the impeller: the volute throat, or the passages
/// between diffuser vanes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollectorThroat {
    /// z_Le
    pub count: u32,
    /// Area of each throat A₃q, m².
    pub area: f64,
}

/// Flow Q_c at which the mean throat velocity of the collector equals c₂u:
/// Q_c = u₂·A₂·γ / (A₂/(z_Le·A₃q) + τ₂/(η_v·tan β₂B)) (Gülich eq. 6.19), with A₂ the
/// whole outlet area of the impeller, m³/s.
pub fn collector_flow(design: &CentrifugalPumpDesign, throat: CollectorThroat) -> f64 {
    let outlet = &design.outlet;
    let area2 = PI * design.outlet_mean_diameter * design.outlet_width;
    let throats = f64::from(throat.count) * throat.area;
    outlet.triangle.u * area2 * outlet.slip_factor
        / (area2 / throats
            + outlet.blockage / (design.efficiencies.volumetric * outlet.blade_angle.tan()))
}

/// Settings of the NPSH₃ curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpshSettings {
    /// λ_c
    pub inlet_coefficient: f64,
    /// λ_w at the design point; `None` for Fig. 6.20.
    pub blade_coefficient: Option<f64>,
    /// x of eq. 6.22.
    pub exponent: f64,
}

impl Default for NpshSettings {
    fn default() -> Self {
        Self {
            inlet_coefficient: AXIAL_INLET_COEFFICIENT,
            blade_coefficient: None,
            exponent: DEFAULT_EXPONENT,
        }
    }
}

/// The NPSH₃ curve of one impeller.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct NpshCurve {
    pub inlet_coefficient: f64,
    /// λ_w at the design point.
    pub blade_coefficient: f64,
    /// The flow angle at the outer streamline lies outside the data of Fig. 6.20.
    pub extrapolated: bool,
    pub exponent: f64,
    /// Flow of shockless entry Q_SF, m³/s.
    pub shockless_flow: f64,
    /// Flow Q_W at which w₁ = w₁q; `None` when the blade throat is not narrower than
    /// the eye, m³/s.
    pub throat_flow: Option<f64>,
    /// Flow Q_c at which the collector throat velocity equals c₂u, m³/s.
    pub collector_flow: Option<f64>,
    /// Q_sa, where the steep rise begins, m³/s.
    pub knee_flow: f64,
    /// NPSH₃ at Q_sa, m.
    pub knee_npsh: f64,
    /// NPSH₃ at the design flow, m.
    pub design_npsh: f64,
    design_flow: f64,
    /// Q_La/Q at the design point.
    impeller_ratio: f64,
    /// c₁m at the design flow, m/s.
    design_cm1: f64,
    /// Peripheral speed at the outer streamline of the leading edge, m/s.
    outer_speed: f64,
}

impl NpshCurve {
    /// Builds the curve of an impeller with main dimensions `design` for the design
    /// flow `flow` (m³/s), with the collector `throat` after it if known.
    pub fn new(
        design: &CentrifugalPumpDesign,
        flow: f64,
        throat: Option<CollectorThroat>,
        settings: &NpshSettings,
    ) -> Self {
        let outer = &design.inlet_shroud;
        let mean = &design.inlet_mean;
        let design_cm1 = outer.triangle.cm;
        let outer_speed = outer.triangle.u;
        let impeller_ratio = design.impeller_flow / flow;
        let flow_angle = design_cm1.atan2(outer_speed);
        let blade_coefficient = settings
            .blade_coefficient
            .unwrap_or_else(|| blade_coefficient(flow_angle));
        let (lo, hi) = BLADE_COEFFICIENT_RANGE;
        let extrapolated = settings.blade_coefficient.is_none() && !(lo..=hi).contains(&flow_angle);

        // Pump flow at which the eye carries the meridional velocity `cm1`.
        let flow_at = |cm1: f64| flow * cm1 / design_cm1;
        // Shockless entry at the outer streamline (eq. 6.17 with α₁ = 90°).
        let shockless_flow = flow_at(outer_speed * outer.blade_angle.tan() / outer.blockage);
        // w₁ = w₁q (eq. 6.18 with α₁ = 90°). The inlet throat of the blades is
        // estimated as width × (pitch × sin β₁B − e) at the mean diameter.
        let d1 = design.inlet_diameter;
        let dn = design.hub_diameter;
        let eye_area = PI / 4.0 * (d1 * d1 - dn * dn);
        let throats = 0.5
            * (d1 - dn)
            * (PI * mean.diameter * mean.blade_angle.sin()
                - f64::from(design.blade_count) * design.blade_thickness);
        let throat_flow = (throats > 0.0)
            .then(|| eye_area / throats)
            .filter(|ratio| *ratio > 1.0)
            .map(|ratio| flow_at(mean.triangle.u / (ratio * ratio - 1.0).sqrt()));
        let pressure_side = match throat_flow {
            Some(q) => 0.5 * (shockless_flow + q),
            None => shockless_flow,
        };
        let collector_flow = throat.map(|t| collector_flow(design, t));
        let knee_flow = collector_flow.map_or(pressure_side, |q| q.min(pressure_side));

        let mut curve = Self {
            inlet_coefficient: settings.inlet_coefficient,
            blade_coefficient,
            extrapolated,
            exponent: settings.exponent,
            shockless_flow,
            throat_flow,
            collector_flow,
            knee_flow,
            knee_npsh: 0.0,
            design_npsh: 0.0,
            design_flow: flow,
            impeller_ratio,
            design_cm1,
            outer_speed,
        };
        curve.knee_npsh = curve.below_knee(knee_flow);
        curve.design_npsh = curve.at(flow);
        curve
    }

    /// Eq. 6.10 with the blade coefficient of eq. 6.21.
    fn below_knee(&self, flow: f64) -> f64 {
        let cm1 = self.design_cm1 * flow / self.design_flow;
        let tan_ratio = cm1 / self.design_cm1;
        let lambda_w = self.blade_coefficient * tan_ratio.max(0.0).powf(0.57);
        let w1 = cm1.hypot(self.outer_speed);
        npsh(self.inlet_coefficient, lambda_w, cm1, w1)
    }

    /// NPSH₃ at pump flow `flow` (m³/s), m.
    pub fn at(&self, flow: f64) -> f64 {
        if flow <= self.knee_flow {
            self.below_knee(flow)
        } else {
            self.knee_npsh * (flow / self.knee_flow).powf(self.exponent)
        }
    }

    /// Q_La/Q used to turn the inlet velocities into pump flows.
    pub fn impeller_ratio(&self) -> f64 {
        self.impeller_ratio
    }

    /// The smallest flow up to `max_flow` at which NPSH₃ reaches `available`, m³/s.
    pub fn cavitation_flow(&self, available: f64, max_flow: f64) -> Option<f64> {
        if self.at(max_flow) < available {
            return None;
        }
        let (mut lo, mut hi) = (0.0, max_flow);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if self.at(mid) < available {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(hi)
    }
}

/// One way of estimating NPSH₃ at the best-efficiency point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NpshMethod {
    /// NPSH₃ = λ_c·c₁m²/2g + λ_w·w₁²/2g with λ_c from 1.1 (axial inflow) to 1.35 (radial
    /// inlet casings) and λ_w,3 from 0.1 to 0.3 for impellers (Gülich 2nd ed., eq. 6.10,
    /// Chap. 6.3.2 and Table D6.3); the coefficients of Pfleiderer's method.
    InletCoefficients,
    /// NPSH₃ = (n·√(Q/f_q)/n_ss)^(4/3), n in rpm, with the n_ss range of the impeller
    /// type (Gülich 2nd ed., eq. 6.7 and Table D6.1).
    SuctionSpecificSpeed,
    /// The suction specific speed S = N·√gpm/NPSH^(3/4) = 7900 of single-suction pumps
    /// (11 200 for double suction with the whole flow, i.e. 7900 per eye) in US units
    /// (Stepanoff, *Centrifugal and Axial Flow Pumps*, 2nd ed., 1957, eqs. 12.16 and
    /// 12.18, p. 254). The same as the Thoma number σ = 1.22·10⁻³·nq^(4/3).
    Stepanoff,
    /// NPSH₃ = (n·√Q/S_q)^(4/3)/g with n in 1/s and the suction number S_q = 0.4 to 0.45
    /// (Petermann, in Pfleiderer and Petermann, *Strömungsmaschinen*).
    Petermann,
    /// NPSH₃ = (0.3 to 0.5)·n·√Q with n in 1/s, Q in m³/s and NPSH₃ in m (Europump, *NPSH
    /// for Rotodynamic Pumps: a Reference Guide*, 1999).
    Europump,
}

impl NpshMethod {
    /// Every method, in the order they are reported.
    pub const ALL: [NpshMethod; 5] = [
        NpshMethod::InletCoefficients,
        NpshMethod::SuctionSpecificSpeed,
        NpshMethod::Stepanoff,
        NpshMethod::Petermann,
        NpshMethod::Europump,
    ];

    /// Short English name.
    pub fn name(self) -> &'static str {
        match self {
            NpshMethod::InletCoefficients => "inlet coefficients λc, λw",
            NpshMethod::SuctionSpecificSpeed => "suction specific speed nss",
            NpshMethod::Stepanoff => "Stepanoff",
            NpshMethod::Petermann => "Petermann",
            NpshMethod::Europump => "Europump",
        }
    }
}

/// The NPSH₃ of one method: a range, or one value when `low == high`, m.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NpshEstimate {
    pub method: NpshMethod,
    pub low: f64,
    pub high: f64,
}

/// NPSH₃ at the best-efficiency point by several published methods. Their scatter is
/// wide: they are estimates for the preliminary design, and NPSH₃ itself is a test result
/// (3 % head drop).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NpshBand {
    pub estimates: Vec<NpshEstimate>,
    /// Lowest and highest value of all the methods, m.
    pub low: f64,
    pub high: f64,
}

/// λ_c of radial inlet casings, the top of Gülich's range (Chap. 6.3.2).
pub const RADIAL_INLET_COEFFICIENT: f64 = 1.35;
/// λ_w,3 of impellers at shockless entry (Gülich Table D6.3).
pub const BLADE_COEFFICIENT_BAND: (f64, f64) = (0.1, 0.3);
/// S of single-suction pumps in US units: rpm, US gpm, ft (Stepanoff eq. 12.18).
pub const STEPANOFF_SUCTION_SPEED: f64 = 7900.0;
/// Suction number S_q of Petermann's method.
pub const PETERMANN_SUCTION_NUMBER: (f64, f64) = (0.4, 0.45);
/// Factor of n·√Q in Europump's estimate.
pub const EUROPUMP_FACTOR: (f64, f64) = (0.3, 0.5);

/// Range of the suction specific speed n_ss (rpm, m³/s, m) of an impeller type (Gülich
/// 2nd ed., Table D6.1):
/// - standard impellers with axial inlet or of between-bearing pumps, 160–220;
/// - suction impellers with axial inlet, 220–280;
/// - suction impellers of between-bearing pumps, single or double entry, 180–240;
/// - high-pressure pumps for short cavities (u₁ > 50 m/s), 160–190.
pub fn suction_specific_speed_band(layout: PumpLayout, inlet: InletDesign, u1: f64) -> (f64, f64) {
    match (layout, inlet) {
        (PumpLayout::DoubleEntry, InletDesign::Suction) => (180.0, 240.0),
        (_, InletDesign::Suction) => (220.0, 280.0),
        _ if u1 > 50.0 => (160.0, 190.0),
        _ => (160.0, 220.0),
    }
}

/// NPSH₃ by every [`NpshMethod`] for speed `omega` (rad/s) and flow `flow_per_eye`
/// (m³/s), with c₁m and w₁ at the outer streamline of the eye and the n_ss range
/// `nss` (rpm, m³/s, m).
pub fn npsh3_band(omega: f64, flow_per_eye: f64, cm1: f64, w1: f64, nss: (f64, f64)) -> NpshBand {
    const FOOT: f64 = 0.3048;
    const US_GALLON: f64 = 3.785_411_784e-3;
    let rpm = omega * 30.0 / PI;
    let per_second = omega / (2.0 * PI);
    let root_q = flow_per_eye.sqrt();
    let suction_speed = |n: f64, s: f64| (n * root_q / s).powf(4.0 / 3.0);
    let gpm = flow_per_eye * 60.0 / US_GALLON;
    let stepanoff = FOOT * (rpm * gpm.sqrt() / STEPANOFF_SUCTION_SPEED).powf(4.0 / 3.0);
    let (lw, hw) = BLADE_COEFFICIENT_BAND;
    let (sq_low, sq_high) = PETERMANN_SUCTION_NUMBER;
    let (e_low, e_high) = EUROPUMP_FACTOR;
    let estimates: Vec<NpshEstimate> = NpshMethod::ALL
        .iter()
        .map(|&method| {
            let (low, high) = match method {
                NpshMethod::InletCoefficients => (
                    npsh(AXIAL_INLET_COEFFICIENT, lw, cm1, w1),
                    npsh(RADIAL_INLET_COEFFICIENT, hw, cm1, w1),
                ),
                // A higher suction specific speed means a lower NPSH₃.
                NpshMethod::SuctionSpecificSpeed => {
                    (suction_speed(rpm, nss.1), suction_speed(rpm, nss.0))
                }
                NpshMethod::Stepanoff => (stepanoff, stepanoff),
                NpshMethod::Petermann => (
                    suction_speed(per_second, sq_high) / G,
                    suction_speed(per_second, sq_low) / G,
                ),
                NpshMethod::Europump => (e_low * per_second * root_q, e_high * per_second * root_q),
            };
            NpshEstimate { method, low, high }
        })
        .collect();
    let low = estimates
        .iter()
        .map(|e| e.low)
        .fold(f64::INFINITY, f64::min);
    let high = estimates
        .iter()
        .map(|e| e.high)
        .fold(f64::NEG_INFINITY, f64::max);
    NpshBand {
        estimates,
        low,
        high,
    }
}

/// Which rule of Gülich's Table 6.2 sets the NPSH available a pump needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MarginRule {
    /// The minimum for any pump: NPSH_A ≥ 1.25·NPSH₃, and at least NPSH₃ + 0.6 m.
    Minimum,
    /// Water at u₁ = 10 to 50 m/s, to limit noise, vibration and erosion:
    /// σ_A ≥ σ₃ + F_R·F_F·{0.05 + 2·(u₁/u_Ref)³}, u_Ref = 100 m/s.
    Water,
    /// Water at u₁ above 50 m/s: past the margins of the table, the cavity length or
    /// noise must be checked against experience (Table 6.1); the minimum still holds.
    CheckCavity,
}

/// The NPSH available a pump needs to run safely at its design flow, after Gülich 2nd
/// ed., Table 6.2 (Chap. 6.7, p. 326), and the rule that set it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AvailableNeed {
    /// NPSH_A needed, m.
    pub required: f64,
    pub rule: MarginRule,
}

/// Peripheral speed u_Ref of Table 6.2, m/s.
const REFERENCE_SPEED: f64 = 100.0;

/// What Gülich's Table 6.2 asks of the NPSH available, from the NPSH₃ at the design
/// flow (m), the peripheral speed u₁ at the impeller inlet (m/s) and whether the liquid
/// is water. For water the fluid factor F_F is 1 (dissolved gas above 5 ppm, below
/// 200 °C) and the risk factor F_R is 1 (operation at 0.8 < q* < 1.1); σ = 2g·NPSH/u₁²
/// (eq. 6.5). Other liquids take the minimum only.
pub fn available_need(npsh3: f64, u1: f64, water: bool) -> AvailableNeed {
    let minimum = (1.25 * npsh3).max(npsh3 + 0.6);
    let head = u1 * u1 / (2.0 * G);
    if water && u1 > 50.0 {
        return AvailableNeed {
            required: minimum,
            rule: MarginRule::CheckCavity,
        };
    }
    if water && u1 >= 10.0 {
        let water = npsh3 + head * (0.05 + 2.0 * (u1 / REFERENCE_SPEED).powi(3));
        if water > minimum {
            return AvailableNeed {
                required: water,
                rule: MarginRule::Water,
            };
        }
    }
    AvailableNeed {
        required: minimum,
        rule: MarginRule::Minimum,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pump::{CentrifugalPumpInput, PumpDuty, design};
    use approx::assert_relative_eq;

    const FT: f64 = 0.3048;

    #[test]
    fn the_band_of_each_method_follows_its_formula() {
        // 0.06687 m³/s at 1750 rpm, c₁m 4.554 m/s and w₁ 13.62 m/s at the eye diameter.
        let omega = 1750.0 * PI / 30.0;
        let (q, cm1, w1) = (0.06687, 4.554, 13.62);
        let band = npsh3_band(omega, q, cm1, w1, (160.0, 220.0));
        let get = |m: NpshMethod| *band.estimates.iter().find(|e| e.method == m).unwrap();
        let g2 = 2.0 * G;
        let c = get(NpshMethod::InletCoefficients);
        assert_relative_eq!(
            c.low,
            (1.1 * cm1 * cm1 + 0.1 * w1 * w1) / g2,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            c.high,
            (1.35 * cm1 * cm1 + 0.3 * w1 * w1) / g2,
            max_relative = 1e-12
        );
        let n = get(NpshMethod::SuctionSpecificSpeed);
        assert_relative_eq!(
            n.high,
            (1750.0 * q.sqrt() / 160.0).powf(4.0 / 3.0),
            max_relative = 1e-12
        );
        assert_relative_eq!(
            n.low,
            (1750.0 * q.sqrt() / 220.0).powf(4.0 / 3.0),
            max_relative = 1e-12
        );
        // Stepanoff in US units: 1060 gpm at 1750 rpm and S = 7900 give 13.94 ft; the
        // Thoma form σ = 1.22e-3·nq^(4/3) gives the same within 0.3 %.
        let s = get(NpshMethod::Stepanoff);
        assert_relative_eq!(s.low / FT, 13.94, max_relative = 2e-3);
        let nq = tp_units::SpecificSpeed::from_head(q, 83.0, omega).nq();
        assert_relative_eq!(
            s.low,
            1.22e-3 * nq.powf(4.0 / 3.0) * 83.0,
            max_relative = 3e-3
        );
        let p = get(NpshMethod::Petermann);
        let rev = 1750.0 / 60.0;
        assert_relative_eq!(
            p.high,
            (rev * q.sqrt() / 0.4).powf(4.0 / 3.0) / G,
            max_relative = 1e-12
        );
        let e = get(NpshMethod::Europump);
        assert_relative_eq!(e.low, 0.3 * rev * q.sqrt(), max_relative = 1e-12);
        assert_relative_eq!(e.high, 0.5 * rev * q.sqrt(), max_relative = 1e-12);
        for m in &band.estimates {
            assert!(
                m.low <= m.high && m.low >= band.low && m.high <= band.high,
                "{m:?}"
            );
        }
    }

    #[test]
    fn suction_impellers_reach_a_higher_suction_specific_speed() {
        use crate::pump::{InletDesign, PumpLayout};
        let at = |layout, inlet| suction_specific_speed_band(layout, inlet, 30.0);
        assert_eq!(
            at(PumpLayout::SingleStage, InletDesign::Normal),
            (160.0, 220.0)
        );
        assert_eq!(
            at(PumpLayout::SingleStage, InletDesign::Suction),
            (220.0, 280.0)
        );
        assert_eq!(
            at(PumpLayout::DoubleEntry, InletDesign::Suction),
            (180.0, 240.0)
        );
        assert_eq!(
            suction_specific_speed_band(PumpLayout::Multistage, InletDesign::Normal, 60.0),
            (160.0, 190.0)
        );
    }

    #[test]
    fn eq_6_10_reproduces_the_npsh_of_the_book_example() {
        // Japikse et al., Fig. 7.73: c₁m = 13.343 ft/s and w₁ = 55.88 ft/s at the inlet
        // tip, a blade cavitation coefficient of 0.200 and NPSH = 12.47 ft.
        let value = npsh(1.0, 0.2, 13.343 * FT, 55.88 * FT);
        assert_relative_eq!(value / FT, 12.47, epsilon = 0.01);
    }

    #[test]
    fn the_blade_coefficient_follows_fig_6_20() {
        // The fitted line of the figure passes about 0.15 at 17° and 0.19 at 24°; at
        // 10° the formula printed with it gives 0.11, a little under the drawn line.
        let at = |deg: f64| blade_coefficient(deg.to_radians());
        assert_relative_eq!(at(10.0), 0.112, epsilon = 0.001);
        assert_relative_eq!(at(17.0), 0.15, epsilon = 0.005);
        assert_relative_eq!(at(24.0), 0.19, epsilon = 0.005);
    }

    fn reference() -> (CentrifugalPumpDesign, f64) {
        let duty = PumpDuty {
            flow: 100.0 / 3600.0,
            head: 32.0,
            machine_head: 32.0,
            stages: 1,
            speed: 2900.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
            kinematic_viscosity: 1.0e-6,
        };
        (
            design(&duty, &CentrifugalPumpInput::default()).unwrap(),
            duty.flow,
        )
    }

    #[test]
    fn the_curve_of_the_reference_pump() {
        let (d, flow) = reference();
        // The reference volute: one throat of 3123 mm².
        let throat = CollectorThroat {
            count: 1,
            area: 3123e-6,
        };
        let curve = NpshCurve::new(&d, flow, Some(throat), &NpshSettings::default());
        // Zero incidence: shockless entry at the design flow.
        assert_relative_eq!(curve.shockless_flow, flow, max_relative = 1e-9);
        // The collector throat reaches c₂u about a quarter above the design flow.
        let qc = curve.collector_flow.unwrap() / flow;
        assert!((1.15..1.35).contains(&qc), "{qc}");
        assert!(curve.knee_flow > flow && curve.knee_flow <= curve.collector_flow.unwrap());
        // At the design point eq. 6.10 with λ_w from Fig. 6.20.
        let outer = &d.inlet_shroud.triangle;
        let lambda = blade_coefficient(outer.cm.atan2(outer.u));
        let expected = npsh(1.1, lambda, outer.cm, outer.cm.hypot(outer.u));
        assert_relative_eq!(curve.design_npsh, expected, max_relative = 1e-9);
        assert!(
            (3.0..6.0).contains(&curve.design_npsh),
            "{}",
            curve.design_npsh
        );
        // The eye of the reference pump is small: its flow angle is above the data.
        assert!(curve.extrapolated);
        // Rising everywhere, continuous at the knee, steeper beyond it.
        let flows: Vec<f64> = (1..=30).map(|i| flow * 0.05 * f64::from(i)).collect();
        assert!(flows.windows(2).all(|w| curve.at(w[1]) > curve.at(w[0])));
        let k = curve.knee_flow;
        assert_relative_eq!(
            curve.at(k * 0.999_999),
            curve.at(k * 1.000_001),
            max_relative = 1e-4
        );
        assert_relative_eq!(
            curve.at(1.2 * k) / curve.knee_npsh,
            1.2f64.powf(DEFAULT_EXPONENT),
            max_relative = 1e-9
        );
        // Low specific speed: NPSH₃ keeps falling at part load (Gülich Fig. 6.23).
        assert!(curve.at(0.6 * flow) < 0.8 * curve.design_npsh);
    }

    #[test]
    fn a_narrow_collector_throat_moves_the_knee_to_lower_flow() {
        let (d, flow) = reference();
        let settings = NpshSettings::default();
        let wide = NpshCurve::new(
            &d,
            flow,
            Some(CollectorThroat {
                count: 1,
                area: 3123e-6,
            }),
            &settings,
        );
        let narrow = NpshCurve::new(
            &d,
            flow,
            Some(CollectorThroat {
                count: 1,
                area: 1500e-6,
            }),
            &settings,
        );
        assert!(narrow.collector_flow.unwrap() < wide.collector_flow.unwrap());
        assert!(narrow.knee_flow < wide.knee_flow);
        assert!(narrow.at(1.3 * flow) > wide.at(1.3 * flow));
    }

    #[test]
    fn cavitation_flow_where_the_curve_meets_the_available_npsh() {
        let (d, flow) = reference();
        let curve = NpshCurve::new(&d, flow, None, &NpshSettings::default());
        let available = curve.at(1.1 * flow);
        let q = curve.cavitation_flow(available, 1.5 * flow).unwrap();
        assert_relative_eq!(q, 1.1 * flow, max_relative = 1e-6);
        assert!(curve.cavitation_flow(100.0, 1.5 * flow).is_none());
    }

    #[test]
    fn the_available_npsh_needs_the_margin_of_table_6_2() {
        // Small NPSH₃: 0.6 m over it rules; large: a quarter of it.
        let small = available_need(1.0, 5.0, true);
        assert_eq!(small.rule, MarginRule::Minimum);
        assert_relative_eq!(small.required, 1.6);
        assert_relative_eq!(available_need(8.0, 5.0, false).required, 10.0);
        // Water at 30 m/s: σ_A ≥ σ₃ + 0.05 + 2·0.3³ = σ₃ + 0.104, or 4.77 m over NPSH₃,
        // more than the minimum.
        let water = available_need(4.0, 30.0, true);
        assert_eq!(water.rule, MarginRule::Water);
        let head = 30.0 * 30.0 / (2.0 * G);
        assert_relative_eq!(water.required, 4.0 + head * 0.104, max_relative = 1e-12);
        // The same pump on another liquid takes the minimum.
        assert_eq!(available_need(4.0, 30.0, false).rule, MarginRule::Minimum);
        // Past 50 m/s the table asks for the cavity to be checked.
        assert_eq!(
            available_need(4.0, 60.0, true).rule,
            MarginRule::CheckCavity
        );
    }
}
