//! Performance with viscous liquids, after Gülich 2nd ed., Chap. 13.1 with Tables 3.10
//! and 13.1. The characteristic is first predicted for cold water (ν = 10⁻⁶ m²/s) and
//! then corrected (Table 13.1): Q_v = f_Q·Q_w, H_v = f_H(q*)·H_w and η_v = f_η·η_w for
//! each point, with q* = Q_w/Q_BEP,w and f_H(q*) = 1 − (1 − f_H)·q*^0.75 (eq. T13.1.6).
//!
//! The factors come from the loss analysis of Table 3.10:
//! - friction coefficient of the hydraulic channels at Re = ω·r₂²/ν: the larger of the
//!   turbulent (eqs. T3.10.16 and 1.33) and the laminar value (eqs. T3.10.17 and 1.33a).
//!   Gülich places the transition at Re_crit = 3·10⁶/(1 + 10⁴·Tu^1.7) (eq. 1.33b); taking
//!   the larger value assumes the early transition of the high turbulence in a pump and
//!   keeps the friction rising as Re falls (our choice). Channel roughness 16 µm, the
//!   equivalent sand roughness of class N9 (Chap. 1.5.2), is our choice too;
//! - f_ηh = 1 − ζ_R/ψ·(c_f,ν/c_f,w − 1) with ζ_R/ψ = (1/η_h − 1)·(0.635 − 0.0016·n_q·√f_q)
//!   (eqs. T3.10.18 and T3.10.19), and f_Q = f_H = f_ηh (eq. T3.10.22);
//! - f_η of eq. T3.10.20 with the disk friction and mechanical losses of the water
//!   prediction and the ratio of the disk friction coefficients at both viscosities
//!   (eq. T3.6.3); leakage unchanged (f_ηvol = 1, as the table assumes).
//!
//! The correlation of Figs. 13.8 and 13.9 (eqs. T13.1.1 to T13.1.4) is given for
//! comparison: f_H = Re_mod^(−6.7/Re_mod^0.735), f_η = Re_mod^(−19/Re_mod^0.705) with
//! Re_mod = Re·ω_s^1.5·f_q^0.75 and ω_s = n_q/52.9. Both sets are divided by their value
//! for water, so that water gives 1 (our choice: the correlation is not exactly 1 there).
//!
//! NPSH₃ at constant flow: f_NPSH = 1 + ζ_E·(c_f,ν/c_f,w)·c₁m²/(2g·NPSH₃) (eq. 13.12),
//! with ζ_E = λ_c − 1 (Chap. 6.3.2), also divided by its value for water.

use serde::Serialize;
use tp_units::STANDARD_GRAVITY as G;

use crate::losses::disk_friction_coefficient;

/// Kinematic viscosity of the cold water the characteristic is predicted for, m²/s.
pub const WATER_VISCOSITY: f64 = 1.0e-6;
/// Relative difference from cold water below which a liquid needs no correction.
pub const VISCOSITY_TOLERANCE: f64 = 0.05;
/// Equivalent sand roughness of the hydraulic channels, m: class N9 (our choice).
pub const CHANNEL_ROUGHNESS: f64 = 16e-6;

/// Friction coefficient of the hydraulic channels at Reynolds number `re` with relative
/// roughness `roughness` (ε/r₂): the larger of eqs. T3.10.16 (turbulent) and T3.10.17
/// (laminar).
pub fn channel_friction(re: f64, roughness: f64) -> f64 {
    let turbulent = 0.136 / (-(0.2 * roughness + 12.5 / re).log10()).powf(2.15);
    let laminar = 2.65 / re.powf(0.875) - 2.0 / (8.0 * re + 0.016 / re) + 1.328 / re.sqrt();
    turbulent.max(laminar)
}

/// The correlation of Gülich Figs. 13.8 and 13.9: (f_H,opt, f_η) at modified Reynolds
/// number `re_mod` (eqs. T13.1.3 and T13.1.4 with the mean exponents).
pub fn correlation_factors(re_mod: f64) -> (f64, f64) {
    let x = 6.7 / re_mod.powf(0.735);
    let y = 19.0 / re_mod.powf(0.705);
    (re_mod.powf(-x), re_mod.powf(-y))
}

/// What the water prediction knows at its best point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterBaseline {
    /// n_q per eye and the number of eyes f_q.
    pub nq: f64,
    pub eyes: f64,
    /// rad/s and m.
    pub omega: f64,
    pub impeller_radius: f64,
    /// Axial gap of the shrouds, m.
    pub axial_gap: f64,
    pub hydraulic_efficiency: f64,
    pub volumetric_efficiency: f64,
    /// Disk friction and mechanical losses over the useful power ρ·g·H·Q.
    pub disk_share: f64,
    pub mechanical_share: f64,
    /// λ_c of the NPSH₃ (Gülich eq. 6.10).
    pub inlet_coefficient: f64,
}

/// The correction of a water characteristic for a viscous liquid.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ViscousCorrection {
    /// m²/s
    pub viscosity: f64,
    /// Re = ω·r₂²/ν and the modified Re_mod of eq. T13.1.2.
    pub reynolds: f64,
    pub modified_reynolds: f64,
    /// c_f,ν/c_f,w of the hydraulic channels.
    pub friction_ratio: f64,
    /// k_RR,ν/k_RR,w of the disks.
    pub disk_ratio: f64,
    /// f_Q = f_H at the best point and f_η, by loss analysis.
    pub head_factor: f64,
    pub efficiency_factor: f64,
    /// The same by the correlation of Figs. 13.8 and 13.9.
    pub correlation_head_factor: f64,
    pub correlation_efficiency_factor: f64,
    /// ζ_E of the inlet for the NPSH₃ factor.
    pub inlet_loss: f64,
    /// Outside the tests behind the method (Table 13.1: 7 < n_q < 50, ν ≤ 4000 mm²/s).
    pub outside_tests: bool,
}

impl ViscousCorrection {
    /// The correction for a liquid of kinematic viscosity `viscosity` (m²/s).
    pub fn new(viscosity: f64, base: &WaterBaseline) -> Self {
        let r2 = base.impeller_radius;
        let reynolds_at = |nu: f64| base.omega * r2 * r2 / nu;
        let (re, re_w) = (reynolds_at(viscosity), reynolds_at(WATER_VISCOSITY));
        let roughness = CHANNEL_ROUGHNESS / r2;
        let friction_ratio = channel_friction(re, roughness) / channel_friction(re_w, roughness);
        let disk_ratio = disk_friction_coefficient(re, r2, base.axial_gap, 1.0)
            / disk_friction_coefficient(re_w, r2, base.axial_gap, 1.0);
        // Eqs. T3.10.18 to T3.10.20.
        let eta_h = base.hydraulic_efficiency;
        let share = (1.0 / eta_h - 1.0) * (0.635 - 0.0016 * base.nq * base.eyes.sqrt());
        let f_h = (1.0 - share * (friction_ratio - 1.0)).max(0.05);
        let loss = base.volumetric_efficiency * eta_h;
        let f_eta = f_h * (1.0 + (base.disk_share + base.mechanical_share) * loss)
            / (1.0 + (base.disk_share * disk_ratio + base.mechanical_share) * loss / f_h);
        // Figs. 13.8 and 13.9, divided by their value for water.
        let omega_s = base.nq / 52.9;
        let modified = |re: f64| re * omega_s.powf(1.5) * base.eyes.powf(0.75);
        let (re_mod, re_mod_w) = (modified(re), modified(re_w));
        let (ch, ce) = correlation_factors(re_mod);
        let (wh, we) = correlation_factors(re_mod_w);
        Self {
            viscosity,
            reynolds: re,
            modified_reynolds: re_mod,
            friction_ratio,
            disk_ratio,
            head_factor: f_h,
            efficiency_factor: f_eta,
            correlation_head_factor: ch / wh,
            correlation_efficiency_factor: ce / we,
            inlet_loss: (base.inlet_coefficient - 1.0).max(0.0),
            outside_tests: !(7.0..=50.0).contains(&base.nq) || viscosity > 4000e-6,
        }
    }

    /// f_Q.
    pub fn flow_factor(&self) -> f64 {
        self.head_factor
    }

    /// f_H at q* = Q_w/Q_BEP,w (eq. T13.1.6).
    pub fn head_factor_at(&self, flow_ratio: f64) -> f64 {
        1.0 - (1.0 - self.head_factor) * flow_ratio.max(0.0).powf(0.75)
    }

    /// NPSH₃ factor at constant flow for meridional velocity `c1m` (m/s) at the eye and
    /// water NPSH₃ `npsh` (m) (eq. 13.12, divided by its value for water).
    pub fn npsh_factor(&self, c1m: f64, npsh: f64) -> f64 {
        if npsh <= 0.0 {
            return 1.0;
        }
        let term = self.inlet_loss * c1m * c1m / (2.0 * G * npsh);
        (1.0 + term * self.friction_ratio) / (1.0 + term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn base() -> WaterBaseline {
        WaterBaseline {
            nq: 25.0,
            eyes: 1.0,
            omega: 1450.0 * std::f64::consts::PI / 30.0,
            impeller_radius: 0.13,
            axial_gap: 0.0065,
            hydraulic_efficiency: 0.88,
            volumetric_efficiency: 0.97,
            disk_share: 0.04,
            mechanical_share: 0.015,
            inlet_coefficient: 1.1,
        }
    }

    #[test]
    fn friction_falls_steadily_with_reynolds() {
        // Laminar and turbulent friction of a plate (eqs. 1.33 and 1.33a).
        let mut last = f64::INFINITY;
        for k in 0..=50 {
            let re = 10f64.powf(2.0 + 6.0 * f64::from(k) / 50.0);
            let cf = channel_friction(re, 2e-4);
            assert!(cf <= last * (1.0 + 1e-12), "{re}");
            last = cf;
        }
        // Rougher channels rub more in turbulent flow.
        assert!(channel_friction(1e7, 1e-3) > channel_friction(1e7, 1e-5));
    }

    #[test]
    fn water_needs_no_correction() {
        let c = ViscousCorrection::new(WATER_VISCOSITY, &base());
        assert_relative_eq!(c.head_factor, 1.0, max_relative = 1e-12);
        assert_relative_eq!(c.efficiency_factor, 1.0, max_relative = 1e-12);
        assert_relative_eq!(c.correlation_head_factor, 1.0, max_relative = 1e-12);
        assert_relative_eq!(c.npsh_factor(4.0, 3.0), 1.0, max_relative = 1e-12);
    }

    #[test]
    fn oils_lower_head_and_efficiency_much_more() {
        let light = ViscousCorrection::new(20e-6, &base());
        let heavy = ViscousCorrection::new(500e-6, &base());
        for c in [&light, &heavy] {
            assert!(c.head_factor < 1.0 && c.efficiency_factor < c.head_factor);
            assert!(c.correlation_efficiency_factor < 1.0);
            assert!(c.npsh_factor(4.0, 3.0) > 1.0);
        }
        assert!(heavy.efficiency_factor < light.efficiency_factor);
        assert!(heavy.head_factor < light.head_factor);
        // The two methods agree within the scatter Gülich reports (±15 %, Chap. 13.1.2).
        for c in [&light, &heavy] {
            assert!(
                (c.efficiency_factor / c.correlation_efficiency_factor - 1.0).abs() < 0.3,
                "{} vs {}",
                c.efficiency_factor,
                c.correlation_efficiency_factor
            );
        }
        // Shut-off keeps its head; the factor grows toward the best point.
        assert_relative_eq!(heavy.head_factor_at(0.0), 1.0);
        assert_relative_eq!(heavy.head_factor_at(1.0), heavy.head_factor);
        assert!(!heavy.outside_tests);
        assert!(ViscousCorrection::new(5000e-6, &base()).outside_tests);
    }
}
