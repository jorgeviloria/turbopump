//! Blades of a radial impeller: properties per span, mean lines, thickness and edges.
//!
//! Each blade section lies on a streamline surface of revolution (a span). Its mean
//! line is defined by the blade angle β along the normalized meridional length m̄ and
//! integrated by conformal mapping, dθ = dm / (r·tan β). θ is measured from the
//! trailing edge in the direction of rotation, so all sections share θ = 0 at the
//! trailing edge (an axial trailing edge) and the wrap angle is θ at the leading edge.

use std::f64::consts::{FRAC_PI_2, PI};

use serde::{Deserialize, Serialize};
use tp_math::{Bezier, DVec2, DVec3, Polyline};

use super::ImpellerBasis;
use super::meridian::{Channel, MeridionalContour};
use crate::triangle::blockage_factor;
use crate::{DesignError, Param};

/// Inputs of the blade-properties step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BladePropertiesInput {
    /// Number of blade sections from hub to shroud (≥ 2).
    pub spans: Param<u32>,
    pub shape: BladeShape,
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
}

/// Inputs of the blade-profile step: normal blade thickness, varying linearly from
/// leading to trailing edge before the edges are shaped. Default: the thickness of the
/// main-dimensions step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileInput {
    pub thickness_leading: Param<f64>,
    pub thickness_trailing: Param<f64>,
}

/// Shape of a blade edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EdgeShape {
    /// Blunt: full thickness up to the edge.
    Cut,
    /// Elliptic nose whose length along the mean line is `ratio` times the half
    /// thickness (1 = round).
    Ellipse { ratio: f64 },
}

/// Inputs of the blade-edge step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EdgeInput {
    pub leading: EdgeShape,
    pub trailing: EdgeShape,
}

impl EdgeShape {
    /// Minimum pressure coefficient c_p,min = Δp_min/(½ρw₁²) of the leading-edge
    /// profile at shockless entry: c_p = 2x + x² with x = 0.373·e/L_p, L_p being the
    /// length of the nose (Gülich 2nd ed., eq. 7.1a). `None` for a blunt edge.
    pub fn min_pressure_coefficient(&self) -> Option<f64> {
        match *self {
            EdgeShape::Cut => None,
            EdgeShape::Ellipse { ratio } => {
                let x = 0.373 * 2.0 / ratio;
                Some(2.0 * x + x * x)
            }
        }
    }
}

impl Default for EdgeInput {
    /// Elliptic 4:1 leading edge (c_p,min ≈ 0.41) and full thickness to the trailing
    /// edge, which leaves room for under-filing (Gülich 2nd ed., pp. 347–349).
    fn default() -> Self {
        Self {
            leading: EdgeShape::Ellipse { ratio: 4.0 },
            trailing: EdgeShape::Cut,
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
    pub leading_edge: LeadingEdge,
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

    /// Angular half-thickness offset of each station: (e/2) / (r·sin β).
    fn half_angle(&self, k: usize) -> f64 {
        0.5 * self.thickness[k] / (self.meridional[k].y * self.beta[k].sin())
    }

    fn to_cartesian(&self, sign: f64, offset: impl Fn(usize) -> f64) -> Vec<DVec3> {
        (0..self.m.len())
            .map(|k| {
                let p = self.meridional[k];
                let phi = sign * (self.theta[k] + offset(k));
                DVec3::new(p.y * phi.cos(), p.y * phi.sin(), p.x)
            })
            .collect()
    }

    /// Mean line in Cartesian coordinates (x, y, z), z along the axis. `sign` is +1
    /// when the impeller turns in the positive sense about +z.
    pub fn mean_line(&self, sign: f64) -> Vec<DVec3> {
        self.to_cartesian(sign, |_| 0.0)
    }

    /// Pressure side: the blade face that faces the direction of rotation.
    pub fn pressure_side(&self, sign: f64) -> Vec<DVec3> {
        self.to_cartesian(sign, |k| self.half_angle(k))
    }

    /// Suction side: the face turned away from the direction of rotation.
    pub fn suction_side(&self, sign: f64) -> Vec<DVec3> {
        self.to_cartesian(sign, |k| -self.half_angle(k))
    }
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

/// Thickness reduction factor of an edge at distance `s` from it along the mean line.
pub(crate) fn edge_factor(shape: EdgeShape, thickness: f64, s: f64) -> f64 {
    match shape {
        EdgeShape::Cut => 1.0,
        EdgeShape::Ellipse { ratio } => {
            let a = ratio * thickness / 2.0;
            if s >= a {
                1.0
            } else {
                let x = (a - s.max(0.0)) / a;
                (1.0 - x * x).max(0.0).sqrt()
            }
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

fn section(
    basis: &ImpellerBasis,
    contour: &MeridionalContour,
    channel: &Channel,
    span_index: usize,
    settings: &SectionSettings,
    plan: Option<&PlanView>,
) -> Result<BladeSection, DesignError> {
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
        channel.cm_at(t_le),
        settings.thickness_leading,
    );
    if !le.blockage.is_finite() {
        return Err(DesignError::Infeasible(format!(
            "at span {:.2} the blades fill the whole leading-edge circumference",
            span.fraction
        )));
    }
    let beta2 = basis.outlet_blade_angle_at(line.point_at_length(s_te).y)?;
    let (beta1, beta2) = (le.blade_angle, beta2);
    let beta = |mbar: f64| beta_at(settings.beta, beta1, beta2, mbar);

    // Conformal mapping on a fine grid: θ(m) = ∫ dm / (r tan β), from the TE backwards.
    let fine: Vec<(f64, f64)> = (0..=INTEGRATION_STEPS)
        .map(|i| {
            let mbar = i as f64 / INTEGRATION_STEPS as f64;
            let r = line.point_at_length(s_le + mbar * meridional_length).y;
            (mbar, 1.0 / (r * beta(mbar).tan()))
        })
        .collect();
    let mut theta_fine = vec![0.0; fine.len()];
    let dm = meridional_length / INTEGRATION_STEPS as f64;
    for i in (0..INTEGRATION_STEPS).rev() {
        theta_fine[i] = theta_fine[i + 1] + 0.5 * dm * (fine[i].1 + fine[i + 1].1);
    }
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
                let dr_dm = radial_slope(&line, s_le + mm);
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
    let thickness: Vec<f64> = mbars
        .iter()
        .zip(&along)
        .map(|(&mbar, &s)| {
            let e = e1 + (e2 - e1) * mbar;
            e * edge_factor(settings.edges.0, e1, s) * edge_factor(settings.edges.1, e2, total - s)
        })
        .collect();

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
        leading_edge: le,
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
    thickness: f64,
) -> (LeadingEdge, f64) {
    let span = &channel.spans[index];
    let line = Polyline::new(span.points.clone()).expect("channel spans have points");
    let t_le = contour.leading_edge_on(span.fraction, &span.points);
    let s = length_at(&line, t_le);
    let position = line.point_at_length(s);
    (
        leading_edge(basis, position, channel.cm_at(t_le), thickness),
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
        .map(|j| span_leading_edge(basis, contour, channel, j, settings.thickness_leading))
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
        shape: BladeShape,
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
        for edge in [edges.leading, edges.trailing] {
            if let EdgeShape::Ellipse { ratio } = edge
                && !(ratio.is_finite() && ratio > 0.0)
            {
                return Err(DesignError::InvalidInput(format!(
                    "edge ellipse ratio must be positive, got {ratio}"
                )));
            }
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
        };
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
        let sections = (0..channel.spans.len())
            .map(|j| section(basis, contour, channel, j, &settings, plan.as_ref()))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            count: basis.blade_count,
            rotation_sign: basis.rotation_sign,
            outlet_blade_angle: basis.outlet_blade_angle,
            sections,
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
            BladeShape::FreeForm,
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
        let blade = reference();
        let s = &blade.sections[2];
        let k = s.m.len() / 2;
        let ps = s.pressure_side(1.0)[k];
        let ss = s.suction_side(1.0)[k];
        // Circumferential distance times sin β is the normal thickness.
        let r = s.meridional[k].y;
        let arc = r * (ps.y.atan2(ps.x) - ss.y.atan2(ss.x));
        assert_relative_eq!(
            arc * s.beta[k].sin(),
            basis().blade_thickness,
            max_relative = 1e-9
        );
        // Pressure side ahead in the direction of rotation.
        assert!(arc > 0.0);
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
            },
        );
        let s = &blunt.sections[0];
        assert_relative_eq!(s.thickness[0], basis().blade_thickness);
        assert_eq!(*s.thickness.last().unwrap(), 0.0);
    }

    #[test]
    fn smooth_distribution_keeps_edge_angles() {
        let blade = blade_with(
            MeanLineInput {
                beta: BetaDistribution::Smooth {
                    inlet: 0.3,
                    outlet: 0.3,
                },
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
            BladeShape::FreeForm,
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
