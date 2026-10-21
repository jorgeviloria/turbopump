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
//!
//! A double volute (Gülich Chap. 7.8.1, Fig. 7.39) splits the spiral with a rib. The
//! first partial volute runs from the cutwater to the rib nose and goes on as the outer
//! channel around the second, inner volute, which wraps ε_sp from the rib nose to the
//! throat. Each partial volute is sized like a single one for the flow of its wrap
//! (item 8), and both meet the discharge diffuser. The rib runs at a constant radius
//! across each section, so the casing is one spiral split by it: the outer channel
//! takes the part of each section beyond the inner volute and the rib. Its nose and tail
//! are profiled like a cutwater (item 7): elliptic, as thick as the rib.
//!
//! [`loss`] estimates the hydraulic losses of a volute against flow.

mod geometry;
pub mod loss;
pub mod section;
pub mod stress;

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use tp_math::roots;

use crate::outlet::{ImpellerOutlet, Reach};
use crate::pump::correlations::{CorrelationInfo, Verification};
use crate::{DesignError, Param};

pub use geometry::{NozzleSection, VoluteGeometry, VoluteSection};
pub use section::{
    BendWall, BezierBase, BezierSide, CrossSectionInput, CrossSections, DEFAULT_CONE_ANGLE_DEG,
    ExternalRound, InnerPosition, InternalRound, RadiusSide, SectionDefinition, SectionShape,
    SegmentPoint, SideShape,
};

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

/// Single or double volute (Gülich Fig. 7.39).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoluteKind {
    #[default]
    Single,
    /// Two partial volutes split by a rib, with a common discharge.
    Double,
}

/// Inputs of the volute-setup step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoluteSetupInput {
    pub kind: VoluteKind,
    /// Wrap angle ε_sp of the inner volute of a double volute, rad.
    pub rib_wrap: Param<f64>,
    /// Thickness of the rib of a double volute, m.
    pub rib_thickness: Param<f64>,
    /// Length over thickness of the elliptic nose and tail of the rib: 1 for a
    /// semicircle.
    pub rib_nose_ratio: Param<f64>,
    /// Outlet area of the outer channel of a double volute over the throat area of the
    /// first partial volute.
    pub channel_area_ratio: Param<f64>,
    /// Q_Le/Q_opt: allowance for leakage that flows through the volute (eq. T7.7.1).
    pub flow_factor: Param<f64>,
    /// Cutwater (base circle) diameter d_z, m.
    pub cutwater_diameter: Param<f64>,
    /// Width of the volute inlet b₃, m.
    pub inlet_width: Param<f64>,
    /// Thickness of the cutwater nose e₃, m.
    pub cutwater_thickness: Param<f64>,
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

/// The wall of a straight (tangential) discharge diffuser that stays parallel to its
/// axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StraightWall {
    /// The cutwater side: the diffuser widens outward only.
    Inner,
    /// Neither: it widens to both sides.
    #[default]
    Centered,
    /// The outer side: it widens toward the cutwater.
    Outer,
}

/// Shape of the discharge diffuser in the plane of the volute (Gülich Chap. 7.8.1,
/// item 4 and Fig. 7.42).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DischargeShape {
    /// Straight along the flow at the throat.
    Tangential { straight_wall: StraightWall },
    /// A bend of radius R_N turning the flow outward, then straight. `axis_angle` is
    /// the angle between the straight part and the line from the pump axis through the
    /// end of the bend, rad: 0 points the discharge straight away from the pump axis (a
    /// centerline discharge).
    Radial { axis_angle: f64 },
}

/// By default a centerline discharge: the bend turns the outlet onto the line through
/// the pump axis, as on process pumps (our choice).
impl Default for DischargeShape {
    fn default() -> Self {
        DischargeShape::Radial { axis_angle: 0.0 }
    }
}

/// Inputs of the discharge-diffuser step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DischargeInput {
    pub shape: DischargeShape,
    /// Mean radius R_N of the bend of a radial discharge, m (eq. 7.29).
    pub bend_radius: Param<f64>,
    /// Length of the diffuser along its centerline from the throat, the bend of a
    /// radial discharge included, m.
    pub length: Param<f64>,
    /// Outlet (discharge nozzle) diameter, m.
    pub outlet_diameter: Param<f64>,
    /// Round the automatic outlet diameter to the nearest nominal size DN.
    pub nominal_size: bool,
    /// Height H of the outlet face: its distance from the pump axis, m. When set, it
    /// fixes the length of the diffuser instead of `length`.
    pub flange_height: Param<f64>,
    /// Turn of the whole volute about the pump axis: the angle of the cutwater from the
    /// x axis, counter-clockwise about z, rad. Automatic: the discharge points along +y,
    /// vertically up in the front view.
    pub rotation: Param<f64>,
}

/// Shape of the cutwater where the spiral meets the discharge diffuser (Gülich Chap.
/// 7.8.1, item 7, and Fig. 7.41b).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CutwaterShape {
    /// The spiral starts where its outer wall clears the inner wall of the diffuser:
    /// a thin wedge tangent to the base circle, without a designed nose.
    Tangent,
    /// The spiral starts at the cutwater and cuts the inner wall of the diffuser: a
    /// sharp edge along their intersection.
    Sharp,
    /// That edge rounded with `radius`, m (automatic: e₃/2, a nose as thick as the
    /// cutwater).
    Rounded { radius: Param<f64> },
}

impl Default for CutwaterShape {
    fn default() -> Self {
        CutwaterShape::Rounded {
            radius: Param::Auto,
        }
    }
}

/// Inputs of the cutwater step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CutwaterInput {
    pub shape: CutwaterShape,
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
    pub flange: FlangeInput,
    /// Design pressure of the casing over the suction pressure, Pa; automatic: the rise
    /// at shut-off.
    pub pressure: Param<f64>,
    /// Allowable stress of the casing material, Pa; automatic: 50 MPa.
    pub allowable_stress: Param<f64>,
}

/// The flange at the outlet of the discharge diffuser. Every size is automatic by
/// default, from proportions of our own: set them to the standard in use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlangeInput {
    /// Draw the flange on the casing.
    pub enabled: bool,
    /// m
    pub outer_diameter: Param<f64>,
    pub thickness: Param<f64>,
    pub bolt_circle: Param<f64>,
    pub bolt_hole: Param<f64>,
    pub bolts: Param<u32>,
}

impl Default for FlangeInput {
    fn default() -> Self {
        Self {
            enabled: true,
            outer_diameter: Param::Auto,
            thickness: Param::Auto,
            bolt_circle: Param::Auto,
            bolt_hole: Param::Auto,
            bolts: Param::Auto,
        }
    }
}

/// A flange of bore `bore` (m): a disk of `outer_diameter` and `thickness` with
/// `bolts` holes of `bolt_hole` on `bolt_circle`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Flange {
    pub bore: f64,
    pub outer_diameter: f64,
    pub thickness: f64,
    pub bolt_circle: f64,
    pub bolt_hole: f64,
    pub bolts: u32,
}

/// Nominal pipe sizes DN, mm (the preferred numbers of ISO 6708).
pub const NOMINAL_SIZES: &[f64] = &[
    10.0, 15.0, 20.0, 25.0, 32.0, 40.0, 50.0, 65.0, 80.0, 100.0, 125.0, 150.0, 200.0, 250.0, 300.0,
    350.0, 400.0, 450.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1200.0,
];

/// The nominal size DN nearest to diameter `d`, m.
pub fn nominal_size(d: f64) -> f64 {
    let mm = d * 1e3;
    NOMINAL_SIZES
        .iter()
        .copied()
        .min_by(|a, b| (a - mm).abs().total_cmp(&(b - mm).abs()))
        .expect("the list is not empty")
        * 1e-3
}

impl Flange {
    /// The flange of a bore `bore` on a casing wall `wall` (m). Automatic proportions
    /// (our own, not a standard): bolt holes max(14 mm, 0.12·d), bolt circle
    /// d + 2·(wall + 1.5·hole), outer diameter the bolt circle plus 3 holes, thickness
    /// max(2·wall, 0.15·d + 6 mm), and bolts in fours about six holes apart.
    pub fn new(bore: f64, wall: f64, input: &FlangeInput) -> Result<Self, DesignError> {
        let hole = input.bolt_hole.resolve(|| (0.12 * bore).max(0.014));
        let circle = input
            .bolt_circle
            .resolve(|| bore + 2.0 * (wall + 1.5 * hole));
        let outer = input.outer_diameter.resolve(|| circle + 3.0 * hole);
        let thickness = input
            .thickness
            .resolve(|| (2.0 * wall).max(0.15 * bore + 0.006));
        let bolts = input.bolts.resolve(|| {
            let fours = (PI * circle / (4.0 * 6.0 * hole)).ceil().max(1.0);
            4 * fours as u32
        });
        require(hole > 0.0 && thickness > 0.0 && bolts >= 3, || {
            "the flange needs a positive thickness and hole size, and three bolts or more".into()
        })?;
        require(
            circle - hole > bore + 2.0 * wall && circle + hole < outer,
            || {
                format!(
                    "the bolt holes on a {:.0} mm circle must clear the bore with its wall and \
                 stay inside the {:.0} mm flange",
                    circle * 1e3,
                    outer * 1e3
                )
            },
        )?;
        require(PI * circle / f64::from(bolts) > 1.5 * hole, || {
            format!(
                "{bolts} bolt holes of {:.0} mm do not fit on the bolt circle",
                hole * 1e3
            )
        })?;
        Ok(Self {
            bore,
            outer_diameter: outer,
            thickness,
            bolt_circle: circle,
            bolt_hole: hole,
            bolts,
        })
    }
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
    /// Height H = r_a − r'_z the section shape is scaled to, m.
    pub height: f64,
    /// Outermost radius r_a, m.
    pub outer_radius: f64,
    /// m²
    pub area: f64,
    /// Radius of the area centroid, m.
    pub centroid_radius: f64,
}

/// Conical diffuser from the volute throat to the discharge nozzle, straight or with a
/// bend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DischargeDiffuser {
    pub shape: DischargeShape,
    /// Mean radius of the bend of a radial discharge; 0 for a straight one, m.
    pub bend_radius: f64,
    /// Length along the centerline from the throat, m: the bend of a radial discharge
    /// and the straight part after it. Gülich (Chap. 7.8.2, item 4) applies the data of
    /// straight diffusers to curved nozzles.
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

/// The rib of a double volute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rib {
    /// Wrap angle ε_sp of the inner volute, rad.
    pub wrap: f64,
    /// Wrap angle of the rib nose from the cutwater, 2π − ε_sp, rad: where the inner
    /// volute starts. The elliptic nose reaches a little ahead of it.
    pub start: f64,
    /// m
    pub thickness: f64,
    /// Length over thickness of the elliptic nose and tail.
    pub nose_ratio: f64,
    /// Throat of the first partial volute, at the rib nose, corrected for the rib like a
    /// cutwater (eq. 7.28).
    pub outer_throat: SectionSize,
    /// Area of the outer channel at the rib nose (the throat of the first partial volute
    /// less the rib) and at the discharge, m².
    pub channel_inlet_area: f64,
    pub channel_outlet_area: f64,
}

impl Rib {
    /// Length of the nose ahead of the start of the inner volute, and of the tail after
    /// the throat, m.
    pub fn nose_length(&self) -> f64 {
        self.nose_ratio * self.thickness / 2.0
    }

    /// Area of the outer channel at wrap angle `angle` between the rib nose and 2π.
    pub fn channel_area(&self, angle: f64) -> f64 {
        let s = ((angle - self.start) / (TAU - self.start)).clamp(0.0, 1.0);
        self.channel_inlet_area + (self.channel_outlet_area - self.channel_inlet_area) * s
    }
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
    /// The shapes of the cross sections round the spiral; for a double volute, the
    /// straight-walled shape it needs.
    pub cross_sections: CrossSections,
    pub kind: VoluteKind,
    /// The rib of a double volute.
    pub rib: Option<Rib>,
    /// Throat section, including the cutwater correction; for a double volute, the
    /// throat of the inner volute.
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
    pub cutwater_shape: CutwaterShape,
    /// Rounding radius of the cutwater nose; 0 unless rounded, m.
    pub nose_radius: f64,
    pub diffuser: DischargeDiffuser,
    pub wall_thickness: f64,
    /// The flange at the outlet, if drawn.
    pub flange: Option<Flange>,
    /// An internal volute: the axial gap between its spiral and the impeller beside it.
    pub side_gap: Option<SideGap>,
    pub warnings: Vec<String>,
    /// Constant K of the angular momentum law: ∫b/r dr = K·ε.
    momentum_constant: f64,
    /// Whether the throats are enlarged for the cutwater (eq. 7.28).
    throat_corrected: bool,
    /// The straight walls of a double volute, in closed form.
    straight: Option<Shape>,
    /// Height and area of the throat of a single volute before the cutwater correction.
    base_throat: (f64, f64),
}

/// The axial gap between the spiral of an internal volute and the impeller beside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideGap {
    /// The gap s_ax asked for, m.
    pub required: f64,
    /// How far the impeller reaches beside the spiral.
    pub reach: Reach,
    /// The gap left between the casing wall round the spiral and the impeller, when they
    /// lie side by side, m.
    pub gap: Option<f64>,
}

/// Automatic side gap s_ax/r₂ between an internal spiral and the impeller: the value of
/// Gülich's examples of impeller sidewall gaps (Figs. 9.7 to 9.10), which he leaves to
/// the mechanical design (Chap. 9.1).
pub const SIDE_GAP_RATIO: f64 = 0.065;
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

/// A rectangular or trapezoidal section in closed form: flanks at slope t = tan δ from
/// the width b₃ at radius `rz` (Gülich eqs. T7.7.8 and T7.7.9). The double volute uses
/// it, since its rib runs at one radius across sections of straight walls.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    slope: f64,
    b3: f64,
    rz: f64,
}

impl Shape {
    fn slope(&self) -> f64 {
        self.slope
    }

    /// ∫ b/r dr over the section of height `a`.
    fn momentum_integral(&self, a: f64) -> f64 {
        let (rz, t) = (self.rz, self.slope);
        (self.b3 - 2.0 * rz * t) * ((rz + a) / rz).ln() + 2.0 * t * a
    }

    fn area(&self, a: f64) -> f64 {
        self.b3 * a + self.slope * a * a
    }

    /// Wetted perimeter of the section of height `a`: all walls but the opening of
    /// width b₃ toward the impeller.
    fn wetted_perimeter(&self, a: f64) -> f64 {
        let t = self.slope;
        self.b3 + 2.0 * t * a + 2.0 * a * t.hypot(1.0)
    }

    /// ∮ r⁻² dℓ along the wetted walls of the section of height `a`, all but the
    /// opening of width b₃ toward the impeller, 1/m.
    fn wall_inverse_square(&self, a: f64) -> f64 {
        let (rz, t) = (self.rz, self.slope);
        let outer = rz + a;
        (self.b3 + 2.0 * t * a) / (outer * outer) + 2.0 * t.hypot(1.0) * (1.0 / rz - 1.0 / outer)
    }

    fn centroid_radius(&self, a: f64) -> f64 {
        let (rz, t) = (self.rz, self.slope);
        // ∫ r·b dr with b = b₃ + 2t·s over s = r − r'_z ∈ [0, a].
        let moment =
            rz * (self.b3 * a + t * a * a) + self.b3 * a * a / 2.0 + 2.0 * t * a * a * a / 3.0;
        moment / self.area(a)
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
        if self.slope == 0.0 {
            // Eq. T7.7.9.
            return Ok(self.rz * ((k / self.b3).exp() - 1.0));
        }
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

    /// Height of the section with area `area`.
    fn height_for_area(&self, area: f64) -> f64 {
        if area <= 0.0 {
            return 0.0;
        }
        let t = self.slope;
        if t < 1e-12 {
            area / self.b3
        } else {
            (-self.b3 + (self.b3 * self.b3 + 4.0 * t * area).sqrt()) / (2.0 * t)
        }
    }
}

/// The size of a section from its properties.
fn size_of(p: &section::SectionProperties) -> SectionSize {
    SectionSize {
        height: p.height,
        outer_radius: p.outer_radius,
        area: p.area,
        centroid_radius: p.centroid_radius,
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
        let stagnation_radius = cutwater_diameter / 2.0 + cutwater_thickness / 2.0;
        let mut warnings = Vec::new();
        // A double volute splits every section with its rib at one radius, so its walls
        // must be straight and keep one cone angle: other shapes become the trapezoid of
        // the first definition's cone angle.
        let mut sections = input.cross_section.clone();
        if setup.kind == VoluteKind::Double {
            let mut active = sections.sections.iter().filter(|d| d.active);
            let first = active.clone().min_by(|a, b| a.angle.total_cmp(&b.angle));
            let cone = first
                .and_then(|d| d.shape.cone_angle())
                .unwrap_or(DEFAULT_CONE_ANGLE_DEG.to_radians());
            if !active.all(|d| d.shape.straight_cone_angle() == Some(cone)) {
                warnings.push(format!(
                    "a double volute needs straight walls at one cone angle: the cross \
                     sections were made trapezoidal at {:.1}°",
                    cone.to_degrees()
                ));
            }
            sections = CrossSectionInput::single(if cone == 0.0 {
                SectionShape::rectangular()
            } else {
                SectionShape::trapezoidal(cone)
            });
        }
        let mut cross_sections = CrossSections::new(&sections, inlet_width, stagnation_radius)?;
        let section = Shape {
            slope: cross_sections.definitions()[0]
                .shape
                .straight_cone_angle()
                .unwrap_or(0.0)
                .tan(),
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

        // A double volute: the inner volute wraps ε_sp, 180° by default and 170° with an
        // even number of blades so that two blades never pass the cutwaters together
        // (Gülich Chap. 7.8.1, item 1).
        let even_blades = basis.impeller_blades.is_multiple_of(2);
        let rib_wrap = match setup.kind {
            VoluteKind::Single => None,
            VoluteKind::Double => {
                let wrap = setup
                    .rib_wrap
                    .resolve(|| if even_blades { 170f64.to_radians() } else { PI });
                require(
                    wrap > 90f64.to_radians() && wrap <= 240f64.to_radians(),
                    || {
                        format!(
                            "the inner volute of a double volute must wrap more than 90° and \
                             at most 240°, got {:.1}°",
                            wrap.to_degrees()
                        )
                    },
                )?;
                Some(wrap)
            }
        };

        // Throat of a partial volute of wrap `wrap` from angular momentum, for both
        // laws, then the correction Δa₃/a₃ = 0.2·(e/a₃)² for a cutwater or rib nose of
        // thickness e (eq. 7.28). A single volute takes the shape defined at its throat.
        let correction_for = |blockage: f64, a: f64| {
            if input.spiral.cutwater_correction {
                0.2 * blockage * blockage / a
            } else {
                0.0
            }
        };
        let partial_throat =
            |wrap: f64, blockage: f64| -> Result<(SectionSize, f64), DesignError> {
                let a = section.height_for_integral(momentum_constant * wrap)?;
                let correction = correction_for(blockage, a);
                Ok((section.size(a + correction), correction))
            };
        let inner_wrap = rib_wrap.unwrap_or(TAU);
        let (throat, throat_correction, throat_aspect, base_throat) = if rib_wrap.is_some() {
            let (throat, correction) = partial_throat(inner_wrap, cutwater_thickness)?;
            let aspect = (inlet_width + 2.0 * section.slope() * throat.height) / throat.height;
            (throat, correction, aspect, (0.0, 0.0))
        } else {
            let a = cross_sections.height_for_momentum(TAU, momentum_constant * TAU)?;
            let base_area = cross_sections.properties(TAU, a)?.area;
            let correction = correction_for(cutwater_thickness, a);
            let p = cross_sections.properties(TAU, a + correction)?;
            let aspect = p.width / (p.outer_radius - p.inner_radius);
            (size_of(&p), correction, aspect, (a, base_area))
        };
        let throat_velocity = flow * inner_wrap / TAU / throat.area;
        let c2 = basis.cu.hypot(basis.cm);
        let deceleration = throat_velocity / c2;

        // Flow angle at the cutwater, eqs. T7.7.3 to T7.7.5 (one cutwater: no blockage).
        let d3 = cutwater_diameter;
        let c3u = d2 * basis.cu / d3;
        let c3m = flow / (PI * d3 * inlet_width);
        let cutwater_flow_angle = c3m.atan2(c3u);
        let incidence = input.cutwater.incidence.resolve(|| 0.0);
        let cutwater_angle = cutwater_flow_angle + incidence;
        let cutwater_shape = input.cutwater.shape;
        let nose_radius = match cutwater_shape {
            CutwaterShape::Rounded { radius } => radius.resolve(|| cutwater_thickness / 2.0),
            _ => 0.0,
        };
        require(nose_radius >= 0.0, || {
            "the cutwater radius cannot be negative".into()
        })?;

        let rib = match rib_wrap {
            None => None,
            Some(wrap) => {
                let start = TAU - wrap;
                let thickness = setup.rib_thickness.resolve(|| cutwater_thickness);
                require(thickness > 0.0, || {
                    "the rib thickness must be positive".into()
                })?;
                let (outer_throat, _) = partial_throat(start, thickness)?;
                require(outer_throat.height > 1.5 * thickness, || {
                    format!(
                        "the rib ({:.1} mm) is too thick for the {:.1} mm high throat of the \
                         first partial volute",
                        thickness * 1e3,
                        outer_throat.height * 1e3
                    )
                })?;
                let nose_ratio = setup.rib_nose_ratio.resolve(|| 1.0);
                require((1.0..=5.0).contains(&nose_ratio), || {
                    format!("the nose ratio of the rib must lie between 1 and 5, got {nose_ratio}")
                })?;
                let ratio = setup.channel_area_ratio.resolve(|| 1.0);
                require((0.8..=3.0).contains(&ratio), || {
                    format!(
                        "the area ratio of the outer channel must lie between 0.8 and 3, \
                         got {ratio}"
                    )
                })?;
                Some(Rib {
                    wrap,
                    start,
                    thickness,
                    nose_ratio,
                    outer_throat,
                    channel_inlet_area: outer_throat.area - section.area(thickness),
                    channel_outlet_area: outer_throat.area * ratio,
                })
            }
        };
        // Both channels feed the discharge diffuser.
        let discharge_area = throat.area + rib.map_or(0.0, |r| r.channel_outlet_area);
        let diffuser = Self::discharge(basis, &input.diffuser, flow, discharge_area)?;
        let wall_thickness = input.wall.thickness.resolve(|| (0.03 * d2).max(0.006));
        require(wall_thickness > 0.0, || {
            "the wall thickness must be positive".into()
        })?;
        // An internal spiral beside the impeller keeps the side gap from it, past its own
        // casing wall. The neck only moves the spiral sideways, so the automatic neck grows
        // by what the gap lacks.
        let mut side_gap = None;
        if let (Some(round), Some(bend)) =
            (cross_sections.internal().copied(), cross_sections.bend())
        {
            let required = round.side_gap.resolve(|| SIDE_GAP_RATIO * r2);
            let toward = bend.face.signum();
            let inner = cross_sections.properties(TAU, throat.height)?.inner_radius;
            let reach = basis.sides.map_or(Reach::Clear, |s| {
                s.farthest(toward, inner, stagnation_radius + bend.top)
            });
            let gap_at = |face: f64, z: f64| toward * (basis.midplane + face - z) - wall_thickness;
            let mut gap = None;
            if let Reach::At(z) = reach {
                let left = gap_at(bend.face, z);
                if round.neck_width.is_auto() && left < required {
                    let neck = round.neck_width.resolve(|| 0.5 * inlet_width) + required - left;
                    for d in sections.sections.iter_mut() {
                        if let SectionShape::RoundInternal(r) = &mut d.shape {
                            r.neck_width = Param::Manual(neck);
                        }
                    }
                    cross_sections = CrossSections::new(&sections, inlet_width, stagnation_radius)?;
                }
                let face = cross_sections.bend().map_or(bend.face, |b| b.face);
                gap = Some(gap_at(face, z));
            }
            match (reach, gap) {
                (Reach::Eye, _) => warnings.push(
                    "the internal spiral reaches into the suction eye of the impeller: turn it \
                     toward +z or make it smaller"
                        .into(),
                ),
                (_, Some(g)) if g < required * (1.0 - 1e-9) => warnings.push(format!(
                    "the internal spiral leaves {:.1} mm to the impeller beside it, less than \
                     the side gap of {:.1} mm: lengthen the neck",
                    g * 1e3,
                    required * 1e3
                )),
                _ => {}
            }
            side_gap = Some(SideGap {
                required,
                reach,
                gap,
            });
        }
        let flange = input
            .wall
            .flange
            .enabled
            .then(|| Flange::new(diffuser.outlet_diameter, wall_thickness, &input.wall.flange))
            .transpose()?;

        if cross_sections.internal().is_some() {
            warnings.push(
                "an internal volute has no cutwater of its own: it is where the spiral and \
                 the discharge diffuser cut each other"
                    .into(),
            );
        }
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
        if let Some(r) = &rib {
            if r.wrap > PI + 1e-9 {
                warnings.push(format!(
                    "the inner volute wraps {:.0}°: past 180° the long outer channel adds \
                     losses and radial force at overload (Gülich Chap. 7.8.1)",
                    r.wrap.to_degrees()
                ));
            }
            if even_blades && (r.wrap - PI).abs() < 0.5f64.to_radians() {
                warnings.push(format!(
                    "with {} blades and a rib at 180° two blades pass the cutwaters at once: \
                     Gülich advises 165° to 170° (Chap. 7.8.1)",
                    basis.impeller_blades
                ));
            }
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
            cross_sections,
            kind: setup.kind,
            rib,
            throat,
            throat_correction,
            throat_velocity,
            deceleration,
            throat_aspect,
            cutwater_flow_angle,
            cutwater_angle,
            cutwater_shape,
            nose_radius,
            diffuser,
            wall_thickness,
            flange,
            side_gap,
            warnings,
            momentum_constant,
            throat_corrected: input.spiral.cutwater_correction,
            straight: rib_wrap.map(|_| section),
            base_throat,
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
        // Eq. 7.29: R_N ≈ 1.5·√(4·A₃q/π).
        let bend_radius = match input.shape {
            DischargeShape::Tangential { .. } => 0.0,
            DischargeShape::Radial { axis_angle } => {
                require((0.0..=80f64.to_radians()).contains(&axis_angle), || {
                    format!(
                        "the axis of a radial discharge must lie between 0° and 80° from the \
                         line through the pump axis, got {:.1}°",
                        axis_angle.to_degrees()
                    )
                })?;
                let radius = input
                    .bend_radius
                    .resolve(|| 1.5 * (4.0 * throat_area / PI).sqrt());
                require(radius > r1, || {
                    format!(
                        "the bend radius {:.1} mm must exceed the throat radius {:.1} mm",
                        radius * 1e3,
                        r1 * 1e3
                    )
                })?;
                radius
            }
        };
        // Eq. 1.44: ϑ_al = 16.5°·√(R₁/L).
        let allowable_angle = (16.5 * (r1 / length).sqrt()).to_radians();
        let outlet_diameter = input.outlet_diameter.resolve(|| {
            let d = if basis.nq > NO_DIFFUSION_NQ {
                2.0 * r1
            } else {
                2.0 * (r1 + length * (DIFFUSER_ANGLE_USE * allowable_angle).tan())
            };
            if input.nominal_size {
                nominal_size(d)
            } else {
                d
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
            shape: input.shape,
            bend_radius,
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

    /// The straight walls of a double volute in closed form; for a single volute, those
    /// of its first definition, or rectangles.
    fn shape(&self) -> Shape {
        self.straight.unwrap_or(Shape {
            slope: self.cross_sections.definitions()[0]
                .shape
                .straight_cone_angle()
                .unwrap_or(0.0)
                .tan(),
            b3: self.inlet_width,
            rz: self.stagnation_radius,
        })
    }

    /// The cross section at wrap angle `angle` (rad) from the cutwater; 2π is the throat.
    /// In a double volute the first partial volute runs to the rib nose and the inner
    /// one from there to 2π.
    pub fn section(&self, angle: f64) -> Result<SectionSize, DesignError> {
        let e3 = self.cutwater_thickness;
        match &self.rib {
            Some(rib) if angle > rib.start => self.partial_section(angle - rib.start, rib.wrap, e3),
            // The first partial volute ends at the rib nose.
            Some(rib) => self.partial_section(angle, rib.start, rib.thickness),
            None => self.spiral_section(angle),
        }
    }

    /// Section of a single volute at `angle`, shaped by the definitions there and sized
    /// like [`Self::partial_section`].
    fn spiral_section(&self, angle: f64) -> Result<SectionSize, DesignError> {
        let sections = &self.cross_sections;
        let (end, end_area) = self.base_throat;
        let correction = if self.throat_corrected {
            0.2 * self.cutwater_thickness.powi(2) / end
        } else {
            0.0
        };
        let fraction = (angle / TAU).clamp(0.0, 1.0);
        let height = match self.law {
            AreaLaw::ConstantVelocity => sections.height_for_area(angle, end_area * fraction)?,
            _ => sections
                .height_for_momentum(angle, self.momentum_constant * angle.clamp(0.0, TAU))?,
        };
        Ok(size_of(
            &sections.properties(angle, height + correction * fraction)?,
        ))
    }

    /// The outline at `angle` of the section of height `height`, resampled for lofting
    /// (s, h).
    pub fn loft_outline(&self, angle: f64, height: f64) -> section::Outline {
        self.cross_sections.loft_outline(angle, height)
    }

    /// Section at `angle` of a partial volute of wrap `wrap`, whose throat is sized by
    /// angular momentum for the flow of its wrap (Gülich Chap. 7.8.1, item 8) and
    /// enlarged for a nose of thickness `blockage` at its end (eq. 7.28).
    fn partial_section(
        &self,
        angle: f64,
        wrap: f64,
        blockage: f64,
    ) -> Result<SectionSize, DesignError> {
        let shape = self.shape();
        let end = shape.height_for_integral(self.momentum_constant * wrap)?;
        let correction = if self.throat_corrected {
            0.2 * blockage * blockage / end
        } else {
            0.0
        };
        let fraction = (angle / wrap).clamp(0.0, 1.0);
        let height = match self.law {
            // Eq. 7.27 on the uncorrected throat, then the correction ramps up.
            AreaLaw::ConstantVelocity => shape.height_for_area(shape.area(end) * fraction),
            _ => shape.height_for_integral(self.momentum_constant * angle.clamp(0.0, wrap))?,
        };
        Ok(shape.size(height + correction * fraction))
    }

    /// Σ c³·ΔA over the walls of the spiral (Gülich eq. T3.8.21) over Q³, m⁻⁴. As
    /// Gülich prescribes (Chap. 3.8), c_u·r = K is constant over each cross section and
    /// K changes round the circumference: K = q·Q/∫(b/r)dr, with q the fraction of the
    /// flow collected up to wrap angle φ, the flow spread evenly over the wrap. A wall
    /// element ΔA = r·dφ·dℓ then gives ∫ (q/∫(b/r)dr)³·∮ r⁻² dℓ dφ. A double volute
    /// sums both partial volutes and adds its outer channel, closed all round, at the
    /// radius just outside the rib with its mean velocity (our estimate).
    pub fn friction_integral(&self) -> f64 {
        const STEPS: u32 = 360;
        let shape = self.shape();
        let step = TAU / f64::from(STEPS);
        let mut sum = 0.0;
        for k in 0..STEPS {
            let angle = (f64::from(k) + 0.5) * step;
            let Ok(section) = self.section(angle) else {
                continue;
            };
            let collected = match &self.rib {
                Some(rib) if angle > rib.start => (angle - rib.start) / TAU,
                _ => angle / TAU,
            };
            let (momentum, walls) = match &self.rib {
                Some(_) => (
                    shape.momentum_integral(section.height),
                    shape.wall_inverse_square(section.height),
                ),
                None => match self.cross_sections.properties(angle, section.height) {
                    Ok(p) => (p.momentum, p.wall_inverse_square),
                    Err(_) => continue,
                },
            };
            if momentum > 0.0 {
                sum += (collected / momentum).powi(3) * walls;
            }
            if let Some(rib) = self.rib.as_ref().filter(|r| angle > r.start) {
                let area = rib.channel_area(angle);
                let height = shape.height_for_area(area);
                let perimeter = shape.wetted_perimeter(height) + self.inlet_width;
                let radius = section.outer_radius + rib.thickness + height / 2.0;
                sum += (rib.start / TAU / area).powi(3) * perimeter * radius;
            }
        }
        sum * step
    }

    /// ∫ b/r dr of the section at wrap angle `angle` scaled to `height`.
    pub fn momentum_integral(&self, angle: f64, height: f64) -> f64 {
        match self.rib {
            Some(_) => self.shape().momentum_integral(height),
            None => self
                .cross_sections
                .properties(angle, height)
                .map_or(0.0, |p| p.momentum),
        }
    }

    /// Wrap angle the angular momentum of the section of height `height` at `angle`
    /// belongs to (eq. 7.25); it equals `angle` on the spiral of angular momentum.
    pub fn momentum_angle(&self, angle: f64, height: f64) -> f64 {
        self.momentum_integral(angle, height) / self.momentum_constant
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
        description: "∫ b/r dr = ε·Q_Le/(2π·c₂u·r₂) from r'_z = r_z + e₃/2; for any \
                      section shape as ∮ ln r dz round its outline (Green's theorem)",
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
        id: "volute.side_gap",
        description: "s_ax = 0.065·r₂ between the casing wall round an internal spiral and \
                      the impeller beside it (hub disk and shroud 0.02·d₂, at least 3 mm)",
        reference: "Gülich 2nd ed., Chap. 9.1: sidewall gaps are left to the mechanical \
                    design; s_ax/r₂ = 0.065 in Figs. 9.7 to 9.10. The value is our choice",
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
            sides: None,
        }
    }

    fn design_with(shape: SectionShape, law: AreaLaw) -> VoluteDesign {
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(shape),
            ..VoluteInput::default()
        };
        input.spiral.law = law;
        VoluteDesign::new(&reference_basis(), &input).unwrap()
    }

    /// ∫ b/r dr computed numerically from the width b(r) of the section at `angle`: the
    /// length of the circle of radius r inside its outline.
    fn numeric_integral(d: &VoluteDesign, angle: f64, height: f64) -> f64 {
        let rz = d.stagnation_radius;
        let outline = d.cross_sections.outline(angle, height);
        let p = &outline.points;
        let width = |r: f64| {
            let h = r - rz;
            let mut crossings: Vec<f64> = (0..p.len())
                .filter_map(|i| {
                    let (a, b) = (p[i], p[(i + 1) % p.len()]);
                    ((a.y <= h) != (b.y <= h)).then(|| a.x + (b.x - a.x) * (h - a.y) / (b.y - a.y))
                })
                .collect();
            crossings.sort_by(f64::total_cmp);
            crossings.chunks(2).map(|c| c[1] - c[0]).sum::<f64>()
        };
        gauss_legendre(|r| width(r) / r, rz, rz + height, 400)
    }

    #[test]
    fn cutwater_diameter_follows_table_10_2() {
        let d = design_with(SectionShape::rectangular(), AreaLaw::AngularMomentum);
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
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::rectangular()),
            ..VoluteInput::default()
        };
        input.spiral.cutwater_correction = false;
        let b = reference_basis();
        let d = VoluteDesign::new(&b, &input).unwrap();
        // a₃ = r'_z·(exp(Q_Le·ε_sp/(b₃·c₂u·r₂·2π)) − 1) with ε_sp = 2π.
        let rz = d.stagnation_radius;
        let a3 = rz * ((d.flow / (d.inlet_width * b.cu * 0.086)).exp() - 1.0);
        assert_relative_eq!(d.throat.height, a3, max_relative = 1e-12);
    }

    #[test]
    fn a_round_throat_is_lower_than_the_circle_of_eq_t7_7_7() {
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::round()),
            ..VoluteInput::default()
        };
        input.spiral.cutwater_correction = false;
        let b = reference_basis();
        let d = VoluteDesign::new(&b, &input).unwrap();
        // X_sp = Q_Le/(π·c₂u·r₂)·(ε_sp/2π), d₃q = X_sp + √(2·d'_z·X_sp) for a whole
        // circle touching r'_z. Ours spans the inlet b₃ at r'_z, which carries more flow
        // near the axis: a little lower for the same angular momentum.
        let x = d.flow / (PI * b.cu * 0.086);
        let d3q = x + (2.0 * 2.0 * d.stagnation_radius * x).sqrt();
        let ratio = d.throat.height / d3q;
        assert!(ratio < 1.0 && ratio > 0.85, "{ratio}");
        let k = d.flow / (b.cu * 0.086);
        assert_relative_eq!(
            d.momentum_integral(TAU, d.throat.height),
            k,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            d.momentum_angle(TAU, d.throat.height),
            TAU,
            max_relative = 1e-9
        );
    }

    fn every_shape() -> Vec<SectionShape> {
        vec![
            SectionShape::rectangular(),
            SectionShape::round(),
            SectionShape::default(),
            SectionShape::symmetric(SideShape::RadiusBased(RadiusSide::default())),
            SectionShape::symmetric(SideShape::Bezier(BezierSide::default())),
            SectionShape::symmetric(SideShape::LineSegments {
                points: section::default_segments(),
            }),
            SectionShape::Sides {
                left: SideShape::Round { ellipse_ratio: 0.7 },
                right: Some(SideShape::trapezoidal(0.4)),
            },
            SectionShape::RoundExternal(ExternalRound::default()),
        ]
    }

    #[test]
    fn every_shape_conserves_angular_momentum() {
        for shape in every_shape() {
            let d = design_with(shape.clone(), AreaLaw::AngularMomentum);
            for angle in [0.5, PI, 1.5 * PI] {
                let s = d.section(angle).unwrap();
                let height = s.height - d.throat_correction * angle / TAU;
                let k = d.flow / (TAU * 13.27 * 0.086) * angle;
                assert_relative_eq!(numeric_integral(&d, angle, height), k, max_relative = 1e-5);
            }
            assert!(d.friction_integral() > 0.0, "{shape:?}");
        }
    }

    #[test]
    fn wall_friction_integrates_over_the_spiral() {
        // Rectangular sections under angular momentum: c_u·r = c₂u·r₂ everywhere, so
        // Σ c³·ΔA = (c₂u·r₂)³·∫∮ r⁻² dℓ dφ, with the outer wall b₃/r_a² and the side
        // walls 2·(1/r'_z − 1/r_a).
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::rectangular()),
            ..VoluteInput::default()
        };
        input.spiral.law = AreaLaw::AngularMomentum;
        input.spiral.cutwater_correction = false;
        let b = reference_basis();
        let d = VoluteDesign::new(&b, &input).unwrap();
        let (b3, rz) = (d.inlet_width, d.stagnation_radius);
        let walls = |phi: f64| {
            let ra = d.section(phi).unwrap().outer_radius;
            b3 / (ra * ra) + 2.0 * (1.0 / rz - 1.0 / ra)
        };
        let k = b.cu * b.impeller_diameter / 2.0 / d.flow;
        assert_relative_eq!(
            d.friction_integral(),
            k.powi(3) * gauss_legendre(walls, 0.0, TAU, 200),
            max_relative = 1e-4
        );
        // A circle: the integral round it, less the opening, at a radius far out
        // tends to its perimeter over r².
        let circle = SectionShape::round().outline(1e-7, 0.02);
        assert_relative_eq!(
            circle.properties(10.0, 0.02).unwrap().wall_inverse_square,
            PI * 0.02 / (10.01 * 10.01),
            max_relative = 1e-4
        );
        // The outer channel of a double volute rubs too.
        let double = double_with(|_| {}).unwrap();
        let single = VoluteDesign::new(&reference_basis(), &VoluteInput::default()).unwrap();
        assert!(double.friction_integral() > single.friction_integral());
    }

    #[test]
    fn flanges_follow_our_proportions_and_nominal_sizes() {
        assert_relative_eq!(nominal_size(0.083), 0.080);
        assert_relative_eq!(nominal_size(0.0921), 0.100);
        // A 100 mm bore on a 6 mm wall: 14 mm holes on 154 mm, a 196 mm disk 21 mm
        // thick, two fours of bolts.
        let f = Flange::new(0.1, 0.006, &FlangeInput::default()).unwrap();
        assert_relative_eq!(f.bolt_hole, 0.014);
        assert_relative_eq!(f.bolt_circle, 0.154, max_relative = 1e-12);
        assert_relative_eq!(f.outer_diameter, 0.196, max_relative = 1e-12);
        assert_relative_eq!(f.thickness, 0.021, max_relative = 1e-12);
        assert_eq!(f.bolts, 8);
        // Holes cutting into the wall around the bore are refused.
        let tight = FlangeInput {
            bolt_circle: Param::Manual(0.11),
            ..FlangeInput::default()
        };
        assert!(Flange::new(0.1, 0.006, &tight).is_err());
        // The volute carries one unless switched off.
        let mut input = VoluteInput::default();
        assert!(
            VoluteDesign::new(&reference_basis(), &input)
                .unwrap()
                .flange
                .is_some()
        );
        input.wall.flange.enabled = false;
        assert!(
            VoluteDesign::new(&reference_basis(), &input)
                .unwrap()
                .flange
                .is_none()
        );
        input.diffuser.nominal_size = true;
        let d = VoluteDesign::new(&reference_basis(), &input).unwrap();
        assert_relative_eq!(
            d.diffuser.outlet_diameter,
            nominal_size(d.diffuser.outlet_diameter)
        );
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
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::rectangular()),
            ..VoluteInput::default()
        };
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

    /// Radius of the suction eye of [`with_sides`], m.
    const EYE: f64 = 0.07;

    /// The reference impeller beside the volute: a hub disk flat at the outlet plus the
    /// wall toward +z, a shroud reaching back to the eye at r = 70 mm toward −z.
    fn with_sides() -> ImpellerOutlet {
        use crate::outlet::{ImpellerSides, SIDE_POINTS};
        let b = reference_basis();
        let (r2, half, wall) = (0.086, b.impeller_width / 2.0, 0.00344);
        let last = (SIDE_POINTS - 1) as f64;
        ImpellerOutlet {
            sides: Some(ImpellerSides {
                plus: std::array::from_fn(|i| {
                    tp_math::DVec2::new(r2 * (1.0 - i as f64 / last), b.midplane + half + wall)
                }),
                minus: std::array::from_fn(|i| {
                    let t = i as f64 / last;
                    tp_math::DVec2::new(r2 + (EYE - r2) * t, (b.midplane - half - wall) * (1.0 - t))
                }),
            }),
            ..b
        }
    }

    fn internal(round: InternalRound) -> VoluteDesign {
        let input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::RoundInternal(round)),
            ..VoluteInput::default()
        };
        VoluteDesign::new(&with_sides(), &input).unwrap()
    }

    #[test]
    fn the_impeller_reaches_its_farthest_over_a_range_of_radii() {
        let sides = with_sides().sides.unwrap();
        let b = reference_basis();
        let back = b.midplane + b.impeller_width / 2.0 + 0.00344;
        assert_eq!(sides.farthest(1.0, 0.05, 0.2), Reach::At(back));
        assert_eq!(sides.farthest(1.0, 0.09, 0.2), Reach::Clear);
        // The shroud comes nearer the axis at smaller radii.
        let Reach::At(z) = sides.farthest(-1.0, 0.075, 0.2) else {
            panic!();
        };
        let t = (0.086 - 0.075) / (0.086 - EYE);
        assert_relative_eq!(
            z,
            (b.midplane - b.impeller_width / 2.0 - 0.00344) * (1.0 - t)
        );
        assert_eq!(sides.farthest(-1.0, 0.06, 0.2), Reach::Eye);
    }

    #[test]
    fn an_internal_spiral_keeps_the_side_gap_from_the_impeller() {
        let r2 = 0.086;
        // Behind the hub disk the automatic neck leaves exactly the side gap.
        let d = internal(InternalRound::default());
        let gap = d.side_gap.unwrap();
        assert_relative_eq!(gap.required, SIDE_GAP_RATIO * r2);
        assert!(matches!(gap.reach, Reach::At(_)));
        let left = gap.gap.unwrap();
        assert!(left >= gap.required - 1e-12, "{left} vs {}", gap.required);
        assert!(d.warnings.iter().all(|w| !w.contains("side gap")));
        // A short manual neck: warned.
        let short = internal(InternalRound {
            neck_width: Param::Manual(0.001),
            side_gap: Param::Manual(0.03),
            ..InternalRound::default()
        });
        assert!(short.side_gap.unwrap().gap.unwrap() < 0.03);
        assert!(
            short.warnings.iter().any(|w| w.contains("side gap")),
            "{:?}",
            short.warnings
        );
        // A gap larger than the default neck leaves moves the spiral away; the throat
        // stays the same.
        assert!(left > 0.02, "{left}");
        let wide = internal(InternalRound {
            side_gap: Param::Manual(0.03),
            ..InternalRound::default()
        });
        assert_relative_eq!(wide.side_gap.unwrap().gap.unwrap(), 0.03, epsilon = 1e-12);
        assert_relative_eq!(wide.throat.area, d.throat.area, max_relative = 1e-12);
        // Toward the suction side the spiral runs into the eye.
        let front = internal(InternalRound {
            open_to_right: false,
            ..InternalRound::default()
        });
        assert_eq!(front.side_gap.unwrap().reach, Reach::Eye);
        assert!(front.warnings.iter().any(|w| w.contains("suction eye")));
        // Without the impeller (after a diffuser) nothing is checked.
        let input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::RoundInternal(
                InternalRound::default(),
            )),
            ..VoluteInput::default()
        };
        let free = VoluteDesign::new(&reference_basis(), &input).unwrap();
        assert_eq!(free.side_gap.unwrap().reach, Reach::Clear);
    }

    fn double_with(input: impl FnOnce(&mut VoluteInput)) -> Result<VoluteDesign, DesignError> {
        let mut volute = VoluteInput::default();
        volute.setup.kind = VoluteKind::Double;
        input(&mut volute);
        VoluteDesign::new(&reference_basis(), &volute)
    }

    #[test]
    fn double_volute_partial_throats_follow_angular_momentum() {
        let single = design_with(SectionShape::rectangular(), AreaLaw::AngularMomentum);
        let d = double_with(|v| {
            v.cross_section = CrossSectionInput::single(SectionShape::rectangular());
            v.spiral.law = AreaLaw::AngularMomentum;
            v.spiral.cutwater_correction = false;
        })
        .unwrap();
        let rib = d.rib.unwrap();
        assert_eq!(d.kind, VoluteKind::Double);
        // Seven blades: 180°, and the two partial volutes are alike.
        assert_relative_eq!(rib.wrap, PI);
        assert_relative_eq!(rib.outer_throat.area, d.throat.area, max_relative = 1e-12);
        // Each throat satisfies eq. 3.14 for half the flow: ∫b/r dr = π·Q/(2π·c₂u·r₂).
        let k = d.flow / (TAU * reference_basis().cu * 0.086);
        assert_relative_eq!(
            d.momentum_integral(TAU, d.throat.height),
            k * PI,
            max_relative = 1e-9
        );
        // Under half the single throat: with b·ln(1 + a/r'_z) the outer part of a
        // section carries less flow, so half the integral needs less than half the area.
        let ratio = d.throat.area / single.throat.area;
        // For rectangles it is 1/(1 + √(1 + a/r'_z)) of the single throat.
        let x = single.throat.height / single.stagnation_radius;
        assert_relative_eq!(ratio, 1.0 / (1.0 + (1.0 + x).sqrt()), max_relative = 1e-3);
        // The first partial volute is the single spiral up to the rib nose (the single
        // one carries a little of its cutwater correction).
        let s90 = single.section(PI / 2.0).unwrap();
        assert_relative_eq!(
            d.section(PI / 2.0).unwrap().area,
            s90.area,
            max_relative = 1e-3
        );
        // The inner volute starts again from zero after the nose.
        assert!(d.section(rib.start + 0.01).unwrap().area < 0.05 * d.throat.area);
        assert_relative_eq!(
            d.section(TAU).unwrap().area,
            d.throat.area,
            max_relative = 1e-12
        );
        // Half the flow through each throat; both feed the discharge diffuser.
        assert_relative_eq!(
            d.throat_velocity,
            0.5 * d.flow / d.throat.area,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            d.diffuser.inlet_area,
            d.throat.area + rib.channel_outlet_area,
            max_relative = 1e-12
        );
        assert_relative_eq!(rib.thickness, d.cutwater_thickness);
        // The outer channel starts as the first throat less the rib, and its nose is a
        // semicircle as thick as the rib.
        assert_relative_eq!(
            rib.channel_inlet_area,
            rib.outer_throat.area - d.inlet_width * rib.thickness,
            max_relative = 1e-12
        );
        assert_relative_eq!(rib.nose_ratio, 1.0);
        assert_relative_eq!(rib.nose_length(), rib.thickness / 2.0);
    }

    #[test]
    fn the_rib_nose_enlarges_the_first_throat_like_a_cutwater() {
        let plain = double_with(|v| v.spiral.cutwater_correction = false).unwrap();
        let e = 0.008;
        let d = double_with(|v| v.setup.rib_thickness = Param::Manual(e)).unwrap();
        let (a, rib) = (plain.rib.unwrap().outer_throat.height, d.rib.unwrap());
        // Eq. 7.28 with the rib for the first throat, the cutwater for the inner one.
        assert_relative_eq!(
            rib.outer_throat.height - a,
            0.2 * e * e / a,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            d.throat.height - plain.throat.height,
            0.2 * d.cutwater_thickness.powi(2) / plain.throat.height,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            d.section(rib.start).unwrap().height,
            rib.outer_throat.height,
            max_relative = 1e-12
        );
    }

    #[test]
    fn double_volute_rib_limits() {
        // An even blade count turns the default rib to 170°.
        let mut basis = reference_basis();
        basis.impeller_blades = 6;
        let mut input = VoluteInput::default();
        input.setup.kind = VoluteKind::Double;
        let d = VoluteDesign::new(&basis, &input).unwrap();
        assert_relative_eq!(d.rib.unwrap().wrap, 170f64.to_radians());
        // 180° with six blades and 200° are allowed with a warning; 80° is not.
        input.setup.rib_wrap = Param::Manual(PI);
        let d = VoluteDesign::new(&basis, &input).unwrap();
        assert!(d.warnings.iter().any(|w| w.contains("two blades")));
        let d = double_with(|v| v.setup.rib_wrap = Param::Manual(200f64.to_radians())).unwrap();
        assert!(d.warnings.iter().any(|w| w.contains("past 180°")));
        assert!(double_with(|v| v.setup.rib_wrap = Param::Manual(80f64.to_radians())).is_err());
        assert!(double_with(|v| v.setup.channel_area_ratio = Param::Manual(5.0)).is_err());
        assert!(double_with(|v| v.setup.rib_nose_ratio = Param::Manual(0.5)).is_err());
        assert!(double_with(|v| v.setup.rib_nose_ratio = Param::Manual(6.0)).is_err());
        assert!(double_with(|v| v.setup.rib_thickness = Param::Manual(0.025)).is_err());
        // Round sections cannot be split at one radius: they become the default trapezoid.
        let d = double_with(|v| {
            v.cross_section = CrossSectionInput::single(SectionShape::round());
        })
        .unwrap();
        assert!(d.warnings.iter().any(|w| w.contains("straight walls")));
        assert_eq!(
            d.cross_sections.definitions()[0].shape,
            SectionShape::default()
        );
        let d = double_with(|_| {}).unwrap();
        assert!(d.warnings.iter().all(|w| !w.contains("straight walls")));
        // A widening outer channel, against the first throat.
        let d = double_with(|v| v.setup.channel_area_ratio = Param::Manual(1.5)).unwrap();
        let rib = d.rib.unwrap();
        assert_relative_eq!(rib.channel_outlet_area, 1.5 * rib.outer_throat.area);
        assert_relative_eq!(
            rib.channel_area(0.5 * (rib.start + TAU)),
            0.5 * (rib.channel_inlet_area + rib.channel_outlet_area),
            max_relative = 1e-12
        );
    }
}
