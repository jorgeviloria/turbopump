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
//! - Balance holes, one per channel, together at least 4 times the flow area of the
//!   rear seal worn to twice its clearance: ¼·z·d² ≥ 4·d_s2·s with s = 2·s₀ (Gülich
//!   Chap. 9.2.2), s₀ from eq. 3.12; rounded up to 0.5 mm (our choice).
//! - Double-entry impellers: |F_ax| = f_ax·ρ/2·u₂²·(d₂² − d_sp²) with f_ax = 0.01 to 0.02
//!   for the steady force (eq. T9.2.15); we take 0.015. Its sign is not known.
//! - Back vanes on the rear shroud, after Gülich Table 9.3, which sums up the tests of
//!   Möhring (1976). In the vane zone the liquid turns with
//!   k_Rs = 1/[1 + 0.13·s/(s + h)·√(r₂/(h·z_v))] (eq. T9.3.2), with h the vane height,
//!   s the axial gap at the vane tips and z_v the number of vanes, computed without
//!   leakage. Vanes trimmed to d_Rs < d₂ give the mean factor on the rear shroud
//!   k_av = √{(d_Rs/d₂)^(2 − 0.9·d_Rs/d₂)·(k_Rs² − k_o²) + k_o²} (eq. T9.3.3). The
//!   pressure on the rear shroud follows eq. T9.1.7 with k_av, and the force eq. T9.2.8
//!   with k_av on the rear side and the front side as it is: Gülich says "eq. T9.2.3 or
//!   9.2.8 using k_av"; putting k_av into the rear term of T9.2.8 is our reading. The
//!   tested range is s/r₂ = 0.008 to 0.015, s/h = 0.1 to 0.2, h/r₂ = 0.03 to 0.05,
//!   z_v = 4 to 8 and d_Rs/d₂ = 0.75 to 0.85, with radial vanes; outside it the force is
//!   computed anyway, with a warning. With balance holes and a rear seal, which Gülich
//!   does not cover, k_av acts only above the rear seal, with the leakage-corrected k as
//!   k_o (our choice, with a warning). The power of the vanes is in [`crate::losses`].
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
    /// Back vanes on the rear shroud; none by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub back_vanes: Option<BackVaneLaw>,
}

/// Back vanes on the rear shroud as the axial force and the disk friction see them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BackVaneLaw {
    /// Number of vanes z_v.
    pub count: u32,
    /// Straight radial vanes; eqs. T9.3.2 and T9.3.4 hold for these only.
    pub radial: bool,
    /// Inner and outer diameters of the vanes, d_Rs the outer one, m.
    pub inner_diameter: f64,
    pub outer_diameter: f64,
    /// Axial height h, m.
    pub height: f64,
    /// Axial gap s between the vane tips and the casing, m.
    pub gap: f64,
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
    /// Reduction of the rear shroud force by the back vanes, N; 0 without them.
    pub back_vanes: f64,
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
    /// Back vanes on the rear shroud.
    pub back_vanes: Option<BackVaneLaw>,
    /// φ_sp at the design point (eq. T9.1.2a).
    pub leakage_coefficient: f64,
    /// m
    pub design_head: f64,
    /// Flow area of the eye π/4·(d₁² − d_n²), m².
    pub eye_area: f64,
    /// Total pressure at the pump inlet, Pa.
    pub inlet_total_pressure: f64,
    /// Factor on the seal clearances of eq. 3.12: 1, or 2 with wide clearances.
    pub clearance_factor: f64,
}

/// Balance holes through the rear shroud.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BalanceHoleSize {
    pub count: u32,
    /// m
    pub diameter: f64,
    /// Rear seal d_s2 and its radial clearance s₀, m.
    pub seal_diameter: f64,
    pub seal_clearance: f64,
}

/// Smallest diameter of `count` balance holes behind a rear seal of diameter
/// `seal_diameter` and radial clearance `clearance` (m): ¼·z·d² = 4·d_s2·(2·s₀)
/// (Gülich Chap. 9.2.2, with the clearance doubled for wear).
pub fn balance_hole_diameter(count: u32, seal_diameter: f64, clearance: f64) -> f64 {
    (16.0 * seal_diameter * 2.0 * clearance / f64::from(count.max(1))).sqrt()
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

/// Rotation factor in the zone of the back vanes, k_Rs = 1/[1 + 0.13·s/(s + h)·
/// √(r₂/(h·z_v))], for vanes of height `height` with tip gap `gap` (m), `count` of them,
/// on an impeller of radius `r2` (Gülich eq. T9.3.2).
pub fn back_vane_rotation(r2: f64, height: f64, gap: f64, count: u32) -> f64 {
    let z = f64::from(count.max(1));
    1.0 / (1.0 + 0.13 * gap / (gap + height) * (r2 / (height * z)).sqrt())
}

/// Mean rotation factor on a rear shroud with back vanes trimmed to `diameter_ratio`
/// = d_Rs/d₂, k_av = √{x^(2 − 0.9·x)·(k_Rs² − k_o²) + k_o²}, with k_Rs from
/// [`back_vane_rotation`] and k_o that of the shroud without vanes (Gülich eq.
/// T9.3.3).
pub fn trimmed_vane_rotation(vane_rotation: f64, rotation: f64, diameter_ratio: f64) -> f64 {
    let x = diameter_ratio;
    let k_o2 = rotation * rotation;
    (x.powf(2.0 - 0.9 * x) * (vane_rotation * vane_rotation - k_o2) + k_o2).sqrt()
}

impl BackVaneLaw {
    /// The mean rotation factor k_av on the rear shroud of an impeller of diameter `d2`
    /// (m) whose shroud would turn the liquid with `rotation` = k_o without the vanes
    /// (Gülich eqs. T9.3.2 and T9.3.3).
    pub fn rotation(&self, d2: f64, rotation: f64) -> f64 {
        let k_rs = back_vane_rotation(d2 / 2.0, self.height, self.gap, self.count);
        trimmed_vane_rotation(k_rs, rotation, self.outer_diameter / d2)
    }

    /// Warnings for vanes outside the range tested behind Gülich Table 9.3, on an
    /// impeller of diameter `d2` (m).
    pub fn warnings(&self, d2: f64) -> Vec<String> {
        let r2 = d2 / 2.0;
        let mut warnings = Vec::new();
        // A small margin keeps the defaults (s/r₂ = 0.008 exactly) inside.
        let outside = |value: f64, low: f64, high: f64| {
            value < low * (1.0 - 1e-6) || value > high * (1.0 + 1e-6)
        };
        for (name, value, low, high) in [
            ("s/r₂", self.gap / r2, 0.008, 0.015),
            ("s/h", self.gap / self.height, 0.1, 0.2),
            ("h/r₂", self.height / r2, 0.03, 0.05),
            ("z_v", f64::from(self.count), 4.0, 8.0),
            ("d_Rs/d₂", self.outer_diameter / d2, 0.75, 0.85),
        ] {
            if outside(value, low, high) {
                warnings.push(format!(
                    "the back vanes have {name} = {value:.3}, outside the {low} to {high} \
                     tested behind Gülich Table 9.3: their effect on the axial force and \
                     the power is an extrapolation"
                ));
            }
        }
        if !self.radial {
            warnings.push(
                "Gülich eqs. T9.3.2 and T9.3.4 hold for radial back vanes only: the axial \
                 force and the power of the curved vanes are estimates"
                    .into(),
            );
        }
        warnings
    }
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
        if let Some(v) = &input.back_vanes {
            require(
                v.count > 0 && v.height > 0.0 && v.gap >= 0.0,
                "the back vanes need a count, a positive height and a gap that is not negative"
                    .into(),
            )?;
            require(
                v.inner_diameter < v.outer_diameter && v.outer_diameter <= d2 * (1.0 + 1e-9),
                format!(
                    "the back vanes must run outward up to d₂ = {:.1} mm at most",
                    d2 * 1e3
                ),
            )?;
        }
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
            back_vanes: input.back_vanes,
            leakage_coefficient: clearance
                * leakage_coefficient(design.pressure_coefficient, design.nq),
            design_head: duty.head,
            eye_area: PI / 4.0 * (design.inlet_diameter.powi(2) - dn * dn),
            inlet_total_pressure: duty.inlet_total_pressure,
            clearance_factor: clearance,
        })
    }

    /// One balance hole per channel of an impeller with `channels` blades, sized as in
    /// [`balance_hole_diameter`] and rounded up to 0.5 mm; `None` without balance holes.
    pub fn balance_holes(&self, channels: u32) -> Option<BalanceHoleSize> {
        let seal = self.rear_seal_diameter?;
        let clearance = self.clearance_factor * crate::losses::seal_clearance(seal);
        let exact = balance_hole_diameter(channels, seal, clearance);
        Some(BalanceHoleSize {
            count: channels,
            diameter: (exact / 0.5e-3).ceil() * 0.5e-3,
            seal_diameter: seal,
            seal_clearance: clearance,
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
                back_vanes: 0.0,
                hydraulic: total,
                momentum: 0.0,
                shaft: 0.0,
                total,
            };
        }

        let front = shroud_force(r2, x_sp, pressure_rise, rho, u2, k_front);
        let plain_rear = shroud_force(r2, x_d, pressure_rise, rho, u2, self.rotation_factor);
        let unbalanced = plain_rear - front;
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
        // Back vanes: k_av of eqs. T9.3.2 and T9.3.3 on the rear side of eq. T9.2.8 (our
        // reading of Gülich's "eq. T9.2.3 or 9.2.8 using k_av"). Without balance holes
        // it acts on the whole rear shroud from the shaft seal; with them only above the
        // rear seal, with the leakage-corrected k there as k_o (our choice: Gülich does
        // not cover a rear seal with back vanes).
        let (rear, back_vanes, rear_rotation) = match &self.back_vanes {
            None => (plain_rear, 0.0, rear_rotation),
            Some(v) => {
                let x_in = self.rear_seal_diameter.map_or(x_d, |d| d / d2);
                let k_av = v.rotation(d2, rear_rotation);
                let reduction = shroud_force(r2, x_in, pressure_rise, rho, u2, rear_rotation)
                    - shroud_force(r2, x_in, pressure_rise, rho, u2, k_av);
                // The rear shroud force without balance holes, as `rear` always is.
                let k_plain = v.rotation(d2, self.rotation_factor);
                let rear = shroud_force(r2, x_d, pressure_rise, rho, u2, k_plain);
                (rear, reduction, k_av)
            }
        };
        let hydraulic = hydraulic - back_vanes;
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
            back_vanes,
            hydraulic,
            momentum,
            shaft,
            total: hydraulic - momentum + shaft,
        }
    }

    /// Warnings on the back vanes: outside the tested range, curved, behind a rear seal
    /// or on a double-entry impeller, which has no rear shroud for them.
    pub fn back_vane_warnings(&self) -> Vec<String> {
        let Some(v) = &self.back_vanes else {
            return Vec::new();
        };
        if self.eyes > 1.5 {
            return vec![
                "a double-entry impeller has no rear shroud: its back vanes are left out of \
                 the axial force and the power"
                    .into(),
            ];
        }
        let mut warnings = v.warnings(self.impeller_diameter);
        if self.rear_seal_diameter.is_some() {
            warnings.push(
                "Gülich gives no rule for back vanes behind a rear seal with balance holes: \
                 they turn the liquid faster above the seal only, an estimate outside the \
                 tested range"
                    .into(),
            );
        }
        warnings
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
    fn balance_holes_pass_four_times_the_worn_seal_gap() {
        // ¼·z·d² = 4·d_s2·2·s₀: 7 holes behind an 84 mm seal with 0.184 mm clearance.
        let d = balance_hole_diameter(7, 0.084, 0.184e-3);
        assert_relative_eq!(
            0.25 * 7.0 * d * d,
            4.0 * 0.084 * 2.0 * 0.184e-3,
            max_relative = 1e-12
        );
        assert!((d - 8.4e-3).abs() < 0.05e-3, "{d}");
    }

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
    /// An overhung impeller with d₂ = 300 mm at u₂ = 30 m/s, without leakage.
    fn law(back_vanes: Option<BackVaneLaw>) -> AxialThrust {
        AxialThrust {
            eyes: 1.0,
            density: 1000.0,
            impeller_diameter: 0.3,
            tip_speed: 30.0,
            seal_diameter: 0.15,
            shaft_seal_diameter: 0.05,
            rear_seal_diameter: None,
            rotation_factor: 0.5,
            back_vanes,
            leakage_coefficient: 0.0,
            design_head: 50.0,
            eye_area: 0.015,
            inlet_total_pressure: 101_325.0,
            clearance_factor: 1.0,
        }
    }

    /// The example of the spec: z_v = 6, h = 6 mm, s = 1.2 mm, vanes out to `d_rs`.
    fn vanes(d_rs: f64) -> BackVaneLaw {
        BackVaneLaw {
            count: 6,
            radial: true,
            inner_diameter: 0.06,
            outer_diameter: d_rs,
            height: 6e-3,
            gap: 1.2e-3,
        }
    }

    #[test]
    fn back_vanes_lower_the_rear_force() {
        let plain = law(None).forces(0.05, 50.0, 60.0);
        let with = law(Some(vanes(0.24))).forces(0.05, 50.0, 60.0);
        assert!(with.back_vanes > 0.0, "{}", with.back_vanes);
        assert!(with.rear < plain.rear);
        assert!(with.rear_rotation > plain.rear_rotation);
        assert_relative_eq!(
            with.hydraulic,
            plain.hydraulic - with.back_vanes,
            max_relative = 1e-12
        );
        assert!(with.total < plain.total);
        // Full vanes turn the liquid faster than trimmed ones.
        let full = law(Some(vanes(0.3))).forces(0.05, 50.0, 60.0);
        assert!(full.back_vanes > with.back_vanes);
    }

    #[test]
    fn back_vane_force_matches_the_reference_equation() {
        // Gülich Table 9.3, eq. T9.3.2 by hand, r₂ = 150 mm:
        // k_Rs = 1/[1 + 0.13·(1.2/7.2)·√(150/(6·6))] = 1/(1 + 0.13·0.16667·2.04124)
        //      = 1/1.044227 = 0.957646.
        let full = law(Some(vanes(0.3))).forces(0.05, 50.0, 60.0);
        assert_relative_eq!(full.rear_rotation, 0.957646, max_relative = 1e-5);
        // Eq. T9.3.3 for d_Rs/d₂ = 0.8 and k_o = 0.5:
        // k_av = √{0.8^(2 − 0.72)·(0.957646² − 0.25) + 0.25} = √(0.751545·0.667086 + 0.25)
        //      = √0.751346 = 0.866802.
        let trimmed = law(Some(vanes(0.24))).forces(0.05, 50.0, 60.0);
        let k_av = 0.866802;
        assert_relative_eq!(trimmed.rear_rotation, k_av, max_relative = 1e-5);
        // Eq. T9.2.8 with k_av on the rear shroud (x_D = 1/6):
        // F_RS = π·r₂²·{(1 − x_D²)·Δp_La − ρ/4·u₂²·k_av²·(1 − x_D²)²}.
        let (r2, a, dp) = (0.15, 1.0 - (1.0f64 / 6.0).powi(2), trimmed.pressure_rise);
        let rear = PI * r2 * r2 * (a * dp - 1000.0 / 4.0 * 900.0 * k_av * k_av * a * a);
        assert_relative_eq!(trimmed.rear, rear, max_relative = 1e-5);
        let plain = PI * r2 * r2 * (a * dp - 1000.0 / 4.0 * 900.0 * 0.25 * a * a);
        assert_relative_eq!(trimmed.back_vanes, plain - rear, max_relative = 1e-4);
    }

    #[test]
    fn without_back_vanes_the_thrust_is_unchanged() {
        let f = law(None).forces(0.05, 50.0, 60.0);
        assert_eq!(f.back_vanes, 0.0);
        assert_eq!(f.rear_rotation, 0.5);
        assert_relative_eq!(
            f.rear,
            shroud_force(0.15, 0.05 / 0.3, f.pressure_rise, 1000.0, 30.0, 0.5),
            max_relative = 1e-12
        );
        assert_relative_eq!(f.hydraulic, f.rear - f.front, max_relative = 1e-12);
    }
    #[test]
    fn back_vane_warnings_flag_the_untested_cases() {
        // The spec example lies inside Table 9.3 but for d_Rs/d₂ = 1.
        assert!(law(Some(vanes(0.24))).back_vane_warnings().is_empty());
        let full = law(Some(vanes(0.3))).back_vane_warnings();
        assert_eq!(full.len(), 1);
        assert!(full[0].contains("d_Rs/d₂"));
        let curved = law(Some(BackVaneLaw {
            radial: false,
            ..vanes(0.24)
        }));
        assert!(curved.back_vane_warnings()[0].contains("radial back vanes only"));
        // Behind a rear seal the vanes act above it only.
        let sealed = AxialThrust {
            rear_seal_diameter: Some(0.15),
            ..law(Some(vanes(0.24)))
        };
        assert!(sealed.back_vane_warnings()[0].contains("rear seal"));
        let above = sealed.forces(0.05, 50.0, 60.0);
        let whole = law(Some(vanes(0.24))).forces(0.05, 50.0, 60.0);
        assert!(above.back_vanes > 0.0 && above.back_vanes < whole.back_vanes);
    }
}
