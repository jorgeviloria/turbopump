//! Empirical correlations for radial (centrifugal) pump impellers.
//!
//! Every correlation used for an automatic value is listed in [`CORRELATIONS`] with its
//! source and how far it has been checked. Coefficients marked
//! [`Verification::Pending`] are reproduced from the literature as the authors know it
//! but have not yet been checked line by line against the original publication. They
//! must be verified before the design results are used for real machines.

use std::f64::consts::PI;

/// How far a correlation has been checked against its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// Standard relation reproduced identically across the literature.
    Standard,
    /// Coefficients not yet checked against the cited publication.
    Pending,
    /// Not a published correlation: a typical engineering value, to be adjusted.
    EngineeringDefault,
}

/// Description of an empirical correlation or default value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorrelationInfo {
    pub id: &'static str,
    pub description: &'static str,
    pub reference: &'static str,
    pub verification: Verification,
}

const GULICH: &str = "J. F. Gülich, Centrifugal Pumps, 4th ed., Springer 2020";

/// Every correlation and default value used by the centrifugal pump design.
pub const CORRELATIONS: &[CorrelationInfo] = &[
    CorrelationInfo {
        id: "pump.pressure_coefficient",
        description: "optimum pressure coefficient ψ = 2gH/u₂² versus nq",
        reference: GULICH,
        verification: Verification::Pending,
    },
    CorrelationInfo {
        id: "pump.hydraulic_efficiency",
        description: "best hydraulic efficiency versus flow and nq",
        reference: GULICH,
        verification: Verification::Pending,
    },
    CorrelationInfo {
        id: "pump.overall_efficiency",
        description: "best overall efficiency of single-stage, single-entry pumps",
        reference: GULICH,
        verification: Verification::Pending,
    },
    CorrelationInfo {
        id: "pump.outlet_width_ratio",
        description: "impeller outlet width ratio b₂/d₂ versus nq",
        reference: GULICH,
        verification: Verification::Pending,
    },
    CorrelationInfo {
        id: "pump.slip_factor",
        description: "slip factor of radial impellers (Wiesner, f₁ by Gülich)",
        reference: "F. J. Wiesner, J. Eng. Power 89 (1967); Gülich ch. 3 for f₁",
        verification: Verification::Pending,
    },
    CorrelationInfo {
        id: "pump.blade_count",
        description: "number of blades z = 6.5·(d₂ + d₁m)/(d₂ − d₁m)·sin β_m",
        reference: "C. Pfleiderer, Die Kreiselpumpen für Flüssigkeiten und Gase, Springer 1961",
        verification: Verification::Standard,
    },
    CorrelationInfo {
        id: "pump.inlet_diameter",
        description: "eye diameter that minimizes the relative velocity at the shroud",
        reference: "continuity and velocity triangle, see docs/theory/dimensiones-principales.md",
        verification: Verification::Standard,
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
    CorrelationInfo {
        id: "pump.blade_thickness",
        description: "blade thickness max(3 mm, 0.016·d₂)",
        reference: "typical value for cast impellers",
        verification: Verification::EngineeringDefault,
    },
];

/// Reference flow Q_Ref = 1 m³/s of the efficiency correlations.
const Q_REF: f64 = 1.0;
/// Reference specific speed nq,Ref = 100 of the ψ and b₂ correlations.
const NQ_REF: f64 = 100.0;

/// Optimum pressure coefficient ψ_opt = 1.21·exp(−0.77·nq/nq,Ref) of radial impellers,
/// with ψ = 2gH/u₂² (Gülich, eq. 3.26 with f_T = 1).
pub fn pressure_coefficient(nq: f64) -> f64 {
    1.21 * (-0.77 * nq / NQ_REF).exp()
}

/// Exponent m of the efficiency correlations (Gülich, Table 3.9).
fn efficiency_exponent(q: f64, nq: f64) -> f64 {
    let a = if q <= 1.0 { 1.0 } else { 0.5 };
    0.08 * a * (Q_REF / q).powf(0.15) * (45.0 / nq).powf(0.06)
}

/// Best hydraulic efficiency η_h,opt for flow `q` (m³/s, per impeller eye) and `nq`
/// (Gülich, Table 3.9).
pub fn hydraulic_efficiency(q: f64, nq: f64) -> f64 {
    let m = efficiency_exponent(q, nq);
    1.0 - 0.055 * (Q_REF / q).powf(m)
        - 0.2 * (0.26 - (nq / 25.0).log10()).powi(2) * (Q_REF / q).powf(0.1)
}

/// Best overall efficiency η_opt of single-stage, single-entry radial pumps
/// (Gülich, Table 3.9).
pub fn overall_efficiency(q: f64, nq: f64) -> f64 {
    let m = efficiency_exponent(q, nq);
    1.0 - 0.095 * (Q_REF / q).powf(m)
        - 0.3 * (0.35 - (nq / 23.0).log10()).powi(2) * (Q_REF / q).powf(0.05)
}

/// Impeller outlet width ratio b₂/d₂ of single-entry radial impellers
/// (Gülich, eq. 7.1).
pub fn outlet_width_ratio(nq: f64) -> f64 {
    let x = nq / NQ_REF;
    0.017 + 0.262 * x - 0.08 * x * x + 0.0093 * x * x * x
}

/// Pfleiderer's blade number z = k·(d₂ + d₁)/(d₂ − d₁)·sin((β₁B + β₂B)/2), k = 6.5 for
/// cast impellers. Not rounded.
pub fn pfleiderer_blade_count(d1: f64, d2: f64, beta1b: f64, beta2b: f64) -> f64 {
    6.5 * (d2 + d1) / (d2 - d1) * (0.5 * (beta1b + beta2b)).sin()
}

/// Eye diameter d₁ that minimizes the relative velocity at the shroud of a swirl-free
/// inlet, for flow `q` (m³/s, through one eye), angular speed `omega` (rad/s) and hub
/// diameter `dn` (m):
///
/// d₁² = d_n² + 2·(4·Q / (π·ω))^(2/3).
///
/// At this diameter tan β₁ = c_m1/u₁ = 1/√2 without hub, i.e. β₁ ≈ 35.3°.
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

/// Range of nq (rpm, m³/s, m) over which the radial-impeller correlations are used.
pub const NQ_RANGE: (f64, f64) = (10.0, 100.0);

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
    }

    #[test]
    fn pressure_coefficient_trend() {
        assert_relative_eq!(pressure_coefficient(0.0), 1.21);
        assert!(pressure_coefficient(20.0) > pressure_coefficient(60.0));
        assert_relative_eq!(pressure_coefficient(100.0), 1.21 * (-0.77f64).exp());
    }

    #[test]
    fn efficiencies_are_plausible_and_ordered() {
        for (q, nq) in [(0.005, 15.0), (0.03, 36.0), (0.3, 50.0), (3.0, 60.0)] {
            let eta_h = hydraulic_efficiency(q, nq);
            let eta = overall_efficiency(q, nq);
            assert!((0.5..1.0).contains(&eta_h), "ηh = {eta_h} at Q = {q}");
            assert!((0.3..eta_h).contains(&eta), "η = {eta} at Q = {q}");
        }
        // Larger pumps are more efficient.
        assert!(overall_efficiency(1.0, 40.0) > overall_efficiency(0.01, 40.0));
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
    fn pfleiderer_typical_value() {
        let z = pfleiderer_blade_count(0.07, 0.2, 20f64.to_radians(), 25f64.to_radians());
        assert_relative_eq!(z, 5.17, epsilon = 0.01);
    }
}
