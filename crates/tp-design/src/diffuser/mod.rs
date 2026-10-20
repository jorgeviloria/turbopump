//! Radial diffusers after an impeller: vaneless rings and vaned diffusers.
//!
//! Procedure (see `docs/theory/voluta-y-difusores.md`), following Gülich 2nd ed.,
//! Chap. 7.9, 7.12 and Table 7.7:
//! 1. inlet diameter d₃ from the gap of Table 10.2, inlet width b₃, outlet diameter d₄;
//! 2. flow at the inlet from the conservation of angular momentum (eqs. T7.7.3 to
//!    T7.7.5) and vane inlet angle α₃B = α₃' + i₃ (eq. T7.7.6);
//! 3. vane number with the vibration rule of Table 10.2, throat width
//!    a₃ = f_a3·(d₃/2)·(X_Le − 1) (eqs. T7.7.8a and T7.7.10);
//! 4. vanes by conformal mapping, with the outlet vane angle chosen for the optimum
//!    area ratio of the diffusing channel (eq. 7.30), checked against criteria A to F
//!    of Chap. 7.9.1.

mod vane;

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::impeller::blade::EdgeShape;
use crate::outlet::ImpellerOutlet;
use crate::pump::correlations::{CorrelationInfo, Verification};
use crate::triangle::blockage_factor;
use crate::{DesignError, Param};

pub use vane::{VaneGeometry, VaneShape};

/// Inputs of the main-dimensions step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffuserMainInput {
    /// Q_Le/Q (eq. T7.7.1).
    pub flow_factor: Param<f64>,
    /// Inlet diameter d₃, m.
    pub inlet_diameter: Param<f64>,
    /// Inlet width b₃, m.
    pub inlet_width: Param<f64>,
    /// Outlet diameter d₄, m.
    pub outlet_diameter: Param<f64>,
    /// Incidence i₃ of the vanes, rad (eq. T7.7.6; ±3°).
    pub incidence: Param<f64>,
}

/// Inputs of the meridional-contour step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffuserContourInput {
    /// Outlet width b₄, m; the side walls are straight from b₃ to b₄.
    pub outlet_width: Param<f64>,
}

/// Inputs of the vane-properties step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VanePropertiesInput {
    /// Number of vanes z_Le.
    pub vane_count: Param<u32>,
    /// Throat width over the width from angular momentum, f_a3 (eq. T7.7.10).
    pub throat_factor: Param<f64>,
    /// Vane thickness at the leading edge e₃, m.
    pub leading_thickness: Param<f64>,
}

/// Inputs of the vane mean-line step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaneMeanLineInput {
    /// Vane angle at the outlet α₄B, rad.
    pub outlet_angle: Param<f64>,
    /// Points along each vane.
    pub points: Param<u32>,
}

/// Inputs of the vane-profile step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaneProfileInput {
    /// Vane thickness at the trailing edge e₄, m; it varies linearly from e₃.
    pub trailing_thickness: Param<f64>,
}

/// Inputs of the vane-edge step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaneEdgeInput {
    pub leading: EdgeShape,
    pub trailing: EdgeShape,
}

impl Default for VaneEdgeInput {
    /// An elliptic leading edge, less sensitive to incidence (Gülich Chap. 7.9.2), and
    /// a blunt trailing edge.
    fn default() -> Self {
        Self {
            leading: EdgeShape::Ellipse { ratio: 3.0 },
            trailing: EdgeShape::Cut,
        }
    }
}

/// Inputs of a radial diffuser, one group per design step. A vaneless diffuser uses
/// only the main dimensions and the meridional contour.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffuserInput {
    pub main: DiffuserMainInput,
    pub contour: DiffuserContourInput,
    pub properties: VanePropertiesInput,
    pub mean_lines: VaneMeanLineInput,
    pub profiles: VaneProfileInput,
    pub edges: VaneEdgeInput,
}

/// Velocities at one radius of the diffuser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowState {
    /// m
    pub radius: f64,
    /// Channel width, m.
    pub width: f64,
    /// m/s
    pub cu: f64,
    /// m/s
    pub cm: f64,
    /// Flow angle from the circumferential direction α, rad.
    pub angle: f64,
}

impl FlowState {
    pub fn velocity(&self) -> f64 {
        self.cu.hypot(self.cm)
    }
}

/// Criteria A to F for vaned diffusers (Gülich 2nd ed., Chap. 7.9.1, item 10).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffuserChecks {
    /// A₄/A₃ of the diffusing channel.
    pub area_ratio: f64,
    /// A_R,opt = 1.05 + 0.184·L₃₋₄/R_eq (eq. 7.30).
    pub optimum_area_ratio: f64,
    /// c_p,opt = 0.36·(L₃₋₄/R_eq)^0.26 (eq. 7.31).
    pub optimum_pressure_recovery: f64,
    /// c₃q/c₂ (B: 0.7 to 0.85).
    pub deceleration: f64,
    /// L₃₋₄/a₃ (C: 2.5 to 6).
    pub length_ratio: f64,
    /// b₃/a₃ (D: 0.8 to 2).
    pub aspect_ratio: f64,
    /// c₄²/(2gH) (E: 0.02 to 0.04 for multistage pumps).
    pub outlet_energy: f64,
    /// Equivalent opening angle ϑ_eq, rad (F, eq. 1.45).
    pub opening_angle: f64,
    /// Allowable opening angle ϑ_al, rad (eq. 1.44).
    pub allowable_angle: f64,
}

/// Vanes of a vaned diffuser.
#[derive(Debug, Clone, PartialEq)]
pub struct VaneDesign {
    pub count: u32,
    /// α₃' with vane blockage, rad.
    pub inlet_flow_angle: f64,
    /// α₃B, rad.
    pub inlet_angle: f64,
    /// α₄B, rad.
    pub outlet_angle: f64,
    pub leading_thickness: f64,
    pub trailing_thickness: f64,
    /// Throat width from eq. T7.7.10, m.
    pub throat_target: f64,
    /// Throat width of the vanes as built, m.
    pub throat: f64,
    /// Channel width at the outlet a₄, m.
    pub outlet_throat: f64,
    /// Length of the diffusing channel L₃₋₄, m.
    pub channel_length: f64,
    pub checks: DiffuserChecks,
    pub geometry: VaneGeometry,
}

/// Result of the diffuser design, in SI units.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffuserDesign {
    /// Design flow Q_Le, m³/s.
    pub flow: f64,
    pub inlet_diameter: f64,
    /// Smallest d₃ of Gülich Table 10.2, m (vaned diffusers).
    pub min_inlet_diameter: f64,
    pub inlet_width: f64,
    pub outlet_diameter: f64,
    pub outlet_width: f64,
    pub inlet: FlowState,
    pub outlet: FlowState,
    /// Ideal static pressure recovery 1 − (c₄/c₃)² (eq. 1.40 without losses).
    pub ideal_pressure_recovery: f64,
    /// Path of a fluid particle through a vaneless diffuser: wrap angle, rad.
    pub wrap_angle: f64,
    /// Path length of a fluid particle through a vaneless diffuser, m.
    pub path_length: f64,
    /// Vanes; `None` for a vaneless diffuser.
    pub vanes: Option<VaneDesign>,
    pub midplane: f64,
    pub rotation_sign: f64,
    pub warnings: Vec<String>,
}

/// Smallest d₃/d₂ for low pressure pulsations (Gülich 2nd ed., Table 10.2): below
/// nq = 40, 1.015 + 0.08·(ρH/(ρ_Ref·H_Ref) − 0.1)^0.8, at least 1.015; above,
/// 1.04 + 0.001·(nq − 40). ρ_Ref = 1000 kg/m³, H_Ref = 1000 m.
pub fn min_inlet_ratio(nq: f64, density: f64, stage_head: f64) -> f64 {
    if nq > 40.0 {
        1.04 + 0.001 * (nq - 40.0)
    } else {
        let x = density * stage_head / (1000.0 * 1000.0) - 0.1;
        if x > 0.0 {
            1.015 + 0.08 * x.powf(0.8)
        } else {
            1.015
        }
    }
}

/// Vane-number pairs that excite the pump: m = ν₃·z_Le − ν₂·z_La = 0 for harmonics
/// ν < 4 (Gülich 2nd ed., Table 10.2). Returns the offending (ν₃, ν₂), if any.
pub fn resonant_harmonics(vanes: u32, blades: u32) -> Option<(u32, u32)> {
    (1..4)
        .flat_map(|n3| (1..4).map(move |n2| (n3, n2)))
        .find(|&(n3, n2)| n3 * vanes == n2 * blades)
}

/// Automatic vane number: 8 below nq = 20 and 12 above (the common choices of Gülich
/// p. 425), moved to the nearest number that passes the rule of Table 10.2.
pub fn default_vane_count(nq: f64, blades: u32) -> u32 {
    let preferred: i64 = if nq < 20.0 { 8 } else { 12 };
    (0..6i64)
        .flat_map(|d| [preferred + d, preferred - d])
        .filter(|&z| z >= 3)
        .map(|z| z as u32)
        .find(|&z| resonant_harmonics(z, blades).is_none())
        .unwrap_or(preferred as u32)
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

impl DiffuserDesign {
    /// Designs a diffuser after `outlet`; `vaned` selects a vaned or vaneless one.
    pub fn new(
        outlet: &ImpellerOutlet,
        input: &DiffuserInput,
        vaned: bool,
    ) -> Result<Self, DesignError> {
        let d2 = outlet.impeller_diameter;
        let r2 = d2 / 2.0;
        let b2 = outlet.impeller_width;
        require(outlet.flow > 0.0 && outlet.cu > 0.0 && d2 > 0.0, || {
            "the diffuser needs a positive flow, c₂u and d₂ from the impeller".into()
        })?;
        let main = &input.main;
        let flow_factor = main.flow_factor.resolve(|| 1.0);
        require((0.8..=1.5).contains(&flow_factor), || {
            format!("Q_Le/Q must lie between 0.8 and 1.5, got {flow_factor}")
        })?;
        let flow = flow_factor * outlet.flow;
        let min_inlet_diameter = if vaned {
            min_inlet_ratio(outlet.nq, outlet.density, outlet.stage_head) * d2
        } else {
            d2
        };
        let inlet_diameter = main.inlet_diameter.resolve(|| min_inlet_diameter);
        require(inlet_diameter >= d2, || {
            format!(
                "the diffuser inlet d₃ = {:.1} mm cannot be inside d₂ = {:.1} mm",
                inlet_diameter * 1e3,
                d2 * 1e3
            )
        })?;
        let inlet_width = positive(
            "the inlet width b₃",
            main.inlet_width
                .resolve(|| if vaned { 1.15 } else { 1.05 } * b2),
        )?;
        let outlet_width = positive(
            "the outlet width b₄",
            input.contour.outlet_width.resolve(|| inlet_width),
        )?;
        let vanes_at = |r4: f64| {
            Self::vanes(
                outlet,
                input,
                flow,
                inlet_diameter,
                inlet_width,
                r4,
                outlet_width,
            )
        };
        // Vaned: d₄/d₂ within (1.05 to 1.15) + 0.01·nq (Gülich p. 426), the value whose
        // channel comes closest to L₃₋₄/a₃ = 4.5, inside criterion C (2.5 to 6).
        let (outlet_diameter, vanes) = match (main.outlet_diameter, vaned) {
            (Param::Manual(d4), false) => (d4, None),
            (Param::Auto, false) => (1.5 * d2, None),
            (Param::Manual(d4), true) => {
                require(d4 > inlet_diameter * 1.01, || {
                    "the outlet diameter d₄ must exceed the inlet diameter d₃".into()
                })?;
                (d4, Some(vanes_at(d4 / 2.0)?))
            }
            (Param::Auto, true) => {
                let (mut low, mut high) = (
                    ((1.05 + 0.01 * outlet.nq) * d2).max(inlet_diameter * 1.05),
                    ((1.15 + 0.01 * outlet.nq) * d2).max(inlet_diameter * 1.1),
                );
                let ratio = |d4: f64| -> Result<f64, DesignError> {
                    let v = vanes_at(d4 / 2.0)?;
                    Ok(v.checks.length_ratio)
                };
                if ratio(low)? >= TARGET_LENGTH_RATIO {
                    high = low;
                } else if ratio(high)? <= TARGET_LENGTH_RATIO {
                    low = high;
                } else {
                    for _ in 0..30 {
                        let mid = 0.5 * (low + high);
                        if ratio(mid)? < TARGET_LENGTH_RATIO {
                            low = mid;
                        } else {
                            high = mid;
                        }
                    }
                }
                let d4 = 0.5 * (low + high);
                (d4, Some(vanes_at(d4 / 2.0)?))
            }
        };
        require(outlet_diameter > inlet_diameter * 1.01, || {
            "the outlet diameter d₄ must exceed the inlet diameter d₃".into()
        })?;
        let (r3, r4) = (inlet_diameter / 2.0, outlet_diameter / 2.0);

        // Free vortex without friction: c_u·r = c₂u·r₂ (eq. T7.7.3).
        let state = |radius: f64, width: f64| {
            let cu = outlet.cu * r2 / radius;
            let cm = flow / (2.0 * PI * radius * width);
            FlowState {
                radius,
                width,
                cu,
                cm,
                angle: cm.atan2(cu),
            }
        };
        let inlet = state(r3, inlet_width);
        let mut outlet_state = state(r4, outlet_width);
        let mut warnings = Vec::new();

        // Path of a particle in a vaneless ring: dθ = dr/(r·tan α), ds = dr/sin α.
        let steps = 400;
        let (mut wrap_angle, mut path_length) = (0.0, 0.0);
        for k in 0..steps {
            let t = (k as f64 + 0.5) / steps as f64;
            let r = r3 + (r4 - r3) * t;
            let s = state(r, inlet_width + (outlet_width - inlet_width) * t);
            let dr = (r4 - r3) / steps as f64;
            wrap_angle += dr / (r * s.angle.tan());
            path_length += dr / s.angle.sin();
        }

        if let Some(v) = &vanes {
            // The vanes turn the flow: it leaves along the vane outlet angle.
            let cm = outlet_state.cm * vane_blockage(v, r4);
            outlet_state = FlowState {
                cm,
                cu: cm / v.outlet_angle.tan(),
                angle: v.outlet_angle,
                ..outlet_state
            };
        }
        let c3 = inlet.velocity();
        let c4 = outlet_state.velocity();

        if vaned && inlet_diameter < min_inlet_diameter * (1.0 - 1e-9) {
            warnings.push(format!(
                "d₃/d₂ = {:.3} is below the {:.3} of Gülich Table 10.2: expect pressure \
                 pulsations",
                inlet_diameter / d2,
                min_inlet_diameter / d2
            ));
        }
        if let Some(v) = &vanes {
            check_vanes(v, outlet, &mut warnings);
        }

        Ok(Self {
            flow,
            inlet_diameter,
            min_inlet_diameter,
            inlet_width,
            outlet_diameter,
            outlet_width,
            inlet,
            outlet: outlet_state,
            ideal_pressure_recovery: 1.0 - (c4 / c3).powi(2),
            wrap_angle,
            path_length,
            vanes,
            midplane: outlet.midplane,
            rotation_sign: outlet.rotation_sign,
            warnings,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn vanes(
        outlet: &ImpellerOutlet,
        input: &DiffuserInput,
        flow: f64,
        d3: f64,
        b3: f64,
        r4: f64,
        b4: f64,
    ) -> Result<VaneDesign, DesignError> {
        let d2 = outlet.impeller_diameter;
        let r2 = d2 / 2.0;
        let r3 = d3 / 2.0;
        let props = &input.properties;
        let count = props
            .vane_count
            .resolve(|| default_vane_count(outlet.nq, outlet.impeller_blades));
        require((3..=40).contains(&count), || {
            format!("the vane number must lie between 3 and 40, got {count}")
        })?;
        let throat_factor = props.throat_factor.resolve(|| 1.15);
        require((0.5..=2.0).contains(&throat_factor), || {
            format!("the throat factor f_a3 must lie between 0.5 and 2, got {throat_factor}")
        })?;
        let e3 = positive(
            "the leading-edge thickness",
            props.leading_thickness.resolve(|| 0.0125 * d2),
        )?;
        let e4 = positive(
            "the trailing-edge thickness",
            input.profiles.trailing_thickness.resolve(|| 1.5 * e3),
        )?;
        let incidence = input.main.incidence.resolve(|| 0.0);

        // Inlet flow angle with the vane blockage, which depends on the vane angle
        // itself (eqs. T7.7.3 to T7.7.6), by fixed-point iteration.
        let c3u = d2 * outlet.cu / d3;
        let cm = flow / (PI * d3 * b3);
        let mut inlet_angle = cm.atan2(c3u) + incidence;
        let mut inlet_flow_angle = cm.atan2(c3u);
        for _ in 0..50 {
            let tau = blockage_factor(count, e3, d3, inlet_angle, std::f64::consts::FRAC_PI_2);
            if !tau.is_finite() {
                return Err(DesignError::Infeasible(
                    "the diffuser vanes fill the whole inlet circumference".into(),
                ));
            }
            inlet_flow_angle = (tau * cm).atan2(c3u);
            let next = inlet_flow_angle + incidence;
            let done = (next - inlet_angle).abs() < 1e-12;
            inlet_angle = next;
            if done {
                break;
            }
        }

        // Eq. T7.7.8a and T7.7.10: a₃ = f_a3·(d₃/2)·(X_Le − 1).
        let x_le = (flow / (b3 * outlet.cu * r2 * f64::from(count))).exp();
        let throat_target = throat_factor * r3 * (x_le - 1.0);

        let points = input.mean_lines.points.resolve(|| 41).max(11) as usize;
        let shape = VaneShape {
            count,
            inlet_radius: r3,
            outlet_radius: r4,
            inlet_angle,
            outlet_angle: 0.0,
            angle_exponent: 1.0,
            leading_thickness: e3,
            trailing_thickness: e4,
            leading_edge: input.edges.leading,
            trailing_edge: input.edges.trailing,
            points,
        };
        let pitch4 = 2.0 * PI * r4 / f64::from(count);
        // For a given outlet angle, the exponent of the angle law that gives the throat
        // of eq. T7.7.10: a larger exponent keeps the vane longer near α₃B and narrows
        // the throat.
        let build = |outlet_angle: f64| -> Result<VaneGeometry, DesignError> {
            let at = |angle_exponent: f64| {
                VaneGeometry::build(&VaneShape {
                    outlet_angle,
                    angle_exponent,
                    ..shape
                })
            };
            let (mut low, mut high) = (MIN_EXPONENT, MAX_EXPONENT);
            if at(low)?.throat <= throat_target {
                return at(low);
            }
            if at(high)?.throat >= throat_target {
                return at(high);
            }
            for _ in 0..40 {
                let mid = 0.5 * (low + high);
                if at(mid)?.throat > throat_target {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            at(0.5 * (low + high))
        };
        let outlet_width_at = |angle: f64| pitch4 * angle.sin() - e4;

        let (outlet_angle, geometry) = match input.mean_lines.outlet_angle {
            Param::Manual(angle) => {
                require(angle > 0.0 && angle < std::f64::consts::FRAC_PI_2, || {
                    "the vane outlet angle must lie between 0° and 90°".into()
                })?;
                (angle, build(angle)?)
            }
            Param::Auto => {
                // Outlet angle for the optimum area ratio of the channel (eq. 7.30),
                // kept within the allowable opening angle (eqs. 1.44, 1.45).
                let mut angle = inlet_angle + 10f64.to_radians();
                let mut g = build(angle)?;
                for _ in 0..12 {
                    let (a3, length) = (g.throat, g.channel_length);
                    let r_eq = (a3 * b3 / PI).sqrt();
                    let optimum = 1.05 + 0.184 * length / r_eq;
                    let allowed = 16.5f64.to_radians() * (r_eq / length).sqrt();
                    let limit = (1.0 + length * (0.9 * allowed).tan() / r_eq).powi(2);
                    let ratio = optimum.min(limit).max(1.0);
                    let a4 = ratio * a3 * b3 / b4;
                    let sine = ((a4 + e4) / pitch4).clamp(0.0, 0.95);
                    let next = sine.asin().clamp(inlet_angle, 45f64.to_radians());
                    let done = (next - angle).abs() < 1e-6;
                    angle = next;
                    g = build(angle)?;
                    if done {
                        break;
                    }
                }
                (angle, g)
            }
        };
        let throat = geometry.throat;
        require(throat > 0.0, || "the diffuser vanes overlap".into())?;
        let outlet_throat = outlet_width_at(outlet_angle);
        require(outlet_throat > 0.0, || {
            "the vanes close the channel at the outlet; reduce the vane number or the \
             thickness, or open the outlet angle"
                .into()
        })?;
        let length = geometry.channel_length;
        let r_eq = (throat * b3 / PI).sqrt();
        let area_ratio = outlet_throat * b4 / (throat * b3);
        let c2 = outlet.cu.hypot(outlet.cm);
        let c3q = flow / (f64::from(count) * throat * b3);
        let c4 = flow / (f64::from(count) * outlet_throat * b4);
        let checks = DiffuserChecks {
            area_ratio,
            optimum_area_ratio: 1.05 + 0.184 * length / r_eq,
            optimum_pressure_recovery: 0.36 * (length / r_eq).powf(0.26),
            deceleration: c3q / c2,
            length_ratio: length / throat,
            aspect_ratio: b3 / throat,
            outlet_energy: c4 * c4 / (2.0 * tp_units::STANDARD_GRAVITY * outlet.stage_head),
            opening_angle: (r_eq / length * (area_ratio.sqrt() - 1.0)).atan(),
            allowable_angle: 16.5f64.to_radians() * (r_eq / length).sqrt(),
        };
        Ok(VaneDesign {
            count,
            inlet_flow_angle,
            inlet_angle,
            outlet_angle,
            leading_thickness: e3,
            trailing_thickness: e4,
            throat_target,
            throat,
            outlet_throat,
            channel_length: length,
            checks,
            geometry,
        })
    }
}

/// Channel length over throat width the automatic outlet diameter aims at.
const TARGET_LENGTH_RATIO: f64 = 4.5;

/// Range of the exponent of the vane angle law α(t) = α₃B + (α₄B − α₃B)·t^p.
const MIN_EXPONENT: f64 = 0.5;
const MAX_EXPONENT: f64 = 6.0;

/// Blockage of the vanes at radius `r`.
fn vane_blockage(v: &VaneDesign, r: f64) -> f64 {
    let tau = blockage_factor(
        v.count,
        v.trailing_thickness,
        2.0 * r,
        v.outlet_angle,
        std::f64::consts::FRAC_PI_2,
    );
    if tau.is_finite() { tau } else { 1.0 }
}

fn check_vanes(v: &VaneDesign, outlet: &ImpellerOutlet, warnings: &mut Vec<String>) {
    let c = &v.checks;
    if let Some((n3, n2)) = resonant_harmonics(v.count, outlet.impeller_blades) {
        warnings.push(format!(
            "{} vanes with {} impeller blades give m = {n3}·{} − {n2}·{} = 0: avoid this \
             combination (Gülich Table 10.2)",
            v.count, outlet.impeller_blades, v.count, outlet.impeller_blades
        ));
    }
    let deviation = v.throat / v.throat_target - 1.0;
    if deviation.abs() > 0.15 {
        warnings.push(format!(
            "the vanes give a throat of {:.1} mm, {:+.0} % from the {:.1} mm of eq. T7.7.10; \
             adjust the vane number, the thickness or the outlet angle",
            v.throat * 1e3,
            deviation * 100.0,
            v.throat_target * 1e3
        ));
    }
    // Below nq = 12 to 15 the deceleration ratio may be higher (Gülich criterion B).
    let upper = if outlet.nq < 15.0 { 1.0 } else { 0.85 };
    if !(0.7..=upper).contains(&c.deceleration) {
        warnings.push(format!(
            "c₃q/c₂ = {:.2} is outside 0.7 to {upper} (Gülich criterion B)",
            c.deceleration
        ));
    }
    if !(2.5..=6.0).contains(&c.length_ratio) {
        warnings.push(format!(
            "channel length L₃₋₄/a₃ = {:.1} is outside 2.5 to 6 (Gülich criterion C)",
            c.length_ratio
        ));
    }
    if !(0.8..=2.0).contains(&c.aspect_ratio) {
        warnings.push(format!(
            "b₃/a₃ = {:.2} is outside 0.8 to 2 (Gülich criterion D)",
            c.aspect_ratio
        ));
    }
    if c.opening_angle > c.allowable_angle {
        warnings.push(format!(
            "the channel opens at {:.1}°, more than the {:.1}° allowed for its length \
             (Gülich criterion F, eq. 1.44)",
            c.opening_angle.to_degrees(),
            c.allowable_angle.to_degrees()
        ));
    }
}

/// Correlations, limits and defaults of the diffuser design.
pub const DIFFUSER_CORRELATIONS: &[CorrelationInfo] = &[
    CorrelationInfo {
        id: "diffuser.inlet_diameter",
        description: "d₃/d₂ ≥ 1.015 + 0.08·(ρH/(ρ_Ref·H_Ref) − 0.1)^0.8 below nq = 40, \
                      1.04 + 0.001·(nq − 40) above",
        reference: "Gülich 2nd ed., Table 10.2",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "diffuser.vane_number",
        description: "8 vanes below nq = 20, 12 above; no m = ν₃·z_Le − ν₂·z_La = 0 for ν < 4",
        reference: "Gülich 2nd ed., p. 425 and Table 10.2",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "diffuser.throat",
        description: "a₃ = f_a3·(d₃/2)·(exp(Q_Le/(b₃·c₂u·r₂·z_Le)) − 1), f_a3 = 1.15",
        reference: "Gülich 2nd ed., eqs. T7.7.8a and T7.7.10 (f_a3 = 1.1 to 1.3)",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "diffuser.dimensions",
        description: "b₃/b₂ = 1.15, d₄/d₂ = 1.10 + 0.01·nq, e₃ = 0.0125·d₂",
        reference: "middle of the ranges of Gülich pp. 425–428",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "diffuser.optimum_channel",
        description: "A_R,opt = 1.05 + 0.184·L₃₋₄/R_eq, c_p,opt = 0.36·(L₃₋₄/R_eq)^0.26",
        reference: "Gülich 2nd ed., eqs. 7.30 and 7.31",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "diffuser.vaneless",
        description: "free vortex c_u·r = c₂u·r₂; d₄/d₂ = 1.5 and b₃/b₂ = 1.05 by default",
        reference: "conservation of angular momentum; the defaults are our choice",
        verification: Verification::EngineeringDefault,
    },
];

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use approx::assert_relative_eq;

    pub(crate) fn reference_outlet() -> ImpellerOutlet {
        ImpellerOutlet {
            flow: 100.0 / 3600.0,
            impeller_diameter: 0.172,
            impeller_width: 0.0174,
            cu: 13.27,
            cm: 3.03,
            nq: 35.9,
            stage_head: 32.0,
            density: 998.2,
            midplane: 0.0378,
            rotation_sign: 1.0,
            impeller_blades: 7,
        }
    }

    #[test]
    fn table_10_2_gap_and_vane_rule() {
        assert_relative_eq!(min_inlet_ratio(35.9, 998.2, 32.0), 1.015);
        // 500 m of water: 1.015 + 0.08·(0.499 − 0.1)^0.8.
        let x: f64 = 998.2 * 500.0 / 1e6 - 0.1;
        assert_relative_eq!(
            min_inlet_ratio(30.0, 998.2, 500.0),
            1.015 + 0.08 * x.powf(0.8)
        );
        assert_relative_eq!(min_inlet_ratio(60.0, 998.2, 32.0), 1.06);
        assert_eq!(resonant_harmonics(14, 7), Some((1, 2)));
        assert_eq!(resonant_harmonics(12, 7), None);
        assert_eq!(default_vane_count(35.0, 7), 12);
        assert_eq!(default_vane_count(15.0, 5), 8);
        // 12 vanes with 6 blades resonate (1·12 = 2·6): the nearest free number is 13.
        assert_eq!(default_vane_count(35.0, 6), 13);
    }

    #[test]
    fn vaneless_ring_keeps_angular_momentum() {
        let o = reference_outlet();
        let d = DiffuserDesign::new(&o, &DiffuserInput::default(), false).unwrap();
        assert!(d.vanes.is_none());
        assert_relative_eq!(
            d.inlet.cu * d.inlet.radius,
            o.cu * 0.086,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            d.outlet.cu * d.outlet.radius,
            o.cu * 0.086,
            max_relative = 1e-12
        );
        // Parallel walls: the flow angle stays constant and the path is a log spiral.
        assert_relative_eq!(d.inlet.angle, d.outlet.angle, max_relative = 1e-12);
        let alpha = d.inlet.angle;
        let (r3, r4) = (d.inlet.radius, d.outlet.radius);
        assert_relative_eq!(
            d.wrap_angle,
            (r4 / r3).ln() / alpha.tan(),
            max_relative = 1e-4
        );
        assert_relative_eq!(d.path_length, (r4 - r3) / alpha.sin(), max_relative = 1e-4);
        assert_relative_eq!(
            d.ideal_pressure_recovery,
            1.0 - (r3 / r4).powi(2),
            max_relative = 1e-9
        );
    }

    #[test]
    fn vaned_diffuser_follows_table_7_7() {
        let o = reference_outlet();
        let d = DiffuserDesign::new(&o, &DiffuserInput::default(), true).unwrap();
        let v = d.vanes.as_ref().unwrap();
        assert_eq!(v.count, 12);
        assert_relative_eq!(d.inlet_diameter, 1.015 * 0.172, max_relative = 1e-12);
        assert_relative_eq!(d.inlet_width, 1.15 * 0.0174, max_relative = 1e-12);
        let x = (d.flow / (d.inlet_width * o.cu * 0.086 * 12.0)).exp();
        assert_relative_eq!(
            v.throat_target,
            1.15 * d.inlet_diameter / 2.0 * (x - 1.0),
            max_relative = 1e-12
        );
        // Flow angle with blockage: tan α₃' = τ₃·c₃m/c₃u.
        let tau = blockage_factor(
            12,
            v.leading_thickness,
            d.inlet_diameter,
            v.inlet_angle,
            PI / 2.0,
        );
        let c3u = 0.172 * o.cu / d.inlet_diameter;
        let c3m = d.flow / (PI * d.inlet_diameter * d.inlet_width);
        assert_relative_eq!(v.inlet_flow_angle, (tau * c3m / c3u).atan(), epsilon = 1e-9);
        // The vanes are shaped to give that throat.
        assert_relative_eq!(v.throat, v.throat_target, max_relative = 1e-6);
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
        // The automatic outlet angle opens the channel toward the optimum area ratio
        // without exceeding the allowable angle.
        let c = &v.checks;
        assert!(v.outlet_angle > v.inlet_angle);
        assert!(c.area_ratio > 1.2, "{c:?}");
        assert!(c.opening_angle <= c.allowable_angle * 0.91, "{c:?}");
    }

    #[test]
    fn manual_inputs_and_errors() {
        let o = reference_outlet();
        let mut input = DiffuserInput::default();
        input.properties.vane_count = Param::Manual(14);
        let d = DiffuserDesign::new(&o, &input, true).unwrap();
        assert!(
            d.warnings.iter().any(|w| w.contains("m = 1·14 − 2·7")),
            "{:?}",
            d.warnings
        );

        let mut input = DiffuserInput::default();
        input.main.inlet_diameter = Param::Manual(0.15);
        assert!(DiffuserDesign::new(&o, &input, true).is_err());
        let mut input = DiffuserInput::default();
        input.main.outlet_diameter = Param::Manual(0.17);
        assert!(DiffuserDesign::new(&o, &input, false).is_err());
    }
}
