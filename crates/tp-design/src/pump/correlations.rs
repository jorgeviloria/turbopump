//! Empirical correlations and design limits for radial (centrifugal) pump impellers.
//!
//! Every correlation and default used for an automatic value or a design check is listed
//! in [`CORRELATIONS`] with its source and how far it has been checked. Entries marked
//! [`Verification::Verified`] were checked equation by equation against the cited
//! edition; equation numbers refer to it.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

/// How far a correlation has been checked against its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// Checked equation by equation against the cited edition.
    Verified,
    /// Standard relation reproduced identically across the literature.
    Standard,
    /// Coefficients not yet checked against the cited publication.
    Pending,
    /// Not a published correlation: a typical engineering value, to be adjusted.
    EngineeringDefault,
}

/// Description of an empirical correlation, design limit or default value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorrelationInfo {
    pub id: &'static str,
    pub description: &'static str,
    pub reference: &'static str,
    pub verification: Verification,
}

/// The edition every `Verified` Gülich entry was checked against.
pub const GULICH: &str = "J. F. Gülich, Centrifugal Pumps, 2nd ed., Springer 2010";

/// Every correlation, limit and default used by the centrifugal pump design.
pub const CORRELATIONS: &[CorrelationInfo] = &[
    CorrelationInfo {
        id: "pump.pressure_coefficient",
        description: "ψ_opt = 1.21·exp(−0.77·nq/100), ψ = 2gH/u₂²",
        reference: "Gülich 2nd ed., eq. 3.26",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.hydraulic_efficiency",
        description: "best hydraulic efficiency: single-stage and multistage radial pumps",
        reference: "Gülich 2nd ed., Table 3.9, eqs. 3.9.6 and 3.9.8",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.overall_efficiency",
        description: "best overall efficiency: single-stage, multistage and double-entry pumps",
        reference: "Gülich 2nd ed., Table 3.9, eqs. 3.9.1, 3.9.2 and 3.9.4",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.efficiency_tolerance",
        description: "scatter of the efficiency correlations Δη = ±0.2·(1 − η)",
        reference: "Gülich 2nd ed., eq. 3.9.10",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.outlet_width_ratio",
        description: "b₂/d₂ = 0.017 + 0.262x − 0.08x² + 0.0093x³, x = nq/100",
        reference: "Gülich 2nd ed., eq. 7.1",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.slip_factor",
        description: "slip factor γ = f₁(1 − √sin β₂B / z^0.7)·k_w, f₁ = 0.98 for radial impellers",
        reference: "Gülich 2nd ed., Table 3.2, eqs. 3.2.4–3.2.7 (after Wiesner 1967)",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.blockage",
        description: "blade blockage τ = 1/(1 − z·e/(π·d·sin β_B·sin λ))",
        reference: "Gülich 2nd ed., eqs. 3.1.7 and 3.2.3",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.inlet_diameter",
        description: "eye diameter d₁ = f_d1·d₁,min of minimum relative velocity; f_d1 1.15→1.05 \
                      (normal) or 1.25→1.15 (suction impeller) for nq 15→40",
        reference: "Gülich 2nd ed., Table 7.1, eq. 7.1.4",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.shaft_diameter",
        description: "d_w = (16·P/(π·ω·τ_al))^(1/3) with the design-point power; the book \
                      uses the maximum power of the operating range",
        reference: "Gülich 2nd ed., eq. 7.1.2",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.blade_count",
        description: "Pfleiderer's estimate z = 6.5·(d₂ + d₁m)/(d₂ − d₁m)·sin β_m, rounded and \
                      kept within the 5–7 blades recommended for 10 < nq < 120, within 3–12 \
                      outside that range; a prescribed head rise picks 3–8",
        reference: "Pfleiderer 1961; range from Gülich 2nd ed., pp. 342–343",
        verification: Verification::Standard,
    },
    CorrelationInfo {
        id: "pump.head_rise",
        description: "head rise from BEP to shut-off (H₀ − H)/H = 0.063·z^−0.632·nq^(0.995 − 0.052·z) \
                      with the standard outlet angle of each blade number; our fit to the chart \
                      (within 2 points for nq 4–68, z 3–8). Another angle enters through the \
                      mean of z and the blade number whose standard angle it is (our \
                      interpolation)",
        reference: "Lobanoff & Ross, Centrifugal Pumps: Design and Application, 2nd ed., Fig. 3-2",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.standard_outlet_angle",
        description: "standard outlet blade angle β₂B = 5.8° + 4.233°·z − 0.1667°·z² for 3–8 \
                      blades (17° for 3, 25° for 6, 29° for 8); our fit to the chart",
        reference: "Lobanoff & Ross, Centrifugal Pumps: Design and Application, 2nd ed., Fig. 3-13",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.inlet_area_ratio",
        description: "area between the blades at the inlet over the eye area, A_v/A_e = \
                      0.275 + 0.00496·nq ± 0.055; A_v = z·a₁·b₁ with the throat \
                      a₁ = π·d₁·sin β₁B/z − e at the outer streamline of the eye and \
                      b₁ = (d₁ − d_n)/2; our fit to the chart. Reported only: the band \
                      reflects large eyes (β₁B ≈ 15–20°), not the eye of minimum w₁",
        reference: "Lobanoff & Ross, Centrifugal Pumps: Design and Application, 2nd ed., Fig. 3-12",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.blade_thickness",
        description: "blade thickness max(3 mm, 0.016·d₂); recommended e/d₂ = 0.016–0.022, at \
                      least 3–5 mm for castings",
        reference: "Gülich 2nd ed., p. 347",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.incidence",
        description: "leading-edge incidence 0–4°",
        reference: "Gülich 2nd ed., Table 7.1, eq. 7.1.7",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.outlet_blade_angle",
        description: "outlet blade angle usually 15–45°, often 20–27°",
        reference: "Gülich 2nd ed., p. 346",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.deviation_angle",
        description: "deviation δ₂ = β₂B − β₂ should not exceed 10–14°",
        reference: "Gülich 2nd ed., p. 346",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.de_haller",
        description: "deceleration w₂/w₁ at the outer streamline ≥ 0.7 (de Haller)",
        reference: "Gülich 2nd ed., p. 346",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.outlet_width_limit",
        description: "outlet width b₂ below inlet width b₁ = (d₁ − d_n)/2",
        reference: "Gülich 2nd ed., p. 345 and eq. 7.2",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "pump.volumetric_efficiency",
        description: "volumetric efficiency 0.97",
        reference: "typical value; replaced by an annular-seal leakage model later",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "pump.shaft_shear_stress",
        description: "allowable torsional shear stress of the shaft, 25 MPa",
        reference: "typical preliminary value for steel shafts",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "pump.suction_specific_speed",
        description: "suction specific speed nss = 200 for the NPSH3 estimate",
        reference: "typical value of conventional impellers (rpm, m³/s, m)",
        verification: Verification::EngineeringDefault,
    },
];

/// Reference flow Q_Ref = 1 m³/s of the efficiency correlations.
const Q_REF: f64 = 1.0;
/// Reference specific speed nq,Ref = 100 of the ψ and b₂ correlations.
const NQ_REF: f64 = 100.0;

/// Smallest flow (m³/s) covered by the efficiency correlations (Gülich, Table 3.9).
pub const MIN_EFFICIENCY_FLOW: f64 = 0.005;

/// Which set of efficiency correlations applies to a pump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpLayout {
    /// Single-stage pump with a single-entry impeller.
    SingleStage,
    /// Multistage pump: nq per stage.
    Multistage,
    /// Single-stage pump with a double-entry impeller: Q of the whole pump, nq per eye.
    DoubleEntry,
}

impl PumpLayout {
    /// Upper nq limit of the efficiency correlations for this layout.
    pub fn max_nq(self) -> f64 {
        match self {
            PumpLayout::SingleStage => 100.0,
            PumpLayout::Multistage => 60.0,
            PumpLayout::DoubleEntry => 50.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PumpLayout::SingleStage => "single-stage",
            PumpLayout::Multistage => "multistage",
            PumpLayout::DoubleEntry => "double-entry",
        }
    }
}

/// Exponent m of the efficiency correlations: `base` is 0.1 for the overall and 0.08
/// for the hydraulic efficiency (Gülich, Table 3.9).
fn efficiency_exponent(base: f64, q: f64, nq: f64) -> f64 {
    let a = if q <= 1.0 { 1.0 } else { 0.5 };
    base * a * (Q_REF / q).powf(0.15) * (45.0 / nq).powf(0.06)
}

/// Best overall efficiency for the pump flow `q` (m³/s) and `nq` per stage and eye
/// (Gülich, Table 3.9, eqs. 3.9.1, 3.9.2, 3.9.4).
pub fn overall_efficiency(layout: PumpLayout, q: f64, nq: f64) -> f64 {
    let r = Q_REF / q;
    let m = efficiency_exponent(0.1, q, nq);
    match layout {
        PumpLayout::SingleStage => {
            1.0 - 0.095 * r.powf(m) - 0.3 * (0.35 - (nq / 23.0).log10()).powi(2) * r.powf(0.05)
        }
        PumpLayout::Multistage => {
            1.0 - 0.116 * r.powf(m) - 0.4 * (0.26 - (nq / 25.0).log10()).powi(2)
        }
        PumpLayout::DoubleEntry => {
            1.0 - 0.095 * r.powf(m) - 0.35 * (0.35 - (nq / 17.7).log10()).powi(2) * r.powf(0.05)
        }
    }
}

/// Best hydraulic efficiency for flow `q` (m³/s) and `nq` (Gülich, Table 3.9,
/// eqs. 3.9.6 and 3.9.8).
///
/// Table 3.9 has no hydraulic efficiency for double-entry pumps; they use the
/// single-stage equation, and callers pass the flow of one eye.
pub fn hydraulic_efficiency(layout: PumpLayout, q: f64, nq: f64) -> f64 {
    let r = Q_REF / q;
    let m = efficiency_exponent(0.08, q, nq);
    match layout {
        PumpLayout::SingleStage | PumpLayout::DoubleEntry => {
            1.0 - 0.055 * r.powf(m) - 0.2 * (0.26 - (nq / 25.0).log10()).powi(2) * r.powf(0.1)
        }
        PumpLayout::Multistage => {
            1.0 - 0.065 * r.powf(m) - 0.23 * (0.3 - (nq / 23.0).log10()).powi(2) * r.powf(0.05)
        }
    }
}

/// Scatter of the efficiency correlations, Δη = 0.2·(1 − η) (Gülich, eq. 3.9.10).
pub fn efficiency_tolerance(eta: f64) -> f64 {
    0.2 * (1.0 - eta)
}

/// Optimum pressure coefficient ψ_opt = 1.21·exp(−0.77·nq/nq,Ref) of radial impellers,
/// with ψ = 2gH/u₂² (Gülich, eq. 3.26).
pub fn pressure_coefficient(nq: f64) -> f64 {
    1.21 * (-0.77 * nq / NQ_REF).exp()
}

/// Impeller outlet width ratio b₂/d₂ of single-entry radial impellers
/// (Gülich, eq. 7.1).
pub fn outlet_width_ratio(nq: f64) -> f64 {
    let x = nq / NQ_REF;
    0.017 + 0.262 * x - 0.08 * x * x + 0.0093 * x * x * x
}

/// Purpose of the impeller inlet, which sets the eye-diameter factor f_d1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InletDesign {
    /// Normal impeller with sufficient NPSH available, e.g. series stages.
    #[default]
    Normal,
    /// Suction impeller designed for low NPSH required.
    Suction,
}

/// Eye-diameter factor f_d1 = d₁ / d₁,min: 1.15 → 1.05 (normal) or 1.25 → 1.15 (suction
/// impeller) falling from nq = 15 to 40 (Gülich, Table 7.1), interpolated linearly and
/// held constant outside that range.
pub fn inlet_diameter_factor(design: InletDesign, nq: f64) -> f64 {
    let start = match design {
        InletDesign::Normal => 1.15,
        InletDesign::Suction => 1.25,
    };
    let x = ((nq - 15.0) / 25.0).clamp(0.0, 1.0);
    start - 0.1 * x
}

/// Pfleiderer's blade number z = k·(d₂ + d₁)/(d₂ − d₁)·sin((β₁B + β₂B)/2), k = 6.5 for
/// cast impellers. Not rounded.
pub fn pfleiderer_blade_count(d1: f64, d2: f64, beta1b: f64, beta2b: f64) -> f64 {
    6.5 * (d2 + d1) / (d2 - d1) * (0.5 * (beta1b + beta2b)).sin()
}

/// Eye diameter d₁,min that minimizes the relative velocity at the shroud of a
/// swirl-free inlet, for flow `q` (m³/s, through one eye), angular speed `omega`
/// (rad/s) and hub diameter `dn` (m):
///
/// d₁,min² = d_n² + 2·(4·Q / (π·ω))^(2/3).
///
/// This is the exact minimum of w₁² = c_m1² + u₁²; Gülich's eq. 7.1.4 is the same
/// relation written with rounded coefficients. Without hub, tan β₁ = 1/√2.
pub fn min_relative_velocity_eye_diameter(q: f64, omega: f64, dn: f64) -> f64 {
    (dn * dn + 2.0 * (4.0 * q / (PI * omega)).powf(2.0 / 3.0)).sqrt()
}

pub const VOLUMETRIC_EFFICIENCY: f64 = 0.97;
pub const SHAFT_SHEAR_STRESS: f64 = 25e6;
pub const SUCTION_SPECIFIC_SPEED: f64 = 200.0;

/// Typical blade thickness of a cast impeller of outlet diameter `d2`.
pub fn blade_thickness(d2: f64) -> f64 {
    (0.016 * d2).max(0.003)
}

/// Head rise from the best efficiency point to shut-off, (H₀ − H)/H, of an impeller
/// with `z` blades at their standard outlet angle ([`standard_outlet_blade_angle`]):
/// 0.063·z^−0.632·nq^(0.995 − 0.052·z). Our fit to Lobanoff & Ross, Fig. 3-2, read at
/// Ns = 200–3500 (US units, Ns = 51.65·nq) for 3–8 blades; it stays within two points of
/// the chart there. Fewer blades and smaller outlet angles give steeper curves.
pub fn head_rise(nq: f64, z: f64) -> f64 {
    0.063 * z.powf(-0.632) * nq.max(0.0).powf(0.995 - 0.052 * z)
}

/// Blade number, not rounded, whose [`standard_outlet_blade_angle`] is `beta2b` (rad),
/// held within 3–8.
pub fn equivalent_blade_count(beta2b: f64) -> f64 {
    let (lo, hi) = (
        standard_outlet_blade_angle(3).to_degrees(),
        standard_outlet_blade_angle(8).to_degrees(),
    );
    let beta = beta2b.to_degrees().clamp(lo, hi);
    let (a, b, c) = (-0.1667, 4.233, 5.8 - beta);
    (-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)
}

/// Head rise (H₀ − H)/H of `z` blades at the outlet angle `beta2b` (rad): [`head_rise`]
/// at the mean of `z` and the [`equivalent_blade_count`] of the angle, so that a smaller
/// angle steepens the curve as fewer blades do. With the standard angle it is the chart
/// value itself; the averaging is our interpolation, the chart only gives the pairs.
pub fn head_rise_for_blades(nq: f64, z: u32, beta2b: f64) -> f64 {
    head_rise(nq, 0.5 * (f64::from(z) + equivalent_blade_count(beta2b)))
}

/// Range of nq covered by [`head_rise`].
pub const HEAD_RISE_NQ_RANGE: (f64, f64) = (4.0, 68.0);

/// Standard outlet blade angle of an impeller with `z` blades, rad:
/// 5.8° + 4.233°·z − 0.1667°·z², our fit to Lobanoff & Ross, Fig. 3-13, for 3–8 blades
/// and held at its end values outside them.
pub fn standard_outlet_blade_angle(z: u32) -> f64 {
    let z = f64::from(z.clamp(3, 8));
    (5.8 + 4.233 * z - 0.1667 * z * z).to_radians()
}

/// Blade numbers of the head-rise chart. With 7 and 8 blades its curves droop toward
/// shut-off, and Gülich advises against 8 or more for the stability of the Q-H curve.
pub const HEAD_RISE_BLADE_RANGE: (u32, u32) = (3, 8);

/// The blade number within [`HEAD_RISE_BLADE_RANGE`] whose head rise at `nq` comes
/// closest to `target`: at the outlet angle `beta2b` (rad) when it is fixed, otherwise
/// at the standard angle of each blade number.
pub fn blade_count_for_head_rise(nq: f64, target: f64, beta2b: Option<f64>) -> u32 {
    let (lo, hi) = HEAD_RISE_BLADE_RANGE;
    let rise = |z: u32| match beta2b {
        Some(beta) => head_rise_for_blades(nq, z, beta),
        None => head_rise(nq, f64::from(z)),
    };
    (lo..=hi)
        .min_by(|&a, &b| {
            (rise(a) - target)
                .abs()
                .total_cmp(&(rise(b) - target).abs())
        })
        .expect("the range is not empty")
}

/// Recommended band (low, high) of the area between the blades at the inlet over the
/// eye area, A_v/A_e = 0.275 + 0.00496·nq ± 0.055: our fit to Lobanoff & Ross, Fig. 3-12.
pub fn inlet_area_ratio_band(nq: f64) -> (f64, f64) {
    let centre = 0.275 + 0.00496 * nq;
    (centre - 0.055, centre + 0.055)
}

/// Range of nq (rpm, m³/s, m) covered by the radial-impeller correlations.
pub const NQ_RANGE: (f64, f64) = (10.0, 120.0);
/// Blade numbers Gülich recommends for radial impellers within [`NQ_RANGE`]; Pfleiderer's
/// automatic estimate stays within them there.
pub const RADIAL_BLADE_COUNT_RANGE: (u32, u32) = (5, 7);
/// Blade numbers Pfleiderer's automatic estimate may take outside [`NQ_RANGE`].
pub const BLADE_COUNT_RANGE: (u32, u32) = (3, 12);
/// Usual outlet blade angles, degrees.
pub const OUTLET_BLADE_ANGLE_RANGE: (f64, f64) = (15.0, 45.0);
/// Recommended incidence, degrees.
pub const INCIDENCE_RANGE: (f64, f64) = (0.0, 4.0);
/// Largest recommended deviation angle δ₂ = β₂B − β₂, degrees.
pub const MAX_DEVIATION_ANGLE: f64 = 14.0;
/// Smallest recommended deceleration w₂/w₁ at the outer streamline.
pub const DE_HALLER_LIMIT: f64 = 0.7;

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn every_correlation_is_documented() {
        for c in CORRELATIONS {
            assert!(
                !c.reference.is_empty() && !c.description.is_empty(),
                "{}",
                c.id
            );
        }
        let mut ids: Vec<_> = CORRELATIONS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CORRELATIONS.len(), "duplicate ids");
        assert!(
            !CORRELATIONS
                .iter()
                .any(|c| c.verification == Verification::Pending),
            "every Gülich correlation has been checked against the 2nd edition"
        );
    }

    #[test]
    fn pressure_coefficient_trend() {
        assert_relative_eq!(pressure_coefficient(0.0), 1.21);
        assert!(pressure_coefficient(20.0) > pressure_coefficient(60.0));
        assert_relative_eq!(pressure_coefficient(100.0), 1.21 * (-0.77f64).exp());
    }

    #[test]
    fn overall_efficiency_of_the_reference_pump() {
        // Q = 100 m³/h, nq = 35.92: m = 0.1·36^0.15·(45/35.92)^0.06 = 0.1735,
        // η = 1 − 0.095·36^m − 0.3·(0.35 − log(35.92/23))²·36^0.05 = 0.8144.
        let q = 100.0 / 3600.0;
        assert_relative_eq!(
            overall_efficiency(PumpLayout::SingleStage, q, 35.92),
            0.8144,
            epsilon = 5e-4
        );
        assert_relative_eq!(
            hydraulic_efficiency(PumpLayout::SingleStage, q, 35.92),
            0.9065,
            epsilon = 5e-4
        );
    }

    #[test]
    fn efficiencies_are_plausible_and_ordered() {
        for layout in [
            PumpLayout::SingleStage,
            PumpLayout::Multistage,
            PumpLayout::DoubleEntry,
        ] {
            for (q, nq) in [(0.005, 15.0), (0.03, 36.0), (0.3, 45.0), (3.0, 50.0)] {
                let eta_h = hydraulic_efficiency(layout, q, nq);
                let eta = overall_efficiency(layout, q, nq);
                assert!(
                    (0.5..1.0).contains(&eta_h),
                    "{layout:?} ηh = {eta_h} at Q = {q}"
                );
                assert!(
                    (0.3..eta_h).contains(&eta),
                    "{layout:?} η = {eta} at Q = {q}"
                );
            }
            // Larger pumps are more efficient.
            assert!(overall_efficiency(layout, 1.0, 30.0) > overall_efficiency(layout, 0.01, 30.0));
        }
        // Multistage pumps lose more in the return channels than single-stage ones.
        assert!(
            overall_efficiency(PumpLayout::Multistage, 0.05, 30.0)
                < overall_efficiency(PumpLayout::SingleStage, 0.05, 30.0)
        );
        // Double-entry pumps beat single-entry ones at low nq (half the disk friction).
        assert!(
            overall_efficiency(PumpLayout::DoubleEntry, 0.1, 20.0)
                > overall_efficiency(PumpLayout::SingleStage, 0.1, 20.0)
        );
        assert_relative_eq!(efficiency_tolerance(0.8), 0.04, epsilon = 1e-12);
    }

    #[test]
    fn outlet_width_grows_with_specific_speed() {
        let ratios: Vec<f64> = [10.0, 20.0, 40.0, 80.0].map(outlet_width_ratio).to_vec();
        assert!(ratios.windows(2).all(|w| w[1] > w[0]));
        assert!(
            (0.03..0.07).contains(&ratios[1]),
            "b2/d2 at nq 20 = {}",
            ratios[1]
        );
    }

    #[test]
    fn inlet_diameter_factor_follows_table_7_1() {
        assert_relative_eq!(inlet_diameter_factor(InletDesign::Normal, 10.0), 1.15);
        assert_relative_eq!(inlet_diameter_factor(InletDesign::Normal, 40.0), 1.05);
        assert_relative_eq!(inlet_diameter_factor(InletDesign::Normal, 27.5), 1.10);
        assert_relative_eq!(inlet_diameter_factor(InletDesign::Suction, 15.0), 1.25);
        assert_relative_eq!(inlet_diameter_factor(InletDesign::Suction, 80.0), 1.15);
    }

    #[test]
    fn eye_diameter_minimizes_relative_velocity() {
        let (q, omega, dn) = (0.03, 300.0, 0.02);
        let d1 = min_relative_velocity_eye_diameter(q, omega, dn);
        let w = |d: f64| {
            let cm = q / (PI / 4.0 * (d * d - dn * dn));
            cm.hypot(omega * d / 2.0)
        };
        assert!(w(d1) < w(d1 * 1.01) && w(d1) < w(d1 * 0.99));

        // Without hub the optimum flow angle at the shroud is atan(1/√2).
        let d = min_relative_velocity_eye_diameter(q, omega, 0.0);
        let cm = q / (PI / 4.0 * d * d);
        assert_relative_eq!(cm / (omega * d / 2.0), 0.5_f64.sqrt(), epsilon = 1e-12);
    }

    #[test]
    fn eye_diameter_agrees_with_gulich_eq_7_1_4() {
        // d₁* = √(d_n*² + 1.48·10⁻³·ψ·nq^1.33/η_v^0.67) for the reference pump gives
        // 71.98 mm; the exact minimum differs only by the rounding of those coefficients.
        let omega = 2900.0 * PI / 30.0;
        let d1 = min_relative_velocity_eye_diameter(0.02864, omega, 0.01912);
        assert_relative_eq!(d1, 0.07198, max_relative = 0.01);
    }

    #[test]
    fn head_rise_follows_the_chart() {
        // Points read off Lobanoff & Ross Fig. 3-2 (Ns US → nq = Ns/51.65).
        for (ns, z, chart) in [
            (600.0, 3, 0.247),
            (1400.0, 4, 0.349),
            (1000.0, 5, 0.201),
            (2200.0, 6, 0.270),
            (3000.0, 7, 0.240),
            (1800.0, 8, 0.133),
        ] {
            let fit = head_rise(ns / 51.65, f64::from(z));
            assert!(
                (fit - chart).abs() < 0.02,
                "z = {z}, Ns = {ns}: {fit} vs {chart}"
            );
        }
        // Steeper with fewer blades, at every nq.
        for nq in [10.0, 30.0, 60.0] {
            assert!((3..12).all(|z| head_rise(nq, f64::from(z)) > head_rise(nq, f64::from(z + 1))));
        }
    }

    #[test]
    fn standard_outlet_angles_follow_the_chart() {
        for (z, chart) in [(3, 17.0), (4, 20.0), (5, 22.8), (6, 25.1), (8, 29.0)] {
            let fit = standard_outlet_blade_angle(z).to_degrees();
            assert!((fit - chart).abs() < 0.4, "z = {z}: {fit}°");
        }
        assert_eq!(
            standard_outlet_blade_angle(12),
            standard_outlet_blade_angle(8)
        );
    }

    #[test]
    fn blade_count_for_a_head_rise() {
        // The book's example: Ns ≈ 1680, a 20 % rise → six blades at 25°.
        let nq = 1684.0 / 51.65;
        assert_eq!(blade_count_for_head_rise(nq, 0.20, None), 6);
        assert_eq!(blade_count_for_head_rise(nq, 0.55, None), 3);
        assert_eq!(
            blade_count_for_head_rise(nq, 0.0, None),
            HEAD_RISE_BLADE_RANGE.1
        );
        // A flatter fixed angle steepens the curve, so it needs more blades for 20 %.
        assert!(blade_count_for_head_rise(nq, 0.20, Some(17f64.to_radians())) > 6);
    }

    #[test]
    fn the_outlet_angle_shapes_the_head_rise() {
        for z in 3..=8 {
            let standard = standard_outlet_blade_angle(z);
            assert_relative_eq!(
                equivalent_blade_count(standard),
                f64::from(z),
                epsilon = 1e-9
            );
            assert_relative_eq!(
                head_rise_for_blades(30.0, z, standard),
                head_rise(30.0, f64::from(z)),
                epsilon = 1e-12
            );
        }
        // A smaller angle steepens the curve; radial blades flatten it.
        let six = head_rise_for_blades(30.0, 6, 25f64.to_radians());
        assert!(head_rise_for_blades(30.0, 6, 18f64.to_radians()) > six);
        assert!(head_rise_for_blades(30.0, 6, 90f64.to_radians()) < six);
    }

    #[test]
    fn inlet_area_band_rises_with_specific_speed() {
        let (lo, hi) = inlet_area_ratio_band(1000.0 / 51.65);
        assert!(
            (0.31..0.33).contains(&lo) && (0.42..0.44).contains(&hi),
            "{lo} {hi}"
        );
    }

    #[test]
    fn pfleiderer_typical_value() {
        let z = pfleiderer_blade_count(0.07, 0.2, 20f64.to_radians(), 25f64.to_radians());
        assert_relative_eq!(z, 5.17, epsilon = 0.01);
    }
}
