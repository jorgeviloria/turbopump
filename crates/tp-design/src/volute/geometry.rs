//! Three-dimensional geometry of the volute: cross sections along the spiral and the
//! sections of the discharge diffuser.
//!
//! The cutwater sits on the +x axis (θ = 0). Wrap angles ε grow in the direction of
//! rotation, so a section at ε lies at θ = sign·ε. Every outline has the same number
//! of points, in the same order, so consecutive outlines can be joined into surfaces.

use std::f64::consts::TAU;

use tp_math::{DVec2, DVec3};

use super::{SectionShape, SectionSize, VoluteDesign, VoluteInput};
use crate::DesignError;
use crate::outlet::ImpellerOutlet;

/// Points on each straight edge of a rectangular or trapezoidal outline.
const EDGE_POINTS: usize = 10;
/// Points on each rounded outer corner.
const CORNER_POINTS: usize = 8;
/// Points of every outline.
pub const OUTLINE_POINTS: usize = 4 * EDGE_POINTS + 2 * CORNER_POINTS;
/// Corner radius as a fraction of the smaller side of the section.
const CORNER_FRACTION: f64 = 0.2;
/// Sections of the discharge diffuser after the throat.
const NOZZLE_SECTIONS: usize = 10;
const DEFAULT_SECTIONS: u32 = 36;

/// One cross section of the spiral, in its radial plane.
#[derive(Debug, Clone, PartialEq)]
pub struct VoluteSection {
    /// Wrap angle from the cutwater, rad.
    pub angle: f64,
    pub size: SectionSize,
    /// Closed outline (r, z), without repeating the first point.
    pub outline: Vec<DVec2>,
}

impl VoluteSection {
    /// The outline in Cartesian coordinates for an impeller turning with `sign`.
    pub fn points(&self, sign: f64) -> Vec<DVec3> {
        let (s, c) = (sign * self.angle).sin_cos();
        self.outline
            .iter()
            .map(|p| DVec3::new(p.x * c, p.x * s, p.y))
            .collect()
    }
}

/// One section of the discharge diffuser, normal to its axis.
#[derive(Debug, Clone, PartialEq)]
pub struct NozzleSection {
    pub center: DVec3,
    /// Closed outline, without repeating the first point.
    pub points: Vec<DVec3>,
    /// m²
    pub area: f64,
}

/// Complete volute geometry.
#[derive(Debug, Clone)]
pub struct VoluteGeometry {
    pub basis: ImpellerOutlet,
    pub design: VoluteDesign,
    /// Wrap angle of the first section, rad; the cutwater fills the space before it.
    pub start_angle: f64,
    /// Sections from the start angle to the throat (2π).
    pub sections: Vec<VoluteSection>,
    /// Discharge diffuser from the throat (first) to the outlet (last).
    pub nozzle: Vec<NozzleSection>,
    /// Unit vector of the diffuser axis.
    pub nozzle_axis: DVec3,
    pub warnings: Vec<String>,
}

impl VoluteGeometry {
    /// Every outline in 3D from the start of the spiral to the diffuser outlet.
    pub fn rings(&self) -> Vec<Vec<DVec3>> {
        let sign = self.basis.rotation_sign;
        self.sections
            .iter()
            .map(|s| s.points(sign))
            .chain(self.nozzle.iter().skip(1).map(|n| n.points.clone()))
            .collect()
    }

    /// Indices where the pieces of every outline start: flanks, rounded corners, outer
    /// wall and inlet for rectangles and trapezoids; a single smooth loop for circles.
    /// The outline is smooth inside a piece and may have a corner between pieces.
    pub fn ring_breaks(&self) -> Vec<usize> {
        match self.design.shape {
            SectionShape::Circular => vec![0],
            _ => {
                let (e, c) = (EDGE_POINTS, CORNER_POINTS);
                vec![0, e, e + c, 2 * e + c, 2 * e + 2 * c, 3 * e + 2 * c]
            }
        }
    }

    /// Inner and outer radius of the vaneless ring between impeller and volute.
    pub fn inlet_ring(&self) -> (f64, f64) {
        (
            self.basis.impeller_diameter / 2.0,
            self.design.stagnation_radius,
        )
    }
}

/// Points from `a` (included) to `b` (excluded).
fn segment(out: &mut Vec<DVec2>, a: DVec2, b: DVec2, count: usize) {
    out.extend((0..count).map(|i| a.lerp(b, i as f64 / count as f64)));
}

/// Arc of `count` points from `from` (included) toward `to` (excluded) about `center`.
fn arc(out: &mut Vec<DVec2>, center: DVec2, from: DVec2, to: DVec2, count: usize) {
    let (a0, a1) = (from - center, to - center);
    let start = a0.y.atan2(a0.x);
    let mut sweep = a1.y.atan2(a1.x) - start;
    // The fillet turns by less than π.
    while sweep > std::f64::consts::PI {
        sweep -= TAU;
    }
    while sweep < -std::f64::consts::PI {
        sweep += TAU;
    }
    let radius = a0.length();
    out.extend((0..count).map(|i| {
        let t = start + sweep * i as f64 / count as f64;
        center + radius * DVec2::new(t.cos(), t.sin())
    }));
}

/// Tangent points and centre of a fillet of radius `r` at corner `v` between the edges
/// from `prev` and to `next`.
fn fillet(prev: DVec2, v: DVec2, next: DVec2, r: f64) -> (DVec2, DVec2, DVec2) {
    let d1 = (prev - v).normalize();
    let d2 = (next - v).normalize();
    let half = 0.5 * d1.dot(d2).clamp(-1.0, 1.0).acos();
    let along = r / half.tan();
    let bisector = (d1 + d2).normalize();
    (
        v + d1 * along,
        v + d2 * along,
        v + bisector * (r / half.sin()),
    )
}

/// Outline of a section of height `a` (or diameter, for circles), starting at the
/// inner corner nearest the hub side and turning counter-clockwise in (r, z).
pub(crate) fn outline(shape: SectionShape, b3: f64, rz: f64, a: f64, midplane: f64) -> Vec<DVec2> {
    let mut out = Vec::with_capacity(OUTLINE_POINTS);
    if let SectionShape::Circular = shape {
        let centre = DVec2::new(rz + a / 2.0, midplane);
        let radius = (a / 2.0).max(1e-9);
        out.extend((0..OUTLINE_POINTS).map(|i| {
            let t = std::f64::consts::PI + TAU * i as f64 / OUTLINE_POINTS as f64;
            centre + radius * DVec2::new(t.cos(), t.sin())
        }));
        return out;
    }
    let t = match shape {
        SectionShape::Trapezoidal { flank_angle } => flank_angle.tan(),
        _ => 0.0,
    };
    let a = a.max(1e-9);
    let half = b3 / 2.0;
    let inner_low = DVec2::new(rz, midplane - half);
    let outer_low = DVec2::new(rz + a, midplane - half - a * t);
    let outer_high = DVec2::new(rz + a, midplane + half + a * t);
    let inner_high = DVec2::new(rz, midplane + half);
    let width = outer_high.y - outer_low.y;
    let flank = inner_low.distance(outer_low);
    let radius = (CORNER_FRACTION * a.min(width)).min(0.45 * flank.min(width));
    let (p1, p2, c1) = fillet(inner_low, outer_low, outer_high, radius);
    let (p3, p4, c2) = fillet(outer_low, outer_high, inner_high, radius);
    segment(&mut out, inner_low, p1, EDGE_POINTS);
    arc(&mut out, c1, p1, p2, CORNER_POINTS);
    segment(&mut out, p2, p3, EDGE_POINTS);
    arc(&mut out, c2, p3, p4, CORNER_POINTS);
    segment(&mut out, p4, inner_high, EDGE_POINTS);
    segment(&mut out, inner_high, inner_low, EDGE_POINTS);
    out
}

/// Outline whose enclosed area equals `area`: rounding the corners and sampling the arcs
/// remove a little area, which a slightly larger height gives back.
fn fitted_outline(
    shape: SectionShape,
    b3: f64,
    rz: f64,
    height: f64,
    midplane: f64,
    area: f64,
) -> Vec<DVec2> {
    let mut h = height;
    let mut points = outline(shape, b3, rz, h, midplane);
    for _ in 0..40 {
        let ratio = area / polygon_area(&points);
        if !ratio.is_finite() || (ratio - 1.0).abs() < 1e-7 {
            break;
        }
        // The area grows with a power of the height between 1 and 2.
        h *= ratio.powf(0.75);
        points = outline(shape, b3, rz, h, midplane);
    }
    points
}

/// Signed area of a closed polygon (positive counter-clockwise).
pub(crate) fn polygon_area(points: &[DVec2]) -> f64 {
    let n = points.len();
    0.5 * (0..n)
        .map(|i| points[i].perp_dot(points[(i + 1) % n]))
        .sum::<f64>()
}

fn centroid(points: &[DVec2]) -> DVec2 {
    let n = points.len();
    let area = polygon_area(points);
    let sum = (0..n).fold(DVec2::ZERO, |acc, i| {
        let (p, q) = (points[i], points[(i + 1) % n]);
        acc + (p + q) * p.perp_dot(q)
    });
    sum / (6.0 * area)
}

impl VoluteGeometry {
    pub fn build(basis: &ImpellerOutlet, input: &VoluteInput) -> Result<Self, DesignError> {
        let design = VoluteDesign::new(basis, input)?;
        let count = input.spiral.sections.resolve(|| DEFAULT_SECTIONS);
        if count < 4 {
            return Err(DesignError::InvalidInput(
                "a volute needs at least 4 cross sections".into(),
            ));
        }
        let sign = basis.rotation_sign;
        let zm = basis.midplane;
        let shape = design.shape;
        let make = |angle: f64| -> Result<VoluteSection, DesignError> {
            let size = design.section(angle)?;
            Ok(VoluteSection {
                angle,
                size,
                outline: fitted_outline(
                    shape,
                    design.inlet_width,
                    design.stagnation_radius,
                    size.height,
                    zm,
                    size.area,
                ),
            })
        };
        let position = |angle: f64, size: &SectionSize| {
            let theta = sign * angle;
            DVec3::new(
                size.centroid_radius * theta.cos(),
                size.centroid_radius * theta.sin(),
                zm,
            )
        };

        // The diffuser continues the path of the section centroids at the throat.
        let throat = make(TAU)?;
        let before = design.section(TAU - 0.02)?;
        let axis = (position(TAU, &throat.size) - position(TAU - 0.02, &before)).normalize();

        // The spiral starts where its outer wall clears the inner wall of the diffuser,
        // a line from the cutwater along the axis.
        let tip = DVec3::new(design.stagnation_radius, 0.0, zm);
        let side = |p: DVec3| axis.truncate().perp_dot((p - tip).truncate());
        let inside = side(DVec3::new(0.0, 0.0, zm));
        let clear = |angle: f64| -> Result<bool, DesignError> {
            let s = make(angle)?;
            Ok(s.points(sign).iter().all(|p| side(*p) * inside >= 0.0))
        };
        let mut start = 2f64.to_radians();
        while !clear(start)? || !clear(start + 0.05)? {
            start += 1f64.to_radians();
            if start > 90f64.to_radians() {
                return Err(DesignError::Infeasible(
                    "the start of the spiral collides with the discharge diffuser".into(),
                ));
            }
        }
        let start = start + 2f64.to_radians();

        let sections = (0..=count)
            .map(|i| make(start + (TAU - start) * f64::from(i) / f64::from(count)))
            .collect::<Result<Vec<_>, _>>()?;

        let nozzle = nozzle(&design, &throat, sign, axis);
        let warnings = design.warnings.clone();
        Ok(Self {
            basis: *basis,
            design,
            start_angle: start,
            sections,
            nozzle,
            nozzle_axis: axis,
            warnings,
        })
    }
}

/// Sections of the discharge diffuser: the throat outline blends into a circle while
/// the equivalent radius grows linearly to the outlet radius. The wall on the cutwater
/// side stays straight and the whole widening goes outward, so the diffuser never
/// closes in on the start of the spiral.
fn nozzle(
    design: &VoluteDesign,
    throat: &VoluteSection,
    sign: f64,
    axis: DVec3,
) -> Vec<NozzleSection> {
    let theta = sign * TAU;
    let radial = DVec3::new(theta.cos(), theta.sin(), 0.0);
    let local_centroid = centroid(&throat.outline);
    let throat_points: Vec<DVec2> = throat.outline.iter().map(|p| *p - local_centroid).collect();
    let throat_area = polygon_area(&throat_points).abs();
    let r1 = (throat_area / std::f64::consts::PI).sqrt();
    let r2 = design.diffuser.outlet_diameter / 2.0;
    let length = design.diffuser.length;
    let start = DVec3::new(
        local_centroid.x * theta.cos(),
        local_centroid.x * theta.sin(),
        local_centroid.y,
    );
    let u = (radial - axis * radial.dot(axis)).normalize();
    let v = DVec3::Z;
    // Offset across the axis of the line from the cutwater along the diffuser axis.
    let tip = radial * design.stagnation_radius;
    let inner = (tip - start).dot(u);
    let circle: Vec<DVec2> = throat_points
        .iter()
        .map(|p| r1 * DVec2::new(p.x, p.y).normalize())
        .collect();
    // The sampled circle encloses a little less than π·r²; scale against it so the
    // outlet points lie exactly on the outlet radius.
    let circle_area = polygon_area(&circle).abs();

    (0..=NOZZLE_SECTIONS)
        .map(|k| {
            let s = k as f64 / NOZZLE_SECTIONS as f64;
            if k == 0 {
                return NozzleSection {
                    center: start,
                    points: throat.points(sign),
                    area: throat_area,
                };
            }
            let w = s * s * (3.0 - 2.0 * s);
            let blend: Vec<DVec2> = throat_points
                .iter()
                .zip(&circle)
                .map(|(t, c)| t.lerp(*c, w))
                .collect();
            let radius = r1 + (r2 - r1) * s;
            let scale = radius / r1 * (circle_area / polygon_area(&blend).abs()).sqrt();
            let lowest = blend.iter().map(|p| p.x).fold(f64::INFINITY, f64::min) * scale;
            let center = start + axis * (length * s) + u * (inner - lowest);
            NozzleSection {
                center,
                points: blend
                    .iter()
                    .map(|p| center + u * (p.x * scale) + v * (p.y * scale))
                    .collect(),
                area: std::f64::consts::PI * radius * radius,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::tests::reference_basis;
    use super::*;
    use crate::volute::AreaLaw;
    use approx::assert_relative_eq;

    fn geometry(shape: SectionShape) -> VoluteGeometry {
        let mut input = VoluteInput::default();
        input.cross_section.shape = shape;
        VoluteGeometry::build(&reference_basis(), &input).unwrap()
    }

    #[test]
    fn outlines_have_a_fixed_point_count() {
        for shape in [
            SectionShape::Rectangular,
            SectionShape::Circular,
            SectionShape::default(),
        ] {
            let g = geometry(shape);
            assert_eq!(g.sections.len(), 37);
            for ring in g.rings() {
                assert_eq!(ring.len(), OUTLINE_POINTS);
            }
            assert_eq!(g.nozzle.len(), NOZZLE_SECTIONS + 1);
        }
    }

    #[test]
    fn outline_areas_match_the_design_sections() {
        let g = geometry(SectionShape::default());
        for s in &g.sections {
            let polygon = polygon_area(&s.outline);
            assert!(polygon > 0.0, "outlines turn counter-clockwise");
            assert_relative_eq!(polygon, s.size.area, max_relative = 1e-6);
        }
        let circles = geometry(SectionShape::Circular);
        let throat = circles.sections.last().unwrap();
        assert_relative_eq!(
            polygon_area(&throat.outline),
            throat.size.area,
            max_relative = 1e-6
        );
    }

    #[test]
    fn sections_wrap_once_around_and_grow() {
        let g = geometry(SectionShape::default());
        assert!(
            g.start_angle > 0.0 && g.start_angle < 1.0,
            "{}",
            g.start_angle
        );
        assert_relative_eq!(g.sections.last().unwrap().angle, TAU);
        for pair in g.sections.windows(2) {
            assert!(pair[1].size.area > pair[0].size.area);
        }
        // Points sit at θ = ε for positive rotation.
        let s = &g.sections[10];
        let p = s.points(1.0)[0];
        assert_relative_eq!(p.y.atan2(p.x).rem_euclid(TAU), s.angle, epsilon = 1e-9);
    }

    #[test]
    fn nozzle_grows_from_the_throat_to_the_outlet_circle() {
        let g = geometry(SectionShape::default());
        let first = &g.nozzle[0];
        let last = g.nozzle.last().unwrap();
        assert_eq!(first.points, g.sections.last().unwrap().points(1.0));
        let r2 = g.design.diffuser.outlet_diameter / 2.0;
        for p in &last.points {
            assert_relative_eq!(p.distance(last.center), r2, max_relative = 1e-6);
        }
        for pair in g.nozzle.windows(2) {
            assert!(pair[1].area > pair[0].area);
        }
        // The axis leaves the throat in the direction of rotation.
        assert!(g.nozzle_axis.y > 0.9);
        assert!(g.nozzle_axis.z.abs() < 1e-12);
        let mut negative = reference_basis();
        negative.rotation_sign = -1.0;
        let mirrored = VoluteGeometry::build(&negative, &VoluteInput::default()).unwrap();
        assert!(mirrored.nozzle_axis.y < -0.9);
    }

    #[test]
    fn diffuser_wall_on_the_cutwater_side_is_straight() {
        let g = geometry(SectionShape::default());
        let axis = g.nozzle_axis;
        let tip = DVec3::new(g.design.stagnation_radius, 0.0, 0.0);
        // Signed distance from the line through the cutwater along the diffuser axis,
        // positive on the side of the axis of rotation.
        let toward_axis = DVec3::new(-axis.y, axis.x, 0.0);
        let toward_axis = if toward_axis.dot(-tip) > 0.0 {
            toward_axis
        } else {
            -toward_axis
        };
        let tolerance = 1e-9 * g.design.diffuser.length;
        for n in &g.nozzle {
            let distances: Vec<f64> = n
                .points
                .iter()
                .map(|p| (*p - tip).truncate().dot(toward_axis.truncate()))
                .collect();
            let nearest = distances.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            // Every section touches the line and none crosses it.
            assert!(
                nearest.abs() < 1e-6 * g.design.diffuser.length + tolerance,
                "{nearest}"
            );
        }
    }

    #[test]
    fn spiral_start_stays_clear_of_the_diffuser() {
        for law in [AreaLaw::AngularMomentum, AreaLaw::ConstantVelocity] {
            let mut input = VoluteInput::default();
            input.spiral.law = law;
            let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
            let tip = DVec3::new(g.design.stagnation_radius, 0.0, 0.0);
            let axis = g.nozzle_axis.truncate();
            for s in g.sections.iter().filter(|s| s.angle < 1.5) {
                for p in s.points(1.0) {
                    // On the axis side of the diffuser's inner wall.
                    assert!(axis.perp_dot((p - tip).truncate()) >= -1e-12);
                }
            }
        }
    }
}
