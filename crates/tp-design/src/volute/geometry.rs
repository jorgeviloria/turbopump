//! Three-dimensional geometry of the volute: cross sections along the spiral and the
//! sections of the discharge diffuser.
//!
//! The cutwater sits on the +x axis (θ = 0). Wrap angles ε grow in the direction of
//! rotation, so a section at ε lies at θ = sign·ε. Every outline has the same number
//! of points, in the same order, so consecutive outlines can be joined into surfaces.

use std::f64::consts::{PI, TAU};

use tp_math::{DVec2, DVec3};

use super::{
    CutwaterShape, DischargeShape, SectionShape, SectionSize, StraightWall, VoluteDesign,
    VoluteInput,
};
use crate::outlet::ImpellerOutlet;
use crate::{DesignError, Param};

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
    /// Angle of the cutwater from the x axis: the turn of the whole volute, rad.
    pub rotation: f64,
}

impl VoluteSection {
    /// The outline in Cartesian coordinates for an impeller turning with `sign`.
    pub fn points(&self, sign: f64) -> Vec<DVec3> {
        let (s, c) = (self.rotation + sign * self.angle).sin_cos();
        self.outline
            .iter()
            .map(|p| DVec3::new(p.x * c, p.x * s, p.y))
            .collect()
    }
}

/// One section of the discharge diffuser, normal to its centerline.
#[derive(Debug, Clone, PartialEq)]
pub struct NozzleSection {
    pub center: DVec3,
    /// Unit tangent of the centerline.
    pub tangent: DVec3,
    /// Closed outline, without repeating the first point.
    pub points: Vec<DVec3>,
    /// m²
    pub area: f64,
}

/// The cutwater in the midplane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutwaterGeometry {
    /// Sharp edge where the outer wall of the spiral meets the inner wall of the
    /// diffuser; for a tangent cutwater, the stagnation point, m.
    pub tip: DVec3,
    /// Wrap angle of the tip, rad.
    pub angle: f64,
    /// Angle between the two walls at the tip, rad.
    pub wedge: f64,
    /// Radius of the nose, m; 0 for a sharp or tangent cutwater.
    pub radius: f64,
    /// Centre of the rounded nose (the tip when sharp), m.
    pub nose_center: DVec3,
    /// Distance from the pump axis to the nearest point of the cutwater: half the
    /// effective cutwater diameter, m.
    pub nose_distance: f64,
}

impl CutwaterGeometry {
    /// Outline in the midplane of the material a rounded cutwater adds to the sharp
    /// wedge: the nose circle and the tangents from it to the tip, closed. Empty
    /// unless rounded.
    pub fn nose_outline(&self) -> Vec<DVec3> {
        if self.radius <= 0.0 {
            return Vec::new();
        }
        let c = self.nose_center;
        let to_tip = self.tip - c;
        let d = to_tip.truncate().length();
        let beta = (self.radius / d).clamp(-1.0, 1.0).acos();
        let base = to_tip.y.atan2(to_tip.x);
        // The arc runs on the far side from the tip, between the two tangent points.
        let (from, to) = (base + beta, base - beta + std::f64::consts::TAU);
        let n = 24;
        let mut out: Vec<DVec3> = (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f64 / n as f64;
                c + DVec3::new(a.cos(), a.sin(), 0.0) * self.radius
            })
            .collect();
        out.push(self.tip);
        out
    }
}

/// The rib of a double volute in the midplane: a band as thick as the rib between the
/// inner volute and the outer channel, with an elliptic nose ahead of the start of the
/// inner volute and an elliptic tail after the throat. Its faces run across the whole
/// section, so the solid rib is this outline extruded along the pump axis.
#[derive(Debug, Clone, PartialEq)]
pub struct RibGeometry {
    /// Inner face, the outer wall of the inner volute, from the start of the inner
    /// volute to the throat.
    pub inner_face: Vec<DVec3>,
    /// Outer face, the inner wall of the outer channel, at the same wrap angles.
    pub outer_face: Vec<DVec3>,
    /// Nose from the start of the outer face around to the start of the inner face.
    pub nose: Vec<DVec3>,
    /// Tail from the end of the inner face around to the end of the outer face.
    pub tail: Vec<DVec3>,
    /// Leading point of the nose.
    pub nose_tip: DVec3,
    /// Wrap angle of the nose tip, rad.
    pub nose_angle: f64,
    /// Distance from the pump axis to the nearest point of the rib, m.
    pub nose_distance: f64,
}

impl RibGeometry {
    /// Closed outline: nose, inner face, tail and outer face back to the nose, without
    /// repeating the first point.
    pub fn outline(&self) -> Vec<DVec3> {
        let mut out = self.nose.clone();
        out.extend(&self.inner_face[1..]);
        out.extend(&self.tail[1..]);
        out.extend(self.outer_face.iter().rev().skip(1));
        out.pop();
        out
    }
}

/// Points along each face of the rib.
const RIB_FACE_POINTS: usize = 48;
/// Points on the nose and on the tail of the rib, an even number.
const RIB_NOSE_POINTS: usize = 16;

/// Spiral start of a sharp or rounded cutwater, rad: the spiral overlaps the diffuser
/// from there to the cutwater.
const CUTWATER_START: f64 = 3.0 * std::f64::consts::PI / 180.0;
/// Smallest angle between the spiral and the diffuser wall for a sharp cutwater; below
/// it the two only graze and the spiral starts where it clears the wall, rad.
const MIN_WEDGE: f64 = 4.0 * std::f64::consts::PI / 180.0;

/// Complete volute geometry.
#[derive(Debug, Clone)]
pub struct VoluteGeometry {
    pub basis: ImpellerOutlet,
    pub design: VoluteDesign,
    /// Wrap angle of the first section, rad; the cutwater fills the space before it.
    pub start_angle: f64,
    /// Sections from the start angle to the throat (2π); for a double volute, to the
    /// rib nose.
    pub sections: Vec<VoluteSection>,
    /// Double volute: sections of the inner volute, from just after the rib nose to 2π.
    pub inner_sections: Vec<VoluteSection>,
    /// Double volute: sections of the outer channel, outside the rib, from the rib nose
    /// to 2π.
    pub outer_channel: Vec<VoluteSection>,
    /// Double volute: whole sections from the rib nose to 2π (inner volute, rib and
    /// outer channel), continuing the first partial volute.
    pub envelope: Vec<VoluteSection>,
    /// Double volute: the rib in the midplane.
    pub rib: Option<RibGeometry>,
    /// Discharge diffuser from the throat (first) to the outlet (last).
    pub nozzle: Vec<NozzleSection>,
    /// Unit vector of the diffuser axis at the outlet.
    pub nozzle_axis: DVec3,
    /// Angle the bend of a radial discharge turns, rad; 0 for a straight one.
    pub nozzle_bend: f64,
    /// Length of the diffuser centerline, m.
    pub nozzle_length: f64,
    /// Distance from the pump axis to the outlet plane, along the outlet axis, m.
    pub outlet_height: f64,
    /// Angle of the cutwater from the x axis, counter-clockwise about z: the turn of
    /// the whole volute, rad. Wrap angles are measured from the cutwater.
    pub rotation: f64,
    /// Outer wall of the spiral in the midplane, finely sampled from the start of the
    /// spiral to the throat.
    pub outer_wall: Vec<DVec3>,
    pub cutwater: CutwaterGeometry,
    pub warnings: Vec<String>,
}

/// Turn by which the channels of a double volute reach into the discharge diffuser, so
/// that the solids overlap where they meet, rad.
const CHANNEL_OVERLAP: f64 = 1.5 * std::f64::consts::PI / 180.0;

impl VoluteGeometry {
    /// The pieces of the flow channel, each a sequence of outlines in 3D to loft
    /// through, for surface meshes. A single volute is one piece from the start of the
    /// spiral to the diffuser outlet. A double volute has three: the first partial
    /// volute with the outer channel, the inner volute, and the discharge diffuser; the
    /// first two reach a little into the third.
    pub fn channels(&self) -> Vec<Vec<Vec<DVec3>>> {
        let joined = matches!(self.design.cutwater_shape, CutwaterShape::Tangent);
        if self.design.rib.is_none() && joined {
            let sign = self.basis.rotation_sign;
            return vec![
                self.sections
                    .iter()
                    .map(|s| s.points(sign))
                    .chain(self.nozzle.iter().skip(1).map(|n| n.points.clone()))
                    .collect(),
            ];
        }
        if self.design.rib.is_none() {
            // A sharp or rounded cutwater: the spiral cuts the diffuser.
            return vec![self.extended(&self.sections), self.nozzle_rings()];
        }
        // The throat of the first partial volute and the start of the outer channel
        // share the angle of the rib nose: the channel takes over there.
        vec![
            self.extended(&self.followed_by(&self.outer_channel)),
            self.extended(&self.inner_sections),
            self.nozzle_rings(),
        ]
    }

    /// The solids to loft for the CAD model: as [`Self::channels`] for a single volute.
    /// A double volute lofts its whole sections as one spiral, and the rib
    /// ([`Self::rib`]) is cut out of it.
    pub fn lofts(&self) -> Vec<Vec<Vec<DVec3>>> {
        if self.design.rib.is_none() {
            return self.channels();
        }
        vec![
            self.extended(&self.followed_by(&self.envelope)),
            self.nozzle_rings(),
        ]
    }

    /// The first partial volute without its throat, followed by `sections` from there.
    fn followed_by(&self, sections: &[VoluteSection]) -> Vec<VoluteSection> {
        let first = &self.sections[..self.sections.len() - 1];
        first.iter().chain(sections).cloned().collect()
    }

    /// Rings of `sections` and one more turned on by [`CHANNEL_OVERLAP`], reaching into
    /// the discharge diffuser.
    fn extended(&self, sections: &[VoluteSection]) -> Vec<Vec<DVec3>> {
        let sign = self.basis.rotation_sign;
        let mut rings: Vec<Vec<DVec3>> = sections.iter().map(|s| s.points(sign)).collect();
        if let Some(last) = sections.last() {
            let beyond = VoluteSection {
                angle: last.angle + CHANNEL_OVERLAP,
                ..last.clone()
            };
            rings.push(beyond.points(sign));
        }
        rings
    }

    fn nozzle_rings(&self) -> Vec<Vec<DVec3>> {
        self.nozzle.iter().map(|n| n.points.clone()).collect()
    }

    /// Every outline in 3D, from the start of the spiral to the diffuser outlet.
    pub fn rings(&self) -> Vec<Vec<DVec3>> {
        self.channels().into_iter().flatten().collect()
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

    /// The outer walls of the casing: every outline of [`Self::lofts`] offset outward by
    /// the wall thickness in its own plane, with the same points and pieces. The inlet
    /// side of the spiral sections moves inward too; the CAD model cuts the flow domain
    /// and the space of the impeller out of the solid these outlines enclose.
    pub fn casing_lofts(&self) -> Vec<Vec<Vec<DVec3>>> {
        let t = self.design.wall_thickness;
        self.lofts()
            .iter()
            .map(|rings| rings.iter().map(|ring| offset_ring(ring, t)).collect())
            .collect()
    }
}

/// The closed planar outline `ring` offset outward by `distance` in its plane: each
/// point moves along the bisector of the normals of its two edges, by `distance` over
/// the cosine of half the turn between them (a mitred corner). The outlines are convex.
pub fn offset_ring(ring: &[DVec3], distance: f64) -> Vec<DVec3> {
    let n = ring.len();
    if n < 3 {
        return ring.to_vec();
    }
    let center = ring.iter().copied().sum::<DVec3>() / n as f64;
    // Newell's normal: the outline runs counterclockwise about it.
    let normal = (0..n)
        .map(|i| (ring[i] - center).cross(ring[(i + 1) % n] - center))
        .sum::<DVec3>()
        .normalize();
    let u = (ring[0] - center).reject_from(normal).normalize();
    let v = normal.cross(u);
    let flat: Vec<DVec2> = ring
        .iter()
        .map(|p| DVec2::new((*p - center).dot(u), (*p - center).dot(v)))
        .collect();
    // Outward normal of the edge from `a` to `b` of a counterclockwise outline.
    let outward = |a: DVec2, b: DVec2| {
        let e = (b - a).normalize_or_zero();
        DVec2::new(e.y, -e.x)
    };
    (0..n)
        .map(|i| {
            let p = flat[i];
            let before = outward(flat[(i + n - 1) % n], p);
            let after = outward(p, flat[(i + 1) % n]);
            let miter = (before + after) / (1.0 + before.dot(after)).max(0.25);
            let q = p + distance * miter;
            center + u * q.x + v * q.y
        })
        .collect()
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
    /// The volute of `input` for the impeller `basis`. A flange height H fixes the
    /// length of the diffuser: past the bend the outlet face moves away from the pump
    /// axis as much as the diffuser grows, so the length is that of a first build plus
    /// the difference in H.
    pub fn build(basis: &ImpellerOutlet, input: &VoluteInput) -> Result<Self, DesignError> {
        let Param::Manual(height) = input.diffuser.flange_height else {
            return Self::build_with_length(basis, input);
        };
        let mut free = input.clone();
        free.diffuser.flange_height = Param::Auto;
        let first = Self::build_with_length(basis, &free)?;
        let bend = first.design.diffuser.bend_radius * first.nozzle_bend;
        let at_bend = first.outlet_height - (first.nozzle_length - bend);
        let length = bend + (height - at_bend);
        if length < bend.max(1e-3) {
            return Err(DesignError::InvalidInput(format!(
                "the flange height H = {:.1} mm leaves no room for the diffuser: it must \
                 exceed {:.1} mm",
                height * 1e3,
                (at_bend + bend.max(1e-3) - bend) * 1e3
            )));
        }
        free.diffuser.length = Param::Manual(length);
        Self::build_with_length(basis, &free)
    }

    fn build_with_length(basis: &ImpellerOutlet, input: &VoluteInput) -> Result<Self, DesignError> {
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
                rotation: 0.0,
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

        // Double volute: the outer channel outside the rib, and the section where both
        // channels meet the discharge diffuser.
        let base_width = |height: f64| match shape {
            SectionShape::Trapezoidal { flank_angle } => {
                design.inlet_width + 2.0 * height * flank_angle.tan()
            }
            _ => design.inlet_width,
        };
        let inner_height = |angle: f64| -> Result<f64, DesignError> {
            match &design.rib {
                Some(rib) if angle > rib.start => Ok(design.section(angle)?.height),
                _ => Ok(0.0),
            }
        };
        // The outer channel: the part of the whole section beyond the rib.
        let channel = |angle: f64| -> Result<VoluteSection, DesignError> {
            let rib = design.rib.as_ref().expect("a double volute");
            let a = inner_height(angle)?;
            let base = base_width(a + rib.thickness);
            let rz = design.stagnation_radius + a + rib.thickness;
            let area = rib.channel_area(angle);
            let part = super::Shape {
                kind: shape,
                b3: base,
                rz,
            };
            let height = part.height_for_area(area);
            Ok(VoluteSection {
                angle,
                size: part.size(height),
                outline: fitted_outline(shape, base, rz, height, zm, area),
                rotation: 0.0,
            })
        };
        // The whole section of a double volute: the inner volute, the rib and the outer
        // channel. At the rib nose it is the throat of the first partial volute.
        let whole = |angle: f64| -> Result<VoluteSection, DesignError> {
            let rib = design.rib.as_ref().expect("a double volute");
            let whole = design.shape();
            let area = whole.area(inner_height(angle)? + rib.thickness) + rib.channel_area(angle);
            let size = whole.size(whole.height_for_area(area));
            Ok(VoluteSection {
                angle,
                size,
                outline: fitted_outline(
                    shape,
                    design.inlet_width,
                    design.stagnation_radius,
                    size.height,
                    zm,
                    area,
                ),
                rotation: 0.0,
            })
        };

        // The diffuser continues the path of the section centroids at the throat.
        let (throat, before) = if design.rib.is_some() {
            (whole(TAU)?, whole(TAU - 0.02)?.size)
        } else {
            (make(TAU)?, design.section(TAU - 0.02)?)
        };
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
        let clear_start = start + 2f64.to_radians();
        let (nozzle, nozzle_bend, nozzle_length) = nozzle(&design, &throat, sign, axis)?;
        let nozzle_axis = nozzle.last().map_or(axis, |n| n.tangent);
        // A sharp or rounded cutwater: the spiral starts at the cutwater and cuts the
        // inner wall of the diffuser. When the wall turns away instead (a radial
        // discharge), the spiral starts where it clears the wall and the cutwater sits
        // there.
        let (start, cutwater) = match design.cutwater_shape {
            CutwaterShape::Tangent => (
                clear_start,
                cutwater(&design, &nozzle, sign, zm, clear_start)?,
            ),
            _ => {
                let early = cutwater(&design, &nozzle, sign, zm, CUTWATER_START)?;
                if early.angle > CUTWATER_START && early.wedge > MIN_WEDGE {
                    (CUTWATER_START, early)
                } else {
                    (
                        clear_start,
                        cutwater(&design, &nozzle, sign, zm, clear_start)?,
                    )
                }
            }
        };

        // Sections spread over their wrap in proportion to the single spiral.
        let spread = |from: f64, to: f64| -> Vec<f64> {
            let n = ((f64::from(count) * (to - from) / (TAU - start)).round() as u32).max(4);
            (0..=n)
                .map(|i| from + (to - from) * f64::from(i) / f64::from(n))
                .collect()
        };
        let (sections, inner_sections, outer_channel, envelope) = match &design.rib {
            None => (
                spread(start, TAU)
                    .into_iter()
                    .map(make)
                    .collect::<Result<Vec<_>, _>>()?,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            Some(rib) => {
                if rib.start <= start + 10f64.to_radians() {
                    return Err(DesignError::Infeasible(
                        "the rib of the double volute starts before the spiral".into(),
                    ));
                }
                (
                    spread(start, rib.start)
                        .into_iter()
                        .map(make)
                        .collect::<Result<Vec<_>, _>>()?,
                    spread(rib.start + CUTWATER_START, TAU)
                        .into_iter()
                        .map(make)
                        .collect::<Result<Vec<_>, _>>()?,
                    spread(rib.start, TAU)
                        .into_iter()
                        .map(channel)
                        .collect::<Result<Vec<_>, _>>()?,
                    spread(rib.start, TAU)
                        .into_iter()
                        .map(whole)
                        .collect::<Result<Vec<_>, _>>()?,
                )
            }
        };
        let rib = rib(&design, sign, zm)?;

        // The outer wall for drawing: the outermost point of each fitted outline, every
        // degree or less.
        let steps = (((TAU - start) / 1f64.to_radians()).ceil() as usize).max(2);
        let outer_wall = (0..=steps)
            .map(|i| {
                let angle = start + (TAU - start) * i as f64 / steps as f64;
                let section = match &design.rib {
                    Some(rib) if angle > rib.start => whole(angle)?,
                    _ => make(angle)?,
                };
                let r = section
                    .outline
                    .iter()
                    .map(|p| p.x)
                    .fold(f64::NEG_INFINITY, f64::max);
                let theta = sign * angle;
                Ok(DVec3::new(r * theta.cos(), r * theta.sin(), zm))
            })
            .collect::<Result<Vec<_>, DesignError>>()?;
        let outlet_height = nozzle
            .last()
            .map_or(0.0, |n| n.center.truncate().dot(n.tangent.truncate()));

        // Turn the whole volute about the pump axis, by default so that the discharge
        // points vertically up. Wrap angles stay measured from the cutwater.
        let rotation = input
            .diffuser
            .rotation
            .resolve(|| std::f64::consts::FRAC_PI_2 - nozzle_axis.y.atan2(nozzle_axis.x));
        let rotation = (rotation + PI).rem_euclid(TAU) - PI;
        let (rs, rc) = rotation.sin_cos();
        let turn = |p: DVec3| DVec3::new(rc * p.x - rs * p.y, rs * p.x + rc * p.y, p.z);
        let (mut sections, mut inner_sections, mut outer_channel, mut envelope) =
            (sections, inner_sections, outer_channel, envelope);
        for list in [
            &mut sections,
            &mut inner_sections,
            &mut outer_channel,
            &mut envelope,
        ] {
            for s in list.iter_mut() {
                s.rotation = rotation;
            }
        }
        let nozzle: Vec<NozzleSection> = nozzle
            .into_iter()
            .map(|n| NozzleSection {
                center: turn(n.center),
                tangent: turn(n.tangent),
                points: n.points.into_iter().map(turn).collect(),
                area: n.area,
            })
            .collect();
        let nozzle_axis = turn(nozzle_axis);
        let cutwater = CutwaterGeometry {
            tip: turn(cutwater.tip),
            nose_center: turn(cutwater.nose_center),
            ..cutwater
        };
        let rib = rib.map(|r| RibGeometry {
            inner_face: r.inner_face.into_iter().map(turn).collect(),
            outer_face: r.outer_face.into_iter().map(turn).collect(),
            nose: r.nose.into_iter().map(turn).collect(),
            tail: r.tail.into_iter().map(turn).collect(),
            nose_tip: turn(r.nose_tip),
            ..r
        });
        let outer_wall: Vec<DVec3> = outer_wall.into_iter().map(turn).collect();

        let mut warnings = design.warnings.clone();
        if nozzle_length > design.diffuser.length * (1.0 + 1e-9) {
            warnings.push(format!(
                "the bend of the radial discharge ({:.0} mm) is longer than the diffuser \
                 ({:.0} mm): the diffuser ends with the bend",
                nozzle_length * 1e3,
                design.diffuser.length * 1e3
            ));
        }
        Ok(Self {
            basis: *basis,
            design,
            start_angle: start,
            sections,
            inner_sections,
            outer_channel,
            envelope,
            rib,
            nozzle,
            nozzle_axis,
            nozzle_bend,
            nozzle_length,
            outlet_height,
            rotation,
            outer_wall,
            cutwater,
            warnings,
        })
    }
}

/// The rib of a double volute in the midplane. Its inner face follows the outer wall
/// of the inner volute from r'_z at the start of the inner volute, its outer face lies
/// one rib thickness beyond. The nose and the tail are half ellipses on the conjugate
/// semi-diameters across the rib and along its mid line, so they meet both faces
/// tangentially: the leading edge points along the inner volute's wall, which leaves
/// r'_z at the flow angle α₃' (Gülich Chap. 7.8.1, item 7, asks the cutwater to match
/// the approach flow).
fn rib(design: &VoluteDesign, sign: f64, zm: f64) -> Result<Option<RibGeometry>, DesignError> {
    let Some(rib) = design.rib else {
        return Ok(None);
    };
    let t = rib.thickness;
    let inner = |angle: f64| -> Result<f64, DesignError> {
        let a = if angle > rib.start {
            design.section(angle)?.height
        } else {
            0.0
        };
        Ok(design.stagnation_radius + a)
    };
    let at = |r: f64, angle: f64| {
        let theta = sign * angle;
        DVec3::new(r * theta.cos(), r * theta.sin(), zm)
    };
    let n = RIB_FACE_POINTS;
    let (mut inner_face, mut outer_face) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
    for i in 0..=n {
        let angle = rib.start + (TAU - rib.start) * i as f64 / n as f64;
        let r = inner(angle)?;
        inner_face.push(at(r, angle));
        outer_face.push(at(r + t, angle));
    }
    // Downstream direction of the mid line at both ends.
    let mid = |angle: f64| -> Result<DVec3, DesignError> { Ok(at(inner(angle)? + t / 2.0, angle)) };
    let step = 1e-4;
    let lead = (mid(rib.start + step)? - mid(rib.start)?).normalize();
    let trail = (mid(TAU)? - mid(TAU - step)?).normalize();
    let length = rib.nose_length();
    let half_ellipse = |inner: DVec3, outer: DVec3, along: DVec3, from: f64, to: f64| {
        let centre = 0.5 * (inner + outer);
        let across = 0.5 * (outer - inner);
        let m = RIB_NOSE_POINTS;
        let mut points: Vec<DVec3> = (0..=m)
            .map(|i| {
                let s = from + (to - from) * i as f64 / m as f64;
                centre + across * s.cos() + along * (length * s.sin())
            })
            .collect();
        // The ends are the corners of the faces, exactly.
        points[0] = if from == 0.0 { outer } else { inner };
        points[m] = if from == 0.0 { inner } else { outer };
        (points, centre + along * length)
    };
    let (nose, nose_tip) = half_ellipse(inner_face[0], outer_face[0], -lead, 0.0, PI);
    let (tail, _) = half_ellipse(inner_face[n], outer_face[n], trail, PI, 0.0);
    let nose_distance = nose
        .iter()
        .chain(&inner_face)
        .map(|p| p.truncate().length())
        .fold(f64::INFINITY, f64::min);
    let nose_angle = (sign * nose_tip.y.atan2(nose_tip.x)).rem_euclid(TAU);
    Ok(Some(RibGeometry {
        inner_face,
        outer_face,
        nose,
        tail,
        nose_tip,
        nose_angle,
        nose_distance,
    }))
}

/// The cutwater in the midplane: where the outer wall of the spiral meets the inner
/// wall of the diffuser, the angle between them, and the nose of radius ρ that touches
/// both. The nose of a thin wedge sits where the wedge is 2ρ thick, along its bisector.
fn cutwater(
    design: &VoluteDesign,
    nozzle: &[NozzleSection],
    sign: f64,
    zm: f64,
    start: f64,
) -> Result<CutwaterGeometry, DesignError> {
    let stagnation = DVec3::new(design.stagnation_radius, 0.0, zm);
    if matches!(design.cutwater_shape, CutwaterShape::Tangent) {
        return Ok(CutwaterGeometry {
            tip: stagnation,
            angle: 0.0,
            wedge: 0.0,
            radius: 0.0,
            nose_center: stagnation,
            nose_distance: design.stagnation_radius,
        });
    }
    // Inner wall of the diffuser near the throat: the innermost points of the first
    // two sections across the centerline.
    let inner = |n: &NozzleSection| -> DVec3 {
        let u = n.tangent.cross(DVec3::Z);
        let u = if u.dot(n.center) > 0.0 { u } else { -u };
        let p = n
            .points
            .iter()
            .min_by(|a, b| (**a - n.center).dot(u).total_cmp(&(**b - n.center).dot(u)))
            .copied()
            .unwrap_or(n.center);
        DVec3::new(p.x, p.y, zm)
    };
    let w0 = inner(&nozzle[0]);
    let along = (inner(&nozzle[1]) - w0).normalize();
    let normal = DVec3::new(along.y, -along.x, 0.0);
    let normal = if normal.dot(w0) > 0.0 {
        normal
    } else {
        -normal
    };
    // Outer wall of the spiral in the midplane.
    let wall = |angle: f64| -> Result<DVec3, DesignError> {
        let r = design.section(angle)?.outer_radius;
        let t = sign * angle;
        Ok(DVec3::new(r * t.cos(), r * t.sin(), zm))
    };
    // Positive where the spiral wall lies inside the diffuser.
    let inside = |angle: f64| -> Result<f64, DesignError> { Ok((wall(angle)? - w0).dot(normal)) };
    // When the wall bends away from the spiral at once (a radial discharge), the two
    // part at the start of the spiral; otherwise the spiral leaves the diffuser where
    // its outer wall crosses the inner wall.
    let angle = if inside(start)? <= 0.0 {
        start
    } else {
        let (mut lo, mut hi) = (start, start);
        let step = 0.25f64.to_radians();
        while inside(hi)? > 0.0 {
            lo = hi;
            hi += step;
            if hi > std::f64::consts::PI {
                return Err(DesignError::Infeasible(
                    "the spiral does not meet the inner wall of the discharge diffuser: use \
                     a tangent cutwater"
                        .into(),
                ));
            }
        }
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if inside(mid)? > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    let tip = wall(angle)?;
    let d_wall = along;
    let d_spiral = (wall(angle + 1e-4)? - wall(angle - 1e-4)?).normalize();
    let wedge = d_wall.dot(d_spiral).clamp(-1.0, 1.0).acos();
    // A rounded nose of radius ρ whose innermost point lies on the cutwater circle d_z
    // (Gülich Fig. 7.41b: the flow stagnates on the nose at r'_z = r_z + e₃/2 when
    // ρ = e₃/2), joined to the tip by its tangents.
    let radius = design.nose_radius;
    let (nose_center, nose_distance) = if radius > 0.0 {
        let c = DVec3::new(design.cutwater_diameter / 2.0 + radius, 0.0, zm);
        // The nose stays inside the wedge: its centre no farther out than the tip.
        let largest = tip.truncate().length() - design.cutwater_diameter / 2.0;
        if radius > largest {
            return Err(DesignError::InvalidInput(format!(
                "a cutwater radius of {:.1} mm puts the nose beyond the tip of the wedge: at \
                 most {:.1} mm here",
                radius * 1e3,
                largest * 1e3
            )));
        }
        (c, design.cutwater_diameter / 2.0)
    } else {
        (tip, tip.truncate().length())
    };
    Ok(CutwaterGeometry {
        tip,
        angle,
        wedge,
        radius,
        nose_center,
        nose_distance,
    })
}

/// A point of the diffuser centerline and its unit tangent.
type PathPoint = (DVec3, DVec3);

/// Centerline of the discharge diffuser in the midplane: positions and unit tangents
/// at `count + 1` equal steps of its length, the turning angle of the bend and the
/// length. A straight diffuser follows the flow at the throat; a radial one first turns
/// outward along an arc of radius R_N until its axis meets the line from the pump axis
/// at the requested angle.
fn centerline(
    design: &VoluteDesign,
    start: DVec3,
    tangent: DVec3,
    outward: DVec3,
    count: usize,
) -> Result<(Vec<PathPoint>, f64, f64), DesignError> {
    let total = design.diffuser.length;
    let (bend, radius) = match design.diffuser.shape {
        DischargeShape::Tangential { .. } => (0.0, 0.0),
        DischargeShape::Radial { axis_angle } => {
            let radius = design.diffuser.bend_radius;
            let center = start + outward * radius;
            let at = |phi: f64| {
                let p = center - outward * (radius * phi.cos()) + tangent * (radius * phi.sin());
                let t = outward * phi.sin() + tangent * phi.cos();
                (p, t)
            };
            // Angle from the radial direction at the end of the bend to its tangent,
            // positive toward the direction of the flow.
            let spin = tangent.truncate().perp_dot(outward.truncate()).signum();
            let angle = |phi: f64| {
                let (p, t) = at(phi);
                -spin
                    * p.truncate()
                        .perp_dot(t.truncate())
                        .atan2(p.truncate().dot(t.truncate()))
            };
            let target = axis_angle;
            if angle(std::f64::consts::PI) > target {
                return Err(DesignError::Infeasible(
                    "the radial discharge cannot turn to the requested axis angle".into(),
                ));
            }
            let phi =
                tp_math::roots::brent(|phi| angle(phi) - target, 1e-6, std::f64::consts::PI, 1e-12)
                    .map_err(|e| DesignError::Infeasible(e.to_string()))?;
            (phi, radius)
        }
    };
    // The bend is part of the diffuser length; a straight part follows if it is longer.
    let arc = radius * bend;
    let length = total.max(arc);
    let points = (0..=count)
        .map(|k| {
            let s = length * k as f64 / count as f64;
            if s <= arc && radius > 0.0 {
                let phi = s / radius;
                let center = start + outward * radius;
                (
                    center - outward * (radius * phi.cos()) + tangent * (radius * phi.sin()),
                    outward * phi.sin() + tangent * phi.cos(),
                )
            } else {
                let (end, t) = if radius > 0.0 {
                    let center = start + outward * radius;
                    (
                        center - outward * (radius * bend.cos()) + tangent * (radius * bend.sin()),
                        outward * bend.sin() + tangent * bend.cos(),
                    )
                } else {
                    (start, tangent)
                };
                (end + t * (s - arc), t)
            }
        })
        .collect();
    Ok((points, bend, length))
}

/// Sections of the discharge diffuser: the throat outline blends into a circle while
/// the equivalent radius grows linearly along the centerline to the outlet radius. A
/// straight diffuser keeps one wall straight or widens to both sides; a radial one
/// stays centred on its centerline.
fn nozzle(
    design: &VoluteDesign,
    throat: &VoluteSection,
    sign: f64,
    axis: DVec3,
) -> Result<(Vec<NozzleSection>, f64, f64), DesignError> {
    let theta = sign * TAU;
    let radial = DVec3::new(theta.cos(), theta.sin(), 0.0);
    let local_centroid = centroid(&throat.outline);
    let throat_points: Vec<DVec2> = throat.outline.iter().map(|p| *p - local_centroid).collect();
    let throat_area = polygon_area(&throat_points).abs();
    let r1 = (throat_area / std::f64::consts::PI).sqrt();
    let r2 = design.diffuser.outlet_diameter / 2.0;
    let start = DVec3::new(
        local_centroid.x * theta.cos(),
        local_centroid.x * theta.sin(),
        local_centroid.y,
    );
    // Across the centerline in the midplane, pointing away from the pump axis at the
    // throat; it turns with the tangent along a bend.
    let u0 = (radial - axis * radial.dot(axis)).normalize();
    let side = axis.cross(DVec3::Z).dot(u0).signum();
    let across = |t: DVec3| t.cross(DVec3::Z) * side;
    let (path, bend, length) = centerline(design, start, axis, u0, NOZZLE_SECTIONS)?;
    // Offsets across the axis of the straight walls: the cutwater side passes through
    // the stagnation point, the outer one through the outermost point of the throat.
    let tip = radial * design.stagnation_radius;
    let inner = (tip - start).dot(u0);
    let outer = throat
        .points(sign)
        .iter()
        .map(|p| (*p - start).dot(u0))
        .fold(f64::NEG_INFINITY, f64::max);
    let circle: Vec<DVec2> = throat_points
        .iter()
        .map(|p| r1 * DVec2::new(p.x, p.y).normalize())
        .collect();
    // The sampled circle encloses a little less than π·r²; scale against it so the
    // outlet points lie exactly on the outlet radius.
    let circle_area = polygon_area(&circle).abs();
    let sections = path
        .iter()
        .enumerate()
        .map(|(k, &(position, tangent))| {
            let s = k as f64 / NOZZLE_SECTIONS as f64;
            if k == 0 {
                return NozzleSection {
                    center: start,
                    tangent,
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
            let highest = blend.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max) * scale;
            let u = across(tangent);
            let shift = match design.diffuser.shape {
                DischargeShape::Tangential {
                    straight_wall: StraightWall::Inner,
                } => inner - lowest,
                DischargeShape::Tangential {
                    straight_wall: StraightWall::Outer,
                } => outer - highest,
                _ => 0.0,
            };
            let center = position + u * shift;
            NozzleSection {
                center,
                tangent,
                points: blend
                    .iter()
                    .map(|p| center + u * (p.x * scale) + DVec3::Z * (p.y * scale))
                    .collect(),
                area: std::f64::consts::PI * radius * radius,
            }
        })
        .collect();
    Ok((sections, bend, length))
}

#[cfg(test)]
mod tests {
    use super::super::tests::reference_basis;
    use super::*;
    use crate::volute::AreaLaw;

    #[test]
    fn outlines_offset_outward_in_their_plane() {
        // A circle in a tilted plane grows by the offset, in that plane, either way
        // round.
        let (u, v) = (
            DVec3::new(1.0, 1.0, 0.0).normalize(),
            DVec3::new(0.0, 0.0, 1.0),
        );
        let c = DVec3::new(0.3, -0.1, 0.05);
        let circle: Vec<DVec3> = (0..72)
            .map(|k| {
                let a = TAU * f64::from(k) / 72.0;
                c + 0.02 * (u * a.cos() + v * a.sin())
            })
            .collect();
        for ring in [circle.clone(), circle.iter().rev().copied().collect()] {
            for p in offset_ring(&ring, 0.005) {
                assert_relative_eq!(p.distance(c), 0.025, max_relative = 2e-3);
                assert_relative_eq!((p - c).dot(u.cross(v)), 0.0, epsilon = 1e-12);
            }
        }
        // A rectangle keeps its sharp corners, mitred.
        let rect = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.04, 0.0, 0.0),
            DVec3::new(0.04, 0.0, 0.02),
            DVec3::new(0.0, 0.0, 0.02),
        ];
        let out = offset_ring(&rect, 0.005);
        assert!(out[0].distance(DVec3::new(-0.005, 0.0, -0.005)) < 1e-12);
        assert!(out[2].distance(DVec3::new(0.045, 0.0, 0.025)) < 1e-12);
    }

    #[test]
    fn the_casing_wraps_every_section_of_the_flow_domain() {
        let g = VoluteGeometry::build(&reference_basis(), &VoluteInput::default()).unwrap();
        let t = g.design.wall_thickness;
        let (flow, casing) = (g.lofts(), g.casing_lofts());
        assert_eq!(flow.len(), casing.len());
        for (rings, outer) in flow.iter().zip(&casing) {
            assert_eq!(rings.len(), outer.len());
            for (ring, wall) in rings.iter().zip(outer) {
                assert_eq!(ring.len(), wall.len());
                // Each point keeps at least the wall thickness from the flow outline.
                for p in wall {
                    let gap = ring.iter().map(|q| q.distance(*p)).fold(f64::MAX, f64::min);
                    assert!(gap > 0.95 * t, "{gap} vs {t}");
                }
            }
        }
    }
    use approx::assert_relative_eq;

    /// Default inputs with a straight (tangential) discharge and the cutwater kept on
    /// the x axis.
    fn unturned() -> VoluteInput {
        let mut input = VoluteInput::default();
        input.diffuser.shape = DischargeShape::Tangential {
            straight_wall: StraightWall::Centered,
        };
        input.diffuser.rotation = crate::Param::Manual(0.0);
        input
    }

    fn geometry(shape: SectionShape) -> VoluteGeometry {
        let mut input = unturned();
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
    fn the_flange_height_sets_the_diffuser_length() {
        use crate::volute::{DischargeShape, StraightWall};
        for shape in [
            DischargeShape::default(),
            DischargeShape::Tangential {
                straight_wall: StraightWall::Centered,
            },
        ] {
            let mut input = VoluteInput::default();
            input.diffuser.shape = shape;
            let free = VoluteGeometry::build(&reference_basis(), &input).unwrap();
            // 50 mm higher: the diffuser grows by as much along its straight part.
            input.diffuser.flange_height = Param::Manual(free.outlet_height + 0.05);
            let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
            assert_relative_eq!(
                g.outlet_height,
                free.outlet_height + 0.05,
                max_relative = 1e-9
            );
            assert_relative_eq!(
                g.design.diffuser.length,
                free.nozzle_length + 0.05,
                max_relative = 1e-9
            );
            // The design sees the longer diffuser: it may widen more.
            assert!(g.design.diffuser.outlet_diameter >= free.design.diffuser.outlet_diameter);
            // An outlet face that the diffuser cannot reach is refused: for a radial
            // discharge anything inside the bend, for a tangential one the throat.
            let unreachable = match shape {
                DischargeShape::Radial { .. } => free.basis.impeller_diameter / 2.0,
                _ => 1e-3,
            };
            input.diffuser.flange_height = Param::Manual(unreachable);
            assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
        }
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
        let mirrored = VoluteGeometry::build(&negative, &unturned()).unwrap();
        assert!(mirrored.nozzle_axis.y < -0.9);
    }

    #[test]
    fn diffuser_wall_on_the_cutwater_side_is_straight() {
        let mut input = unturned();
        input.diffuser.shape = DischargeShape::Tangential {
            straight_wall: StraightWall::Inner,
        };
        let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
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
    fn the_volute_turns_to_a_vertical_discharge() {
        assert_eq!(
            DischargeShape::default(),
            DischargeShape::Radial { axis_angle: 0.0 }
        );
        for shape in [
            DischargeShape::Tangential {
                straight_wall: StraightWall::Centered,
            },
            DischargeShape::default(),
        ] {
            let mut input = VoluteInput::default();
            input.diffuser.shape = shape;
            let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
            assert_relative_eq!(g.nozzle_axis.y, 1.0, epsilon = 1e-9);
            // Sections keep their wrap angle from the turned cutwater, and the diffuser
            // still starts from the throat.
            let s = &g.sections[10];
            let p = s.points(1.0)[0];
            let theta = (p.y.atan2(p.x) - g.rotation).rem_euclid(TAU);
            assert_relative_eq!(theta, s.angle, epsilon = 1e-9);
            let throat = g.sections.last().unwrap().points(1.0);
            for (a, b) in g.nozzle[0].points.iter().zip(&throat) {
                assert!(a.distance(*b) < 1e-12);
            }
            let tip = g.cutwater.tip;
            let at = (tip.y.atan2(tip.x) - g.rotation).rem_euclid(TAU);
            assert_relative_eq!(at, g.cutwater.angle, epsilon = 1e-9);
            if let DischargeShape::Radial { .. } = shape {
                // A central discharge stands right above the pump axis.
                let out = g.nozzle.last().unwrap();
                assert!(out.center.x.abs() < 1e-9, "{}", out.center.x);
                assert_relative_eq!(out.center.y, g.outlet_height, max_relative = 1e-9);
            }
        }
        // A given turn puts the cutwater there.
        let mut input = VoluteInput::default();
        input.diffuser.rotation = crate::Param::Manual(1.0);
        let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        assert_relative_eq!(g.rotation, 1.0);
        let p = g.sections[0].points(1.0)[0];
        assert_relative_eq!(p.y.atan2(p.x), 1.0 + g.sections[0].angle, epsilon = 1e-9);
    }

    #[test]
    fn a_tangent_cutwater_starts_the_spiral_clear_of_the_diffuser() {
        for law in [AreaLaw::AngularMomentum, AreaLaw::ConstantVelocity] {
            let mut input = unturned();
            input.spiral.law = law;
            input.cutwater.shape = CutwaterShape::Tangent;
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

    fn double(wrap: Option<f64>) -> VoluteGeometry {
        let mut input = unturned();
        input.setup.kind = crate::volute::VoluteKind::Double;
        if let Some(w) = wrap {
            input.setup.rib_wrap = crate::Param::Manual(w);
        }
        VoluteGeometry::build(&reference_basis(), &input).unwrap()
    }

    #[test]
    fn a_double_volute_has_an_inner_volute_and_an_outer_channel() {
        let g = double(None);
        let rib = g.design.rib.unwrap();
        // Seven blades: the rib wraps 180°.
        assert_relative_eq!(rib.wrap, std::f64::consts::PI);
        assert_relative_eq!(g.sections.last().unwrap().angle, rib.start);
        assert_relative_eq!(g.outer_channel.first().unwrap().angle, rib.start);
        assert_relative_eq!(g.outer_channel.last().unwrap().angle, TAU);
        assert_relative_eq!(g.inner_sections.last().unwrap().angle, TAU);
        assert!(g.inner_sections.first().unwrap().angle > rib.start);
        // The rib keeps the outer channel off the inner volute by its thickness.
        for c in &g.outer_channel {
            let inner = g.design.section(c.angle).unwrap();
            let gap = c.outline.iter().map(|p| p.x).fold(f64::INFINITY, f64::min)
                - if c.angle > rib.start {
                    inner.outer_radius
                } else {
                    g.design.stagnation_radius
                };
            assert_relative_eq!(gap, rib.thickness, epsilon = 1e-6);
            assert_relative_eq!(
                polygon_area(&c.outline).abs(),
                rib.channel_area(c.angle),
                max_relative = 1e-5
            );
        }
        // Three pieces to loft, each with the same outline structure.
        let channels = g.channels();
        assert_eq!(channels.len(), 3);
        for ring in channels.iter().flatten() {
            assert_eq!(ring.len(), OUTLINE_POINTS);
        }
        // The diffuser starts from both channels and the rib between them.
        let first = &g.nozzle[0];
        let outer = g.outer_channel.last().unwrap();
        assert!(first.area > g.design.throat.area + outer.size.area);
        assert_eq!(g.nozzle.len(), NOZZLE_SECTIONS + 1);
    }

    #[test]
    fn the_rib_splits_whole_sections_and_has_an_elliptic_nose() {
        let g = double(None);
        let rib = g.design.rib.unwrap();
        // The whole section continues the first partial volute at the rib nose.
        let (throat, start) = (g.sections.last().unwrap(), &g.envelope[0]);
        assert_relative_eq!(start.angle, rib.start);
        for (a, b) in throat.outline.iter().zip(&start.outline) {
            assert!(a.distance(*b) < 1e-9);
        }
        // The inner volute, the rib and the outer channel fill it.
        let shape = g.design.shape();
        for e in &g.envelope[1..] {
            let a = g.design.section(e.angle).unwrap().height;
            assert_relative_eq!(
                polygon_area(&e.outline).abs(),
                shape.area(a + rib.thickness) + rib.channel_area(e.angle),
                max_relative = 1e-6
            );
        }
        assert_eq!(g.lofts().len(), 2);
        // The rib lies on the inner volute's wall, one thickness thick.
        let r = g.rib.as_ref().unwrap();
        let radius = |p: &DVec3| p.truncate().length();
        assert_relative_eq!(radius(&r.inner_face[0]), g.design.stagnation_radius);
        for (i, o) in r.inner_face.iter().zip(&r.outer_face) {
            assert_relative_eq!(radius(o) - radius(i), rib.thickness, epsilon = 1e-12);
        }
        // A semicircular nose half a thickness ahead of the inner volute, pointing along
        // the flow angle at the cutwater.
        let mid = 0.5 * (r.inner_face[0] + r.outer_face[0]);
        assert_relative_eq!(
            r.nose_tip.distance(mid),
            rib.thickness / 2.0,
            max_relative = 1e-9
        );
        let chord = (mid - r.nose_tip).normalize();
        let circumferential = DVec3::new(-mid.y, mid.x, 0.0).normalize();
        let radial = DVec3::new(mid.x, mid.y, 0.0).normalize();
        let pitch = chord.dot(radial).atan2(chord.dot(circumferential));
        assert_relative_eq!(
            pitch,
            g.design.cutwater_flow_angle,
            epsilon = 0.5f64.to_radians()
        );
        assert!(r.nose_angle < rib.start && r.nose_angle > rib.start - 2f64.to_radians());
        assert!((r.nose_distance - g.design.stagnation_radius).abs() < 1e-4);
        // One closed outline; the tail mirrors the nose behind the throat.
        assert_eq!(r.outline().len(), 2 * (RIB_FACE_POINTS + RIB_NOSE_POINTS));
        assert_relative_eq!(
            radius(&r.tail[RIB_NOSE_POINTS / 2]),
            radius(&r.inner_face[RIB_FACE_POINTS]) + rib.thickness / 2.0,
            epsilon = 1e-3
        );
        // An elliptic nose three times longer.
        let mut input = unturned();
        input.setup.kind = crate::volute::VoluteKind::Double;
        input.setup.rib_nose_ratio = crate::Param::Manual(3.0);
        let long = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        let r = long.rib.unwrap();
        let mid = 0.5 * (r.inner_face[0] + r.outer_face[0]);
        assert_relative_eq!(
            r.nose_tip.distance(mid),
            1.5 * rib.thickness,
            max_relative = 1e-9
        );
    }

    #[test]
    fn rectangles_and_trapezoids_build_as_a_double_volute() {
        for shape in [SectionShape::Rectangular, SectionShape::default()] {
            let mut input = unturned();
            input.setup.kind = crate::volute::VoluteKind::Double;
            input.cross_section.shape = shape;
            let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
            assert!(!g.inner_sections.is_empty() && !g.outer_channel.is_empty());
            assert_eq!(g.envelope.len(), g.outer_channel.len());
        }
        // A shorter rib moves the nose later.
        let short = double(Some(150f64.to_radians()));
        assert_relative_eq!(short.design.rib.unwrap().start, 210f64.to_radians());
    }

    fn with_discharge(shape: DischargeShape) -> VoluteGeometry {
        let mut input = unturned();
        input.diffuser.shape = shape;
        VoluteGeometry::build(&reference_basis(), &input).unwrap()
    }

    /// Smallest and largest offset of the outline points across the centerline.
    fn spread(n: &NozzleSection, sign_across: DVec3) -> (f64, f64) {
        n.points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                let d = (*p - n.center).dot(sign_across);
                (lo.min(d), hi.max(d))
            })
    }

    #[test]
    fn a_central_radial_discharge_points_away_from_the_pump_axis() {
        let g = with_discharge(DischargeShape::Radial { axis_angle: 0.0 });
        let d = &g.design.diffuser;
        // Eq. 7.29 with the throat area.
        let expected = 1.5 * (4.0 * g.design.throat.area / std::f64::consts::PI).sqrt();
        assert_relative_eq!(d.bend_radius, expected, max_relative = 1e-12);
        assert!(
            g.nozzle_bend > 0.5 && g.nozzle_bend < 2.5,
            "{}",
            g.nozzle_bend
        );
        // The straight part lies on a line through the pump axis.
        let out = g.nozzle.last().unwrap();
        let cross = out.center.truncate().perp_dot(out.tangent.truncate());
        assert!(cross.abs() < 1e-6 * out.center.length(), "{cross}");
        assert!(out.center.truncate().dot(out.tangent.truncate()) > 0.0);
        // The bend is part of the diffuser length, and a short straight part follows;
        // the outlet face is that far from the pump axis.
        let arc = d.bend_radius * g.nozzle_bend;
        assert!(arc < d.length, "{arc} vs {}", d.length);
        assert_relative_eq!(g.nozzle_length, d.length, max_relative = 1e-12);
        assert_relative_eq!(
            g.outlet_height,
            out.center.truncate().length(),
            max_relative = 1e-9
        );
        assert!(
            g.outlet_height < 1.5 * g.basis.impeller_diameter,
            "{}",
            g.outlet_height
        );
        // A short diffuser ends with the bend.
        let mut input = unturned();
        input.diffuser.shape = DischargeShape::Radial { axis_angle: 0.0 };
        input.diffuser.length = crate::Param::Manual(0.5 * arc);
        let short = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        assert_relative_eq!(short.nozzle_length, arc, max_relative = 1e-6);
        assert!(
            short
                .warnings
                .iter()
                .any(|w| w.contains("ends with the bend"))
        );
        let outlet = std::f64::consts::PI * d.outlet_diameter * d.outlet_diameter / 4.0;
        assert_relative_eq!(out.area, outlet, max_relative = 1e-12);
        for n in &g.nozzle {
            assert_relative_eq!(n.tangent.length(), 1.0, epsilon = 1e-12);
        }
    }

    #[test]
    fn a_tilted_radial_discharge_turns_less() {
        let central = with_discharge(DischargeShape::Radial { axis_angle: 0.0 });
        let tilted = with_discharge(DischargeShape::Radial {
            axis_angle: 30f64.to_radians(),
        });
        assert!(tilted.nozzle_bend < central.nozzle_bend);
        let out = tilted.nozzle.last().unwrap();
        let c = out.center.truncate();
        let t = out.tangent.truncate();
        // The angle is measured at the end of the bend; along the straight part the
        // centre drifts, so check at the first straight section.
        let first = tilted
            .nozzle
            .iter()
            .find(|n| (n.tangent - out.tangent).length() < 1e-9)
            .unwrap();
        let p = first.center.truncate();
        let angle = p.perp_dot(t).atan2(p.dot(t)).abs();
        assert!(
            (angle - 30f64.to_radians()).abs() < 5f64.to_radians(),
            "{angle}"
        );
        assert!(c.length() > p.length());
    }

    #[test]
    fn a_straight_discharge_keeps_the_chosen_wall_straight() {
        for (wall, inner_fixed) in [(StraightWall::Inner, true), (StraightWall::Outer, false)] {
            let g = with_discharge(DischargeShape::Tangential {
                straight_wall: wall,
            });
            let axis = g.nozzle_axis;
            let u = axis.cross(DVec3::Z);
            let u = if u.dot(DVec3::X) > 0.0 { u } else { -u };
            let edges: Vec<(f64, f64)> = g
                .nozzle
                .iter()
                .map(|n| {
                    let (lo, hi) = spread(n, u);
                    let c = n.center.dot(u);
                    (c + lo, c + hi)
                })
                .collect();
            let fixed: Vec<f64> = edges
                .iter()
                .map(|e| if inner_fixed { e.0 } else { e.1 })
                .collect();
            for f in &fixed {
                assert_relative_eq!(*f, fixed[0], epsilon = 1e-9);
            }
        }
        // Centred: both walls open.
        let g = with_discharge(DischargeShape::Tangential {
            straight_wall: StraightWall::Centered,
        });
        let first = &g.nozzle[1];
        let last = g.nozzle.last().unwrap();
        assert_relative_eq!(
            (last.center - g.nozzle[0].center)
                .cross(g.nozzle_axis)
                .length(),
            0.0,
            epsilon = 1e-9
        );
        assert!(last.area > first.area);
    }

    #[test]
    fn a_rounded_cutwater_has_its_nose_on_the_cutwater_circle() {
        let mut input = unturned();
        input.cutwater.shape = CutwaterShape::Sharp;
        let sharp = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        let g = VoluteGeometry::build(&reference_basis(), &unturned()).unwrap();
        let (c, d) = (g.cutwater, &g.design);
        // Default: rounded with ρ = e₃/2, so the nose is centred on r'_z.
        assert_relative_eq!(c.radius, d.cutwater_thickness / 2.0);
        assert_relative_eq!(c.nose_distance, d.cutwater_diameter / 2.0);
        assert_relative_eq!(c.nose_center.x, d.stagnation_radius, epsilon = 1e-12);
        // The spiral starts at the cutwater and leaves the diffuser past it, at the
        // tip of the sharp wedge, a few millimetres outside r'_z.
        assert!(g.start_angle < 5f64.to_radians());
        assert_eq!(c.tip, sharp.cutwater.tip);
        assert!(c.angle > 5f64.to_radians() && c.angle < 30f64.to_radians());
        let tip = c.tip.truncate().length();
        assert!(tip > d.stagnation_radius && tip < 1.08 * d.stagnation_radius);
        assert!(c.wedge > 1f64.to_radians() && c.wedge < 20f64.to_radians());
        // The nose outline: an arc on the far side from the tip, then the tip.
        let outline = c.nose_outline();
        assert_eq!(*outline.last().unwrap(), c.tip);
        for p in &outline[..outline.len() - 1] {
            assert_relative_eq!((*p - c.nose_center).length(), c.radius, epsilon = 1e-12);
        }
        assert!(sharp.cutwater.nose_outline().is_empty());
        // Too large a radius reaches the tip.
        input.cutwater.shape = CutwaterShape::Rounded {
            radius: crate::Param::Manual(0.02),
        };
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
        // Two pieces now: the spiral reaches into the diffuser.
        assert_eq!(g.channels().len(), 2);
    }
}
