//! Volute casings of radial pumps: cutwater, throat area, spiral development of the
//! cross sections and discharge diffuser.
//!
//! Procedure (see `docs/theory/voluta-y-difusores.md`), following Gülich 2nd ed.,
//! Chap. 7.8 and Table 7.7:
//! 1. design flow Q_Le, cutwater diameter d_z (Table 10.2), inlet width b₃ and cutwater
//!    thickness e₃;
//! 2. cross sections from the conservation of angular momentum (eqs. 7.24 and 7.25) or
//!    for constant velocity with the throat area from angular momentum (eqs. 7.26 and
//!    7.27), corrected for the cutwater thickness (eq. 7.28);
//! 3. flow angle at the cutwater (eqs. T7.7.3 to T7.7.6);
//! 4. discharge diffuser checked against the allowable opening angle (eqs. 1.44, 1.45).

mod geometry;

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use tp_math::roots;

use crate::outlet::ImpellerOutlet;
use crate::pump::correlations::{CorrelationInfo, Verification};
use crate::{DesignError, Param};

pub use geometry::{NozzleSection, VoluteGeometry, VoluteSection};

/// Shape of the volute cross sections (Gülich Fig. 7.40).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SectionShape {
    /// Constant width b₃.
    Rectangular,
    /// Side walls opening at `flank_angle` (rad) from the radial plane, from width b₃
    /// at the base circle.
    Trapezoidal { flank_angle: f64 },
    /// Circles touching the base circle.
    Circular,
}

impl Default for SectionShape {
    fn default() -> Self {
        SectionShape::Trapezoidal {
            flank_angle: DEFAULT_FLANK_ANGLE_DEG.to_radians(),
        }
    }
}

/// How the cross sections grow with the wrap angle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaLaw {
    /// Constant velocity below nq = 30, angular momentum above (Gülich p. 418).
    #[default]
    Auto,
    /// c_u·r = c₂u·r₂ in every section (eqs. 7.24 and 7.25).
    AngularMomentum,
    /// Areas proportional to the wrap angle, with the throat from angular momentum
    /// (eqs. 7.26 and 7.27).
    ConstantVelocity,
}

/// Inputs of the volute-setup step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoluteSetupInput {
    /// Q_Le/Q_opt: allowance for leakage that flows through the volute (eq. T7.7.1).
    pub flow_factor: Param<f64>,
    /// Cutwater (base circle) diameter d_z, m.
    pub cutwater_diameter: Param<f64>,
    /// Width of the volute inlet b₃, m.
    pub inlet_width: Param<f64>,
    /// Thickness of the cutwater nose e₃, m.
    pub cutwater_thickness: Param<f64>,
}

/// Inputs of the cross-section step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossSectionInput {
    pub shape: SectionShape,
}

/// Inputs of the spiral-development step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpiralInput {
    pub law: AreaLaw,
    /// Number of cross sections from the start of the spiral to the throat.
    pub sections: Param<u32>,
    /// Enlarge the throat for the blockage of the cutwater (eq. 7.28).
    pub cutwater_correction: bool,
}

impl Default for SpiralInput {
    fn default() -> Self {
        Self {
            law: AreaLaw::Auto,
            sections: Param::Auto,
            cutwater_correction: true,
        }
    }
}

/// Inputs of the discharge-diffuser step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DischargeInput {
    /// Length of the diffuser along its axis, from the throat, m.
    pub length: Param<f64>,
    /// Outlet (discharge nozzle) diameter, m.
    pub outlet_diameter: Param<f64>,
}

/// Inputs of the cutwater step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CutwaterInput {
    /// Incidence i₃ between the cutwater camber line and the approaching flow, rad
    /// (eq. T7.7.6; ±3°).
    pub incidence: Param<f64>,
}

/// Inputs of the wall-thickness step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WallInput {
    /// Casing wall thickness, m.
    pub thickness: Param<f64>,
}

/// Inputs of the volute design steps, one group per step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoluteInput {
    pub setup: VoluteSetupInput,
    pub cross_section: CrossSectionInput,
    pub spiral: SpiralInput,
    pub diffuser: DischargeInput,
    pub cutwater: CutwaterInput,
    pub wall: WallInput,
}

/// Size of one cross section.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionSize {
    /// Radial height a = r_a − r'_z of a rectangular or trapezoidal section, or the
    /// diameter of a circular one, m.
    pub height: f64,
    /// Outermost radius r_a, m.
    pub outer_radius: f64,
    /// m²
    pub area: f64,
    /// Radius of the area centroid, m.
    pub centroid_radius: f64,
}

/// Straight conical diffuser from the volute throat to the discharge nozzle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DischargeDiffuser {
    /// m
    pub length: f64,
    /// Throat area A₃q, m².
    pub inlet_area: f64,
    /// m
    pub outlet_diameter: f64,
    /// A₂/A₁
    pub area_ratio: f64,
    /// Equivalent half opening angle ϑ_eq, rad (eq. 1.45).
    pub opening_angle: f64,
    /// Allowable half opening angle ϑ_al, rad (eq. 1.44).
    pub allowable_angle: f64,
    /// m/s
    pub outlet_velocity: f64,
    /// c_p,id = 1 − 1/A_R² (eq. 1.40).
    pub ideal_pressure_recovery: f64,
}

/// Result of the volute design, in SI units.
#[derive(Debug, Clone, PartialEq)]
pub struct VoluteDesign {
    /// Design flow Q_Le, m³/s.
    pub flow: f64,
    pub cutwater_diameter: f64,
    /// Smallest cutwater diameter of Gülich Table 10.2, m.
    pub min_cutwater_diameter: f64,
    pub inlet_width: f64,
    pub cutwater_thickness: f64,
    /// Stagnation radius r'_z = r_z + e₃/2 where the sections start, m.
    pub stagnation_radius: f64,
    /// The law actually used (never `Auto`).
    pub law: AreaLaw,
    pub shape: SectionShape,
    /// Throat section, including the cutwater correction.
    pub throat: SectionSize,
    /// Throat height added for the cutwater blockage Δa₃, m.
    pub throat_correction: f64,
    /// Mean throat velocity c₃q = Q_Le/A₃q, m/s.
    pub throat_velocity: f64,
    /// c₃q/c₂; Gülich Table 10.2 advises not much below 0.8 for low pressure
    /// pulsations, which only flat sections approach under angular momentum.
    pub deceleration: f64,
    /// Width over height B/H of the throat; flat sections with B/H = 2 to 3 are
    /// considered optimal (Gülich p. 414).
    pub throat_aspect: f64,
    /// Flow angle at the cutwater α₃', rad.
    pub cutwater_flow_angle: f64,
    /// Camber angle of the cutwater α₃B, rad.
    pub cutwater_angle: f64,
    pub diffuser: DischargeDiffuser,
    pub wall_thickness: f64,
    pub warnings: Vec<String>,
    /// Constant K of the angular momentum law: ∫b/r dr = K·ε.
    momentum_constant: f64,
}

/// Default flank angle of trapezoidal sections, degrees.
pub const DEFAULT_FLANK_ANGLE_DEG: f64 = 30.0;
/// Specific speed separating the constant-velocity and angular-momentum laws.
pub const AREA_LAW_NQ: f64 = 30.0;
/// Above this specific speed the discharge nozzle should not decelerate further.
pub const NO_DIFFUSION_NQ: f64 = 80.0;
/// Fraction of the allowable opening angle used by the automatic diffuser.
const DIFFUSER_ANGLE_USE: f64 = 0.85;
/// Automatic diffuser length in throat-equivalent radii.
const DIFFUSER_LENGTH_RADII: f64 = 4.0;

/// Smallest cutwater diameter d_z/d₂ for low pressure pulsations:
/// 1.03 + 0.1·nq/nq,Ref + 0.07·ρ·H/(ρ_Ref·H_Ref) with nq,Ref = 40, ρ_Ref = 1000 kg/m³
/// and H_Ref = 1000 m (Gülich 2nd ed., Table 10.2).
pub fn min_cutwater_ratio(nq: f64, density: f64, stage_head: f64) -> f64 {
    1.03 + 0.1 * nq / 40.0 + 0.07 * density * stage_head / (1000.0 * 1000.0)
}

/// Automatic volute inlet width b₃/b₂: 2.0 at nq ≤ 15 falling linearly to 1.2 at
/// nq ≥ 55, inside the ranges Gülich gives for low (2 to 4) and high (1.05 to 1.2)
/// specific speeds (p. 415).
pub fn inlet_width_ratio(nq: f64) -> f64 {
    (2.0 - 0.02 * (nq - 15.0)).clamp(1.2, 2.0)
}

/// Shape-dependent geometry of one cross section starting at radius `rz` with inlet
/// width `b3`.
#[derive(Debug, Clone, Copy)]
struct Shape {
    kind: SectionShape,
    b3: f64,
    rz: f64,
}

impl Shape {
    fn slope(&self) -> f64 {
        match self.kind {
            SectionShape::Rectangular | SectionShape::Circular => 0.0,
            SectionShape::Trapezoidal { flank_angle } => flank_angle.tan(),
        }
    }

    /// ∫ b/r dr over the section of height (or diameter) `a`.
    fn momentum_integral(&self, a: f64) -> f64 {
        let rz = self.rz;
        match self.kind {
            SectionShape::Circular => {
                // A circle of radius ρ centred at R: 2π·(R − √(R² − ρ²)).
                let rho = a / 2.0;
                let centre = rz + rho;
                TAU * (centre - (centre * centre - rho * rho).sqrt())
            }
            _ => {
                let t = self.slope();
                (self.b3 - 2.0 * rz * t) * ((rz + a) / rz).ln() + 2.0 * t * a
            }
        }
    }

    fn area(&self, a: f64) -> f64 {
        match self.kind {
            SectionShape::Circular => PI * a * a / 4.0,
            _ => self.b3 * a + self.slope() * a * a,
        }
    }

    fn centroid_radius(&self, a: f64) -> f64 {
        let rz = self.rz;
        match self.kind {
            SectionShape::Circular => rz + a / 2.0,
            _ => {
                let t = self.slope();
                // ∫ r·b dr with b = b₃ + 2t·s over s = r − r'_z ∈ [0, a].
                let moment = rz * (self.b3 * a + t * a * a)
                    + self.b3 * a * a / 2.0
                    + 2.0 * t * a * a * a / 3.0;
                moment / self.area(a)
            }
        }
    }

    fn size(&self, a: f64) -> SectionSize {
        SectionSize {
            height: a,
            outer_radius: self.rz + a,
            area: self.area(a),
            centroid_radius: self.centroid_radius(a),
        }
    }

    /// Height whose momentum integral is `k` (eq. 7.24).
    fn height_for_integral(&self, k: f64) -> Result<f64, DesignError> {
        if k <= 0.0 {
            return Ok(0.0);
        }
        match self.kind {
            SectionShape::Circular => {
                // Gülich eq. T7.7.7 with X = K/π: d = X + √(2·d'_z·X).
                let x = k / PI;
                Ok(x + (2.0 * (2.0 * self.rz) * x).sqrt())
            }
            SectionShape::Rectangular => Ok(self.rz * ((k / self.b3).exp() - 1.0)),
            SectionShape::Trapezoidal { .. } => {
                let mut hi = self.rz.max(self.b3);
                while self.momentum_integral(hi) < k {
                    hi *= 2.0;
                    if hi > 1e3 * self.rz {
                        return Err(DesignError::Infeasible(
                            "the volute cross section does not converge".into(),
                        ));
                    }
                }
                roots::brent(|a| self.momentum_integral(a) - k, 0.0, hi, 1e-12 * hi)
                    .map_err(|e| DesignError::Infeasible(e.to_string()))
            }
        }
    }

    /// Height of the section with area `area`.
    fn height_for_area(&self, area: f64) -> f64 {
        if area <= 0.0 {
            return 0.0;
        }
        match self.kind {
            SectionShape::Circular => (4.0 * area / PI).sqrt(),
            _ => {
                let t = self.slope();
                if t < 1e-12 {
                    area / self.b3
                } else {
                    (-self.b3 + (self.b3 * self.b3 + 4.0 * t * area).sqrt()) / (2.0 * t)
                }
            }
        }
    }
}

fn require(ok: bool, message: impl FnOnce() -> String) -> Result<(), DesignError> {
    if ok {
        Ok(())
    } else {
        Err(DesignError::InvalidInput(message()))
    }
}

impl VoluteDesign {
    /// Designs the volute for `basis` with the given inputs.
    pub fn new(basis: &ImpellerOutlet, input: &VoluteInput) -> Result<Self, DesignError> {
        let d2 = basis.impeller_diameter;
        let r2 = basis.impeller_diameter / 2.0;
        require(basis.flow > 0.0 && basis.cu > 0.0 && d2 > 0.0, || {
            "the volute needs a positive flow, c₂u and d₂ from the impeller".into()
        })?;

        let setup = &input.setup;
        let flow_factor = setup.flow_factor.resolve(|| 1.0);
        require((0.8..=1.5).contains(&flow_factor), || {
            format!("Q_Le/Q must lie between 0.8 and 1.5, got {flow_factor}")
        })?;
        let flow = flow_factor * basis.flow;
        let min_ratio = min_cutwater_ratio(basis.nq, basis.density, basis.stage_head);
        let min_cutwater_diameter = min_ratio * d2;
        let cutwater_diameter = setup.cutwater_diameter.resolve(|| min_cutwater_diameter);
        require(cutwater_diameter > d2, || {
            format!(
                "the cutwater diameter {:.1} mm must exceed d₂ = {:.1} mm",
                cutwater_diameter * 1e3,
                d2 * 1e3
            )
        })?;
        let inlet_width = setup
            .inlet_width
            .resolve(|| inlet_width_ratio(basis.nq) * basis.impeller_width);
        require(inlet_width >= basis.impeller_width, || {
            format!(
                "the volute inlet width b₃ = {:.1} mm cannot be smaller than b₂ = {:.1} mm",
                inlet_width * 1e3,
                basis.impeller_width * 1e3
            )
        })?;
        let cutwater_thickness = setup.cutwater_thickness.resolve(|| 0.02 * d2);
        require(cutwater_thickness > 0.0, || {
            "the cutwater thickness must be positive".into()
        })?;
        let shape = input.cross_section.shape;
        if let SectionShape::Trapezoidal { flank_angle } = shape {
            require((0.0..=60f64.to_radians()).contains(&flank_angle), || {
                format!(
                    "the flank angle must lie between 0° and 60°, got {:.1}°",
                    flank_angle.to_degrees()
                )
            })?;
        }

        let stagnation_radius = cutwater_diameter / 2.0 + cutwater_thickness / 2.0;
        let section = Shape {
            kind: shape,
            b3: inlet_width,
            rz: stagnation_radius,
        };
        // Eq. 7.24 for a partial volute of wrap angle ε: ∫ b/r dr = ε·Q_Le/(2π·c₂u·r₂).
        let momentum_constant = flow / (TAU * basis.cu * r2);
        let law = match input.spiral.law {
            AreaLaw::Auto if basis.nq < AREA_LAW_NQ => AreaLaw::ConstantVelocity,
            AreaLaw::Auto => AreaLaw::AngularMomentum,
            law => law,
        };

        // Throat from angular momentum for both laws, then the cutwater correction
        // Δa₃/a₃ = 0.2·(e₃/a₃)² (eq. 7.28).
        let a3 = section.height_for_integral(momentum_constant * TAU)?;
        let throat_correction = if input.spiral.cutwater_correction {
            0.2 * cutwater_thickness * cutwater_thickness / a3
        } else {
            0.0
        };
        let throat = section.size(a3 + throat_correction);
        let throat_velocity = flow / throat.area;
        let throat_aspect = match shape {
            SectionShape::Circular => 1.0,
            _ => (inlet_width + 2.0 * section.slope() * throat.height) / throat.height,
        };
        let c2 = basis.cu.hypot(basis.cm);
        let deceleration = throat_velocity / c2;

        // Flow angle at the cutwater, eqs. T7.7.3 to T7.7.5 (one cutwater: no blockage).
        let d3 = cutwater_diameter;
        let c3u = d2 * basis.cu / d3;
        let c3m = flow / (PI * d3 * inlet_width);
        let cutwater_flow_angle = c3m.atan2(c3u);
        let incidence = input.cutwater.incidence.resolve(|| 0.0);
        let cutwater_angle = cutwater_flow_angle + incidence;

        let diffuser = Self::discharge(basis, &input.diffuser, flow, throat.area)?;
        let wall_thickness = input.wall.thickness.resolve(|| (0.03 * d2).max(0.006));
        require(wall_thickness > 0.0, || {
            "the wall thickness must be positive".into()
        })?;

        let mut warnings = Vec::new();
        if cutwater_diameter < min_cutwater_diameter * (1.0 - 1e-9) {
            warnings.push(format!(
                "d_z/d₂ = {:.3} is below the {:.3} of Gülich Table 10.2: expect pressure \
                 pulsations and hydraulic excitation forces",
                cutwater_diameter / d2,
                min_ratio
            ));
        }
        if incidence.abs() > 3f64.to_radians() + 1e-12 {
            warnings.push(format!(
                "cutwater incidence {:.1}° is outside ±3° (Gülich eq. T7.7.6)",
                incidence.to_degrees()
            ));
        }
        if diffuser.opening_angle > diffuser.allowable_angle {
            warnings.push(format!(
                "the discharge diffuser opens at {:.1}°, more than the {:.1}° allowed for \
                 its length (Gülich eq. 1.44): the flow may separate",
                diffuser.opening_angle.to_degrees(),
                diffuser.allowable_angle.to_degrees()
            ));
        }
        if basis.nq > NO_DIFFUSION_NQ && diffuser.area_ratio > 1.05 {
            warnings.push(format!(
                "above nq ≈ {NO_DIFFUSION_NQ} the discharge nozzle should not decelerate \
                 further (area ratio {:.2})",
                diffuser.area_ratio
            ));
        }

        Ok(Self {
            flow,
            cutwater_diameter,
            min_cutwater_diameter,
            inlet_width,
            cutwater_thickness,
            stagnation_radius,
            law,
            shape,
            throat,
            throat_correction,
            throat_velocity,
            deceleration,
            throat_aspect,
            cutwater_flow_angle,
            cutwater_angle,
            diffuser,
            wall_thickness,
            warnings,
            momentum_constant,
        })
    }

    fn discharge(
        basis: &ImpellerOutlet,
        input: &DischargeInput,
        flow: f64,
        throat_area: f64,
    ) -> Result<DischargeDiffuser, DesignError> {
        let r1 = (throat_area / PI).sqrt();
        let length = input.length.resolve(|| DIFFUSER_LENGTH_RADII * r1);
        require(length > 0.0, || {
            "the diffuser length must be positive".into()
        })?;
        // Eq. 1.44: ϑ_al = 16.5°·√(R₁/L).
        let allowable_angle = (16.5 * (r1 / length).sqrt()).to_radians();
        let outlet_diameter = input.outlet_diameter.resolve(|| {
            if basis.nq > NO_DIFFUSION_NQ {
                2.0 * r1
            } else {
                2.0 * (r1 + length * (DIFFUSER_ANGLE_USE * allowable_angle).tan())
            }
        });
        require(outlet_diameter > 0.0, || {
            "the discharge diameter must be positive".into()
        })?;
        let outlet_area = PI * outlet_diameter * outlet_diameter / 4.0;
        let area_ratio = outlet_area / throat_area;
        // Eq. 1.45: tan ϑ_eq = (R₁,eq/L)·(√(A₂/A₁) − 1).
        let opening_angle = (r1 / length * (area_ratio.sqrt() - 1.0)).atan();
        Ok(DischargeDiffuser {
            length,
            inlet_area: throat_area,
            outlet_diameter,
            area_ratio,
            opening_angle,
            allowable_angle,
            outlet_velocity: flow / outlet_area,
            ideal_pressure_recovery: 1.0 - 1.0 / (area_ratio * area_ratio),
        })
    }

    fn shape(&self) -> Shape {
        Shape {
            kind: self.shape,
            b3: self.inlet_width,
            rz: self.stagnation_radius,
        }
    }

    /// The cross section at wrap angle `angle` (rad) from the cutwater; 2π is the throat.
    pub fn section(&self, angle: f64) -> Result<SectionSize, DesignError> {
        let shape = self.shape();
        let fraction = (angle / TAU).clamp(0.0, 1.0);
        let height = match self.law {
            AreaLaw::ConstantVelocity => {
                // Eq. 7.27 on the uncorrected throat, then the correction ramps up.
                let throat = shape.area(self.throat.height - self.throat_correction);
                shape.height_for_area(throat * fraction)
            }
            _ => shape.height_for_integral(self.momentum_constant * angle.max(0.0))?,
        };
        Ok(shape.size(height + self.throat_correction * fraction))
    }

    /// ∫ b/r dr of a section of the given height.
    pub fn momentum_integral(&self, height: f64) -> f64 {
        self.shape().momentum_integral(height)
    }

    /// Wrap angle at which a section of the given height satisfies the conservation of
    /// angular momentum (eq. 7.25).
    pub fn momentum_angle(&self, height: f64) -> f64 {
        self.momentum_integral(height) / self.momentum_constant
    }
}

/// Correlations, limits and defaults of the volute design.
pub const VOLUTE_CORRELATIONS: &[CorrelationInfo] = &[
    CorrelationInfo {
        id: "volute.cutwater_diameter",
        description: "d_z/d₂ ≥ 1.03 + 0.1·nq/40 + 0.07·ρH/(ρ_Ref·H_Ref)",
        reference: "Gülich 2nd ed., Table 10.2",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "volute.throat",
        description: "∫ b/r dr = ε·Q_Le/(2π·c₂u·r₂) from r'_z = r_z + e₃/2; analytic for \
                      rectangular and circular sections",
        reference: "Gülich 2nd ed., eqs. 7.24, 7.25, T7.7.7 to T7.7.9",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "volute.constant_velocity",
        description: "A(ε) = A₃q·ε/ε_sp below nq = 30",
        reference: "Gülich 2nd ed., eqs. 7.26, 7.27 and p. 418 (threshold 25 to 35)",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "volute.cutwater_correction",
        description: "Δa₃/a₃ = 0.2·(e₃/a₃)²",
        reference: "Gülich 2nd ed., eq. 7.28",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "volute.cutwater_thickness",
        description: "e₃ ≈ 0.02·d₂",
        reference: "Gülich 2nd ed., Chap. 7.8.1 item 7",
        verification: Verification::Verified,
    },
    CorrelationInfo {
        id: "volute.inlet_width",
        description: "b₃/b₂ from 2.0 (nq ≤ 15) to 1.2 (nq ≥ 55)",
        reference: "inside the ranges of Gülich p. 415; the interpolation is our choice",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "volute.discharge_diffuser",
        description: "ϑ_al = 16.5°·√(R₁/L), tan ϑ_eq = (R₁/L)·(√A_R − 1); automatic \
                      length 4·R₁ at 85 % of ϑ_al",
        reference: "Gülich 2nd ed., eqs. 1.44 and 1.45; length and margin are our choice",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "volute.deceleration",
        description: "c₃q/c₂ not much below 0.8",
        reference: "Gülich 2nd ed., Table 10.2",
        verification: Verification::Verified,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use tp_math::quad::gauss_legendre;

    /// Volute basis of the reference pump: 100 m³/h, 32 m, 2900 rpm, d₂ = 172 mm.
    pub(crate) fn reference_basis() -> ImpellerOutlet {
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

    fn design_with(shape: SectionShape, law: AreaLaw) -> VoluteDesign {
        let mut input = VoluteInput::default();
        input.cross_section.shape = shape;
        input.spiral.law = law;
        VoluteDesign::new(&reference_basis(), &input).unwrap()
    }

    /// ∫ b/r dr computed numerically from the section width b(r).
    fn numeric_integral(d: &VoluteDesign, height: f64) -> f64 {
        let rz = d.stagnation_radius;
        let width = |r: f64| match d.shape {
            SectionShape::Circular => {
                let rho = height / 2.0;
                let dr = r - (rz + rho);
                2.0 * (rho * rho - dr * dr).max(0.0).sqrt()
            }
            SectionShape::Rectangular => d.inlet_width,
            SectionShape::Trapezoidal { flank_angle } => {
                d.inlet_width + 2.0 * (r - rz) * flank_angle.tan()
            }
        };
        gauss_legendre(|r| width(r) / r, rz, rz + height, 400)
    }

    #[test]
    fn cutwater_diameter_follows_table_10_2() {
        let d = design_with(SectionShape::Rectangular, AreaLaw::AngularMomentum);
        let expected = 1.03 + 0.1 * 35.9 / 40.0 + 0.07 * 998.2 * 32.0 / 1e6;
        assert_relative_eq!(d.cutwater_diameter / 0.172, expected, epsilon = 1e-12);
        assert_relative_eq!(d.cutwater_thickness, 0.02 * 0.172);
        assert_relative_eq!(
            d.stagnation_radius,
            d.cutwater_diameter / 2.0 + d.cutwater_thickness / 2.0
        );
    }

    #[test]
    fn rectangular_throat_matches_eq_t7_7_9() {
        let mut input = VoluteInput::default();
        input.cross_section.shape = SectionShape::Rectangular;
        input.spiral.cutwater_correction = false;
        let b = reference_basis();
        let d = VoluteDesign::new(&b, &input).unwrap();
        // a₃ = r'_z·(exp(Q_Le·ε_sp/(b₃·c₂u·r₂·2π)) − 1) with ε_sp = 2π.
        let rz = d.stagnation_radius;
        let a3 = rz * ((d.flow / (d.inlet_width * b.cu * 0.086)).exp() - 1.0);
        assert_relative_eq!(d.throat.height, a3, max_relative = 1e-12);
    }

    #[test]
    fn circular_throat_matches_eq_t7_7_7() {
        let mut input = VoluteInput::default();
        input.cross_section.shape = SectionShape::Circular;
        input.spiral.cutwater_correction = false;
        let b = reference_basis();
        let d = VoluteDesign::new(&b, &input).unwrap();
        // X_sp = Q_Le/(π·c₂u·r₂)·(ε_sp/2π), d₃q = X_sp + √(2·d'_z·X_sp).
        let x = d.flow / (PI * b.cu * 0.086);
        let d3q = x + (2.0 * 2.0 * d.stagnation_radius * x).sqrt();
        assert_relative_eq!(d.throat.height, d3q, max_relative = 1e-12);
    }

    #[test]
    fn every_shape_conserves_angular_momentum() {
        for shape in [
            SectionShape::Rectangular,
            SectionShape::Circular,
            SectionShape::default(),
        ] {
            let d = design_with(shape, AreaLaw::AngularMomentum);
            for angle in [0.5, PI, 1.5 * PI] {
                let s = d.section(angle).unwrap();
                let height = s.height - d.throat_correction * angle / TAU;
                let k = d.flow / (TAU * 13.27 * 0.086) * angle;
                assert_relative_eq!(numeric_integral(&d, height), k, max_relative = 1e-6);
            }
        }
    }

    #[test]
    fn constant_velocity_areas_grow_linearly() {
        let d = design_with(SectionShape::default(), AreaLaw::ConstantVelocity);
        let mut input = VoluteInput::default();
        input.spiral.law = AreaLaw::AngularMomentum;
        let momentum = VoluteDesign::new(&reference_basis(), &input).unwrap();
        // Both laws share the throat.
        assert_relative_eq!(d.throat.area, momentum.throat.area, max_relative = 1e-12);
        let mut nocorr = VoluteInput::default();
        nocorr.spiral.law = AreaLaw::ConstantVelocity;
        nocorr.spiral.cutwater_correction = false;
        let d = VoluteDesign::new(&reference_basis(), &nocorr).unwrap();
        for f in [0.25, 0.5, 0.75] {
            let s = d.section(f * TAU).unwrap();
            assert_relative_eq!(s.area, f * d.throat.area, max_relative = 1e-9);
        }
    }

    #[test]
    fn law_is_chosen_by_specific_speed() {
        let input = VoluteInput::default();
        let mut b = reference_basis();
        assert_eq!(
            VoluteDesign::new(&b, &input).unwrap().law,
            AreaLaw::AngularMomentum
        );
        b.nq = 20.0;
        assert_eq!(
            VoluteDesign::new(&b, &input).unwrap().law,
            AreaLaw::ConstantVelocity
        );
    }

    #[test]
    fn cutwater_correction_follows_eq_7_28() {
        let mut input = VoluteInput::default();
        input.cross_section.shape = SectionShape::Rectangular;
        let corrected = VoluteDesign::new(&reference_basis(), &input).unwrap();
        input.spiral.cutwater_correction = false;
        let plain = VoluteDesign::new(&reference_basis(), &input).unwrap();
        let a3 = plain.throat.height;
        let e3 = plain.cutwater_thickness;
        assert_relative_eq!(
            corrected.throat.height - a3,
            0.2 * (e3 / a3).powi(2) * a3,
            max_relative = 1e-12
        );
    }

    #[test]
    fn automatic_diffuser_respects_the_allowable_angle() {
        let d = design_with(SectionShape::default(), AreaLaw::Auto);
        let dif = &d.diffuser;
        assert!(dif.opening_angle < dif.allowable_angle);
        assert!(
            dif.area_ratio > 1.5 && dif.area_ratio < 3.0,
            "{}",
            dif.area_ratio
        );
        assert!(d.warnings.iter().all(|w| !w.contains("diffuser")));

        let mut input = VoluteInput::default();
        input.diffuser.outlet_diameter = Param::Manual(0.2);
        let wide = VoluteDesign::new(&reference_basis(), &input).unwrap();
        assert!(wide.warnings.iter().any(|w| w.contains("diffuser")));
    }

    #[test]
    fn cutwater_angle_is_the_flow_angle_plus_incidence() {
        let mut input = VoluteInput::default();
        input.cutwater.incidence = Param::Manual(2f64.to_radians());
        let d = VoluteDesign::new(&reference_basis(), &input).unwrap();
        let b = reference_basis();
        let c3u = b.impeller_diameter * b.cu / d.cutwater_diameter;
        let c3m = d.flow / (PI * d.cutwater_diameter * d.inlet_width);
        assert_relative_eq!(d.cutwater_flow_angle, (c3m / c3u).atan(), epsilon = 1e-12);
        assert_relative_eq!(
            d.cutwater_angle - d.cutwater_flow_angle,
            2f64.to_radians(),
            epsilon = 1e-12
        );
    }

    #[test]
    fn rejects_impossible_inputs() {
        let b = reference_basis();
        let mut input = VoluteInput::default();
        input.setup.cutwater_diameter = Param::Manual(0.15);
        assert!(VoluteDesign::new(&b, &input).is_err());
        let mut input = VoluteInput::default();
        input.setup.inlet_width = Param::Manual(0.01);
        assert!(VoluteDesign::new(&b, &input).is_err());
        let mut input = VoluteInput::default();
        input.setup.cutwater_diameter = Param::Manual(0.18);
        let close = VoluteDesign::new(&b, &input).unwrap();
        assert!(close.warnings.iter().any(|w| w.contains("Table 10.2")));
    }
}
