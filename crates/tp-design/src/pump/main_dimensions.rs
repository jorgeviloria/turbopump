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
use crate::slip::{F1_RADIAL, f1_semi_axial, wiesner_gulich};
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

/// Meridional shape of the impeller outlet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpellerType {
    /// Semi-axial above nq = 60, radial below (Gülich Chap. 7.5).
    #[default]
    Auto,
    /// Radial outlet at one diameter d₂.
    Radial,
    /// Semi-axial (mixed-flow): the outlet edge runs from d₂i at the hub to d₂a at the
    /// shroud, the outer streamline inclined ε_s to a normal to the axis.
    SemiAxial,
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
    /// Number of impeller stages in series (≥ 1).
    pub stages: u32,
    /// Rotational speed, rad/s.
    pub speed: f64,
    /// kg/m³
    pub density: f64,
    /// Vapour pressure at the inlet temperature, Pa.
    pub vapor_pressure: f64,
    /// Total pressure at the pump inlet, Pa.
    pub inlet_total_pressure: f64,
    /// Kinematic viscosity of the liquid, m²/s.
    pub kinematic_viscosity: f64,
}

/// User inputs of the main-dimensions step. Every value is automatic by default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CentrifugalPumpInput {
    pub suction: Suction,
    pub impeller_type: ImpellerType,
    /// Inclination ε_s of the outer streamline at a semi-axial outlet to a normal to the
    /// axis, rad (Gülich eq. 7.12).
    pub outlet_inclination: Param<f64>,
    /// Hub diameter d₂i of a semi-axial outlet, m (Gülich eqs. 7.13–7.14).
    pub outlet_hub_diameter: Param<f64>,
    /// Normal or suction impeller; sets the automatic eye-diameter factor.
    pub inlet_design: corr::InletDesign,
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
    /// Head rise from the best efficiency point to shut-off, (H₀ − H)/H. Manual: the
    /// blade number and the outlet blade angle follow Lobanoff & Ross (Figs. 3-2 and
    /// 3-13) unless they are manual themselves. Automatic: only estimated.
    pub head_rise: Param<f64>,
    /// β₂B, rad. Automatic: solved so that the blades produce the head at the outlet
    /// diameter of ψ. Manual (or set by the head rise): the outlet diameter is solved
    /// instead, or the outlet width when the diameter is manual.
    pub outlet_blade_angle: Param<f64>,
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
    /// Number of impeller eyes f_q: 1 or 2.
    pub eyes: f64,
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
    /// Correlation set used for the automatic efficiencies.
    pub layout: corr::PumpLayout,
    /// Expected scatter ±Δη of the automatic overall efficiency.
    pub efficiency_tolerance: f64,
    /// Deviation angle δ₂ = β₂B − β₂, rad.
    pub deviation_angle: f64,
    /// Deceleration w₂/w₁ at the outer streamline (de Haller number).
    pub de_haller: f64,
    /// Semi-axial impeller: the outlet edge runs from d₂i to d₂a. The outlet triangle,
    /// d₂ and the slip factor belong to the outer streamline (d₂ = d₂a).
    pub semi_axial: bool,
    /// d₂i at the hub end of the outlet edge; d₂ for a radial impeller, m.
    pub outlet_hub_diameter: f64,
    /// d₂m = √((d₂a² + d₂i²)/2), which sets the outlet area π·d₂m·b₂, m.
    pub outlet_mean_diameter: f64,
    /// ε_s of the outer streamline at the outlet, rad; 0 for a radial impeller.
    pub outlet_inclination: f64,
    /// Head rise (H₀ − H)/H of the blade number and outlet angle (Lobanoff & Ross,
    /// Fig. 3-2) for nq 4–68; outside, that of Gülich eq. 3.28 with a volute.
    pub head_rise: f64,
    /// Area between the blades at the inlet over the eye area, A_v/A_e (Lobanoff & Ross,
    /// Fig. 3-12).
    pub inlet_area_ratio: f64,
    pub warnings: Vec<String>,
}

impl CentrifugalPumpDesign {
    /// `triangle` with the meridional velocity of the pump flow Q = η_v·Q_La instead of
    /// the impeller flow Q_La, for comparison with references that size the triangles
    /// with Q. The design itself keeps Q_La, which is what passes the impeller.
    pub fn at_pump_flow(&self, triangle: &VelocityTriangle) -> VelocityTriangle {
        VelocityTriangle::new(
            triangle.u,
            triangle.cm * self.efficiencies.volumetric,
            triangle.cu,
        )
    }
}

const MIN_BLADES: u32 = 3;

/// Outlet of the impeller for one blade number.
#[derive(Debug, Clone, Copy)]
struct Outlet {
    diameter: f64,
    width: f64,
    thickness: f64,
    blade_angle: f64,
    slip_factor: f64,
    blockage: f64,
}

/// Root of an increasing `f` above `lo`: grows the upper end from `start` until `f`
/// changes sign. `None` when it never does.
fn rising_root(f: impl Fn(f64) -> f64, lo: f64, start: f64) -> Option<f64> {
    if f(lo) >= 0.0 {
        return None;
    }
    let mut hi = start.max(lo * 1.01);
    for _ in 0..60 {
        if f(hi) > 0.0 {
            return roots::brent(&f, lo, hi, 1e-12).ok();
        }
        hi *= 1.5;
    }
    None
}

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
#[allow(clippy::too_many_arguments)]
fn outlet_blade_angle(
    target_cu2: f64,
    u2: f64,
    cm2: f64,
    d2: f64,
    d1m: f64,
    z: u32,
    e: f64,
    f1: f64,
) -> Result<(f64, f64, f64), DesignError> {
    let phi2 = cm2 / u2;
    let residual = |beta: f64| {
        let gamma = wiesner_gulich(beta, z, d1m / d2, f1);
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
    let gamma = wiesner_gulich(beta, z, d1m / d2, f1);
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

    require(duty.stages >= 1, || "a pump has at least one stage".into())?;

    // 1. Specific speed per stage and eye.
    let eyes = input.suction.eyes();
    let q_eye = flow / eyes;
    let nq = SpecificSpeed::from_head(q_eye, head, omega).nq();
    let semi_axial = match input.impeller_type {
        ImpellerType::Auto => nq > corr::SEMI_AXIAL_NQ,
        ImpellerType::Radial => false,
        ImpellerType::SemiAxial => true,
    };
    let (nq_min, nq_max) = corr::NQ_RANGE;
    if semi_axial {
        let (lo, hi) = corr::SEMI_AXIAL_NQ_RANGE;
        if !(lo..=hi).contains(&nq) {
            warnings.push(format!(
                "semi-axial impellers are at their best for nq = {lo}–{hi}, this one has \
                 nq = {nq:.1} (Gülich Chap. 7.5)"
            ));
        }
    } else if !(nq_min..=nq_max).contains(&nq) {
        warnings.push(format!(
            "nq = {nq:.1} is outside {nq_min}–{nq_max}, the range of radial impellers with \
             5–7 blades; check every automatic value"
        ));
    }
    let layout = if semi_axial {
        corr::PumpLayout::SemiAxial
    } else if duty.stages > 1 {
        corr::PumpLayout::Multistage
    } else if input.suction == Suction::Double {
        corr::PumpLayout::DoubleEntry
    } else {
        corr::PumpLayout::SingleStage
    };

    // 2. Efficiencies. Table 3.9 uses the flow of the pump; double-entry impellers use
    // the single-stage hydraulic equation with the flow of one eye.
    let auto_efficiencies =
        input.hydraulic_efficiency.is_auto() || input.overall_efficiency.is_auto();
    if auto_efficiencies {
        if flow < corr::MIN_EFFICIENCY_FLOW {
            warnings.push(format!(
                "the efficiency correlations are valid for Q ≥ {} m³/s",
                corr::MIN_EFFICIENCY_FLOW
            ));
        }
        if nq > layout.max_nq() {
            warnings.push(format!(
                "the {} efficiency correlation is valid up to nq = {}",
                layout.name(),
                layout.max_nq()
            ));
        }
    }
    let hydraulic_flow = match layout {
        corr::PumpLayout::DoubleEntry => q_eye,
        _ => flow,
    };
    let eta_h = fraction(
        "hydraulic efficiency",
        input
            .hydraulic_efficiency
            .resolve(|| corr::hydraulic_efficiency(layout, hydraulic_flow, nq)),
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
            .resolve(|| corr::overall_efficiency(layout, flow, nq)),
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

    // 4. Eye diameter: it depends only on the flow, the speed and the hub.
    let inlet_diameter = match input.inlet_diameter {
        Param::Manual(d1) => positive("inlet diameter", d1)?,
        Param::Auto => {
            let factor = positive(
                "inlet diameter factor",
                input
                    .inlet_diameter_factor
                    .resolve(|| corr::inlet_diameter_factor(input.inlet_design, nq)),
            )?;
            factor * corr::min_relative_velocity_eye_diameter(q_la_eye, omega, hub_diameter)
        }
    };
    require(inlet_diameter > hub_diameter, || {
        format!(
            "inlet diameter {inlet_diameter:.4} m must exceed the hub diameter {hub_diameter:.4} m"
        )
    })?;
    let incidence = input.incidence.resolve(|| 0.0);
    let (i_min, i_max) = corr::INCIDENCE_RANGE;
    if !(i_min..=i_max).contains(&incidence.to_degrees()) {
        warnings.push(format!(
            "incidence {:.1}° is outside the recommended {i_min}°–{i_max}°",
            incidence.to_degrees()
        ));
    }

    // 5. What sets the outlet. A head rise picks the blade number and the outlet blade
    // angle (Lobanoff & Ross, Figs. 3-2 and 3-13); a prescribed outlet angle leaves the
    // outlet diameter, or the width when the diameter is fixed, to the head.
    let head_rise_target = match input.head_rise {
        Param::Manual(h) => {
            require(h.is_finite() && h > 0.0 && h < 2.0, || {
                format!(
                    "the head rise must lie between 0 and 200 %, got {:.1} %",
                    h * 100.0
                )
            })?;
            let (lo, hi) = corr::HEAD_RISE_NQ_RANGE;
            if !(lo..=hi).contains(&nq) {
                warnings.push(format!(
                    "the head-rise chart covers nq = {lo}–{hi}; at nq = {nq:.1} the blade \
                     number it gives is extrapolated"
                ));
            }
            Some(h)
        }
        Param::Auto => None,
    };
    if let Param::Manual(beta) = input.outlet_blade_angle {
        require(
            beta.is_finite() && beta >= 5f64.to_radians() && beta <= FRAC_PI_2,
            || {
                format!(
                    "the outlet blade angle must lie between 5° and 90°, got {:.1}°",
                    beta.to_degrees()
                )
            },
        )?;
    }
    let prescribed_angle = |z: u32| match input.outlet_blade_angle {
        Param::Manual(beta) => Some(beta),
        Param::Auto => head_rise_target.map(|_| corr::standard_outlet_blade_angle(z)),
    };
    let angle_prescribed = !input.outlet_blade_angle.is_auto() || head_rise_target.is_some();

    // Outlet diameter from ψ, unless it is manual or follows from the outlet angle.
    let psi_diameter = match input.outlet_diameter {
        Param::Manual(d2) => {
            if !input.pressure_coefficient.is_auto() {
                warnings.push(
                    "the pressure coefficient is ignored because the outlet diameter is fixed"
                        .into(),
                );
            }
            Some(positive("outlet diameter", d2)?)
        }
        Param::Auto if angle_prescribed => {
            if !input.pressure_coefficient.is_auto() {
                warnings.push(
                    "the pressure coefficient is ignored because the outlet blade angle sets \
                     the outlet diameter"
                        .into(),
                );
            }
            None
        }
        Param::Auto => {
            let psi = positive(
                "pressure coefficient",
                input
                    .pressure_coefficient
                    .resolve(|| corr::pressure_coefficient(nq)),
            )?;
            Some(2.0 * (2.0 * G * head / psi).sqrt() / omega)
        }
    };
    if angle_prescribed && !input.outlet_diameter.is_auto() && !input.outlet_width.is_auto() {
        return Err(DesignError::InvalidInput(
            "with the outlet blade angle prescribed, the outlet diameter or the outlet width \
             must be automatic"
                .into(),
        ));
    }

    // 6. Velocity triangles and blades.
    let mean_diameter = ((inlet_diameter.powi(2) + hub_diameter.powi(2)) / 2.0).sqrt();
    // Outlet edge of a semi-axial impeller: d₂i and the d₂m that sets the outlet area.
    let f1 = if semi_axial {
        f1_semi_axial(nq)
    } else {
        F1_RADIAL
    };
    let (mean_ratio, hub_ratio) = corr::outlet_diameter_ratios(nq);
    if let Param::Manual(d2i) = input.outlet_hub_diameter
        && semi_axial
    {
        positive("outlet hub diameter", d2i)?;
    }
    let outlet_hub_for = |d2: f64| match (semi_axial, input.outlet_hub_diameter) {
        (false, _) => d2,
        (true, Param::Manual(d2i)) => d2i,
        (true, Param::Auto) => hub_ratio * d2,
    };
    let outlet_mean_for = |d2: f64| match (semi_axial, input.outlet_hub_diameter) {
        (false, _) => d2,
        (true, Param::Manual(d2i)) => corr::mean_outlet_diameter(d2, d2i),
        (true, Param::Auto) => mean_ratio * d2,
    };
    let cm1 = q_la_eye / (PI / 4.0 * (inlet_diameter.powi(2) - hub_diameter.powi(2)));
    let width_for = |d2: f64| {
        input
            .outlet_width
            .resolve(|| eyes * corr::outlet_width_ratio(nq) * d2)
    };
    let thickness_for = |d2: f64| input.blade_thickness.resolve(|| corr::blade_thickness(d2));
    // Swirl-free inflow: c_u1 = 0, so g·H_th = u₂·c_u2. What the blades deliver at the
    // angle β₂B through the slip relation, minus what the head needs.
    let excess = |z: u32, beta: f64, d2: f64, b2: f64, e: f64| {
        let u2 = omega * d2 / 2.0;
        let cm2 = impeller_flow / (PI * outlet_mean_for(d2) * b2);
        let gamma = wiesner_gulich(beta, z, mean_diameter / d2, f1);
        let tau = blockage_factor(z, e, d2, beta, FRAC_PI_2);
        if !tau.is_finite() {
            return -G * theoretical_head;
        }
        u2 * (gamma * u2 - cm2 * tau / beta.tan()) - G * theoretical_head
    };
    let outlet_for = |z: u32| -> Result<Outlet, DesignError> {
        let (d2, b2, beta) = match (prescribed_angle(z), psi_diameter) {
            (None, Some(d2)) => {
                let b2 = positive("outlet width", width_for(d2))?;
                let e = positive("blade thickness", thickness_for(d2))?;
                let u2 = omega * d2 / 2.0;
                let cm2 = impeller_flow / (PI * outlet_mean_for(d2) * b2);
                let cu2 = G * theoretical_head / u2;
                let (beta, gamma, tau) =
                    outlet_blade_angle(cu2, u2, cm2, d2, mean_diameter, z, e, f1)?;
                return Ok(Outlet {
                    diameter: d2,
                    width: b2,
                    thickness: e,
                    blade_angle: beta,
                    slip_factor: gamma,
                    blockage: tau,
                });
            }
            (Some(beta), None) => {
                let f = |d2: f64| excess(z, beta, d2, width_for(d2), thickness_for(d2));
                let start = 2.0 * (2.0 * G * head / corr::pressure_coefficient(nq)).sqrt() / omega;
                let d2 = rising_root(f, 1.02 * inlet_diameter, start).ok_or_else(|| {
                    DesignError::Infeasible(format!(
                        "no outlet diameter gives the head with {z} blades at β2B = {:.1}°",
                        beta.to_degrees()
                    ))
                })?;
                (d2, positive("outlet width", width_for(d2))?, beta)
            }
            (Some(beta), Some(d2)) => {
                let e = positive("blade thickness", thickness_for(d2))?;
                let f = |b2: f64| excess(z, beta, d2, b2, e);
                let start = eyes * corr::outlet_width_ratio(nq) * d2;
                let b2 = rising_root(f, 1e-4 * d2, start).ok_or_else(|| {
                    DesignError::Infeasible(format!(
                        "with d2 = {:.1} mm, {z} blades and β2B = {:.1}° no outlet width gives \
                         the head; increase the outlet diameter, the blade angle or the \
                         number of blades",
                        d2 * 1e3,
                        beta.to_degrees()
                    ))
                })?;
                (d2, b2, beta)
            }
            (None, None) => unreachable!("ψ sets the diameter unless the angle is prescribed"),
        };
        let e = positive("blade thickness", thickness_for(d2))?;
        Ok(Outlet {
            diameter: d2,
            width: b2,
            thickness: e,
            blade_angle: beta,
            slip_factor: wiesner_gulich(beta, z, mean_diameter / d2, f1),
            blockage: blockage_factor(z, e, d2, beta, FRAC_PI_2),
        })
    };

    let solve = |z: u32| -> Result<_, DesignError> {
        require(z >= MIN_BLADES, || {
            format!("at least {MIN_BLADES} blades are needed")
        })?;
        let outlet = outlet_for(z)?;
        let section = |d| inlet_section(d, cm1, omega, z, outlet.thickness, incidence);
        let (hub, mean, shroud) = (
            section(hub_diameter),
            section(mean_diameter),
            section(inlet_diameter),
        );
        Ok((hub, mean, shroud, outlet))
    };

    let blade_count = match (input.blade_count, head_rise_target) {
        (Param::Manual(z), _) => z,
        (Param::Auto, Some(h)) => {
            corr::blade_count_for_head_rise(nq, h, input.outlet_blade_angle.manual_value())
        }
        (Param::Auto, None) => {
            // Gülich's 5–7 blades where they apply, the wider range elsewhere.
            let (lo, hi) = if (nq_min..=nq_max).contains(&nq) {
                corr::RADIAL_BLADE_COUNT_RANGE
            } else {
                corr::BLADE_COUNT_RANGE
            };
            let mut z = 6;
            for _ in 0..10 {
                let (_, mean, _, outlet) = solve(z)?;
                let proposed = corr::pfleiderer_blade_count(
                    mean_diameter,
                    outlet.diameter,
                    mean.blade_angle,
                    outlet.blade_angle,
                )
                .round()
                .clamp(f64::from(lo), f64::from(hi)) as u32;
                if proposed == z {
                    break;
                }
                z = proposed;
            }
            z
        }
    };
    let (inlet_hub, inlet_mean, inlet_shroud, out) = solve(blade_count)?;
    let Outlet {
        diameter: outlet_diameter,
        width: outlet_width,
        thickness: blade_thickness,
        blade_angle: beta2b,
        slip_factor,
        blockage: tau2,
    } = out;
    require(inlet_diameter < outlet_diameter, || {
        format!(
            "inlet diameter {inlet_diameter:.4} m must be smaller than the outlet diameter \
             {outlet_diameter:.4} m; the pressure coefficient is too high for this nq"
        )
    })?;
    let u2 = omega * outlet_diameter / 2.0;
    let pressure_coefficient = 2.0 * G * head / (u2 * u2);
    let outlet_hub_diameter = outlet_hub_for(outlet_diameter);
    let outlet_mean_diameter = outlet_mean_for(outlet_diameter);
    require(
        outlet_hub_diameter > 0.0 && outlet_hub_diameter <= outlet_diameter,
        || {
            format!(
                "the outlet hub diameter {:.1} mm must lie between 0 and d2 = {:.1} mm",
                outlet_hub_diameter * 1e3,
                outlet_diameter * 1e3
            )
        },
    )?;
    let outlet_inclination = if semi_axial {
        let eps = input
            .outlet_inclination
            .resolve(|| corr::outlet_inclination(nq));
        require(eps.is_finite() && eps > 0.0, || {
            "the outlet inclination must be positive".into()
        })?;
        if eps > corr::MAX_OUTLET_INCLINATION {
            warnings.push(format!(
                "the outlet inclination {:.0}° is nearly axial; the contour is limited to {:.0}° \
                 (an axial impeller would suit better)",
                eps.to_degrees(),
                corr::MAX_OUTLET_INCLINATION.to_degrees()
            ));
        }
        eps.min(corr::MAX_OUTLET_INCLINATION)
    } else {
        0.0
    };
    let cm2 = impeller_flow / (PI * outlet_mean_diameter * outlet_width);
    let cu2 = G * theoretical_head / u2;
    let inlet_width = (inlet_diameter - hub_diameter) / 2.0;
    if outlet_width / eyes >= inlet_width {
        warnings.push(format!(
            "outlet width {:.1} mm is not below the inlet width b1 = (d1 − dn)/2 = {:.1} mm",
            outlet_width / eyes * 1e3,
            inlet_width * 1e3
        ));
    }
    if !inlet_shroud.blockage.is_finite() || !inlet_hub.blockage.is_finite() {
        return Err(DesignError::Infeasible(
            "the blades block the whole inlet circumference; reduce thickness or blade number"
                .into(),
        ));
    }

    // Outside the chart the shut-off head of Gülich eq. 3.28 sets it, as in the curve.
    let head_rise = {
        let (lo, hi) = corr::HEAD_RISE_NQ_RANGE;
        if (lo..=hi).contains(&nq) {
            corr::head_rise_for_blades(nq, blade_count, beta2b)
        } else {
            crate::performance::shutoff_coefficient(nq, crate::performance::Collector::Volute)
                / pressure_coefficient
                - 1.0
        }
    };
    if let Some(target) = head_rise_target {
        if (head_rise - target).abs() > 0.03 {
            warnings.push(format!(
                "the head rise of {:.0} % is out of reach at nq = {nq:.1}: {blade_count} blades \
                 give about {:.0} %",
                target * 100.0,
                head_rise * 100.0
            ));
        }
        if blade_count >= 7 && input.blade_count.is_auto() {
            warnings.push(format!(
                "{blade_count} blades give a flat head curve that droops toward shut-off \
                 (Lobanoff & Ross Fig. 3-2); check its stability"
            ));
        }
    }
    // Throat area between the blades at the eye over the eye area (Lobanoff & Ross). Only
    // reported: their band reflects large eyes with β₁B ≈ 15–20°, while the eye of minimum
    // w₁ used here gives β₁B ≈ 35° and a larger ratio.
    let throat = PI * inlet_diameter * inlet_shroud.blade_angle.sin() / f64::from(blade_count)
        - blade_thickness;
    let eye_area = PI / 4.0 * (inlet_diameter.powi(2) - hub_diameter.powi(2));
    let inlet_area_ratio = f64::from(blade_count) * throat.max(0.0) * inlet_width / eye_area;
    let outlet = OutletSection {
        triangle: VelocityTriangle::new(u2, cm2, cu2),
        blade_angle: beta2b,
        slip_factor,
        blockage: tau2,
    };
    let beta2_deg = beta2b.to_degrees();
    let (b_min, b_max) = corr::OUTLET_BLADE_ANGLE_RANGE;
    if !(b_min..=b_max).contains(&beta2_deg) {
        warnings.push(format!(
            "outlet blade angle {beta2_deg:.1}° is outside the usual {b_min}°–{b_max}°"
        ));
    }
    let deviation_angle = beta2b - outlet.triangle.beta();
    if deviation_angle.to_degrees() > corr::MAX_DEVIATION_ANGLE {
        warnings.push(format!(
            "deviation angle β2B − β2 = {:.1}° exceeds {}°: expect high mixing losses",
            deviation_angle.to_degrees(),
            corr::MAX_DEVIATION_ANGLE
        ));
    }
    let de_haller = outlet.triangle.w() / inlet_shroud.triangle.w();
    if de_haller < corr::DE_HALLER_LIMIT {
        warnings.push(format!(
            "deceleration w2/w1 = {de_haller:.2} at the shroud is below {} (de Haller): \
             risk of flow separation",
            corr::DE_HALLER_LIMIT
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
        eyes,
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
        layout,
        efficiency_tolerance: corr::efficiency_tolerance(eta),
        deviation_angle,
        de_haller,
        semi_axial,
        outlet_hub_diameter,
        outlet_mean_diameter,
        outlet_inclination,
        head_rise,
        inlet_area_ratio,
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
            stages: 1,
            speed: 2900.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
            kinematic_viscosity: 1.0e-6,
        }
    }

    #[test]
    fn triangles_with_the_pump_flow_satisfy_continuity_with_q() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let outlet = d.at_pump_flow(&d.outlet.triangle);
        let area = PI * d.outlet_mean_diameter * d.outlet_width;
        assert_relative_eq!(outlet.cm * area, duty().flow, max_relative = 1e-12);
        assert_relative_eq!(outlet.cu, d.outlet.triangle.cu);
        assert_relative_eq!(outlet.u, d.outlet.triangle.u);
        let inlet = d.at_pump_flow(&d.inlet_mean.triangle);
        let eye = PI / 4.0 * (d.inlet_diameter.powi(2) - d.hub_diameter.powi(2));
        assert_relative_eq!(inlet.cm * eye, duty().flow, max_relative = 1e-12);
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
    fn eye_diameter_factor_follows_table_7_1() {
        // nq = 35.92: f_d1 = 1.15 − 0.1·(35.92 − 15)/25 = 1.0663 for a normal impeller.
        let base = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let f_auto = corr::inlet_diameter_factor(corr::InletDesign::Normal, base.nq);
        assert_relative_eq!(f_auto, 1.0663, epsilon = 1e-4);
        let suction = CentrifugalPumpInput {
            inlet_design: corr::InletDesign::Suction,
            ..Default::default()
        };
        let s = design(&duty(), &suction).unwrap();
        assert_relative_eq!(
            s.inlet_diameter / base.inlet_diameter,
            (f_auto + 0.1) / f_auto,
            max_relative = 1e-12
        );
    }

    #[test]
    fn larger_eye_lowers_inlet_velocity() {
        let base = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let f_auto = corr::inlet_diameter_factor(corr::InletDesign::Normal, base.nq);
        let input = CentrifugalPumpInput {
            inlet_diameter_factor: Param::Manual(1.25),
            ..Default::default()
        };
        let wide = design(&duty(), &input).unwrap();
        assert_relative_eq!(
            wide.inlet_diameter,
            1.25 / f_auto * base.inlet_diameter,
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

    /// g·H_th = u₂·c_u2 with c_u2 from the slip relation at the solved geometry.
    fn delivers_head(d: &CentrifugalPumpDesign) {
        let o = &d.outlet;
        let phi2 = o.triangle.cm / o.triangle.u;
        assert_relative_eq!(
            o.triangle.cu / o.triangle.u,
            o.slip_factor - phi2 * o.blockage / o.blade_angle.tan(),
            epsilon = 1e-9
        );
        assert_relative_eq!(
            o.triangle.u * o.triangle.cu / G,
            d.theoretical_head,
            max_relative = 1e-9
        );
    }

    #[test]
    fn head_rise_picks_blades_and_outlet_angle() {
        let steep = CentrifugalPumpInput {
            head_rise: Param::Manual(0.40),
            ..Default::default()
        };
        let flat = CentrifugalPumpInput {
            head_rise: Param::Manual(0.15),
            ..Default::default()
        };
        let s = design(&duty(), &steep).unwrap();
        let f = design(&duty(), &flat).unwrap();
        assert!(
            s.blade_count < f.blade_count,
            "{} vs {}",
            s.blade_count,
            f.blade_count
        );
        assert!(s.outlet.blade_angle < f.outlet.blade_angle);
        assert_relative_eq!(
            s.outlet.blade_angle,
            corr::standard_outlet_blade_angle(s.blade_count)
        );
        assert!((s.head_rise - 0.40).abs() < 0.05, "{}", s.head_rise);
        assert!((f.head_rise - 0.15).abs() < 0.05, "{}", f.head_rise);
        // Fewer blades at a flatter angle need a larger impeller for the same head.
        assert!(s.outlet_diameter > f.outlet_diameter);
        delivers_head(&s);
        delivers_head(&f);
    }

    #[test]
    fn automatic_design_estimates_its_head_rise() {
        let d = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        assert_relative_eq!(
            d.head_rise,
            corr::head_rise_for_blades(d.nq, d.blade_count, d.outlet.blade_angle)
        );
        assert!(d.inlet_area_ratio > 0.0 && d.inlet_area_ratio < 1.0);
    }

    #[test]
    fn prescribed_outlet_angle_sets_the_diameter() {
        let base = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        let input = CentrifugalPumpInput {
            blade_count: Param::Manual(base.blade_count),
            outlet_blade_angle: Param::Manual(base.outlet.blade_angle),
            ..Default::default()
        };
        let d = design(&duty(), &input).unwrap();
        // The same angle gives back the same impeller.
        assert_relative_eq!(d.outlet_diameter, base.outlet_diameter, max_relative = 1e-6);
        // Radial blades reach the head with a smaller impeller.
        let radial = CentrifugalPumpInput {
            outlet_blade_angle: Param::Manual(FRAC_PI_2),
            ..input
        };
        let r = design(&duty(), &radial).unwrap();
        assert!(r.outlet_diameter < base.outlet_diameter);
        assert_relative_eq!(r.outlet.blade_angle, FRAC_PI_2);
        delivers_head(&r);
    }

    #[test]
    fn prescribed_angle_with_fixed_diameter_sets_the_width() {
        let input = CentrifugalPumpInput {
            outlet_diameter: Param::Manual(0.19),
            blade_count: Param::Manual(4),
            outlet_blade_angle: Param::Manual(20f64.to_radians()),
            ..Default::default()
        };
        let d = design(&duty(), &input).unwrap();
        assert_eq!(d.outlet_diameter, 0.19);
        delivers_head(&d);
        let both = CentrifugalPumpInput {
            outlet_width: Param::Manual(0.02),
            ..input
        };
        assert!(matches!(
            design(&duty(), &both),
            Err(DesignError::InvalidInput(_))
        ));
    }

    #[test]
    fn automatic_blade_number_leaves_five_to_seven_outside_their_range() {
        // nq ≈ 210: an eye almost as large as the impeller asks Pfleiderer for many blades.
        let mut high = duty();
        high.flow = 600.0 / 3600.0;
        high.head = 10.0;
        high.machine_head = 10.0;
        let input = CentrifugalPumpInput {
            hydraulic_efficiency: Param::Manual(0.8),
            overall_efficiency: Param::Manual(0.6),
            ..Default::default()
        };
        let d = design(&high, &input).unwrap();
        assert!(d.blade_count > 7, "z = {} at nq {}", d.blade_count, d.nq);
        // Within 10 < nq < 120 Gülich's 5–7 still hold.
        let r = design(&duty(), &CentrifugalPumpInput::default()).unwrap();
        assert!((5..=7).contains(&r.blade_count));
    }

    /// 800 m³/h, 12 m, 1450 rpm: nq ≈ 106, a semi-axial impeller.
    fn mixed_flow_duty() -> PumpDuty {
        PumpDuty {
            flow: 800.0 / 3600.0,
            head: 12.0,
            machine_head: 12.0,
            speed: 1450.0 * PI / 30.0,
            ..duty()
        }
    }

    #[test]
    fn high_nq_gives_a_semi_axial_impeller() {
        let d = design(&mixed_flow_duty(), &CentrifugalPumpInput::default()).unwrap();
        assert!(
            d.semi_axial && (100.0..110.0).contains(&d.nq),
            "nq {}",
            d.nq
        );
        assert_eq!(d.layout, corr::PumpLayout::SemiAxial);
        let (mean, hub) = corr::outlet_diameter_ratios(d.nq);
        assert_relative_eq!(
            d.outlet_hub_diameter / d.outlet_diameter,
            hub,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            d.outlet_mean_diameter / d.outlet_diameter,
            mean,
            max_relative = 1e-12
        );
        assert_relative_eq!(d.outlet_inclination, corr::outlet_inclination(d.nq));
        // A₂ = π·d₂m·b₂ and the semi-axial f₁ in the slip factor.
        let o = &d.outlet;
        assert_relative_eq!(
            o.triangle.cm * PI * d.outlet_mean_diameter * d.outlet_width,
            d.impeller_flow,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            o.slip_factor,
            wiesner_gulich(
                o.blade_angle,
                d.blade_count,
                d.inlet_mean.diameter / d.outlet_diameter,
                f1_semi_axial(d.nq)
            ),
            max_relative = 1e-12
        );
        delivers_head(&d);
        // Forced radial, the outlet stays at one diameter.
        let radial = CentrifugalPumpInput {
            impeller_type: ImpellerType::Radial,
            ..Default::default()
        };
        let r = design(&mixed_flow_duty(), &radial).unwrap();
        assert!(!r.semi_axial);
        assert_eq!(r.outlet_hub_diameter, r.outlet_diameter);
        assert_eq!(r.outlet_inclination, 0.0);
        // Below nq = 60 the automatic type stays radial.
        assert!(
            !design(&duty(), &CentrifugalPumpInput::default())
                .unwrap()
                .semi_axial
        );
    }

    #[test]
    fn manual_outlet_hub_diameter_sets_the_mean_diameter() {
        let input = CentrifugalPumpInput {
            outlet_hub_diameter: Param::Manual(0.2),
            ..Default::default()
        };
        let d = design(&mixed_flow_duty(), &input).unwrap();
        assert_eq!(d.outlet_hub_diameter, 0.2);
        assert_relative_eq!(
            d.outlet_mean_diameter,
            corr::mean_outlet_diameter(d.outlet_diameter, 0.2),
            max_relative = 1e-12
        );
        delivers_head(&d);
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
