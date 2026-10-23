//! Blades of a radial impeller: properties per span, mean lines, thickness and edges.
//!
//! Each blade section lies on a streamline surface of revolution (a span). Its mean
//! line is defined by the blade angle β along the normalized meridional length m̄ and
//! integrated by conformal mapping, dθ = dm / (r·tan β). θ is measured from the
//! trailing edge in the direction of rotation, so all sections share θ = 0 at the
//! trailing edge (an axial trailing edge) and the wrap angle is θ at the leading edge.
//!
//! The thickness is laid off normal to the mean line on the stream surface, in the
//! conformal plane (m̄ = ∫dm/r, θ), which keeps angles and scales lengths by 1/r. A
//! rounded edge is then a symmetric nose about the mean line, and a blunt edge is cut on
//! the meridional edge.

use std::f64::consts::{FRAC_PI_2, PI};

use serde::{Deserialize, Serialize};
use tp_math::{Bezier, DVec2, DVec3, Polyline, roots};

use super::ImpellerBasis;
use super::meridian::{Channel, MeridionalContour, SpanLine};
use crate::triangle::blockage_factor;
use crate::{DesignError, Param};

/// Inputs of the blade-properties step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BladePropertiesInput {
    /// Number of blade sections from hub to shroud (≥ 2).
    pub spans: Param<u32>,
    pub shape: BladeShape,
    /// Which meridional flow places the sections and gives their meridional velocity.
    pub meridional_flow: MeridionalFlow,
    /// Which meridional velocity sets the leading-edge angles.
    pub inlet_velocity: InletVelocity,
}

/// The meridional velocity the leading-edge triangles are designed with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InletVelocity {
    /// Each section's own c_m where its stream line meets the leading edge, from the
    /// chosen meridional flow.
    #[default]
    Local,
    /// The same c₁m = Q_La/(π/4·(d₁² − d_n²)) at every section: Gülich (2nd ed.,
    /// Table 7.1, eq. T7.1.7) designs the inlet blade angles "with constant c1m", and
    /// draws the stream surfaces of equal part flows "frequently with the assumption
    /// cm = constant" (Chap. 7.2.2.2, step 1). The stream surfaces and the c_m along
    /// the blade still come from the meridional flow.
    Uniform,
}

/// The meridional flow the blade sections are built on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeridionalFlow {
    /// Stream surfaces and meridional velocity of the two-dimensional potential flow:
    /// faster on the inside of the bend (the shroud), slower at the hub.
    #[default]
    Potential,
    /// Equal-area stream surfaces with c_m uniform along each quasi-orthogonal.
    QuasiOneDimensional,
}

/// Shape of the blade surface.
///
/// The cylindrical (2D) shapes sweep one plan-view curve θ(r) parallel to the axis, so
/// every section shares it: the blade has no lean and can be cast in a one-piece core,
/// at the price of a leading-edge angle that matches the flow only near one span
/// (Lobanoff & Ross 2nd ed., ch. 3, accept it for Ns 500–1000, nq ≈ 10–19; Gülich
/// 2nd ed., Chap. 7.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BladeShape {
    /// Doubly curved: each section has its own β from its leading-edge triangle.
    #[default]
    FreeForm,
    /// Cylindrical, β(r) following the mean-line distribution from the leading edge of
    /// the middle span to the outlet.
    Cylindrical,
    /// Cylindrical, plan view one circular arc through the leading edge of the middle
    /// span and the outlet (Gülich eq. 7.10).
    CircularArc,
    /// Cylindrical, plan view a straight line: r·cos β = r₂·cos β₂B. With β₂B = 90° the
    /// blades are radial (Gülich Chap. 7.3.3).
    Straight,
}

impl BladeShape {
    /// Cylindrical (2D) blade.
    pub fn is_cylindrical(self) -> bool {
        self != BladeShape::FreeForm
    }
}

/// Plan view θ(r) of a cylindrical blade, θ from the trailing edge (r₂) in the direction
/// of rotation.
#[derive(Debug, Clone)]
struct PlanView {
    /// Radii from the smallest leading-edge radius to r₂.
    r: Vec<f64>,
    theta: Vec<f64>,
    beta: Vec<f64>,
}

impl PlanView {
    /// `reference` is the leading edge (radius, blade angle) the curve passes through;
    /// `r_min` the smallest radius it must reach.
    fn build(
        shape: BladeShape,
        distribution: BetaDistribution,
        reference: (f64, f64),
        r2: f64,
        beta2: f64,
        r_min: f64,
    ) -> Result<Self, DesignError> {
        let (r1, beta1) = reference;
        // cos β(r) = (r₂·cos β₂ + k·(r² − r₂²))/r: a circle for k ≠ 0, a straight line
        // for k = 0; k makes the arc pass through (r₁, β₁) (Gülich eq. 7.10 rearranged).
        let k = match shape {
            BladeShape::CircularArc => (r2 * beta2.cos() - r1 * beta1.cos()) / (r2 * r2 - r1 * r1),
            _ => 0.0,
        };
        let beta_at = |r: f64| -> Option<f64> {
            match shape {
                BladeShape::FreeForm | BladeShape::Cylindrical => {
                    let mbar = ((r - r1) / (r2 - r1)).clamp(0.0, 1.0);
                    Some(beta_at(distribution, beta1, beta2, mbar))
                }
                BladeShape::CircularArc | BladeShape::Straight => {
                    let c = (r2 * beta2.cos() + k * (r * r - r2 * r2)) / r;
                    (c.abs() < 1.0).then(|| c.acos())
                }
            }
        };
        let n = INTEGRATION_STEPS;
        let r: Vec<f64> = (0..=n)
            .map(|i| r_min + (r2 - r_min) * i as f64 / n as f64)
            .collect();
        let beta = r
            .iter()
            .map(|&ri| {
                beta_at(ri).ok_or_else(|| {
                    DesignError::Infeasible(format!(
                        "the plan view of the blade with β2B = {:.1}° cannot reach the leading \
                         edge at r = {:.1} mm; raise β2B (90° gives radial blades) or choose \
                         another shape",
                        beta2.to_degrees(),
                        ri * 1e3
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        // θ(r) = ∫ dr / (r·tan β) from r₂ inwards.
        let mut theta = vec![0.0; r.len()];
        for i in (0..n).rev() {
            let f = |j: usize| beta[j].cos() / (r[j] * beta[j].sin());
            theta[i] = theta[i + 1] + 0.5 * (r[i + 1] - r[i]) * (f(i) + f(i + 1));
        }
        Ok(Self { r, theta, beta })
    }

    /// θ and the plan-view blade angle at radius `r`.
    fn at(&self, r: f64) -> (f64, f64) {
        let n = self.r.len() - 1;
        let (r0, r1) = (self.r[0], self.r[n]);
        let x = ((r - r0) / (r1 - r0)).clamp(0.0, 1.0) * n as f64;
        let i = (x.floor() as usize).min(n - 1);
        let w = x - i as f64;
        (
            self.theta[i] + w * (self.theta[i + 1] - self.theta[i]),
            self.beta[i] + w * (self.beta[i + 1] - self.beta[i]),
        )
    }
}

pub const DEFAULT_SPANS: u32 = 5;
pub const DEFAULT_POINTS: u32 = 41;

/// Blade angle distribution between leading and trailing edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BetaDistribution {
    /// β varies linearly with m̄.
    #[default]
    Linear,
    /// Cubic Bézier in (m̄, β) that leaves the leading edge and reaches the trailing
    /// edge with zero slope; `inlet` and `outlet` (0–1) set how long β stays close to
    /// its edge values.
    Smooth { inlet: f64, outlet: f64 },
}

/// Inputs of the blade mean-line step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeanLineInput {
    pub beta: BetaDistribution,
    /// Points per section from leading to trailing edge.
    pub points: Param<u32>,
    pub wrap: WrapMode,
    /// Wrap angle every free-form section shares with [`WrapMode::Common`], rad;
    /// automatic: the mean of the wraps their own β distributions give them. With
    /// [`WrapMode::Linear`], the wrap at the hub; automatic: the hub section's own.
    pub wrap_angle: Param<f64>,
    /// Wrap at the shroud with [`WrapMode::Linear`], rad; automatic: the shroud
    /// section's own.
    pub wrap_angle_shroud: Param<f64>,
}

/// How the wrap angles of the sections relate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WrapMode {
    /// Every section wraps the same angle, so the leading edge has no tangential sweep:
    /// each section keeps its edge angles and their gradients, and a bump
    /// a·16m̄²(1 − m̄)² added to its β distribution takes it to the common wrap (as commercial programs set an average
    /// wrap angle by moving the inner points of their mean lines).
    #[default]
    Common,
    /// Each section wraps what its own β distribution gives.
    Free,
    /// The wrap goes linearly with the flow fraction of the section from the hub value
    /// to the shroud value (our construction), with the same bump as the common wrap.
    /// Gülich (2nd ed., Chap. 7.2.2.2, step 7) gives 120°–140° for six blades.
    Linear,
}

/// Blade angles the wrap bump may reach, rad.
const BUMP_BETA_RANGE: (f64, f64) = (5.0 * PI / 180.0, FRAC_PI_2);

/// How far the bump of a common or linear wrap may lift β above both edge angles of a
/// section before a warning, rad. Gülich (2nd ed., Chap. 7.2.2.2, Fig. 7.6) develops
/// β smoothly between its edge values; the limit is our criterion.
pub const WRAP_HUMP_WARNING: f64 = 15.0 * PI / 180.0;

/// Inputs of the blade-profile step: normal blade thickness, varying linearly from
/// leading to trailing edge before the edges are shaped. Default: the thickness of the
/// main-dimensions step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileInput {
    pub thickness_leading: Param<f64>,
    pub thickness_trailing: Param<f64>,
}

pub use super::edge::{EdgeProfile, EdgeShape};

/// Inputs of the blade-edge step: the shapes at the hub, and at the shroud when they
/// vary linearly from hub to shroud.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeInput {
    pub leading: EdgeShape,
    pub trailing: EdgeShape,
    /// The leading edge at the shroud, the spans between blending it with `leading`
    /// parameter by parameter; `None`: `leading` on every span.
    pub leading_shroud: Option<EdgeShape>,
    /// The trailing edge at the shroud, as `leading_shroud`.
    pub trailing_shroud: Option<EdgeShape>,
}

impl EdgeInput {
    /// The (leading, trailing) shapes of the span at flow fraction `span`.
    pub fn at(&self, span: f64) -> Result<(EdgeShape, EdgeShape), DesignError> {
        let blend = |hub: EdgeShape, shroud: Option<EdgeShape>, error: &str| match shroud {
            None => Ok(hub),
            Some(shroud) => hub
                .lerp(shroud, span)
                .ok_or_else(|| DesignError::InvalidInput(error.into())),
        };
        Ok((
            blend(
                self.leading,
                self.leading_shroud,
                "the leading edge needs the same kind of shape at hub and shroud",
            )?,
            blend(
                self.trailing,
                self.trailing_shroud,
                "the trailing edge needs the same kind of shape at hub and shroud",
            )?,
        ))
    }

    /// Every shape given, hub and shroud.
    pub fn shapes(&self) -> impl Iterator<Item = EdgeShape> + '_ {
        [self.leading, self.trailing]
            .into_iter()
            .chain(self.leading_shroud)
            .chain(self.trailing_shroud)
    }
}

impl Default for EdgeInput {
    /// Elliptic 4:1 leading edge (c_p,min ≈ 0.41) and full thickness to the trailing
    /// edge, which leaves room for under-filing (Gülich 2nd ed., pp. 347–349).
    fn default() -> Self {
        Self {
            leading: EdgeShape::Ellipse { ratio: 4.0 },
            trailing: EdgeShape::Cut,
            leading_shroud: None,
            trailing_shroud: None,
        }
    }
}

/// Flow and blade at the leading edge of one section.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LeadingEdge {
    pub position: DVec2,
    pub cm: f64,
    pub u: f64,
    /// Inflow angle β₁ (swirl-free), rad.
    pub flow_angle: f64,
    /// Blade angle β₁B including blockage and incidence, rad.
    pub blade_angle: f64,
    pub blockage: f64,
}

/// One blade section, sampled at stations from leading to trailing edge.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BladeSection {
    /// Flow fraction of the span (0 hub, 1 shroud).
    pub span: f64,
    /// (z, r) of each station.
    pub meridional: Vec<DVec2>,
    /// Meridional length from the leading edge, m.
    pub m: Vec<f64>,
    /// Blade angle β_B, rad.
    pub beta: Vec<f64>,
    /// Angular position of the mean line, rad, from the trailing edge in the
    /// direction of rotation.
    pub theta: Vec<f64>,
    /// Normal thickness, m.
    pub thickness: Vec<f64>,
    /// Meridional velocity of the stream surface at each station, m/s.
    pub cm: Vec<f64>,
    pub leading_edge: LeadingEdge,
    /// The face toward the direction of rotation.
    pub pressure: Face,
    /// The face turned away from the direction of rotation.
    pub suction: Face,
}

/// One face of a blade section, from the leading to the trailing edge: as many points as
/// the mean line, but at their own meridional positions.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Face {
    /// (z, r) of each point.
    pub meridional: Vec<DVec2>,
    /// Angular position, rad, as the mean line's θ.
    pub theta: Vec<f64>,
}

impl BladeSection {
    /// θ at the leading edge.
    pub fn wrap_angle(&self) -> f64 {
        self.theta[0]
    }

    /// Length of the mean line on the stream surface, m.
    pub fn mean_line_length(&self) -> f64 {
        self.m
            .windows(2)
            .zip(self.beta.windows(2))
            .map(|(m, b)| (m[1] - m[0]) / (0.5 * (b[0] + b[1])).sin())
            .sum()
    }

    /// Mean line in Cartesian coordinates (x, y, z), z along the axis. `sign` is +1
    /// when the impeller turns in the positive sense about +z.
    pub fn mean_line(&self, sign: f64) -> Vec<DVec3> {
        cartesian(&self.meridional, &self.theta, sign)
    }

    /// Pressure side: the blade face that faces the direction of rotation.
    pub fn pressure_side(&self, sign: f64) -> Vec<DVec3> {
        cartesian(&self.pressure.meridional, &self.pressure.theta, sign)
    }

    /// Suction side: the face turned away from the direction of rotation.
    pub fn suction_side(&self, sign: f64) -> Vec<DVec3> {
        cartesian(&self.suction.meridional, &self.suction.theta, sign)
    }
}

/// Points (z, r) at angles θ as Cartesian (x, y, z), turned by `sign`.
fn cartesian(points: &[DVec2], theta: &[f64], sign: f64) -> Vec<DVec3> {
    points
        .iter()
        .zip(theta)
        .map(|(p, &t)| {
            let phi = sign * t;
            DVec3::new(p.y * phi.cos(), p.y * phi.sin(), p.x)
        })
        .collect()
}

/// Conformal meridional coordinate m̄(s) = ∫ds/r along a stream line, on a fine grid of
/// its arc length, and its inverse.
struct ConformalLine {
    step: f64,
    mbar: Vec<f64>,
}

impl ConformalLine {
    fn new(line: &Polyline<DVec2>) -> Self {
        let n = INTEGRATION_STEPS;
        let step = line.length() / n as f64;
        let mut mbar = vec![0.0; n + 1];
        let mut r_prev = line.point_at_length(0.0).y;
        for i in 1..=n {
            let r = line.point_at_length(i as f64 * step).y;
            mbar[i] = mbar[i - 1] + step * 0.5 * (1.0 / r_prev + 1.0 / r);
            r_prev = r;
        }
        Self { step, mbar }
    }

    /// m̄ at arc length `s`, extended linearly past the ends.
    fn at(&self, s: f64) -> f64 {
        let n = self.mbar.len() - 1;
        let x = s / self.step;
        let i = (x.floor().max(0.0) as usize).min(n - 1);
        self.mbar[i] + (x - i as f64) * (self.mbar[i + 1] - self.mbar[i])
    }

    /// Arc length at conformal coordinate `mbar`, extended linearly past the ends.
    fn arc_length(&self, mbar: f64) -> f64 {
        let n = self.mbar.len() - 1;
        let i = self.mbar.partition_point(|&v| v < mbar).clamp(1, n) - 1;
        let w = (mbar - self.mbar[i]) / (self.mbar[i + 1] - self.mbar[i]);
        (i as f64 + w) * self.step
    }
}

/// Value of the piecewise-linear `values` at fractional index `p`, extended along the end
/// segments.
fn at_index(values: &[f64], p: f64) -> f64 {
    let n = values.len() - 1;
    let i = (p.floor().max(0.0) as usize).min(n - 1);
    values[i] + (p - i as f64) * (values[i + 1] - values[i])
}

/// Fractional index where the increasing `values` reach `target`, searched from the
/// start (or from the end), extended along the end segments; `None` if they do not
/// reach it within the half of the line on that side.
fn index_of(values: &[f64], target: f64, from_start: bool) -> Option<f64> {
    let n = values.len() - 1;
    let solve = |i: usize| {
        let d = values[i + 1] - values[i];
        (d.abs() > f64::EPSILON).then(|| i as f64 + (target - values[i]) / d)
    };
    if from_start {
        if values[0] >= target {
            return solve(0);
        }
        (0..n.div_ceil(2))
            .find(|&i| values[i + 1] >= target)
            .and_then(solve)
    } else {
        if values[n] <= target {
            return solve(n - 1);
        }
        (n / 2..n)
            .rev()
            .find(|&i| values[i] <= target)
            .and_then(solve)
    }
}

/// One face of a section: the mean line moved by `offsets` (m, + toward +θ: the pressure
/// face) along its normal in the conformal plane.
/// A blunt edge is cut on the meridional edge, where the face is trimmed or carried on
/// along its end segment; only the quarter of the points next to a cut edge moves, so
/// a rounded nose keeps one face point beside each mean-line point.
#[allow(clippy::too_many_arguments)]
fn face(
    line: &Polyline<DVec2>,
    conformal: &ConformalLine,
    s_le: f64,
    m: &[f64],
    beta: &[f64],
    theta: &[f64],
    offsets: &[f64],
    edges: (EdgeShape, EdgeShape),
) -> Face {
    let n = m.len();
    let (mut x, mut y) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for k in 0..n {
        let s = s_le + m[k];
        let h = offsets[k] / line.point_at_length(s).y;
        x.push(conformal.at(s) + h * beta[k].cos());
        y.push(theta[k] + h * beta[k].sin());
    }
    let start = match edges.0 {
        EdgeShape::Cut => index_of(&x, conformal.at(s_le), true).unwrap_or(0.0),
        _ => 0.0,
    };
    let last = (n - 1) as f64;
    let end = match edges.1 {
        EdgeShape::Cut => index_of(&x, conformal.at(s_le + m[n - 1]), false).unwrap_or(last),
        _ => last,
    };
    // The first and last quarter of the points stretch to the cut edges.
    let (head, tail) = (
        (n / 4).max(1) as f64,
        (last - (n / 4).max(1) as f64).max(1.0),
    );
    let index = |j: f64| {
        if j < head {
            start + (head - start) * j / head
        } else if j > tail {
            tail + (end - tail) * (j - tail) / (last - tail)
        } else {
            j
        }
    };
    let (meridional, theta) = (0..n)
        .map(|j| {
            let p = index(j as f64);
            let s = conformal
                .arc_length(at_index(&x, p))
                .clamp(0.0, line.length());
            (line.point_at_length(s), at_index(&y, p))
        })
        .unzip();
    Face { meridional, theta }
}

/// All blade sections of one blade; the other blades are copies rotated by 2π/z.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Blade {
    pub count: u32,
    /// +1 for positive rotation about +z (clockwise seen from the inlet looking
    /// downstream), −1 otherwise.
    pub rotation_sign: f64,
    pub outlet_blade_angle: f64,
    pub sections: Vec<BladeSection>,
    pub warnings: Vec<String>,
}

/// Normalized station positions m̄ ∈ [0, 1], clustered at both edges.
fn stations(count: usize) -> Vec<f64> {
    (0..count)
        .map(|k| 0.5 * (1.0 - (PI * k as f64 / (count - 1) as f64).cos()))
        .collect()
}

/// Blade angle at m̄ for the chosen distribution.
fn beta_at(distribution: BetaDistribution, beta1: f64, beta2: f64, mbar: f64) -> f64 {
    match distribution {
        BetaDistribution::Linear => beta1 + (beta2 - beta1) * mbar,
        BetaDistribution::Smooth { inlet, outlet } => {
            let curve = Bezier::new(vec![
                DVec2::new(0.0, beta1),
                DVec2::new(inlet, beta1),
                DVec2::new(1.0 - outlet, beta2),
                DVec2::new(1.0, beta2),
            ])
            .expect("four control points");
            curve.y_at_x(mbar.clamp(0.0, 1.0)).unwrap_or(beta1)
        }
    }
}

/// Arc length along a span polyline at channel position `t` (its points are evenly
/// spaced in t).
fn length_at(line: &Polyline<DVec2>, t: f64) -> f64 {
    let cumulative = line.cumulative_lengths();
    let n = cumulative.len();
    let x = t.clamp(0.0, 1.0) * (n - 1) as f64;
    let i = (x.floor() as usize).min(n - 2);
    let w = x - i as f64;
    cumulative[i] + w * (cumulative[i + 1] - cumulative[i])
}

/// Channel position t of arc length `s` along a span polyline (the inverse of
/// [`length_at`]).
fn position_at(line: &Polyline<DVec2>, s: f64) -> f64 {
    let cumulative = line.cumulative_lengths();
    let n = cumulative.len() - 1;
    let i = cumulative.partition_point(|&c| c < s).clamp(1, n) - 1;
    let step = cumulative[i + 1] - cumulative[i];
    let w = if step > 0.0 {
        ((s - cumulative[i]) / step).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (i as f64 + w) / n as f64
}

/// Smallest dr/dm at the reference leading edge of a cylindrical blade: below it the
/// plan-view angle would have to be nearly tangential.
const MIN_CYLINDRICAL_SLOPE: f64 = 0.2;

/// Steps of the fine grid used to integrate the conformal mapping.
const INTEGRATION_STEPS: usize = 2000;

/// Inputs that shape all sections, resolved from the step inputs.
#[derive(Debug, Clone, Copy)]
struct SectionSettings {
    beta: BetaDistribution,
    points: usize,
    thickness_leading: f64,
    thickness_trailing: f64,
    edges: (EdgeShape, EdgeShape),
    /// c_m of every leading edge with [`InletVelocity::Uniform`].
    inlet_cm: Option<f64>,
}

fn leading_edge(basis: &ImpellerBasis, position: DVec2, cm: f64, thickness: f64) -> LeadingEdge {
    let u = basis.omega * position.y;
    let flow_angle = cm.atan2(u);
    let mut blade_angle = flow_angle + basis.incidence;
    let mut blockage = 1.0;
    for _ in 0..50 {
        blockage = blockage_factor(
            basis.blade_count,
            thickness,
            2.0 * position.y,
            blade_angle,
            FRAC_PI_2,
        );
        if !blockage.is_finite() {
            break;
        }
        let next = (blockage * cm).atan2(u) + basis.incidence;
        let done = (next - blade_angle).abs() < 1e-12;
        blade_angle = next;
        if done {
            break;
        }
    }
    LeadingEdge {
        position,
        cm,
        u,
        flow_angle,
        blade_angle,
        blockage,
    }
}

/// What a section's mean line rests on before its wrap is chosen: the stream line, the
/// edges and their angles, and the radius on a fine grid of m̄.
struct SpanFrame<'a> {
    span: &'a SpanLine,
    line: Polyline<DVec2>,
    s_le: f64,
    meridional_length: f64,
    le: LeadingEdge,
    beta1: f64,
    beta2: f64,
    radius: Vec<f64>,
}

/// The shape of the wrap bump: 0 at both edges, with zero slope there so that the β
/// gradients at the edges stay as the distribution gives them, and 1 halfway.
fn bump_shape(mbar: f64) -> f64 {
    let q = 4.0 * mbar * (1.0 - mbar);
    q * q
}

impl SpanFrame<'_> {
    /// Blade angle at m̄ with the wrap bump `bump`.
    fn beta(&self, distribution: BetaDistribution, bump: f64, mbar: f64) -> f64 {
        beta_at(distribution, self.beta1, self.beta2, mbar) + bump * bump_shape(mbar)
    }

    /// θ on the fine grid from the trailing edge back, θ = ∫ dm/(r·tan β).
    fn theta_fine(&self, distribution: BetaDistribution, bump: f64) -> Vec<f64> {
        let n = INTEGRATION_STEPS;
        let f: Vec<f64> = (0..=n)
            .map(|i| {
                let mbar = i as f64 / n as f64;
                1.0 / (self.radius[i] * self.beta(distribution, bump, mbar).tan())
            })
            .collect();
        let dm = self.meridional_length / n as f64;
        let mut theta = vec![0.0; n + 1];
        for i in (0..n).rev() {
            theta[i] = theta[i + 1] + 0.5 * dm * (f[i] + f[i + 1]);
        }
        theta
    }

    fn wrap(&self, distribution: BetaDistribution, bump: f64) -> f64 {
        self.theta_fine(distribution, bump)[0]
    }

    /// The bump that gives the section the wrap `target`, within the blade angles of
    /// [`BUMP_BETA_RANGE`]; `Err` with the nearest bump when the target is out of reach.
    fn bump_for(&self, distribution: BetaDistribution, target: f64) -> Result<f64, f64> {
        let n = INTEGRATION_STEPS;
        let (lo_beta, hi_beta) = BUMP_BETA_RANGE;
        let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
        for i in 1..n {
            let mbar = i as f64 / n as f64;
            let q = bump_shape(mbar);
            let base = beta_at(distribution, self.beta1, self.beta2, mbar);
            lo = lo.max((lo_beta - base) / q);
            hi = hi.min((hi_beta - base) / q);
        }
        let (lo, hi) = (lo.min(0.0), hi.max(0.0));
        // More angle, less wrap.
        let excess = |a: f64| self.wrap(distribution, a) - target;
        if excess(lo) < 0.0 {
            return Err(lo);
        }
        if excess(hi) > 0.0 {
            return Err(hi);
        }
        roots::brent(excess, lo, hi, 1e-10).map_err(|_| 0.0)
    }
}

fn frame<'a>(
    basis: &ImpellerBasis,
    contour: &MeridionalContour,
    channel: &'a Channel,
    span_index: usize,
    settings: &SectionSettings,
) -> Result<SpanFrame<'a>, DesignError> {
    let span = &channel.spans[span_index];
    let line = Polyline::new(span.points.clone()).expect("channel spans have points");
    // Leading-edge channel position on this stream line.
    let t_le = contour.leading_edge_on(span.fraction, &span.points);
    let s_le = length_at(&line, t_le);
    let s_te = line.length();
    let meridional_length = s_te - s_le;

    let le_position = line.point_at_length(s_le);
    let le = leading_edge(
        basis,
        le_position,
        settings.inlet_cm.unwrap_or_else(|| span.cm_at(t_le)),
        settings.thickness_leading,
    );
    if !le.blockage.is_finite() {
        return Err(DesignError::Infeasible(format!(
            "at span {:.2} the blades fill the whole leading-edge circumference",
            span.fraction
        )));
    }
    let beta2 = basis.outlet_blade_angle_at(line.point_at_length(s_te).y)?;
    let radius = (0..=INTEGRATION_STEPS)
        .map(|i| {
            let mbar = i as f64 / INTEGRATION_STEPS as f64;
            line.point_at_length(s_le + mbar * meridional_length).y
        })
        .collect();
    Ok(SpanFrame {
        span,
        beta1: le.blade_angle,
        beta2,
        line,
        s_le,
        meridional_length,
        le,
        radius,
    })
}

/// One section on its frame, its β distribution raised by the wrap bump `bump`.
fn section(
    frame: &SpanFrame<'_>,
    basis: &ImpellerBasis,
    settings: &SectionSettings,
    plan: Option<&PlanView>,
    bump: f64,
) -> Result<BladeSection, DesignError> {
    let SpanFrame {
        span,
        line,
        s_le,
        meridional_length,
        le,
        ..
    } = frame;
    let (s_le, meridional_length, le) = (*s_le, *meridional_length, *le);
    let beta = |mbar: f64| frame.beta(settings.beta, bump, mbar);

    // Conformal mapping on a fine grid: θ(m) = ∫ dm / (r tan β), from the TE backwards.
    let theta_fine = frame.theta_fine(settings.beta, bump);
    let theta_at = |mbar: f64| {
        let x = mbar * INTEGRATION_STEPS as f64;
        let i = (x.floor() as usize).min(INTEGRATION_STEPS - 1);
        let w = x - i as f64;
        theta_fine[i] + w * (theta_fine[i + 1] - theta_fine[i])
    };

    let mbars = stations(settings.points);
    let m: Vec<f64> = mbars.iter().map(|&x| x * meridional_length).collect();
    let meridional: Vec<DVec2> = m
        .iter()
        .map(|&mm| line.point_at_length(s_le + mm))
        .collect();
    let cm: Vec<f64> = m
        .iter()
        .map(|&mm| span.cm_at(position_at(line, s_le + mm)))
        .collect();
    let (betas, theta): (Vec<f64>, Vec<f64>) = match plan {
        None => (
            mbars.iter().map(|&x| beta(x)).collect(),
            mbars.iter().map(|&x| theta_at(x)).collect(),
        ),
        // A cylindrical blade: θ follows the plan view at the local radius. On the stream
        // surface cot β = (dr/dm)·cot β_plan, so β → 90° where the section runs axially.
        Some(plan) => m
            .iter()
            .zip(&meridional)
            .map(|(&mm, p)| {
                let dr_dm = radial_slope(line, s_le + mm);
                if dr_dm < -1e-3 {
                    return Err(DesignError::Infeasible(format!(
                        "a cylindrical blade needs the radius to grow along each section, \
                             but it falls at span {:.2}; move the leading edge or use the \
                             free-form shape",
                        span.fraction
                    )));
                }
                let (theta, beta_plan) = plan.at(p.y);
                let beta = beta_plan.sin().atan2(beta_plan.cos() * dr_dm.max(0.0));
                Ok((beta, theta))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip(),
    };

    // Distance along the mean line from the leading edge, for edge shaping.
    let mut along = vec![0.0; m.len()];
    for k in 1..m.len() {
        let b = 0.5 * (betas[k - 1] + betas[k]);
        along[k] = along[k - 1] + (m[k] - m[k - 1]) / b.sin();
    }
    let total = *along.last().expect("at least two stations");
    let (e1, e2) = (settings.thickness_leading, settings.thickness_trailing);
    let profile = |shape, e| EdgeProfile::new(shape, e).map_err(DesignError::InvalidInput);
    let (leading, trailing) = (
        profile(settings.edges.0, e1)?,
        profile(settings.edges.1, e2)?,
    );
    if leading.length() + trailing.length() > total {
        return Err(DesignError::Infeasible(format!(
            "the leading- and trailing-edge shapes overlap at span {:.2}: shorten them",
            span.fraction
        )));
    }
    // Signed offsets of both faces from the mean line, m: (pressure, suction).
    let offsets: Vec<(f64, f64)> = mbars
        .iter()
        .zip(&along)
        .map(|(&mbar, &s)| {
            let half = 0.5 * (e1 + (e2 - e1) * mbar);
            let (p, q) = if s < leading.length() {
                leading.at(s)
            } else if total - s < trailing.length() {
                trailing.at(total - s)
            } else {
                (1.0, -1.0)
            };
            (half * p, half * q)
        })
        .collect();
    let thickness: Vec<f64> = offsets.iter().map(|(p, q)| p - q).collect();

    let conformal = ConformalLine::new(line);
    let faces = [true, false].map(|pressure| {
        let face_offsets: Vec<f64> = offsets
            .iter()
            .map(|&(p, q)| if pressure { p } else { q })
            .collect();
        face(
            line,
            &conformal,
            s_le,
            &m,
            &betas,
            &theta,
            &face_offsets,
            settings.edges,
        )
    });
    let [pressure, suction] = faces;

    let pitch = 2.0 * PI / f64::from(basis.blade_count);
    for k in 0..m.len() {
        let occupied = thickness[k] / (meridional[k].y * betas[k].sin());
        if occupied >= pitch {
            return Err(DesignError::Infeasible(format!(
                "neighbouring blades intersect at span {:.2}; reduce thickness or blade number",
                span.fraction
            )));
        }
    }

    Ok(BladeSection {
        span: span.fraction,
        meridional,
        m,
        beta: betas,
        theta,
        thickness,
        cm,
        leading_edge: le,
        pressure,
        suction,
    })
}

/// dr/dm of a span polyline at arc length `s`.
fn radial_slope(line: &Polyline<DVec2>, s: f64) -> f64 {
    let h = 1e-4 * line.length();
    let a = line.point_at_length((s - h).max(0.0));
    let b = line.point_at_length((s + h).min(line.length()));
    (b.y - a.y) / a.distance(b)
}

/// Leading edge of span `index`, with the flow there and dr/dm of the span at it.
fn span_leading_edge(
    basis: &ImpellerBasis,
    contour: &MeridionalContour,
    channel: &Channel,
    index: usize,
    settings: &SectionSettings,
) -> (LeadingEdge, f64) {
    let span = &channel.spans[index];
    let line = Polyline::new(span.points.clone()).expect("channel spans have points");
    let t_le = contour.leading_edge_on(span.fraction, &span.points);
    let s = length_at(&line, t_le);
    let position = line.point_at_length(s);
    (
        leading_edge(
            basis,
            position,
            settings.inlet_cm.unwrap_or_else(|| span.cm_at(t_le)),
            settings.thickness_leading,
        ),
        radial_slope(&line, s),
    )
}

/// The plan view of a cylindrical blade: through the leading edge of the middle span
/// (the flow matches there) and β₂B at r₂, reaching in to the smallest leading-edge radius.
fn plan_view(
    basis: &ImpellerBasis,
    contour: &MeridionalContour,
    channel: &Channel,
    shape: BladeShape,
    settings: &SectionSettings,
) -> Result<PlanView, DesignError> {
    let edges: Vec<(LeadingEdge, f64)> = (0..channel.spans.len())
        .map(|j| span_leading_edge(basis, contour, channel, j, settings))
        .collect();
    let (middle, slope) = edges
        .iter()
        .zip(&channel.spans)
        .min_by(|a, b| {
            (a.1.fraction - 0.5)
                .abs()
                .total_cmp(&(b.1.fraction - 0.5).abs())
        })
        .map(|(le, _)| *le)
        .expect("a channel has spans");
    let r2 = basis.outlet_diameter / 2.0;
    let r_min = edges
        .iter()
        .map(|(le, _)| le.position.y)
        .fold(f64::INFINITY, f64::min);
    if middle.position.y >= r2 * (1.0 - 1e-6) {
        return Err(DesignError::Infeasible(
            "the leading edge sits at the outlet radius".into(),
        ));
    }
    // Where the section still runs partly axially the blade angle on the stream surface
    // exceeds the plan-view angle: tan β = tan β_plan/(dr/dm).
    if slope < MIN_CYLINDRICAL_SLOPE {
        return Err(DesignError::Infeasible(
            "a cylindrical blade needs its leading edge where the channel already turns \
             outward; move the leading edge downstream"
                .into(),
        ));
    }
    let beta1 = (middle.blade_angle.tan() * slope).atan();
    PlanView::build(
        shape,
        settings.beta,
        (middle.position.y, beta1),
        r2,
        basis.outlet_blade_angle,
        r_min.min(middle.position.y),
    )
}

impl Blade {
    pub(crate) fn build(
        basis: &ImpellerBasis,
        contour: &MeridionalContour,
        channel: &Channel,
        properties: &BladePropertiesInput,
        mean_lines: &MeanLineInput,
        profiles: &ProfileInput,
        edges: &EdgeInput,
    ) -> Result<Self, DesignError> {
        let points = mean_lines.points.resolve(|| DEFAULT_POINTS);
        if points < 3 {
            return Err(DesignError::InvalidInput(
                "a blade section needs at least 3 points".into(),
            ));
        }
        if let BetaDistribution::Smooth { inlet, outlet } = mean_lines.beta {
            let ok = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
            if !(ok(inlet) && ok(outlet) && inlet + outlet <= 1.0) {
                return Err(DesignError::InvalidInput(
                    "smooth β distribution needs inlet, outlet in [0, 1] with inlet + outlet ≤ 1"
                        .into(),
                ));
            }
        }
        for edge in edges.shapes() {
            edge.validate().map_err(DesignError::InvalidInput)?;
        }
        let positive = |name: &str, v: f64| {
            if v.is_finite() && v > 0.0 {
                Ok(v)
            } else {
                Err(DesignError::InvalidInput(format!(
                    "{name} must be positive, got {v}"
                )))
            }
        };
        let settings = SectionSettings {
            beta: mean_lines.beta,
            points: points as usize,
            thickness_leading: positive(
                "leading-edge thickness",
                profiles.thickness_leading.resolve(|| basis.blade_thickness),
            )?,
            thickness_trailing: positive(
                "trailing-edge thickness",
                profiles
                    .thickness_trailing
                    .resolve(|| basis.blade_thickness),
            )?,
            edges: (edges.leading, edges.trailing),
            inlet_cm: match properties.inlet_velocity {
                InletVelocity::Local => None,
                InletVelocity::Uniform => {
                    let (d1, dn) = (basis.inlet_diameter, basis.hub_diameter);
                    Some(basis.flow / (PI / 4.0 * (d1 * d1 - dn * dn)))
                }
            },
        };
        let shape = properties.shape;
        if shape.is_cylindrical() && basis.is_semi_axial() {
            return Err(DesignError::InvalidInput(
                "cylindrical blades need a radial outlet; a semi-axial impeller takes the \
                 free-form shape"
                    .into(),
            ));
        }
        let plan = if shape.is_cylindrical() {
            Some(plan_view(basis, contour, channel, shape, &settings)?)
        } else {
            None
        };
        let frames = (0..channel.spans.len())
            .map(|j| frame(basis, contour, channel, j, &settings))
            .collect::<Result<Vec<_>, _>>()?;
        let mut warnings = Vec::new();
        // A common or linear wrap: each free-form section gets the bump that takes it
        // to its target.
        let own = |f: &SpanFrame| f.wrap(settings.beta, 0.0);
        let targets: Option<Vec<f64>> = match mean_lines.wrap {
            WrapMode::Common if plan.is_none() => {
                let target = match mean_lines.wrap_angle {
                    Param::Manual(w) => positive("wrap angle", w)?,
                    Param::Auto => frames.iter().map(own).sum::<f64>() / frames.len() as f64,
                };
                Some(vec![target; frames.len()])
            }
            WrapMode::Linear if plan.is_none() => {
                let (first, last) = (&frames[0], &frames[frames.len() - 1]);
                let hub = match mean_lines.wrap_angle {
                    Param::Manual(w) => positive("hub wrap angle", w)?,
                    Param::Auto => own(first),
                };
                let shroud = match mean_lines.wrap_angle_shroud {
                    Param::Manual(w) => positive("shroud wrap angle", w)?,
                    Param::Auto => own(last),
                };
                Some(
                    frames
                        .iter()
                        .map(|f| hub + (shroud - hub) * f.span.fraction)
                        .collect(),
                )
            }
            _ => None,
        };
        let bumps: Vec<f64> = match targets {
            Some(targets) => frames
                .iter()
                .zip(targets)
                .map(|(f, target)| {
                    f.bump_for(settings.beta, target).unwrap_or_else(|nearest| {
                        warnings.push(format!(
                            "the wrap of {:.0}° is out of reach at span {:.2}: the \
                                 section wraps {:.0}°",
                            target.to_degrees(),
                            f.span.fraction,
                            f.wrap(settings.beta, nearest).to_degrees()
                        ));
                        nearest
                    })
                })
                .collect(),
            None => vec![0.0; frames.len()],
        };
        let sections: Vec<BladeSection> = frames
            .iter()
            .zip(bumps)
            .map(|(f, bump)| {
                let settings = SectionSettings {
                    edges: edges.at(f.span.fraction)?,
                    ..settings
                };
                section(f, basis, &settings, plan.as_ref(), bump)
            })
            .collect::<Result<_, _>>()?;
        // A wrap far from what the edge angles give bends the blade into a hump of β.
        // Only an imposed wrap does that; a cylindrical blade turns axial on purpose.
        let imposed =
            plan.is_none() && matches!(mean_lines.wrap, WrapMode::Common | WrapMode::Linear);
        for s in sections.iter().filter(|_| imposed) {
            let (Some(&first), Some(&last)) = (s.beta.first(), s.beta.last()) else {
                continue;
            };
            let peak = s.beta.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if peak - first.max(last) > WRAP_HUMP_WARNING {
                warnings.push(format!(
                    "the wrap of {:.0}° lifts β to {:.0}° at span {:.2}, {:.0}° above its edge \
                     angles: a freer wrap or larger inlet angles keep the blade smoother",
                    s.wrap_angle().to_degrees(),
                    peak.to_degrees(),
                    s.span,
                    (peak - first.max(last)).to_degrees()
                ));
            }
        }
        Ok(Self {
            count: basis.blade_count,
            rotation_sign: basis.rotation_sign,
            outlet_blade_angle: basis.outlet_blade_angle,
            sections,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::meridian::MeridionalInput;
    use crate::impeller::meridian::tests::basis;
    use approx::assert_relative_eq;

    fn blade_with(mean_lines: MeanLineInput, edges: EdgeInput) -> Blade {
        let b = basis();
        let contour = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let channel = Channel::build(&contour, b.flow, 61, 5);
        Blade::build(
            &b,
            &contour,
            &channel,
            &BladePropertiesInput::default(),
            &mean_lines,
            &ProfileInput::default(),
            &edges,
        )
        .unwrap()
    }

    fn reference() -> Blade {
        blade_with(MeanLineInput::default(), EdgeInput::default())
    }

    #[test]
    fn conformal_mapping_of_a_log_spiral() {
        // Radial streamline with constant β: θ = ln(r2/r1)/tan β.
        let beta = 25f64.to_radians();
        let (r1, r2) = (0.04, 0.09);
        let n = 2000;
        let dm = (r2 - r1) / n as f64;
        let theta: f64 = (0..n)
            .map(|i| {
                let ra = r1 + i as f64 * dm;
                let rb = ra + dm;
                0.5 * dm * (1.0 / (ra * beta.tan()) + 1.0 / (rb * beta.tan()))
            })
            .sum();
        assert_relative_eq!(theta, (r2 / r1).ln() / beta.tan(), max_relative = 1e-6);
    }

    #[test]
    fn sections_start_at_leading_edge_and_end_at_outlet() {
        let blade = reference();
        assert_eq!(blade.sections.len(), 5);
        let r2 = basis().outlet_diameter / 2.0;
        for s in &blade.sections {
            assert_eq!(s.m[0], 0.0);
            assert_relative_eq!(s.meridional.last().unwrap().y, r2, max_relative = 1e-9);
            assert_eq!(*s.theta.last().unwrap(), 0.0);
            assert_relative_eq!(*s.beta.last().unwrap(), 22f64.to_radians());
            assert_relative_eq!(s.beta[0], s.leading_edge.blade_angle);
        }
    }

    #[test]
    fn hub_leading_edge_sits_at_a_larger_radius_than_the_eye_hub() {
        let blade = reference();
        let hub = &blade.sections[0];
        assert!(hub.leading_edge.position.y > basis().hub_diameter / 2.0 * 1.2);
        // Angles grow from shroud to hub at the leading edge (lower u at the hub).
        let shroud = blade.sections.last().unwrap();
        assert!(hub.leading_edge.blade_angle > shroud.leading_edge.blade_angle);
    }

    #[test]
    fn wrap_angles_are_typical_for_a_radial_pump() {
        for s in &reference().sections {
            let wrap = s.wrap_angle().to_degrees();
            assert!((60.0..200.0).contains(&wrap), "span {} wrap {wrap}", s.span);
        }
    }

    #[test]
    fn sides_are_one_normal_thickness_apart() {
        // Many points, so that the chords of the faces stay close to the curves.
        let blade = blade_with(
            MeanLineInput {
                points: Param::Manual(161),
                ..Default::default()
            },
            EdgeInput::default(),
        );
        let e = basis().blade_thickness;
        for s in &blade.sections {
            let (ps, ss) = (s.pressure_side(1.0), s.suction_side(1.0));
            // Away from the edges the faces are parallel curves one thickness apart.
            let n = ps.len();
            for &p in &ps[n / 4..3 * n / 4] {
                let gap = ss
                    .windows(2)
                    .map(|w| distance_to_segment(p, w[0], w[1]))
                    .fold(f64::INFINITY, f64::min);
                assert_relative_eq!(gap, e, max_relative = 0.02);
            }
            // The pressure face is ahead in the direction of rotation.
            let k = n / 2;
            let angle = |p: DVec3| p.y.atan2(p.x);
            assert!(angle(ps[k]) > angle(s.mean_line(1.0)[k]));
            assert!(angle(ss[k]) < angle(s.mean_line(1.0)[k]));
        }
    }

    fn distance_to_segment(p: DVec3, a: DVec3, b: DVec3) -> f64 {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        p.distance(a + ab * t)
    }

    #[test]
    fn a_rounded_leading_edge_is_a_symmetric_nose() {
        let blade = reference();
        for s in &blade.sections {
            let mean = s.mean_line(1.0);
            let (ps, ss) = (s.pressure_side(1.0), s.suction_side(1.0));
            // Both faces close on the leading-edge point.
            assert!(ps[0].distance(mean[0]) < 1e-9 && ss[0].distance(mean[0]) < 1e-9);
            // Each pair of face points straddles its mean-line point, half the local
            // thickness to either side, and neither face reaches ahead of the edge.
            let along = (mean[1] - mean[0]).normalize();
            for k in 1..s.m.len() / 4 {
                let half = 0.5 * s.thickness[k];
                assert_relative_eq!(ps[k].distance(mean[k]), half, max_relative = 0.02);
                assert_relative_eq!(ss[k].distance(mean[k]), half, max_relative = 0.02);
                for p in [ps[k], ss[k]] {
                    assert!((p - mean[0]).dot(along) > -0.02 * basis().blade_thickness);
                }
            }
        }
    }

    #[test]
    fn a_blunt_edge_is_cut_on_the_meridional_edge() {
        let blade = blade_with(
            MeanLineInput::default(),
            EdgeInput {
                leading: EdgeShape::Cut,
                trailing: EdgeShape::Cut,
                ..EdgeInput::default()
            },
        );
        let r2 = basis().outlet_diameter / 2.0;
        for s in &blade.sections {
            for face in [&s.pressure, &s.suction] {
                // Trailing edge on the outlet radius, leading edge on the meridional edge.
                assert_relative_eq!(face.meridional.last().unwrap().y, r2, max_relative = 1e-6);
                assert!(face.meridional[0].distance(s.meridional[0]) < 1e-6);
            }
            // The cut spans the thickness across the face: e/sin β along the circumference.
            let n = s.m.len();
            let arc = r2 * (s.pressure.theta[n - 1] - s.suction.theta[n - 1]);
            assert_relative_eq!(
                arc * s.beta[n - 1].sin(),
                basis().blade_thickness,
                max_relative = 0.02
            );
        }
    }

    #[test]
    fn edges_shape_the_thickness() {
        let blade = reference();
        let s = &blade.sections[0];
        assert_eq!(s.thickness[0], 0.0);
        assert_relative_eq!(*s.thickness.last().unwrap(), basis().blade_thickness);
        let blunt = blade_with(
            MeanLineInput::default(),
            EdgeInput {
                leading: EdgeShape::Cut,
                trailing: EdgeShape::Ellipse { ratio: 1.0 },
                ..EdgeInput::default()
            },
        );
        let s = &blunt.sections[0];
        assert_relative_eq!(s.thickness[0], basis().blade_thickness);
        assert_eq!(*s.thickness.last().unwrap(), 0.0);
    }

    #[test]
    fn edges_can_change_from_hub_to_shroud() {
        let blade = blade_with(
            MeanLineInput::default(),
            EdgeInput {
                leading: EdgeShape::Ellipse { ratio: 2.0 },
                leading_shroud: Some(EdgeShape::Ellipse { ratio: 6.0 }),
                ..EdgeInput::default()
            },
        );
        // The nose is longer toward the shroud: at the same distance from the edge the
        // blade is thinner there.
        let thin = |s: &BladeSection| s.thickness[2] / s.thickness.last().unwrap();
        let (hub, shroud) = (&blade.sections[0], blade.sections.last().unwrap());
        assert!(
            thin(hub) > thin(shroud),
            "{} vs {}",
            thin(hub),
            thin(shroud)
        );
        let mixed = EdgeInput {
            leading_shroud: Some(EdgeShape::Cut),
            ..EdgeInput::default()
        };
        let b = basis();
        let contour = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let channel = Channel::build(&contour, b.flow, 61, 5);
        let built = Blade::build(
            &b,
            &contour,
            &channel,
            &BladePropertiesInput::default(),
            &MeanLineInput::default(),
            &ProfileInput::default(),
            &mixed,
        );
        assert!(built.is_err());
    }

    #[test]
    fn an_asymmetric_nose_closes_off_the_mean_line() {
        let lean = EdgeShape::Bezier {
            start: [2.0, 2.0],
            pull: [0.6, 0.6],
            tip: 0.5,
            symmetric: false,
        };
        let blade = blade_with(
            MeanLineInput::default(),
            EdgeInput {
                leading: lean,
                ..EdgeInput::default()
            },
        );
        for s in &blade.sections {
            let (ps, ss) = (s.pressure_side(1.0), s.suction_side(1.0));
            let mean = s.mean_line(1.0);
            // Both faces close on the tip of the nose, off the mean line.
            assert!(ps[0].distance(ss[0]) < 1e-9);
            let e = basis().blade_thickness;
            let tip = EdgeProfile::new(lean, e).unwrap().faces()[0][0];
            assert!(tip.y > 0.1);
            assert_relative_eq!(
                ps[0].distance(mean[0]),
                tip.y * e / 2.0,
                max_relative = 0.02
            );
            assert!(s.thickness[0].abs() < 1e-12);
        }
    }

    #[test]
    fn overlapping_edges_are_rejected() {
        let b = basis();
        let contour = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let channel = Channel::build(&contour, b.flow, 61, 5);
        let long = EdgeShape::Ellipse { ratio: 60.0 };
        let built = Blade::build(
            &b,
            &contour,
            &channel,
            &BladePropertiesInput::default(),
            &MeanLineInput::default(),
            &ProfileInput::default(),
            &EdgeInput {
                leading: long,
                trailing: long,
                ..EdgeInput::default()
            },
        );
        assert!(matches!(built, Err(DesignError::Infeasible(m)) if m.contains("overlap")));
    }

    #[test]
    fn smooth_distribution_keeps_edge_angles() {
        let blade = blade_with(
            MeanLineInput {
                beta: BetaDistribution::Smooth {
                    inlet: 0.3,
                    outlet: 0.3,
                },
                wrap: WrapMode::Free,
                ..Default::default()
            },
            EdgeInput::default(),
        );
        let s = &blade.sections[4];
        assert_relative_eq!(s.beta[0], s.leading_edge.blade_angle, epsilon = 1e-9);
        assert_relative_eq!(*s.beta.last().unwrap(), 22f64.to_radians(), epsilon = 1e-9);
        // Monotone between the edge values.
        let (lo, hi) = (22f64.to_radians(), s.beta[0]);
        assert!(s.beta.iter().all(|&b| b >= lo - 1e-9 && b <= hi + 1e-9));
    }

    #[test]
    fn a_linear_wrap_goes_from_hub_to_shroud() {
        let free = blade_with(
            MeanLineInput {
                wrap: WrapMode::Free,
                ..Default::default()
            },
            EdgeInput::default(),
        );
        let linear = |hub: Param<f64>, shroud: Param<f64>| {
            blade_with(
                MeanLineInput {
                    wrap: WrapMode::Linear,
                    wrap_angle: hub,
                    wrap_angle_shroud: shroud,
                    ..Default::default()
                },
                EdgeInput::default(),
            )
        };
        let (hub, shroud) = (120f64.to_radians(), 125f64.to_radians());
        let blade = linear(Param::Manual(hub), Param::Manual(shroud));
        assert!(blade.warnings.is_empty(), "{:?}", blade.warnings);
        for (s, f) in blade.sections.iter().zip(&free.sections) {
            let expected = hub + (shroud - hub) * s.span;
            assert_relative_eq!(s.wrap_angle(), expected, max_relative = 1e-6);
            // The edge angles stay those of the section.
            assert_relative_eq!(s.beta[0], f.beta[0], epsilon = 1e-12);
        }
        // Automatic ends keep the hub and shroud sections as they are.
        let own = linear(Param::Auto, Param::Auto);
        let (first, last) = (&free.sections[0], free.sections.last().unwrap());
        assert_relative_eq!(
            own.sections[0].wrap_angle(),
            first.wrap_angle(),
            max_relative = 1e-6
        );
        assert_relative_eq!(
            own.sections.last().unwrap().wrap_angle(),
            last.wrap_angle(),
            max_relative = 1e-6
        );
    }

    #[test]
    fn a_wrap_that_bends_the_blade_is_flagged() {
        let wrapped = |degrees: f64| {
            blade_with(
                MeanLineInput {
                    wrap_angle: Param::Manual(degrees.to_radians()),
                    ..Default::default()
                },
                EdgeInput::default(),
            )
        };
        let hump = |b: &Blade| b.warnings.iter().any(|w| w.contains("lifts β"));
        // The reference sections wrap about 120° on their own: that keeps β smooth.
        assert!(!hump(&wrapped(120.0)));
        // Far less needs a hump of β well above its edge angles.
        let bent = wrapped(70.0);
        assert!(hump(&bent), "{:?}", bent.warnings);
        let rise = |s: &BladeSection| {
            let peak = s.beta.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            peak - s.beta[0].max(*s.beta.last().unwrap())
        };
        assert!(bent.sections.iter().any(|s| rise(s) > WRAP_HUMP_WARNING));
        // A free wrap never bends the blade.
        let free = blade_with(
            MeanLineInput {
                wrap: WrapMode::Free,
                ..Default::default()
            },
            EdgeInput::default(),
        );
        assert!(!hump(&free));
    }

    #[test]
    fn a_uniform_inlet_velocity_sets_every_leading_edge_with_the_eye_velocity() {
        let b = basis();
        let contour = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let channel = Channel::build(&contour, b.flow, 61, 5);
        let blade = Blade::build(
            &b,
            &contour,
            &channel,
            &BladePropertiesInput {
                inlet_velocity: InletVelocity::Uniform,
                ..Default::default()
            },
            &MeanLineInput::default(),
            &ProfileInput::default(),
            &EdgeInput::default(),
        )
        .unwrap();
        let eye = PI / 4.0 * (b.inlet_diameter.powi(2) - b.hub_diameter.powi(2));
        for s in &blade.sections {
            let le = &s.leading_edge;
            assert_relative_eq!(le.cm, b.flow / eye, max_relative = 1e-12);
            // Gülich eq. T7.1.7: tan β1B = τ1·c1m/u1 with incidence 0.
            let u = b.omega * le.position.y;
            assert_relative_eq!(
                le.blade_angle.tan(),
                le.blockage * le.cm / u,
                max_relative = 1e-9
            );
        }
        // With constant c1m and τ1 the angles follow r·tan β1B ≈ constant (eq. T7.1.7):
        // the hub angle is the steepest.
        let angles: Vec<f64> = blade
            .sections
            .iter()
            .map(|s| s.leading_edge.blade_angle)
            .collect();
        assert!(angles.windows(2).all(|w| w[0] >= w[1]), "{angles:?}");
    }

    #[test]
    fn a_common_wrap_lines_up_the_leading_edge() {
        let free = blade_with(
            MeanLineInput {
                wrap: WrapMode::Free,
                ..Default::default()
            },
            EdgeInput::default(),
        );
        let common = reference();
        let wraps: Vec<f64> = free.sections.iter().map(BladeSection::wrap_angle).collect();
        let mean = wraps.iter().sum::<f64>() / wraps.len() as f64;
        assert!(wraps.iter().any(|w| (w - mean).abs() > 1f64.to_radians()));
        for (c, f) in common.sections.iter().zip(&free.sections) {
            // The mean of the free wraps, on every section, with the edge angles kept.
            assert_relative_eq!(c.wrap_angle(), mean, max_relative = 1e-6);
            assert_relative_eq!(c.beta[0], f.beta[0], epsilon = 1e-12);
            assert_relative_eq!(
                *c.beta.last().unwrap(),
                *f.beta.last().unwrap(),
                epsilon = 1e-12
            );
        }
        assert!(common.warnings.is_empty());
        // A manual wrap is met too, and one out of reach is reported.
        let wrapped = |angle: f64| {
            blade_with(
                MeanLineInput {
                    wrap_angle: Param::Manual(angle.to_radians()),
                    ..Default::default()
                },
                EdgeInput::default(),
            )
        };
        let b = wrapped(150.0);
        for s in &b.sections {
            assert_relative_eq!(s.wrap_angle().to_degrees(), 150.0, max_relative = 1e-6);
        }
        assert!(b.warnings.is_empty());
        let far = wrapped(5.0);
        assert!(far.warnings.iter().any(|w| w.contains("out of reach")));
    }

    #[test]
    fn rotation_mirrors_the_blade() {
        let s = &reference().sections[2];
        let a = s.mean_line(1.0);
        let b = s.mean_line(-1.0);
        for (p, q) in a.iter().zip(&b) {
            assert_relative_eq!(p.x, q.x, epsilon = 1e-15);
            assert_relative_eq!(p.y, -q.y, epsilon = 1e-15);
        }
    }

    #[test]
    fn thick_blades_are_rejected() {
        let b = basis();
        let contour = MeridionalContour::build(&b, &MeridionalInput::default()).unwrap();
        let channel = Channel::build(&contour, b.flow, 61, 5);
        let profiles = ProfileInput {
            thickness_leading: Param::Manual(0.03),
            thickness_trailing: Param::Manual(0.03),
        };
        let err = Blade::build(
            &b,
            &contour,
            &channel,
            &BladePropertiesInput::default(),
            &MeanLineInput::default(),
            &profiles,
            &EdgeInput::default(),
        );
        assert!(err.is_err());
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn leading_edge_pressure_coefficient_matches_gulich_examples() {
        // Gülich eq. 7.1a: semi-circle (e/L_p = 2) gives 2.05, ellipse e/L_p = 0.2 gives 0.155.
        let semi_circle = EdgeShape::Ellipse { ratio: 1.0 };
        assert_relative_eq!(
            semi_circle.min_pressure_coefficient().unwrap(),
            2.05,
            epsilon = 5e-3
        );
        let long = EdgeShape::Ellipse { ratio: 10.0 };
        assert_relative_eq!(
            long.min_pressure_coefficient().unwrap(),
            0.155,
            epsilon = 5e-4
        );
        let default = EdgeInput::default().leading;
        assert_relative_eq!(
            default.min_pressure_coefficient().unwrap(),
            0.408,
            epsilon = 1e-3
        );
        assert_eq!(EdgeShape::Cut.min_pressure_coefficient(), None);
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// The plan-view blade angle β(r) of `shape` through (r₁, β₁) and (r₂, β₂).
    pub(crate) fn plan(
        shape: BladeShape,
        reference: (f64, f64),
        r2: f64,
        beta2: f64,
    ) -> impl Fn(f64) -> f64 {
        let view = PlanView::build(
            shape,
            BetaDistribution::Linear,
            reference,
            r2,
            beta2,
            reference.0,
        )
        .unwrap();
        move |r| view.at(r).1
    }
}
