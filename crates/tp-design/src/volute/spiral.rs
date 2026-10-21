//! The spiral development: how the cross sections grow from the start of the spiral to
//! the throat.
//!
//! Each section at wrap angle φ from the start of the spiral collects the flow
//! Q(φ) = Q₀ + Q_i·φ/2π: the impeller delivers Q_i evenly round its circumference
//! (Gülich eq. 7.25), and Q₀ is a blind flow already in the spiral at its start. The
//! velocity rules size the section for that flow:
//! - Pfleiderer's swirl c_u·r^x = c_u,4·r₄^x carries Q(φ) = c_u,4·∬(r₄/r)^x dA through
//!   the section; x = 1 conserves the angular momentum, Gülich eq. 7.24, and x = 0
//!   keeps the velocity c_u,4 of the volute inlet r₄ = r'_z;
//! - a constant velocity c gives A = Q(φ)/c (Gülich eqs. 7.26 and 7.27 after
//!   Stepanoff, with c from the throat by angular momentum; or Stepanoff eq. 7.1,
//!   c₃ = K₃·√(2gH)).
//!
//! The geometric rules let the height H, the area A or A/r_c grow to an end value along
//! a progression instead.
//!
//! The arcs of [`fit_arcs`] approximate the outer contour of a spiral of straight-walled
//! sections by a chain of tangent circular arcs, as volutes are laid out on the drawing
//! board (our construction).

use std::f64::consts::TAU;

use tp_math::{DVec2, optim};

use super::section::{CrossSections, Measure, SectionProperties};
use super::{AreaLaw, Progression, SectionSize, size_of};
use crate::DesignError;

/// The cutwater compensation, resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Compensation {
    /// Rise of the inner radius at the end of the spiral, m.
    pub thickness: f64,
    /// Wrap angle φ_C from the start of the spiral where the rise starts, rad.
    pub start: f64,
}

/// A circular arc of the outer contour in the plane of the volute: x points from the
/// pump axis to the cutwater and wrap angles grow counter-clockwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub center: DVec2,
    pub radius: f64,
    /// Ends on the contour of the rule, in the order of the spiral.
    pub start: DVec2,
    pub end: DVec2,
    /// Whether the arc turns counter-clockwise about its center.
    pub counter_clockwise: bool,
}

impl Arc {
    /// Wrap angles from the cutwater of its ends, rad.
    pub fn angles(&self) -> (f64, f64) {
        let angle = |p: DVec2| p.y.atan2(p.x).rem_euclid(TAU);
        let (from, to) = (angle(self.start), angle(self.end));
        (from, if to <= from { to + TAU } else { to })
    }

    /// Turning angle of the arc about its center, rad.
    pub fn sweep(&self) -> f64 {
        let (a, b) = (self.start - self.center, self.end - self.center);
        let turn = a.perp_dot(b).atan2(a.dot(b)).rem_euclid(TAU);
        if self.counter_clockwise {
            turn
        } else {
            TAU - turn
        }
    }

    /// Distance from the pump axis where the ray at `angle` meets the arc.
    fn hit(&self, angle: f64) -> Option<f64> {
        let u = DVec2::from_angle(angle);
        let b = u.dot(self.center);
        let disc = b * b - self.center.length_squared() + self.radius * self.radius;
        if disc < 0.0 {
            return None;
        }
        let root = disc.sqrt();
        let on_arc = |s: f64| {
            let p = u * s - self.center;
            let a = self.start - self.center;
            let turn = a.perp_dot(p).atan2(a.dot(p)).rem_euclid(TAU);
            let turn = if self.counter_clockwise {
                turn
            } else {
                (TAU - turn) % TAU
            };
            turn <= self.sweep() + 1e-9
        };
        [b + root, b - root]
            .into_iter()
            .filter(|&s| s > 0.0)
            .find(|&s| on_arc(s))
            .or((b + root > 0.0).then_some(b + root))
    }
}

/// The outer contour as tangent circular arcs.
#[derive(Debug, Clone, PartialEq)]
pub struct SpiralArcs {
    pub arcs: Vec<Arc>,
    /// Largest radial distance between the arcs and the contour of the rule, m.
    pub max_deviation: f64,
}

impl SpiralArcs {
    /// Outer radius of the arcs at wrap angle `angle` from the cutwater.
    pub fn radius_at(&self, angle: f64) -> Option<f64> {
        let arc = self
            .arcs
            .iter()
            .find(|a| angle <= a.angles().1 + 1e-12)
            .or(self.arcs.last())?;
        arc.hit(angle)
    }
}

/// The rule of the spiral development, resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct SpiralDevelopment {
    /// The law used (never `Auto`).
    pub law: AreaLaw,
    /// Wrap angle φ_wrap from the start of the spiral to the throat, rad.
    pub wrap: f64,
    /// Wrap angle from the cutwater where the spiral starts, 2π − φ_wrap, rad.
    pub start: f64,
    /// Radius r₄ = r'_z of the reference velocity, m.
    pub reference_radius: f64,
    /// c_u,4 at r₄, m/s: of the swirl rules, and of the throat that the rule of Gülich
    /// and the automatic end values take from angular momentum.
    pub reference_velocity: f64,
    /// Mean velocity c of the constant-velocity rules, m/s; 0 for the others.
    pub velocity: f64,
    /// Stepanoff's K₃, when c₃ comes from it.
    pub velocity_factor: Option<f64>,
    /// Q_i, m³/s
    pub reference_flow: f64,
    /// Q₀, m³/s
    pub blind_flow: f64,
    /// Exponent x of the swirl c_u·r^x.
    pub exponent: f64,
    /// End value of a geometric rule: m, m² or m; 0 for the velocity rules.
    pub end_value: f64,
    pub progression: Progression,
    /// Height added to the end section for the cutwater (Gülich eq. 7.28), growing
    /// linearly from the start of the spiral, m.
    pub correction: f64,
    pub compensation: Option<Compensation>,
    pub arcs: Option<SpiralArcs>,
}

impl SpiralDevelopment {
    /// Wrap angle from the start of the spiral of the wrap angle `angle` from the
    /// cutwater, inside the spiral.
    pub fn phi(&self, angle: f64) -> f64 {
        (angle - self.start).clamp(0.0, self.wrap)
    }

    /// Flow Q(φ) = Q₀ + Q_i·φ/2π through the section at `phi` from the start, m³/s.
    pub fn flow_at(&self, phi: f64) -> f64 {
        self.blind_flow + self.reference_flow * phi / TAU
    }

    /// Rise of the inner radius at `phi` from the start, m.
    pub fn lift_at(&self, phi: f64) -> f64 {
        self.compensation.map_or(0.0, |c| {
            c.thickness * ((phi - c.start) / (self.wrap - c.start)).clamp(0.0, 1.0)
        })
    }

    /// Height of the section of the rule at wrap angle `angle` from the cutwater,
    /// raised by `lift`.
    fn rule_height(
        &self,
        sections: &CrossSections,
        angle: f64,
        lift: f64,
    ) -> Result<f64, DesignError> {
        if angle < self.start {
            return Ok(0.0);
        }
        let phi = self.phi(angle);
        let fraction = phi / self.wrap;
        let flow = self.flow_at(phi);
        let along = |measure: Measure| {
            let target = self.end_value * self.progression.value(fraction);
            sections.height_for(angle, lift, measure, target)
        };
        let height = match self.law {
            AreaLaw::Auto | AreaLaw::AngularMomentum => sections.height_for(
                angle,
                lift,
                Measure::Swirl {
                    exponent: self.exponent,
                    reference_radius: self.reference_radius,
                },
                flow / self.reference_velocity,
            )?,
            AreaLaw::ConstantVelocity | AreaLaw::Stepanoff => {
                sections.height_for(angle, lift, Measure::Area, flow / self.velocity)?
            }
            AreaLaw::Height => self.end_value * self.progression.value(fraction),
            AreaLaw::Area => along(Measure::Area)?,
            AreaLaw::AreaRadius => along(Measure::AreaOverRadius)?,
        };
        Ok(height + self.correction * fraction)
    }

    /// The section at wrap angle `angle` from the cutwater.
    pub fn size(&self, sections: &CrossSections, angle: f64) -> Result<SectionSize, DesignError> {
        let lift = self.lift_at(self.phi(angle));
        let arc_radius = self
            .arcs
            .as_ref()
            .filter(|_| angle >= self.start)
            .and_then(|a| a.radius_at(angle));
        let height = match arc_radius {
            Some(r) => sections.height_for(angle, lift, Measure::OuterRadius, r)?,
            None => self.rule_height(sections, angle, lift)?,
        };
        let p = sections.section_properties(angle, height, lift)?;
        Ok(SectionSize {
            lift,
            ..size_of(&p)
        })
    }

    /// Properties of the section at `angle`, the strip of the compensation included.
    pub fn properties(
        &self,
        sections: &CrossSections,
        angle: f64,
    ) -> Result<SectionProperties, DesignError> {
        let size = self.size(sections, angle)?;
        sections.section_properties(angle, size.height, size.lift)
    }
}

/// Stepanoff's volute velocity constant K₃ = c₃/√(2gH) (A. J. Stepanoff, *Centrifugal
/// and Axial Flow Pumps*, 2nd ed., Wiley 1957, eq. 7.1), read off his Fig. 7.5: a
/// straight line on its log–log scales, K₃ = 4.144·n_s^−0.325, between n_s = 600 and
/// 15 000 in US units (rpm, gpm, ft), with n_s = 51.64·nq. Outside that range the
/// chart's ends hold. The fit of the line is our reading of the chart.
pub fn stepanoff_velocity_factor(nq: f64) -> f64 {
    let ns = (US_SPECIFIC_SPEED * nq).clamp(600.0, 15_000.0);
    4.144 * ns.powf(-0.325)
}

/// n_s in US units (rpm, gpm, ft) over nq (rpm, m³/s, m).
pub const US_SPECIFIC_SPEED: f64 = 51.64;

/// Arcs per spiral: one cannot turn all round from the start to the throat.
pub const MIN_ARCS: u32 = 2;
pub const MAX_ARCS: u32 = 8;

/// Approximates the outer contour of the spiral by `count` tangent circular arcs: they
/// start at the start of the spiral and end at the throat, and meet on the contour.
/// The direction at the start and the angles where the arcs meet are chosen for the
/// least largest radial distance to the contour (our construction).
pub fn fit_arcs(
    development: &SpiralDevelopment,
    sections: &CrossSections,
    count: u32,
) -> Result<SpiralArcs, DesignError> {
    const SAMPLES: usize = 120;
    let first = development.start;
    let span = TAU - first;
    let angles: Vec<f64> = (0..=SAMPLES)
        .map(|i| first + span * i as f64 / SAMPLES as f64)
        .collect();
    let radii = angles
        .iter()
        .map(|&a| Ok(development.size(sections, a)?.outer_radius))
        .collect::<Result<Vec<f64>, DesignError>>()?;
    let radius_at = |angle: f64| {
        let x = ((angle - first) / span * SAMPLES as f64).clamp(0.0, SAMPLES as f64);
        let i = (x.floor() as usize).min(SAMPLES - 1);
        let t = x - i as f64;
        radii[i] + (radii[i + 1] - radii[i]) * t
    };
    let point = |angle: f64| DVec2::from_angle(angle) * radius_at(angle);
    let start = point(first);
    let tangent = (point(first + 1e-3) - start).normalize();
    let n = count as usize;
    let chain = |x: &[f64]| -> Option<Vec<Arc>> {
        let mut cuts: Vec<f64> = x[1..].iter().map(|f| f.clamp(0.01, 0.99)).collect();
        cuts.sort_by(f64::total_cmp);
        let mut points = vec![start];
        points.extend(cuts.iter().map(|f| point(first + span * f)));
        points.push(point(TAU));
        let mut t = DVec2::from_angle(x[0]).rotate(tangent);
        let mut arcs = Vec::with_capacity(n);
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let d = b - a;
            let normal = t.perp();
            let across = normal.dot(d);
            if across.abs() < 1e-9 * d.length() {
                return None;
            }
            // The circle tangent to t at a through b, its radius signed to the left.
            let r = d.length_squared() / (2.0 * across);
            let center = a + normal * r;
            arcs.push(Arc {
                center,
                radius: r.abs(),
                start: a,
                end: b,
                counter_clockwise: r > 0.0,
            });
            t = (b - center).perp() / r;
        }
        Some(arcs)
    };
    let deviation = |arcs: &SpiralArcs| -> f64 {
        angles
            .iter()
            .zip(&radii)
            .map(|(&a, &r)| arcs.radius_at(a).map_or(f64::INFINITY, |s| (s - r).abs()))
            .fold(0.0, f64::max)
    };
    let objective = |x: &[f64]| -> f64 {
        chain(x).map_or(f64::INFINITY, |arcs| {
            deviation(&SpiralArcs {
                arcs,
                max_deviation: 0.0,
            })
        })
    };
    let mut x: Vec<f64> = std::iter::once(0.0)
        .chain((1..n).map(|k| k as f64 / n as f64))
        .collect();
    let steps: Vec<f64> = std::iter::once(0.05)
        .chain((1..n).map(|_| 0.5 / n as f64))
        .collect();
    let scale = radii.iter().copied().fold(0.0, f64::max);
    for _ in 0..3 {
        x = optim::nelder_mead(objective, &x, &steps, 1e-9 * scale, 400 * n).0;
    }
    let arcs = chain(&x).ok_or_else(|| {
        DesignError::Infeasible("no chain of circular arcs follows this spiral".into())
    })?;
    let mut result = SpiralArcs {
        arcs,
        max_deviation: 0.0,
    };
    result.max_deviation = deviation(&result);
    if !result.max_deviation.is_finite() {
        return Err(DesignError::Infeasible(
            "no chain of circular arcs follows this spiral".into(),
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::tests::reference_basis;
    use super::super::{
        CrossSectionInput, SectionShape, SpiralInput, VoluteDesign, VoluteGeometry, VoluteInput,
    };
    use super::*;
    use crate::Param;
    use approx::assert_relative_eq;
    use std::f64::consts::PI;

    fn design(shape: SectionShape, spiral: SpiralInput) -> VoluteDesign {
        let input = VoluteInput {
            cross_section: CrossSectionInput::single(shape),
            spiral,
            ..VoluteInput::default()
        };
        VoluteDesign::new(&reference_basis(), &input).unwrap()
    }

    fn plain(law: AreaLaw) -> SpiralInput {
        SpiralInput {
            law,
            cutwater_correction: false,
            ..SpiralInput::default()
        }
    }

    #[test]
    fn swirl_area_matches_quadrature_of_the_width() {
        let d = design(SectionShape::round(), plain(AreaLaw::AngularMomentum));
        let (rz, h) = (d.stagnation_radius, 0.04);
        let outline = d.cross_sections.outline(1.0, h);
        let p = &outline.points;
        // Width b(r): the length of the circle of radius r inside the outline.
        let width = |r: f64| {
            let y = r - rz;
            let mut crossings: Vec<f64> = (0..p.len())
                .filter_map(|i| {
                    let (a, b) = (p[i], p[(i + 1) % p.len()]);
                    ((a.y <= y) != (b.y <= y)).then(|| a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y))
                })
                .collect();
            crossings.sort_by(f64::total_cmp);
            crossings.chunks(2).map(|c| c[1] - c[0]).sum::<f64>()
        };
        let r4 = rz;
        for x in [0.0, 0.35, 1.0, 1.6] {
            let exact = outline.swirl_area(rz, r4, x).unwrap();
            let numeric =
                tp_math::quad::gauss_legendre(|r| width(r) * (r4 / r).powf(x), rz, rz + h, 400);
            assert_relative_eq!(exact, numeric, max_relative = 1e-4);
        }
    }

    #[test]
    fn pfleiderer_with_x_one_conserves_angular_momentum() {
        let d = design(SectionShape::default(), plain(AreaLaw::AngularMomentum));
        let b = reference_basis();
        // r₄·∬dA/r = Q/c_u,4 with c_u,4 = c₂u·r₂/r₄: ∬dA/r = Q/(c₂u·r₂), eq. 7.24.
        assert_relative_eq!(
            d.momentum_integral(TAU, d.throat.height),
            d.flow / (b.cu * b.impeller_diameter / 2.0),
            max_relative = 1e-9
        );
        let half = d.section(PI_HALF_WRAP).unwrap();
        assert_relative_eq!(
            d.momentum_integral(PI_HALF_WRAP, half.height),
            0.5 * d.flow / (b.cu * b.impeller_diameter / 2.0),
            max_relative = 1e-9
        );
    }

    const PI_HALF_WRAP: f64 = PI;

    #[test]
    fn pfleiderer_with_x_zero_keeps_the_inlet_velocity() {
        let mut spiral = plain(AreaLaw::AngularMomentum);
        spiral.exponent = Param::Manual(0.0);
        let d = design(SectionShape::default(), spiral);
        let c4 = d.development.reference_velocity;
        let b = reference_basis();
        assert_relative_eq!(
            c4,
            b.cu * b.impeller_diameter / 2.0 / d.stagnation_radius,
            max_relative = 1e-12
        );
        // c_u = c_u,4 everywhere: A(φ) = Q(φ)/c_u,4.
        for angle in [1.0, 3.0, TAU] {
            let s = d.section(angle).unwrap();
            assert_relative_eq!(s.area, d.flow * angle / TAU / c4, max_relative = 1e-9);
        }
        // A smaller x leaves the velocity higher out in the section: smaller sections.
        let momentum = design(SectionShape::default(), plain(AreaLaw::AngularMomentum));
        assert!(d.throat.area < momentum.throat.area);
    }

    #[test]
    fn stepanoff_k3_follows_fig_7_5() {
        // Points read off the chart: K₃ ≈ 0.52 at n_s = 600, 0.29 at 3 750 and 0.18 at
        // 15 000.
        let at = |ns: f64| stepanoff_velocity_factor(ns / US_SPECIFIC_SPEED);
        assert_relative_eq!(at(600.0), 0.518, epsilon = 0.005);
        assert_relative_eq!(at(3750.0), 0.287, epsilon = 0.005);
        assert_relative_eq!(at(15_000.0), 0.182, epsilon = 0.005);
        // The ends of the chart hold outside it.
        assert_relative_eq!(at(300.0), at(600.0));
    }

    #[test]
    fn stepanoff_sizes_every_section_for_one_velocity() {
        let d = design(SectionShape::default(), plain(AreaLaw::Stepanoff));
        let b = reference_basis();
        let k3 = stepanoff_velocity_factor(b.nq);
        let c3 = k3 * (2.0 * tp_units::STANDARD_GRAVITY * b.stage_head).sqrt();
        assert_eq!(d.development.velocity_factor, Some(k3));
        assert_relative_eq!(d.development.velocity, c3, max_relative = 1e-12);
        assert_relative_eq!(d.throat_velocity, c3, max_relative = 1e-9);
        for angle in [0.5, 2.0, 5.0] {
            let s = d.section(angle).unwrap();
            assert_relative_eq!(s.area, d.flow * angle / TAU / c3, max_relative = 1e-9);
        }
        // A manual velocity replaces K₃·√(2gH).
        let mut spiral = plain(AreaLaw::Stepanoff);
        spiral.reference_velocity = Param::Manual(8.0);
        let manual = design(SectionShape::default(), spiral);
        assert_eq!(manual.development.velocity_factor, None);
        assert_relative_eq!(manual.throat.area, manual.flow / 8.0, max_relative = 1e-9);
    }

    #[test]
    fn geometric_rules_reach_their_end_values_along_the_progression() {
        let mut spiral = plain(AreaLaw::Height);
        spiral.end_height = Param::Manual(0.03);
        let d = design(SectionShape::default(), spiral.clone());
        assert_relative_eq!(d.throat.height, 0.03, max_relative = 1e-12);
        assert_relative_eq!(d.section(PI).unwrap().height, 0.015, max_relative = 1e-12);

        spiral.law = AreaLaw::Area;
        spiral.end_area = Param::Manual(2e-3);
        let d = design(SectionShape::default(), spiral.clone());
        assert_relative_eq!(d.throat.area, 2e-3, max_relative = 1e-9);
        assert_relative_eq!(d.section(PI).unwrap().area, 1e-3, max_relative = 1e-9);

        spiral.law = AreaLaw::AreaRadius;
        spiral.end_area_radius = Param::Manual(0.02);
        spiral.progression = Progression::Bezier {
            points: [[0.5, 0.0], [0.5, 1.0]],
        };
        let d = design(SectionShape::default(), spiral);
        let ratio = |s: SectionSize| s.area / s.centroid_radius;
        assert_relative_eq!(ratio(d.throat), 0.02, max_relative = 1e-9);
        // Symmetric S curve: half the end value at half the wrap.
        assert_relative_eq!(ratio(d.section(PI).unwrap()), 0.01, max_relative = 1e-6);
        assert!(ratio(d.section(1.0).unwrap()) < 0.02 * 1.0 / TAU);
    }

    #[test]
    fn automatic_end_values_are_the_throat_by_angular_momentum() {
        let momentum = design(SectionShape::default(), plain(AreaLaw::AngularMomentum));
        for law in [AreaLaw::Height, AreaLaw::Area, AreaLaw::AreaRadius] {
            let d = design(SectionShape::default(), plain(law));
            assert_relative_eq!(d.throat.height, momentum.throat.height, max_relative = 1e-9);
        }
    }

    #[test]
    fn a_shorter_spiral_starts_past_the_cutwater_with_a_blind_flow() {
        let mut spiral = plain(AreaLaw::Stepanoff);
        spiral.wrap = Param::Manual(330f64.to_radians());
        let d = design(SectionShape::default(), spiral);
        let dev = &d.development;
        assert_relative_eq!(dev.start, 30f64.to_radians(), max_relative = 1e-12);
        // Q₀ = Q·30/360: the throat still carries the whole flow.
        assert_relative_eq!(dev.blind_flow, d.flow / 12.0, max_relative = 1e-12);
        assert_relative_eq!(d.throat.area, d.flow / dev.velocity, max_relative = 1e-9);
        assert_eq!(d.section(0.2).unwrap().height, 0.0);
        let first = d.section(dev.start).unwrap();
        assert_relative_eq!(
            first.area,
            dev.blind_flow / dev.velocity,
            max_relative = 1e-9
        );

        let input = VoluteInput {
            spiral: SpiralInput {
                wrap: Param::Manual(330f64.to_radians()),
                ..SpiralInput::default()
            },
            ..VoluteInput::default()
        };
        let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        assert!(g.start_angle >= 30f64.to_radians());
        assert_eq!(g.cutwater.radius, 0.0);
    }

    #[test]
    fn the_compensation_raises_the_inner_radius_to_the_end() {
        let mut spiral = plain(AreaLaw::AngularMomentum);
        spiral.compensation.enabled = true;
        let d = design(SectionShape::rectangular(), spiral);
        let e = d.cutwater_thickness;
        let dev = &d.development;
        assert_eq!(dev.lift_at(270f64.to_radians()), 0.0);
        assert_relative_eq!(
            dev.lift_at(315f64.to_radians()),
            e / 2.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(d.throat.lift, e, max_relative = 1e-12);
        // The rule sizes the rectangle above r'_z + e: ∫ b/r dr from there.
        let plain = design(SectionShape::rectangular(), plain(AreaLaw::AngularMomentum));
        let rz = d.stagnation_radius;
        let k = plain.momentum_integral(TAU, plain.throat.height);
        let a = (rz + e) * ((k / d.inlet_width).exp() - 1.0);
        assert_relative_eq!(d.throat.height, a, max_relative = 1e-9);
        // The end section takes the strip below it as well.
        assert_relative_eq!(d.throat.area, d.inlet_width * (a + e), max_relative = 1e-9);
        assert_relative_eq!(d.throat.outer_radius, rz + e + a, max_relative = 1e-12);
    }

    #[test]
    fn the_compensated_volute_lofts_the_strip_too() {
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::trapezoidal(20f64.to_radians())),
            ..VoluteInput::default()
        };
        input.spiral.compensation.enabled = true;
        let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        let last = g.sections.last().unwrap();
        assert!(last.size.lift > 0.0);
        let area = super::super::geometry::polygon_area(&last.outline).abs();
        assert_relative_eq!(area, last.size.area, max_relative = 1e-6);
        let lowest = last
            .outline
            .iter()
            .map(|p| p.x)
            .fold(f64::INFINITY, f64::min);
        assert_relative_eq!(lowest, g.design.stagnation_radius, epsilon = 1e-9);
        assert!(!g.inner_wall.is_empty());
    }

    #[test]
    fn circular_arcs_follow_the_spiral_tangentially() {
        let mut spiral = plain(AreaLaw::AngularMomentum);
        spiral.arcs.enabled = true;
        let reference = design(SectionShape::default(), plain(AreaLaw::AngularMomentum));
        let mut previous = f64::INFINITY;
        for count in [2, 3, 5] {
            spiral.arcs.segments = count;
            let d = design(SectionShape::default(), spiral.clone());
            let arcs = d.development.arcs.as_ref().unwrap();
            assert_eq!(arcs.arcs.len(), count as usize);
            // Tangent where they meet: each junction lies on the line of both centers.
            for w in arcs.arcs.windows(2) {
                let (a, b) = (w[0], w[1]);
                assert_relative_eq!(a.end.distance(b.start), 0.0, epsilon = 1e-12);
                let cross = (a.center - a.end).perp_dot(b.center - a.end);
                assert!(cross.abs() < 1e-9, "{cross}");
            }
            // The throat stays where the rule puts it.
            assert_relative_eq!(
                d.throat.outer_radius,
                reference.throat.outer_radius,
                max_relative = 1e-6
            );
            assert!(arcs.max_deviation <= previous * 1.0001, "{count}");
            previous = arcs.max_deviation;
        }
        assert!(
            previous < 0.01 * reference.throat.height,
            "{} mm",
            previous * 1e3
        );
    }

    #[test]
    fn arcs_need_straight_walls() {
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::round()),
            ..VoluteInput::default()
        };
        input.spiral.arcs.enabled = true;
        assert!(VoluteDesign::new(&reference_basis(), &input).is_err());
    }
}
