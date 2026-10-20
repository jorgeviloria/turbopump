//! Main dimensions of a radial pump impeller by one-dimensional streamline theory.
//!
//! Procedure (see `docs/theory/dimensiones-principales.md`):
//! 1. specific speed per stage and eye;
//! 2. efficiencies → impeller flow Q_La = Q/η_v and theoretical head H_th = H/η_h;
//! 3. shaft diameter from the torque and an allowable shear stress;
//! 4. outlet diameter from the pressure coefficient ψ;
//! 5. eye diameter at minimum relative velocity, outlet width from b₂/d₂;
//! 6. blade number, inlet and outlet blade angles with blockage and slip;
//! 7. NPSH3 estimate from the suction specific speed.

use std::f64::consts::{FRAC_PI_2, PI};

use serde::{Deserialize, Serialize};
use tp_math::roots;
use tp_units::{STANDARD_GRAVITY as G, SpecificSpeed};

use super::correlations as corr;
use crate::slip::{F1_RADIAL, wiesner_gulich};
use crate::triangle::{VelocityTriangle, blockage_factor};
use crate::{DesignError, Param};

/// Number of impeller eyes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Suction {
    #[default]
    Single,
    /// Double-entry impeller: two eyes, each taking half the flow.
    Double,
}

impl Suction {
    pub fn eyes(self) -> f64 {
        match self {
            Suction::Single => 1.0,
            Suction::Double => 2.0,
        }
    }
}

/// Operating point and fluid seen by one impeller stage, in SI units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PumpDuty {
    /// Volume flow through the machine, m³/s.
    pub flow: f64,
    /// Head of this stage, m.
    pub head: f64,
    /// Head of the whole machine, m; sets the torque on the shaft.
    pub machine_head: f64,
    /// Rotational speed, rad/s.
    pub speed: f64,
    /// kg/m³
    pub density: f64,
    /// Vapour pressure at the inlet temperature, Pa.
    pub vapor_pressure: f64,
    /// Total pressure at the pump inlet, Pa.
    pub inlet_total_pressure: f64,
}

/// User inputs of the main-dimensions step. Every value is automatic by default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CentrifugalPumpInput {
    pub suction: Suction,
    pub hydraulic_efficiency: Param<f64>,
    pub volumetric_efficiency: Param<f64>,
    pub overall_efficiency: Param<f64>,
    /// ψ = 2gH/u₂²; ignored when the outlet diameter is manual.
    pub pressure_coefficient: Param<f64>,
    /// d₂, m.
    pub outlet_diameter: Param<f64>,
    /// b₂, m.
    pub outlet_width: Param<f64>,
    /// Multiplier on the eye diameter of minimum relative velocity; values above 1
    /// lower the inlet velocity and improve suction behaviour.
    pub inlet_diameter_factor: Param<f64>,
    /// d₁, m; overrides the factor when manual.
    pub inlet_diameter: Param<f64>,
    pub blade_count: Param<u32>,
    /// Blade thickness at inlet and outlet, m.
    pub blade_thickness: Param<f64>,
    /// Allowable torsional shear stress of the shaft, Pa.
    pub shaft_shear_stress: Param<f64>,
    /// m
    pub shaft_diameter: Param<f64>,
    /// Hub diameter at the impeller eye, m.
    pub hub_diameter: Param<f64>,
    /// Incidence between blade and inflow at the leading edge, rad.
    pub incidence: Param<f64>,
    /// Suction specific speed nss (rpm, m³/s, m) used to estimate NPSH3.
    pub suction_specific_speed: Param<f64>,
}

/// Efficiencies at the design point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Efficiencies {
    pub hydraulic: f64,
    pub volumetric: f64,
    /// η / (η_h·η_v): mechanical losses plus disc friction.
    pub mechanical: f64,
    pub overall: f64,
}

/// Inlet streamline (hub, mean or shroud) at the leading edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct InletSection {
    /// m
    pub diameter: f64,
    pub triangle: VelocityTriangle,
    /// Blade angle β₁B, rad.
    pub blade_angle: f64,
    pub blockage: f64,
}

/// Impeller outlet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct OutletSection {
    pub triangle: VelocityTriangle,
    /// Blade angle β₂B, rad.
    pub blade_angle: f64,
    pub slip_factor: f64,
    pub blockage: f64,
}

/// Result of the main-dimensions step, in SI units.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CentrifugalPumpDesign {
    /// nq per stage and eye.
    pub nq: f64,
    pub efficiencies: Efficiencies,
    /// Flow through the impeller including leakage, Q/η_v, m³/s.
    pub impeller_flow: f64,
    /// H/η_h, m.
    pub theoretical_head: f64,
    /// Shaft power of this stage, W.
    pub stage_power: f64,
    /// Torque at the coupling for the whole machine, N·m.
    pub shaft_torque: f64,
    pub shaft_diameter: f64,
    pub hub_diameter: f64,
    pub inlet_diameter: f64,
    pub outlet_diameter: f64,
    pub outlet_width: f64,
    pub blade_count: u32,
    pub blade_thickness: f64,
    /// ψ = 2gH/u₂² of the resulting geometry.
    pub pressure_coefficient: f64,
    pub inlet_hub: InletSection,
    pub inlet_mean: InletSection,
    pub inlet_shroud: InletSection,
    pub outlet: OutletSection,
    /// m
    pub npsh_required: f64,
    /// m
    pub npsh_available: f64,
    pub warnings: Vec<String>,
}

const MAX_BLADES: u32 = 12;
const MIN_BLADES: u32 = 3;

fn require(ok: bool, message: impl FnOnce() -> String) -> Result<(), DesignError> {
    if ok {
        Ok(())
    } else {
        Err(DesignError::InvalidInput(message()))
    }
}

fn positive(name: &str, value: f64) -> Result<f64, DesignError> {
    require(value.is_finite() && value > 0.0, || {
        format!("{name} must be positive, got {value}")
    })?;
    Ok(value)
}

fn fraction(name: &str, value: f64) -> Result<f64, DesignError> {
    require(value.is_finite() && value > 0.0 && value <= 1.0, || {
        format!("{name} must lie in (0, 1], got {value}")
    })?;
    Ok(value)
}

/// Blade angle at a leading-edge section: β₁B = atan(τ₁·c_m1/u₁) + i, where τ₁ itself
/// depends on β₁B (solved by fixed-point iteration).
fn inlet_section(
    diameter: f64,
    cm: f64,
    omega: f64,
    z: u32,
    e: f64,
    incidence: f64,
) -> InletSection {
    let u = omega * diameter / 2.0;
    let triangle = VelocityTriangle::new(u, cm, 0.0);
    let mut blade_angle = triangle.beta() + incidence;
    let mut blockage = 1.0;
    for _ in 0..50 {
        blockage = blockage_factor(z, e, diameter, blade_angle, FRAC_PI_2);
        if !blockage.is_finite() {
            break;
        }
        let next = (blockage * cm).atan2(u) + incidence;
        let converged = (next - blade_angle).abs() < 1e-12;
        blade_angle = next;
        if converged {
            break;
        }
    }
    InletSection {
        diameter,
        triangle,
        blade_angle,
        blockage,
    }
}

/// Outlet blade angle β₂B that produces c_u2 with `z` blades:
/// c_u2/u₂ = γ(β₂B) − φ₂·τ₂(β₂B)/tan β₂B.
fn outlet_blade_angle(
    target_cu2: f64,
    u2: f64,
    cm2: f64,
    d2: f64,
    d1m: f64,
    z: u32,
    e: f64,
) -> Result<(f64, f64, f64), DesignError> {
    let phi2 = cm2 / u2;
    let residual = |beta: f64| {
        let gamma = wiesner_gulich(beta, z, d1m / d2, F1_RADIAL);
        let tau = blockage_factor(z, e, d2, beta, FRAC_PI_2);
        if !tau.is_finite() {
            return -1.0;
        }
        gamma - phi2 * tau / beta.tan() - target_cu2 / u2
    };
    let (lo, hi) = (5f64.to_radians(), 90f64.to_radians());
    if residual(hi) < 0.0 {
        return Err(DesignError::Infeasible(format!(
            "with {z} blades the head needs an outlet blade angle above 90°; \
             increase the outlet diameter, the outlet width or the number of blades"
        )));
    }
    if residual(lo) > 0.0 {
        return Err(DesignError::Infeasible(format!(
            "with {z} blades the head needs an outlet blade angle below 5°; \
             reduce the outlet diameter or the number of blades"
        )));
    }
    let beta = roots::brent(residual, lo, hi, 1e-12)
        .map_err(|e| DesignError::Infeasible(e.to_string()))?;
    let gamma = wiesner_gulich(beta, z, d1m / d2, F1_RADIAL);
    let tau = blockage_factor(z, e, d2, beta, FRAC_PI_2);
    Ok((beta, gamma, tau))
}

/// Computes the main dimensions and velocity triangles of a radial pump impeller.
pub fn design(
    duty: &PumpDuty,
    input: &CentrifugalPumpInput,
) -> Result<CentrifugalPumpDesign, DesignError> {
    let flow = positive("flow rate", duty.flow)?;
    let head = positive("head", duty.head)?;
    let machine_head = positive("machine head", duty.machine_head)?;
    let omega = positive("rotational speed", duty.speed)?;
    let rho = positive("density", duty.density)?;
    require(
        duty.vapor_pressure.is_finite() && duty.vapor_pressure >= 0.0,
        || "vapour pressure must not be negative".into(),
    )?;
    let mut warnings = Vec::new();

    // 1. Specific speed per stage and eye.
    let eyes = input.suction.eyes();
    let q_eye = flow / eyes;
    let nq = SpecificSpeed::from_head(q_eye, head, omega).nq();
    let (nq_min, nq_max) = corr::NQ_RANGE;
    if !(nq_min..=nq_max).contains(&nq) {
        warnings.push(format!(
            "nq = {nq:.1} is outside {nq_min}–{nq_max}, the range of the radial-impeller \
             correlations; check every automatic value"
        ));
    }

    // 2. Efficiencies.
    let eta_h = fraction(
        "hydraulic efficiency",
        input
            .hydraulic_efficiency
            .resolve(|| corr::hydraulic_efficiency(q_eye, nq)),
    )?;
    let eta_v = fraction(
        "volumetric efficiency",
        input
            .volumetric_efficiency
            .resolve(|| corr::VOLUMETRIC_EFFICIENCY),
    )?;
    let eta = fraction(
        "overall efficiency",
        input
            .overall_efficiency
            .resolve(|| corr::overall_efficiency(q_eye, nq)),
    )?;
    let eta_m = eta / (eta_h * eta_v);
    if eta_m > 1.0 {
        warnings.push(format!(
            "overall efficiency {eta:.3} exceeds η_h·η_v = {:.3}; the efficiencies are \
             inconsistent",
            eta_h * eta_v
        ));
    }
    let impeller_flow = flow / eta_v;
    let q_la_eye = impeller_flow / eyes;
    let theoretical_head = head / eta_h;

    // 3. Shaft.
    let stage_power = rho * G * flow * head / eta;
    let shaft_torque = rho * G * flow * machine_head / eta / omega;
    let shear = positive(
        "allowable shear stress",
        input
            .shaft_shear_stress
            .resolve(|| corr::SHAFT_SHEAR_STRESS),
    )?;
    let shaft_diameter = positive(
        "shaft diameter",
        input
            .shaft_diameter
            .resolve(|| (16.0 * shaft_torque / (PI * shear)).cbrt()),
    )?;
    let hub_diameter = input.hub_diameter.resolve(|| shaft_diameter);
    require(hub_diameter.is_finite() && hub_diameter >= 0.0, || {
        "hub diameter must not be negative".into()
    })?;
    if hub_diameter < shaft_diameter {
        warnings.push("the hub diameter is smaller than the shaft diameter".into());
    }

    // 4. Outlet diameter.
    let outlet_diameter = match input.outlet_diameter {
        Param::Manual(d2) => {
            if !input.pressure_coefficient.is_auto() {
                warnings.push(
                    "the pressure coefficient is ignored because the outlet diameter is fixed"
                        .into(),
                );
            }
            positive("outlet diameter", d2)?
        }
        Param::Auto => {
            let psi = positive(
                "pressure coefficient",
                input
                    .pressure_coefficient
                    .resolve(|| corr::pressure_coefficient(nq)),
            )?;
            2.0 * (2.0 * G * head / psi).sqrt() / omega
        }
    };
    let u2 = omega * outlet_diameter / 2.0;
    let pressure_coefficient = 2.0 * G * head / (u2 * u2);

    // 5. Eye diameter and outlet width.
    let inlet_diameter = match input.inlet_diameter {
        Param::Manual(d1) => positive("inlet diameter", d1)?,
        Param::Auto => {
            let factor = positive(
                "inlet diameter factor",
                input.inlet_diameter_factor.resolve(|| 1.0),
            )?;
            factor * corr::min_relative_velocity_eye_diameter(q_la_eye, omega, hub_diameter)
        }
    };
    require(inlet_diameter > hub_diameter, || {
        format!(
            "inlet diameter {inlet_diameter:.4} m must exceed the hub diameter {hub_diameter:.4} m"
        )
    })?;
    require(inlet_diameter < outlet_diameter, || {
        format!(
            "inlet diameter {inlet_diameter:.4} m must be smaller than the outlet diameter \
             {outlet_diameter:.4} m; the pressure coefficient is too high for this nq"
        )
    })?;
    let outlet_width = positive(
        "outlet width",
        input
            .outlet_width
            .resolve(|| eyes * corr::outlet_width_ratio(nq) * outlet_diameter),
    )?;
    let blade_thickness = positive(
        "blade thickness",
        input
            .blade_thickness
            .resolve(|| corr::blade_thickness(outlet_diameter)),
    )?;
    let incidence = input.incidence.resolve(|| 0.0);

    // 6. Velocity triangles and blades.
    let mean_diameter = ((inlet_diameter.powi(2) + hub_diameter.powi(2)) / 2.0).sqrt();
    let cm1 = q_la_eye / (PI / 4.0 * (inlet_diameter.powi(2) - hub_diameter.powi(2)));
    let cm2 = impeller_flow / (PI * outlet_diameter * outlet_width);
    // Swirl-free inflow: c_u1 = 0, so g·H_th = u₂·c_u2.
    let cu2 = G * theoretical_head / u2;

    let solve = |z: u32| -> Result<_, DesignError> {
        require(z >= MIN_BLADES, || {
            format!("at least {MIN_BLADES} blades are needed")
        })?;
        let section = |d| inlet_section(d, cm1, omega, z, blade_thickness, incidence);
        let (hub, mean, shroud) = (
            section(hub_diameter),
            section(mean_diameter),
            section(inlet_diameter),
        );
        let (beta2b, gamma, tau2) = outlet_blade_angle(
            cu2,
            u2,
            cm2,
            outlet_diameter,
            mean_diameter,
            z,
            blade_thickness,
        )?;
        Ok((hub, mean, shroud, beta2b, gamma, tau2))
    };

    let blade_count = match input.blade_count {
        Param::Manual(z) => z,
        Param::Auto => {
            let mut z = 6;
            for _ in 0..10 {
                let (_, mean, _, beta2b, _, _) = solve(z)?;
                let proposed = corr::pfleiderer_blade_count(
                    mean_diameter,
                    outlet_diameter,
                    mean.blade_angle,
                    beta2b,
                )
                .round()
                .clamp(f64::from(MIN_BLADES), f64::from(MAX_BLADES))
                    as u32;
                if proposed == z {
                    break;
                }
                z = proposed;
            }
            z
        }
    };
    let (inlet_hub, inlet_mean, inlet_shroud, beta2b, slip_factor, tau2) = solve(blade_count)?;
    if !inlet_shroud.blockage.is_finite() || !inlet_hub.blockage.is_finite() {
        return Err(DesignError::Infeasible(
            "the blades block the whole inlet circumference; reduce thickness or blade number"
                .into(),
        ));
    }
    let outlet = OutletSection {
        triangle: VelocityTriangle::new(u2, cm2, cu2),
        blade_angle: beta2b,
        slip_factor,
        blockage: tau2,
    };
    let beta2_deg = beta2b.to_degrees();
    if !(15.0..=40.0).contains(&beta2_deg) {
        warnings.push(format!(
            "outlet blade angle {beta2_deg:.1}° is outside the usual 15°–40°"
        ));
    }

    // 7. Cavitation.
    let nss = positive(
        "suction specific speed",
        input
            .suction_specific_speed
            .resolve(|| corr::SUCTION_SPECIFIC_SPEED),
    )?;
    let n_rpm = omega * 30.0 / PI;
    let npsh_required = (n_rpm * q_eye.sqrt() / nss).powf(4.0 / 3.0);
    let npsh_available = (duty.inlet_total_pressure - duty.vapor_pressure) / (rho * G);
    if npsh_available < npsh_required {
        warnings.push(format!(
            "NPSH available {npsh_available:.2} m is below the estimated NPSH3 \
             {npsh_required:.2} m: the pump will cavitate"
        ));
    } else if npsh_available < 1.3 * npsh_required {
        warnings.push(format!(
            "NPSH margin is small: available {npsh_available:.2} m, estimated NPSH3 \
             {npsh_required:.2} m (less than 30 % margin)"
        ));
    }

    Ok(CentrifugalPumpDesign {
        nq,
        efficiencies: Efficiencies {
            hydraulic: eta_h,
            volumetric: eta_v,
            mechanical: eta_m,
            overall: eta,
        },
        impeller_flow,
        theoretical_head,
        stage_power,
        shaft_torque,
        shaft_diameter,
        hub_diameter,
        inlet_diameter,
        outlet_diameter,
        outlet_width,
        blade_count,
        blade_thickness,
        pressure_coefficient,
        inlet_hub,
        inlet_mean,
        inlet_shroud,
        outlet,
        npsh_required,
        npsh_available,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triangle::euler_work;
    use approx::assert_relative_eq;

    /// 100 m³/h, 32 m, 2900 rpm, water at 20 °C and 1 atm.
    fn duty() -> PumpDuty {
        PumpDuty {
            flow: 100.0 / 3600.0,
            head: 32.0,
            machine_head: 32.0,
            speed: 2900.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
        }
    }

    #[test]
    fn reference_pump_has_plausible_dimensions() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        assert_relative_eq!(d.nq, 35.92, epsilon = 0.01);
        // ψ_opt(35.9) = 0.918 → d₂ ≈ 172 mm, a typical 65-160 size.
        assert!(
            (0.165..0.180).contains(&d.outlet_diameter),
            "d2 = {}",
            d.outlet_diameter
        );
        assert!(
            (0.060..0.085).contains(&d.inlet_diameter),
            "d1 = {}",
            d.inlet_diameter
        );
        assert!(
            (0.012..0.022).contains(&d.outlet_width),
            "b2 = {}",
            d.outlet_width
        );
        assert!((5..=7).contains(&d.blade_count), "z = {}", d.blade_count);
        let beta2 = d.outlet.blade_angle.to_degrees();
        assert!((18.0..30.0).contains(&beta2), "β2B = {beta2}");
        assert!(
            (2.0..5.0).contains(&d.npsh_required),
            "NPSH3 = {}",
            d.npsh_required
        );
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    }

    #[test]
    fn outlet_triangle_delivers_theoretical_head() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let inlet = d.inlet_mean.triangle;
        assert_relative_eq!(
            euler_work(&inlet, &d.outlet.triangle) / G,
            d.theoretical_head,
            max_relative = 1e-12
        );
        // The slip relation holds at the solved blade angle.
        let o = &d.outlet;
        let phi2 = o.triangle.cm / o.triangle.u;
        assert_relative_eq!(
            o.triangle.cu / o.triangle.u,
            o.slip_factor - phi2 * o.blockage / o.blade_angle.tan(),
            epsilon = 1e-9
        );
        // Continuity at the outlet.
        assert_relative_eq!(
            o.triangle.cm * PI * d.outlet_diameter * d.outlet_width,
            d.impeller_flow,
            max_relative = 1e-12
        );
    }

    #[test]
    fn efficiencies_multiply_to_overall() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let e = d.efficiencies;
        assert_relative_eq!(e.hydraulic * e.volumetric * e.mechanical, e.overall);
        assert_relative_eq!(
            d.stage_power,
            998.2 * G * duty().flow * 32.0 / e.overall,
            max_relative = 1e-12
        );
    }

    #[test]
    fn manual_values_are_respected() {
        let input = CentrifugalPumpInput {
            outlet_diameter: Param::Manual(0.18),
            blade_count: Param::Manual(7),
            hydraulic_efficiency: Param::Manual(0.88),
            ..Default::default()
        };
        let d = design(&duty(), &input).unwrap();
        assert_eq!(d.outlet_diameter, 0.18);
        assert_eq!(d.blade_count, 7);
        assert_relative_eq!(d.theoretical_head, 32.0 / 0.88);
        let u2 = d.outlet.triangle.u;
        assert_relative_eq!(d.pressure_coefficient, 2.0 * G * 32.0 / (u2 * u2));
    }

    #[test]
    fn double_suction_halves_flow_per_eye() {
        let mut big = duty();
        big.flow *= 2.0;
        let input = CentrifugalPumpInput {
            suction: Suction::Double,
            ..Default::default()
        };
        let d = design(&big, &input).unwrap();
        assert_relative_eq!(d.nq, 35.92, epsilon = 0.01);
    }

    #[test]
    fn larger_eye_lowers_inlet_velocity() {
        let base = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let input = CentrifugalPumpInput {
            inlet_diameter_factor: Param::Manual(1.15),
            ..Default::default()
        };
        let wide = design(&duty(), &input).unwrap();
        assert_relative_eq!(
            wide.inlet_diameter,
            1.15 * base.inlet_diameter,
            max_relative = 1e-12
        );
        assert!(wide.inlet_shroud.triangle.cm < base.inlet_shroud.triangle.cm);
        assert!(wide.inlet_shroud.blade_angle < base.inlet_shroud.blade_angle);
    }

    #[test]
    fn infeasible_head_is_reported() {
        // A tiny impeller cannot reach the head with any blade angle.
        let input = CentrifugalPumpInput {
            outlet_diameter: Param::Manual(0.12),
            ..Default::default()
        };
        assert!(matches!(
            design(&duty(), &input),
            Err(DesignError::Infeasible(_))
        ));
    }

    #[test]
    fn low_npsh_available_is_warned() {
        let mut d = duty();
        d.inlet_total_pressure = 30_000.0;
        let result = design(&d, &CentrifugalPumpInput::default()).unwrap();
        assert!(result.warnings.iter().any(|w| w.contains("cavitate")));
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let input = CentrifugalPumpInput {
            overall_efficiency: Param::Manual(1.2),
            ..Default::default()
        };
        assert!(matches!(
            design(&duty(), &input),
            Err(DesignError::InvalidInput(_))
        ));
        let mut d = duty();
        d.flow = 0.0;
        assert!(design(&d, &CentrifugalPumpInput::default()).is_err());
    }

    #[test]
    fn input_serializes_compactly() {
        let input = CentrifugalPumpInput {
            blade_count: Param::Manual(6),
            ..Default::default()
        };
        let json = serde_json::to_string(&input).unwrap();
        let back: CentrifugalPumpInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back, input);
        let partial: CentrifugalPumpInput =
            serde_json::from_str(r#"{"suction":"double"}"#).unwrap();
        assert_eq!(partial.suction, Suction::Double);
        assert!(partial.blade_count.is_auto());
    }
}
