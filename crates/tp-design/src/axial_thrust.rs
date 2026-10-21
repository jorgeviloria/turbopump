//! Axial force on a closed radial impeller, after Gülich 2nd ed., Chap. 9.2 and Tables
//! 9.1 and 9.2. Forces toward the suction side are positive.
//!
//! - Static pressure rise in the impeller Δp_La = ρ·g·H·(1 − g·H_th/(2·u₂²)): eq. T9.2.2
//!   with η_h,La/η_h = 1, as Gülich advises, and H/η_h = H_th.
//! - The liquid in the sidewall gaps turns with k = c_u/u. Without leakage k_o, eq.
//!   T9.1.3: 0.5 for gaps open to the collector (r_w = r₂, t_ax = 0) with equal friction
//!   on casing and impeller. A leakage flowing inward raises it:
//!   k = k_o·exp{300·φ_sp·(r₂/r_sp − 1)} (eq. T9.1.4, b = 1), with the leakage
//!   coefficient φ_sp of eq. T9.1.2a for the clearances of eq. 3.12.
//! - Force on one shroud F = π·r₂²·{(1 − x²)·Δp_La − ρ/4·u₂²·k²·(1 − x²)²}
//!   (eq. T9.2.3), x = d_sp/d₂ on the front shroud and d_D/d₂ on the rear one, and
//!   F_Hy = F_RS − F_FS (eqs. T9.2.6 and T9.2.8).
//! - Momentum F_I = ρ·Q_La·c₁m for radial impellers (eq. T9.2.9 with ε₂ = 90°) and the
//!   unbalanced force on the shaft F_w = π/4·d_D²·(p_amb − p₁) (eq. T9.2.10).
//! - F_ax = F_Hy − F_I + F_w (eq. T9.2.11), without coupling thrust.
//! - With balance holes and a rear annular seal of diameter d_s2:
//!   F_ax = (0.1 to 0.2)·F_Hy + F_Hy,T9.2.13 − F_I + F_w (eq. T9.2.14); we take 0.15 of
//!   the force without holes.
//! - Double-entry impellers: |F_ax| = f_ax·ρ/2·u₂²·(d₂² − d_sp²) with f_ax = 0.01 to 0.02
//!   for the steady force (eq. T9.2.15); we take 0.015. Its sign is not known.
//!
//! The leakage at other flows scales with √(H/H_opt), our choice: it follows the
//! pressure difference across the seal.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use tp_units::STANDARD_GRAVITY as G;

use crate::pump::{CentrifugalPumpDesign, PumpDuty};
use crate::radial_thrust::default_shroud_thickness;
use crate::{DesignError, Param};

/// Standard atmosphere outside the shaft seal, Pa.
pub const AMBIENT_PRESSURE: f64 = 101_325.0;
/// Rotation factor without leakage for sidewall gaps open to the collector with equal
/// wall friction (Gülich eq. T9.1.3 with r_w = r₂ and t_ax = 0).
pub const OPEN_GAP_ROTATION: f64 = 0.5;
/// Residual of the force without balance holes that stays with them (Gülich eq.
/// T9.2.14: 0.1 to 0.2).
pub const BALANCE_RESIDUAL: f64 = 0.15;
/// Steady axial force coefficient of double-entry impellers (Gülich eq. T9.2.15: 0.01
/// to 0.02).
pub const DOUBLE_ENTRY_COEFFICIENT: f64 = 0.015;

/// Inputs of the axial force. Every value is automatic by default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AxialThrustInput {
    /// Annular seal diameter at the impeller inlet d_sp, m; automatic: the eye d₁ plus
    /// the shroud thickness on both sides.
    pub seal_diameter: Param<f64>,
    /// Shaft seal diameter on the rear shroud d_D, m; automatic: the hub diameter.
    pub shaft_seal_diameter: Param<f64>,
    /// Balance holes with an annular seal on the rear shroud.
    pub balance_holes: bool,
    /// Diameter of that rear seal d_s2, m; automatic: d_sp.
    pub rear_seal_diameter: Param<f64>,
    /// Rotation factor k_o of the liquid in the sidewall gaps without leakage;
    /// automatic: 0.5.
    pub rotation_factor: Param<f64>,
}

/// The parts of the axial force at one flow, N, positive toward the suction side.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AxialForces {
    /// Static pressure rise in the impeller Δp_La, Pa.
    pub pressure_rise: f64,
    /// Rotation factors on the front and rear shrouds.
    pub front_rotation: f64,
    pub rear_rotation: f64,
    /// Pressure on the front and rear shrouds, N (without balance holes).
    pub front: f64,
    pub rear: f64,
    /// Net hydraulic force on the shrouds F_Hy, N.
    pub hydraulic: f64,
    /// Momentum of the flow F_I, N.
    pub momentum: f64,
    /// Unbalanced force on the shaft F_w, N.
    pub shaft: f64,
    /// F_ax, N.
    pub total: f64,
}

/// The axial force law of one impeller.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AxialThrust {
    /// Impeller eyes: 1 or 2.
    pub eyes: f64,
    pub density: f64,
    /// m
    pub impeller_diameter: f64,
    /// m/s
    pub tip_speed: f64,
    /// d_sp, m.
    pub seal_diameter: f64,
    /// d_D, m.
    pub shaft_seal_diameter: f64,
    /// d_s2 with balance holes, m.
    pub rear_seal_diameter: Option<f64>,
    /// k_o.
    pub rotation_factor: f64,
    /// φ_sp at the design point (eq. T9.1.2a).
    pub leakage_coefficient: f64,
    /// m
    pub design_head: f64,
    /// Flow area of the eye π/4·(d₁² − d_n²), m².
    pub eye_area: f64,
    /// Total pressure at the pump inlet, Pa.
    pub inlet_total_pressure: f64,
}

/// φ_sp = 5.5·10⁻⁴·ψ_opt^1.5·(n_q/20)^0.4 for the clearances of eq. 3.12 (Gülich eq.
/// T9.1.2a).
pub fn leakage_coefficient(pressure_coefficient: f64, nq: f64) -> f64 {
    5.5e-4 * pressure_coefficient.powf(1.5) * (nq / 20.0).powf(0.4)
}

/// k = k_o·exp{300·φ_sp·(r₂/r_sp − 1)} for a leakage flowing inward (Gülich eq. T9.1.4
/// with b = 1).
pub fn rotation_with_leakage(rotation: f64, leakage: f64, radius_ratio: f64) -> f64 {
    rotation * (300.0 * leakage * (1.0 / radius_ratio - 1.0)).exp()
}

/// Force on one shroud between diameter ratio `x` and d₂ (Gülich eq. T9.2.3), N.
fn shroud_force(r2: f64, x: f64, pressure_rise: f64, density: f64, u2: f64, k: f64) -> f64 {
    let a = 1.0 - x * x;
    PI * r2 * r2 * (a * pressure_rise - density / 4.0 * u2 * u2 * k * k * a * a)
}

/// F_Hy for equal rotation on both shrouds between seal diameters `d1` and `d2`
/// (Gülich eqs. T9.2.7 and T9.2.13), N.
fn equal_rotation_force(
    d_front: f64,
    d_rear: f64,
    d2: f64,
    pressure_rise: f64,
    density: f64,
    u2: f64,
    k: f64,
) -> f64 {
    PI / 4.0
        * (d_front * d_front - d_rear * d_rear)
        * (pressure_rise
            - density / 2.0
                * k
                * k
                * u2
                * u2
                * (1.0 - (d_front * d_front + d_rear * d_rear) / (2.0 * d2 * d2)))
}

impl AxialThrust {
    /// The law for impeller `design` at `duty`, with seal clearances twice those of eq.
    /// 3.12 when `wide_clearances` (leakage doubled, our choice).
    pub fn new(
        duty: &PumpDuty,
        design: &CentrifugalPumpDesign,
        input: &AxialThrustInput,
        wide_clearances: bool,
    ) -> Result<Self, DesignError> {
        let d2 = design.outlet_diameter;
        let require = |ok: bool, message: String| {
            if ok {
                Ok(())
            } else {
                Err(DesignError::InvalidInput(message))
            }
        };
        let seal_diameter = input
            .seal_diameter
            .resolve(|| design.inlet_diameter + 2.0 * default_shroud_thickness(d2));
        let shaft_seal_diameter = input.shaft_seal_diameter.resolve(|| design.hub_diameter);
        require(
            shaft_seal_diameter > 0.0 && shaft_seal_diameter < seal_diameter,
            format!(
                "the shaft seal diameter must be positive and below the {:.1} mm annular seal",
                seal_diameter * 1e3
            ),
        )?;
        require(
            seal_diameter < d2,
            format!(
                "the annular seal diameter {:.1} mm must stay below d₂ = {:.1} mm",
                seal_diameter * 1e3,
                d2 * 1e3
            ),
        )?;
        let rear_seal_diameter = input
            .balance_holes
            .then(|| input.rear_seal_diameter.resolve(|| seal_diameter));
        if let Some(d) = rear_seal_diameter {
            require(
                d > shaft_seal_diameter && d < d2,
                format!(
                    "the rear seal diameter {:.1} mm must lie between the shaft seal and d₂",
                    d * 1e3
                ),
            )?;
        }
        let rotation_factor = input.rotation_factor.resolve(|| OPEN_GAP_ROTATION);
        require(
            (0.1..=1.0).contains(&rotation_factor),
            format!("the rotation factor k_o must lie between 0.1 and 1, got {rotation_factor}"),
        )?;
        let clearance = if wide_clearances { 2.0 } else { 1.0 };
        let dn = design.hub_diameter;
        Ok(Self {
            eyes: design.eyes,
            density: duty.density,
            impeller_diameter: d2,
            tip_speed: duty.speed * d2 / 2.0,
            seal_diameter,
            shaft_seal_diameter,
            rear_seal_diameter,
            rotation_factor,
            leakage_coefficient: clearance
                * leakage_coefficient(design.pressure_coefficient, design.nq),
            design_head: duty.head,
            eye_area: PI / 4.0 * (design.inlet_diameter.powi(2) - dn * dn),
            inlet_total_pressure: duty.inlet_total_pressure,
        })
    }

    /// The parts of the axial force at `flow` (m³/s) with head `head` and Euler head
    /// `theoretical_head` (m).
    pub fn forces(&self, flow: f64, head: f64, theoretical_head: f64) -> AxialForces {
        let (rho, u2, d2) = (self.density, self.tip_speed, self.impeller_diameter);
        let r2 = d2 / 2.0;
        let pressure_rise = rho * G * head * (1.0 - G * theoretical_head / (2.0 * u2 * u2));
        let leakage = self.leakage_coefficient * (head.max(0.0) / self.design_head).sqrt();
        let x_sp = self.seal_diameter / d2;
        let x_d = self.shaft_seal_diameter / d2;
        let k_front = rotation_with_leakage(self.rotation_factor, leakage, x_sp);

        if self.eyes > 1.5 {
            // A double-entry impeller balances itself but for a residual of either sign.
            let total = DOUBLE_ENTRY_COEFFICIENT * rho / 2.0
                * u2
                * u2
                * (d2 * d2 - self.seal_diameter.powi(2));
            return AxialForces {
                pressure_rise,
                front_rotation: k_front,
                rear_rotation: k_front,
                front: f64::NAN,
                rear: f64::NAN,
                hydraulic: total,
                momentum: 0.0,
                shaft: 0.0,
                total,
            };
        }

        let front = shroud_force(r2, x_sp, pressure_rise, rho, u2, k_front);
        let rear = shroud_force(r2, x_d, pressure_rise, rho, u2, self.rotation_factor);
        let unbalanced = rear - front;
        let (hydraulic, rear_rotation) = match self.rear_seal_diameter {
            None => (unbalanced, self.rotation_factor),
            // Balance holes: leakage flows inward on both shrouds.
            Some(d_s2) => {
                let k_rear = rotation_with_leakage(self.rotation_factor, leakage, d_s2 / d2);
                let seals = equal_rotation_force(
                    self.seal_diameter,
                    d_s2,
                    d2,
                    pressure_rise,
                    rho,
                    u2,
                    0.5 * (k_front + k_rear),
                );
                (BALANCE_RESIDUAL * unbalanced + seals, k_rear)
            }
        };
        // The impeller takes the leakage of its front seal as well.
        let impeller_flow = flow + leakage * PI * r2 * r2 * u2;
        let c1m = impeller_flow / self.eye_area;
        let momentum = rho * impeller_flow * c1m;
        let suction_pressure = self.inlet_total_pressure - rho / 2.0 * c1m * c1m;
        let shaft =
            PI / 4.0 * self.shaft_seal_diameter.powi(2) * (AMBIENT_PRESSURE - suction_pressure);
        AxialForces {
            pressure_rise,
            front_rotation: k_front,
            rear_rotation,
            front,
            rear,
            hydraulic,
            momentum,
            shaft,
            total: hydraulic - momentum + shaft,
        }
    }

    /// The rough estimate (0.7 to 0.9)·ρ·g·H·π/4·(d_sp² − d_D²) of Gülich eq. T9.2.12 at
    /// head `head` (m), N.
    pub fn estimate(&self, head: f64) -> (f64, f64) {
        let area = PI / 4.0 * (self.seal_diameter.powi(2) - self.shaft_seal_diameter.powi(2));
        let force = self.density * G * head * area;
        (0.7 * force, 0.9 * force)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn rotation_and_leakage_follow_table_9_1() {
        // nq = 20, ψ = 1: φ_sp = 5.5·10⁻⁴.
        assert_relative_eq!(leakage_coefficient(1.0, 20.0), 5.5e-4, max_relative = 1e-12);
        // No leakage leaves k_o; an inward leakage speeds the liquid up.
        assert_relative_eq!(rotation_with_leakage(0.5, 0.0, 0.5), 0.5);
        assert_relative_eq!(
            rotation_with_leakage(0.5, 1e-3, 0.5),
            0.5 * 0.3f64.exp(),
            max_relative = 1e-12
        );
    }

    #[test]
    fn shroud_force_is_the_integral_of_the_pressure_distribution() {
        // Eq. T9.2.3 against 2π·∫(p − p₁)·r·dr with p from eq. T9.1.7.
        let (r2, x, dp, rho, u2, k) = (0.1, 0.4, 3e5, 1000.0, 30.0, 0.5);
        let n = 20_000;
        let r_in = x * r2;
        let integral: f64 = (0..n)
            .map(|i| {
                let r = r_in + (r2 - r_in) * (i as f64 + 0.5) / n as f64;
                let p = dp - rho / 2.0 * u2 * u2 * k * k * (1.0 - (r / r2).powi(2));
                2.0 * PI * p * r * (r2 - r_in) / n as f64
            })
            .sum();
        assert_relative_eq!(
            shroud_force(r2, x, dp, rho, u2, k),
            integral,
            max_relative = 1e-6
        );
        // Equal rotation on both shrouds: eq. T9.2.7 is the difference of two T9.2.3.
        let d2 = 2.0 * r2;
        assert_relative_eq!(
            equal_rotation_force(0.08, 0.03, d2, dp, rho, u2, k),
            shroud_force(r2, 0.03 / d2, dp, rho, u2, k)
                - shroud_force(r2, 0.08 / d2, dp, rho, u2, k),
            max_relative = 1e-9
        );
    }
}
