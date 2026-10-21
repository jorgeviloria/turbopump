//! Secondary losses of a pump stage, after Gülich 2nd ed., Chap. 3.6 and Tables 3.5 to
//! 3.7. With the hydraulic efficiency they give the efficiency as a sum of losses:
//! P = ρ·g·H·Q/(η_v·η_h) + ΣP_RR + P_m (eqs. T3.5.1 and T3.5.3).
//!
//! - Leakage through the annular seal at the impeller inlet (Table 3.7 (1)): radial
//!   clearance s from eq. 3.12, length L_sp = 0.12·d_sp (middle of the 0.1 to 0.14 of
//!   Chap. 3.6.2), sand roughness 4 µm of a seal machined to N7 (Gülich's example).
//!   The head across the seal is the static rise of the impeller less the drop in the
//!   rotating sidewall gap (eq. T3.7.3), with the rotation factor of eq. T3.7.2. The axial
//!   velocity comes from the seal resistance (eq. T3.7.6 with ζ_EA = 1.1, the middle of
//!   1 to 1.2) and the friction factor of eqs. T3.7.9 to T3.7.14, by iteration.
//! - Disk friction on each shroud, P_RR = k_RR·ρ·ω³·R⁵·{1 − (R_n/R)⁵} (eq. T3.6.2), with
//!   k_RR of eq. T3.6.3 for hydraulically smooth shrouds and the leakage factor f_L of
//!   eq. T3.6.7. The front shroud runs from the seal, the rear one from the shaft seal.
//! - Mechanical losses P_m/P = 0.0045·(Q_Ref/Q)^0.4·(n_Ref/n)^0.3 (eq. T3.5.6a), constant
//!   with flow.
//!
//! Double-entry impellers have two front seals and two front shrouds; balance holes
//! add the leakage of the rear seal.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use tp_units::STANDARD_GRAVITY as G;

use crate::axial_thrust::AxialThrust;
use crate::pump::{CentrifugalPumpDesign, PumpDuty};
use crate::{DesignError, Param};

/// Seal length over diameter (Gülich Chap. 3.6.2: 0.1 to 0.14).
pub const SEAL_LENGTH_RATIO: f64 = 0.12;
/// Sand roughness of a seal machined to N7, m (Gülich Chap. 3.6.2).
pub const SEAL_ROUGHNESS: f64 = 4e-6;
/// Inlet plus outlet loss of an annular seal (Gülich eq. T3.7.8: 1 to 1.2).
pub const SEAL_INLET_OUTLET_LOSS: f64 = 1.1;
/// Axial gap between shroud and casing over r₂ (our choice; Gülich finds its effect
/// small, Chap. 3.6.1).
pub const AXIAL_GAP_RATIO: f64 = 0.05;

/// Inputs of the loss analysis. Every value is automatic by default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LossInput {
    /// Use the efficiencies of the design statistics (Gülich Table 3.9) instead of the
    /// loss analysis for the leakage, disk friction and mechanical losses.
    pub statistics: bool,
    /// Radial clearance of the annular seals s, m; automatic: eq. 3.12, twice that
    /// with wide clearances.
    pub seal_clearance: Param<f64>,
    /// Length of the annular seal L_sp, m; automatic: 0.12·d_sp.
    pub seal_length: Param<f64>,
    /// Sand roughness of the seal surfaces, m; automatic: 4 µm.
    pub seal_roughness: Param<f64>,
    /// Axial gap between the shrouds and the casing s_ax, m; automatic: 0.05·r₂.
    pub axial_gap: Param<f64>,
}

/// Radial clearance s = Δd/2 of an annular seal of diameter `d_sp` (m):
/// Δd/d_sp = 0.004·(d_Ref/d_sp)^0.53 with d_Ref = 100 mm (Gülich eq. 3.12).
pub fn seal_clearance(d_sp: f64) -> f64 {
    0.5 * 0.004 * d_sp * (0.1 / d_sp).powf(0.53)
}

/// Rotation factor of the liquid ahead of a seal with radially inward leakage:
/// k = 0.9·y_sp^0.087, y_sp = Re_u2^0.3·(s·d_sp/d₂²)·√(s/L_sp) (Gülich eq. T3.7.2).
pub fn seal_rotation(reynolds: f64, clearance: f64, d_sp: f64, d2: f64, length: f64) -> f64 {
    let y = reynolds.powf(0.3) * clearance * d_sp / (d2 * d2) * (clearance / length).sqrt();
    0.9 * y.powf(0.087)
}

/// An annular seal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AnnularSeal {
    /// d_sp, m.
    pub diameter: f64,
    /// Radial clearance s, m.
    pub clearance: f64,
    /// L_sp, m.
    pub length: f64,
    /// Sand roughness, m.
    pub roughness: f64,
}

/// The flow through a seal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SealFlow {
    /// m³/s
    pub flow: f64,
    /// Axial velocity in the gap, m/s.
    pub axial_velocity: f64,
    /// Friction factor λ with the effect of rotation.
    pub friction: f64,
    /// Re* of eq. T3.7.10.
    pub reynolds: f64,
}

impl AnnularSeal {
    /// Leakage for a head `head` (m) across the seal at shaft speed `omega` (rad/s) in
    /// a liquid of kinematic viscosity `nu` (m²/s) (Gülich eqs. T3.7.6 to T3.7.14).
    pub fn leakage(&self, head: f64, omega: f64, nu: f64) -> SealFlow {
        if head <= 0.0 {
            return SealFlow {
                flow: 0.0,
                axial_velocity: 0.0,
                friction: f64::NAN,
                reynolds: 0.0,
            };
        }
        let s = self.clearance;
        let u_sp = omega * self.diameter / 2.0;
        let re_u = 2.0 * s * u_sp / nu;
        let resistance = |lambda: f64| SEAL_INLET_OUTLET_LOSS + lambda * self.length / (2.0 * s);
        let mut lambda = 0.05;
        let mut c_ax = (2.0 * G * head / resistance(lambda)).sqrt();
        let mut re_star = 0.0;
        for _ in 0..100 {
            let re = 2.0 * s * c_ax / nu;
            re_star = (re * re + 0.25 * re_u * re_u).sqrt();
            lambda = if re_star < 2000.0 {
                // Laminar, concentric (eqs. T3.7.11 and T3.7.14).
                96.0 / re * (1.0 + 0.2 * (re_u / 2000.0).powf(1.03))
            } else {
                // Turbulent (eqs. T3.7.12 and T3.7.13).
                let a = 0.135 * self.roughness / s;
                let lambda0 = 0.31 / (a + 6.5 / re).log10().powi(2);
                lambda0 * (1.0 + 0.19 * (re_u / re).powi(2)).powf(0.375)
            };
            let next = (2.0 * G * head / resistance(lambda)).sqrt();
            let done = (next - c_ax).abs() < 1e-10 * next;
            c_ax = 0.5 * (c_ax + next);
            if done {
                break;
            }
        }
        SealFlow {
            flow: PI * self.diameter * s * c_ax,
            axial_velocity: c_ax,
            friction: lambda,
            reynolds: re_star,
        }
    }
}

/// Disk friction coefficient k_RR of a hydraulically smooth shroud of radius `r` with
/// axial gap `gap` at Re = ω·r²/ν (Gülich eq. T3.6.3), times the leakage factor `f_l`.
pub fn disk_friction_coefficient(reynolds: f64, r: f64, gap: f64, f_l: f64) -> f64 {
    PI * r / (2.0 * reynolds * gap)
        + 0.02 / reynolds.powf(0.2) * (1.0 + gap / r) / (1.0 + gap / (2.0 * r)) * f_l
}

/// f_L = exp{−350·φ_sp·[(r₂/r_sp)^a − 1]} for a leakage flowing radially inward (a = 1;
/// Gülich eq. T3.6.7).
pub fn leakage_factor(leakage_coefficient: f64, radius_ratio: f64) -> f64 {
    (-350.0 * leakage_coefficient * (1.0 / radius_ratio - 1.0)).exp()
}

/// Friction power of one side of a disk from radius `inner` to `r` (Gülich eq.
/// T3.6.2 with δ = 0), W.
pub fn disk_friction_power(k_rr: f64, density: f64, omega: f64, r: f64, inner: f64) -> f64 {
    k_rr * density * omega.powi(3) * r.powi(5) * (1.0 - (inner / r).powi(5))
}

/// Mechanical losses for power `power` (W) at flow `flow` (m³/s) and speed `omega`
/// (rad/s): P_m = 0.0045·P·(Q_Ref/Q)^0.4·(n_Ref/n)^0.3 with Q_Ref = 1 m³/s and
/// n_Ref = 1500 rpm (Gülich eq. T3.5.6a), W.
pub fn mechanical_loss(power: f64, flow: f64, omega: f64) -> f64 {
    let rpm = omega * 30.0 / PI;
    0.0045 * power * (1.0 / flow).powf(0.4) * (1500.0 / rpm).powf(0.3)
}

/// The secondary losses of one impeller.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SecondaryLosses {
    /// The seal at the impeller inlet (one per eye) and, with balance holes, the rear
    /// one.
    pub seal: AnnularSeal,
    pub rear_seal: Option<AnnularSeal>,
    pub eyes: f64,
    /// Rotation factor ahead of the seal (eq. T3.7.2).
    pub seal_rotation: f64,
    /// Head across the front seal at the design point, m.
    pub seal_head: f64,
    /// Flow through the front seal at the design point.
    pub seal_flow: SealFlow,
    /// Leakage through all seals at the design point, m³/s.
    pub leakage: f64,
    /// η_v = Q/(Q + Q_sp) (eq. T3.5.9).
    pub volumetric_efficiency: f64,
    /// Disk friction of the front and rear shrouds (both front shrouds of a double-entry
    /// impeller), W.
    pub front_friction: f64,
    pub rear_friction: f64,
    /// k_RR of the front shroud.
    pub friction_coefficient: f64,
    /// Mechanical losses, W.
    pub mechanical: f64,
    /// Static rise and the impeller data for the leakage at other flows.
    tip_speed: f64,
    omega: f64,
    viscosity: f64,
    impeller_diameter: f64,
}

impl SecondaryLosses {
    /// The losses of impeller `design` at `duty`, with its seals from the axial force
    /// law `axial`, at the design head and Euler head `theoretical_head`.
    pub fn new(
        duty: &PumpDuty,
        design: &CentrifugalPumpDesign,
        axial: &AxialThrust,
        input: &LossInput,
        wide_clearances: bool,
        theoretical_head: f64,
    ) -> Result<Self, DesignError> {
        let d2 = design.outlet_diameter;
        let r2 = d2 / 2.0;
        let d_sp = axial.seal_diameter;
        let clearance = input
            .seal_clearance
            .resolve(|| seal_clearance(d_sp) * if wide_clearances { 2.0 } else { 1.0 });
        let length = input.seal_length.resolve(|| SEAL_LENGTH_RATIO * d_sp);
        let roughness = input.seal_roughness.resolve(|| SEAL_ROUGHNESS);
        let gap = input.axial_gap.resolve(|| AXIAL_GAP_RATIO * r2);
        for (name, value) in [
            ("seal clearance", clearance),
            ("seal length", length),
            ("axial gap", gap),
        ] {
            if value.is_nan() || value <= 0.0 {
                return Err(DesignError::InvalidInput(format!(
                    "the {name} must be positive"
                )));
            }
        }
        if roughness < 0.0 {
            return Err(DesignError::InvalidInput(
                "the seal roughness cannot be negative".into(),
            ));
        }
        let nu = duty.kinematic_viscosity;
        if nu.is_nan() || nu <= 0.0 {
            return Err(DesignError::InvalidInput(
                "the loss analysis needs the viscosity of the liquid".into(),
            ));
        }
        let omega = duty.speed;
        let u2 = omega * r2;
        let seal = AnnularSeal {
            diameter: d_sp,
            clearance,
            length,
            roughness,
        };
        let rear_seal = axial.rear_seal_diameter.map(|d| AnnularSeal {
            diameter: d,
            clearance: input
                .seal_clearance
                .resolve(|| seal_clearance(d) * if wide_clearances { 2.0 } else { 1.0 }),
            length: input.seal_length.resolve(|| SEAL_LENGTH_RATIO * d),
            roughness,
        });
        let mut losses = Self {
            seal,
            rear_seal,
            eyes: design.eyes,
            seal_rotation: seal_rotation(u2 * r2 / nu, clearance, d_sp, d2, length),
            seal_head: 0.0,
            seal_flow: seal.leakage(0.0, omega, nu),
            leakage: 0.0,
            volumetric_efficiency: 1.0,
            front_friction: 0.0,
            rear_friction: 0.0,
            friction_coefficient: 0.0,
            mechanical: 0.0,
            tip_speed: u2,
            omega,
            viscosity: nu,
            impeller_diameter: d2,
        };
        losses.seal_head = losses.seal_head_at(&seal, duty.head, theoretical_head);
        losses.seal_flow = seal.leakage(losses.seal_head, omega, nu);
        losses.leakage = losses.leakage_at(duty.head, theoretical_head);
        losses.volumetric_efficiency = duty.flow / (duty.flow + losses.leakage);

        // Disk friction: the front shroud from the seal, the rear one from the shaft
        // seal; a double-entry impeller has two front shrouds.
        let rho = duty.density;
        let reynolds = omega * r2 * r2 / nu;
        let phi = losses.seal_flow.flow / (PI * r2 * r2 * u2);
        let k_front = disk_friction_coefficient(reynolds, r2, gap, leakage_factor(phi, d_sp / d2));
        let front = disk_friction_power(k_front, rho, omega, r2, d_sp / 2.0);
        losses.friction_coefficient = k_front;
        if design.eyes > 1.5 {
            losses.front_friction = 2.0 * front;
        } else {
            losses.front_friction = front;
            let f_l = match &rear_seal {
                Some(s) => {
                    let flow = s
                        .leakage(
                            losses.seal_head_at(s, duty.head, theoretical_head),
                            omega,
                            nu,
                        )
                        .flow;
                    leakage_factor(flow / (PI * r2 * r2 * u2), s.diameter / d2)
                }
                None => 1.0,
            };
            let k_rear = disk_friction_coefficient(reynolds, r2, gap, f_l);
            losses.rear_friction =
                disk_friction_power(k_rear, rho, omega, r2, axial.shaft_seal_diameter / 2.0);
        }
        // Mechanical losses against the coupling power of the design statistics.
        losses.mechanical = mechanical_loss(design.stage_power, duty.flow, omega);
        Ok(losses)
    }

    /// Head across `seal` (m) for pump head `head` and Euler head `theoretical_head`:
    /// the static rise ρ·g·H·(1 − g·H_th/(2u₂²)) (Gülich eq. T9.2.2, which gives
    /// R_G ≈ 0.75 as eq. T3.7.1 does) less the drop in the rotating sidewall gap (eq.
    /// T3.7.3).
    fn seal_head_at(&self, seal: &AnnularSeal, head: f64, theoretical_head: f64) -> f64 {
        let u2 = self.tip_speed;
        let static_rise = head * (1.0 - G * theoretical_head / (2.0 * u2 * u2));
        let x = seal.diameter / self.impeller_diameter;
        static_rise - self.seal_rotation.powi(2) * u2 * u2 / (2.0 * G) * (1.0 - x * x)
    }

    /// Leakage through all seals (m³/s) at pump head `head` and Euler head
    /// `theoretical_head` (m).
    pub fn leakage_at(&self, head: f64, theoretical_head: f64) -> f64 {
        let flow = |seal: &AnnularSeal| {
            seal.leakage(
                self.seal_head_at(seal, head.max(0.0), theoretical_head),
                self.omega,
                self.viscosity,
            )
            .flow
        };
        self.eyes * flow(&self.seal) + self.rear_seal.as_ref().map_or(0.0, flow)
    }

    /// Disk friction and mechanical losses, W.
    pub fn secondary_power(&self) -> f64 {
        self.front_friction + self.rear_friction + self.mechanical
    }
}

/// Friction coefficient of a flat plate in parallel flow at Reynolds number `re` with
/// relative roughness `roughness` (ε/L): c_f = 0.136/{−log(0.2·ε/L + 12.5/Re)}^2.15
/// (Gülich eq. T3.8.3).
pub fn plate_friction(re: f64, roughness: f64) -> f64 {
    0.136 / (-(0.2 * roughness + 12.5 / re.max(1.0)).log10()).powf(2.15)
}

/// Shock loss where the flow leaves the impeller into a wider collector, m:
/// ζ_LS = φ₂²·(τ₂ − b₂/b₃)² with ζ = 2gZ/u₂² (Gülich eq. T3.8.24), for flow coefficient
/// `flow_coefficient` φ₂ = c₂m/u₂, outlet blockage τ₂ and width ratio b₂/b₃.
pub fn outlet_shock(flow_coefficient: f64, blockage: f64, width_ratio: f64, u2: f64) -> f64 {
    (flow_coefficient * (blockage - width_ratio)).powi(2) * u2 * u2 / (2.0 * G)
}

/// The blade channels of an impeller for its hydraulic losses (Gülich Table 3.8 (1)).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ImpellerChannels {
    pub blades: f64,
    pub eyes: f64,
    /// Inlet throat of one channel a₁ × b₁: width across the blades × height, m.
    pub inlet_throat_width: f64,
    pub inlet_height: f64,
    /// Outlet throat of one channel a₂ × b₂, m.
    pub outlet_throat_width: f64,
    pub outlet_width: f64,
    /// Channel length L_sch, m.
    pub length: f64,
    /// Sand roughness of the channels, m.
    pub roughness: f64,
    /// b₂/d₂.
    pub width_ratio: f64,
    /// u₂ and u₁ at the mean inlet diameter, m/s.
    pub tip_speed: f64,
    pub inlet_speed: f64,
    /// Flow area of one eye, m².
    pub eye_area: f64,
}

/// Head lost in the impeller at one flow, m (Gülich Table 3.8 (1)).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ImpellerLosses {
    /// Friction and mixing in the channels Z_La,R (eqs. T3.8.2 to T3.8.6).
    pub friction: f64,
    /// Shock at the inlet throat Z_La,C (eqs. T3.8.7 to T3.8.9).
    pub inlet_shock: f64,
    /// Friction factor c_f of the channels (eq. T3.8.3).
    pub friction_coefficient: f64,
    /// w₁q/w₁m.
    pub throat_ratio: f64,
}

impl ImpellerLosses {
    pub fn total(&self) -> f64 {
        self.friction + self.inlet_shock
    }
}

impl ImpellerChannels {
    /// The channels of `design` turning at `omega` (rad/s), with channel roughness
    /// `roughness` (m). The inlet throat is the one the NPSH₃ uses: height (d₁ − d_n)/2
    /// and width π·d₁m·sin β₁B/z − e; the outlet throat π·d₂·sin β₂B/z − e. The channel
    /// length is (r₂ − r₁m)/sin β_m with β_m the mean of the inlet and outlet blade
    /// angles, a logarithmic spiral (our choice).
    pub fn new(design: &CentrifugalPumpDesign, omega: f64, roughness: f64) -> Self {
        let z = f64::from(design.blade_count);
        let e = design.blade_thickness;
        let mean = &design.inlet_mean;
        let d1m = mean.diameter;
        let (d1, dn, d2) = (
            design.inlet_diameter,
            design.hub_diameter,
            design.outlet_diameter,
        );
        let beta2 = design.outlet.blade_angle;
        let beta_m = 0.5 * (mean.blade_angle + beta2);
        Self {
            blades: z,
            eyes: design.eyes,
            inlet_throat_width: (PI * d1m * mean.blade_angle.sin() / z - e).max(1e-4),
            inlet_height: 0.5 * (d1 - dn),
            outlet_throat_width: (PI * d2 * beta2.sin() / z - e).max(1e-4),
            outlet_width: design.outlet_width,
            length: (0.5 * (d2 - d1m) / beta_m.sin()).max(0.1 * d2),
            roughness,
            width_ratio: design.outlet_width / d2,
            tip_speed: omega * d2 / 2.0,
            inlet_speed: omega * d1m / 2.0,
            eye_area: PI / 4.0 * (d1 * d1 - dn * dn),
        }
    }

    /// Losses at impeller flow `impeller_flow` (m³/s, all eyes) for a liquid of
    /// kinematic viscosity `nu` (m²/s).
    pub fn losses(&self, impeller_flow: f64, nu: f64) -> ImpellerLosses {
        let channel = self.eyes * self.blades;
        let inlet_area = self.inlet_throat_width * self.inlet_height;
        let outlet_area = self.outlet_throat_width * self.outlet_width;
        let u2 = self.tip_speed;
        // Friction and mixing (eqs. T3.8.2 to T3.8.6).
        let w_av = 2.0 * impeller_flow / (channel * (outlet_area + inlet_area));
        let cf = plate_friction(w_av * self.length / nu, self.roughness / self.length);
        let hydraulic_diameter = 2.0 * (outlet_area + inlet_area)
            / (self.inlet_throat_width
                + self.inlet_height
                + self.outlet_throat_width
                + self.outlet_width);
        let cd = (cf + 0.0015) * (1.1 + 4.0 * self.width_ratio);
        let zeta_r = 4.0 * cd * self.length / hydraulic_diameter * (w_av / u2).powi(2);
        // Shock at the inlet throat (eqs. T3.8.7 to T3.8.9): 30 % of a Carnot shock
        // from w₁m to w₁q, swirl-free inflow; nothing when the flow accelerates into
        // the throat (our choice).
        let c1m = impeller_flow / (self.eyes * self.eye_area);
        let w1m = c1m.hypot(self.inlet_speed);
        let w1q = impeller_flow / (channel * inlet_area);
        let zeta_c = if w1q < w1m {
            0.3 * ((w1m - w1q) / u2).powi(2)
        } else {
            0.0
        };
        let to_head = u2 * u2 / (2.0 * G);
        ImpellerLosses {
            friction: zeta_r * to_head,
            inlet_shock: zeta_c * to_head,
            friction_coefficient: cf,
            throat_ratio: w1q / w1m,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn seal_clearance_follows_eq_3_12() {
        // At d_sp = 100 mm: Δd = 0.4 mm.
        assert_relative_eq!(seal_clearance(0.1), 0.2e-3, max_relative = 1e-12);
        // Smaller seals get a relatively larger clearance.
        assert!(seal_clearance(0.05) / 0.05 > seal_clearance(0.1) / 0.1);
    }

    #[test]
    fn seal_leakage_obeys_its_resistance() {
        let seal = AnnularSeal {
            diameter: 0.1,
            clearance: 0.2e-3,
            length: 0.012,
            roughness: 4e-6,
        };
        let f = seal.leakage(20.0, 300.0, 1e-6);
        // c_ax = √(2gΔH/(ζ + λ·L/2s)) and Q = π·d·s·c_ax (eqs. T3.7.6 and T3.7.7).
        let c = (2.0 * G * 20.0 / (1.1 + f.friction * 0.012 / 0.4e-3)).sqrt();
        assert_relative_eq!(f.axial_velocity, c, max_relative = 1e-8);
        assert_relative_eq!(f.flow, PI * 0.1 * 0.2e-3 * c, max_relative = 1e-8);
        assert!(f.reynolds > 2000.0 && f.friction > 0.02 && f.friction < 0.2);
        // Four times the head doubles the leakage when the friction barely changes.
        let g = seal.leakage(80.0, 300.0, 1e-6);
        assert!(g.flow / f.flow > 1.9 && g.flow / f.flow < 2.1);
        assert_relative_eq!(seal.leakage(-1.0, 300.0, 1e-6).flow, 0.0);
        // A viscous oil runs laminar.
        let oil = seal.leakage(20.0, 300.0, 1e-4);
        assert!(oil.reynolds < 2000.0 && oil.flow < f.flow);
    }

    #[test]
    fn disk_friction_matches_the_free_disk_order() {
        // Turbulent, smooth: about 0.02/Re^0.2 and well below the free disk value
        // 0.0365/Re^0.2 (Chap. 3.6.1).
        let re = 2e6;
        let k = disk_friction_coefficient(re, 0.1, 0.005, 1.0);
        assert!(k > 0.02 / re.powf(0.2) && k < 0.0365 / re.powf(0.2));
        // An inward leakage lowers it.
        assert!(disk_friction_coefficient(re, 0.1, 0.005, leakage_factor(1e-3, 0.5)) < k);
        // P ∝ ρ·ω³·r⁵.
        let p = disk_friction_power(k, 1000.0, 300.0, 0.1, 0.0);
        assert_relative_eq!(p, k * 1000.0 * 2.7e7 * 1e-5, max_relative = 1e-12);
    }

    #[test]
    fn disk_friction_of_japikse_example() {
        // Japikse et al., Chap. 7.5: r₂ = 8.82 in at 1000 rpm in water; the parasitic
        // disk friction of the worked example is 1.46 hp. Both shrouds, from the axis,
        // without leakage and with an axial gap of 5 % of r₂.
        let (r2, omega) = (8.82 * 0.0254, 1000.0 * PI / 30.0);
        let re = omega * r2 * r2 / 1e-6;
        let k = disk_friction_coefficient(re, r2, AXIAL_GAP_RATIO * r2, 1.0);
        let power = 2.0 * disk_friction_power(k, 998.0, omega, r2, 0.0);
        assert_relative_eq!(power / 745.7, 1.46, max_relative = 0.1);
    }

    #[test]
    fn impeller_losses_follow_table_3_8() {
        let channels = ImpellerChannels {
            blades: 7.0,
            eyes: 1.0,
            inlet_throat_width: 0.02,
            inlet_height: 0.03,
            outlet_throat_width: 0.025,
            outlet_width: 0.017,
            length: 0.13,
            roughness: 16e-6,
            width_ratio: 0.1,
            tip_speed: 26.0,
            inlet_speed: 12.0,
            eye_area: 4.4e-3,
        };
        let flow = 0.028;
        let l = channels.losses(flow, 1e-6);
        // Eq. T3.8.6 by hand.
        let (a1, a2) = (0.02 * 0.03, 0.025 * 0.017);
        let w_av = 2.0 * flow / (7.0 * (a1 + a2));
        let dh = 2.0 * (a1 + a2) / (0.02 + 0.03 + 0.025 + 0.017);
        let zeta =
            4.0 * (l.friction_coefficient + 0.0015) * 1.5 * 0.13 / dh * (w_av / 26.0).powi(2);
        assert_relative_eq!(
            l.friction,
            zeta * 26.0 * 26.0 / (2.0 * G),
            max_relative = 1e-12
        );
        // The shock loss vanishes where the throat no longer decelerates the flow.
        assert!(l.inlet_shock > 0.0 && l.throat_ratio < 1.0);
        assert_relative_eq!(channels.losses(10.0 * flow, 1e-6).inlet_shock, 0.0);
        // A rougher or more viscous channel loses more.
        let rough = ImpellerChannels {
            roughness: 200e-6,
            ..channels
        };
        assert!(rough.losses(flow, 1e-6).friction > l.friction);
        assert!(channels.losses(flow, 1e-4).friction > l.friction);
    }

    #[test]
    fn outlet_shock_follows_eq_t3_8_24() {
        // ζ_LS = 0.1²·(1.1 − 0.8)² = 9·10⁻⁴.
        assert_relative_eq!(
            outlet_shock(0.1, 1.1, 0.8, 26.0),
            9e-4 * 26.0 * 26.0 / (2.0 * G),
            max_relative = 1e-12
        );
        assert_relative_eq!(outlet_shock(0.1, 1.0, 1.0, 26.0), 0.0);
    }

    #[test]
    fn mechanical_losses_follow_eq_t3_5_6a() {
        // At Q_Ref and n_Ref: 0.45 %.
        let omega = 1500.0 * PI / 30.0;
        assert_relative_eq!(
            mechanical_loss(1e5, 1.0, omega),
            450.0,
            max_relative = 1e-12
        );
        // Small pumps lose a larger share.
        assert!(mechanical_loss(1e4, 0.01, omega) / 1e4 > 0.01);
    }
}
