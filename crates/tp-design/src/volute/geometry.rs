//! Three-dimensional geometry of the volute: cross sections along the spiral and the
//! sections of the discharge diffuser.
//!
//! The cutwater sits on the +x axis (θ = 0). Wrap angles ε grow in the direction of
//! rotation, so a section at ε lies at θ = sign·ε. Every outline has the same number
//! of points, in the same order, so consecutive outlines can be joined into surfaces.

use std::f64::consts::{PI, TAU};

use tp_math::{DVec2, DVec3};

use super::cutwater::CutwaterSlice;
use super::section::Bend;
use super::stress::WallStress;
use super::{
    CrossSections, CutwaterShape, DiffuserGrowth, DischargeShape, OutletShape, SectionSize,
    VoluteDesign, VoluteInput, ZAlignment, ZBend,
};
use crate::outlet::ImpellerOutlet;
use crate::{DesignError, Param};

/// Sections of the discharge diffuser after the throat.
const NOZZLE_SECTIONS: usize = 10;
const DEFAULT_SECTIONS: u32 = 36;
/// Share of the length of the diffuser over which the height offset builds up.
const OFFSET_REACH: f64 = 0.4;
/// Share of the length of the diffuser over which its base side rounds.
const BASE_REACH: f64 = 0.3;

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

/// How the last section of the spiral runs into the diffuser, measured along the path
/// of the section centroids.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThroatJunction {
    /// Growth of the area per metre of path in the first step of the diffuser over that
    /// of the last step of the spiral: 1 when the area law has no break at the throat.
    pub area_slope_ratio: f64,
    /// Curvature of the centroid path over the last three sections of the spiral, 1/m.
    pub curvature_before: f64,
    /// Curvature of the centroid path over the first three sections of the diffuser, 1/m.
    pub curvature_after: f64,
    /// Turn of the path direction across the throat, rad.
    pub direction_turn: f64,
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
    /// Centre of the circle of the rounded nose (the tip when sharp), m.
    pub nose_center: DVec3,
    /// Distance from the pump axis to the nearest point of the cutwater: half the
    /// effective cutwater diameter, m.
    pub nose_distance: f64,
    /// Angle of the outer wall of the spiral at the tip from the circumferential
    /// direction, positive away from the pump axis, rad; 0 without a wedge.
    pub spiral_angle: f64,
    /// The same for the inner wall of the diffuser, rad. The wedge is the difference.
    pub diffuser_angle: f64,
}

impl CutwaterGeometry {
    /// Mean of the angles of the two walls: the direction of the wedge, rad.
    pub fn mean_angle(&self) -> f64 {
        0.5 * (self.spiral_angle + self.diffuser_angle)
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
    /// The rib in planes z = const across its height, from −z to +z: the nose turns
    /// from plane to plane with the obliquity of the cutwater, the tail stays.
    pub planes: Vec<RibPlane>,
}

/// The rib of a double volute in one plane z = const; each plane has as many points on
/// each part as the others.
#[derive(Debug, Clone, PartialEq)]
pub struct RibPlane {
    pub z: f64,
    pub inner_face: Vec<DVec3>,
    pub outer_face: Vec<DVec3>,
    pub nose: Vec<DVec3>,
    pub tail: Vec<DVec3>,
}

impl RibPlane {
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
/// Planes across the height of the rib.
pub const RIB_PLANES: usize = 9;

/// Spiral start of a sharp or rounded cutwater, rad: the spiral overlaps the diffuser
/// from there to the cutwater.
const CUTWATER_START: f64 = 3.0 * std::f64::consts::PI / 180.0;
/// Smallest angle between the spiral and the diffuser wall for a sharp cutwater; below
/// it the two only graze and the spiral starts where it clears the wall, rad.
const MIN_WEDGE: f64 = 4.0 * std::f64::consts::PI / 180.0;
/// Earliest start of the spiral of a sharp or rounded cutwater, rad.
const MIN_START: f64 = 1.0 * std::f64::consts::PI / 180.0;
/// The spiral starts at least this much before its wall crosses the diffuser's in every
/// plane, rad.
const START_MARGIN: f64 = 0.5 * std::f64::consts::PI / 180.0;
/// Least wrap of the spiral, from its start to the throat, for a sharp or rounded
/// cutwater to be built, rad.
const MIN_WRAP: f64 = 330.0 * std::f64::consts::PI / 180.0;
/// Wrap of the spiral past its start in which the cutwater planes look for the walls,
/// rad.
const CUTWATER_SPAN: f64 = 60.0 * std::f64::consts::PI / 180.0;

/// Complete volute geometry.
#[derive(Debug, Clone)]
pub struct VoluteGeometry {
    pub basis: ImpellerOutlet,
    pub design: VoluteDesign,
    /// Wrap angle of the first section, rad; the cutwater fills the space before it.
    pub start_angle: f64,
    /// Starts of the spiral of a sharp or rounded cutwater that see its wall cross the
    /// diffuser's in every plane across the inlet width, rad; `None` when no start does
    /// or the cutwater is of another kind.
    pub start_range: Option<(f64, f64)>,
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
    /// Double volute whose rib stops short of the throat: the whole sections from the end
    /// of the rib to 2π, where the two channels have joined; empty otherwise.
    pub merged: Vec<VoluteSection>,
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
    /// Inner wall of a spiral with the cutwater compensation, where it rises off r'_z,
    /// in the midplane; empty without it.
    pub inner_wall: Vec<DVec3>,
    /// Membrane stresses in the casing wall.
    pub wall_stress: WallStress,
    pub cutwater: CutwaterGeometry,
    /// A sharp or rounded cutwater in planes across the inlet width, from −z to +z;
    /// empty for a tangent one, a shorter spiral or an internal volute.
    pub cutwater_slices: Vec<CutwaterSlice>,
    /// The body of a rounded cutwater as outlines from −z to +z, between its planes
    /// too and widening into the side walls ([`super::cutwater::body_outlines`]).
    cutwater_body: Vec<Vec<DVec3>>,
    /// Internal sections: the bend and the neck from the inlet to the spiral, in the
    /// plane of a section (s, h), the same all round.
    pub bend: Option<Bend>,
    pub warnings: Vec<String>,
}

/// Outlines of the body of the cutwater per gap between two of its planes, for meshes.
const BODY_STEPS: usize = 4;

/// How far the mean direction of the walls at the cutwater tip may be from the flow
/// angle α₃' before it is flagged: Gülich (eq. T7.7.6) asks for an incidence within ±3°.
const WALL_ALIGNMENT_BAND: f64 = 3.0 * std::f64::consts::PI / 180.0;

/// Turn by which the channels of a double volute reach into the discharge diffuser, so
/// that the solids overlap where they meet, rad.
const CHANNEL_OVERLAP: f64 = 1.5 * std::f64::consts::PI / 180.0;

/// A value at the lower side of the cutwater (the lowest plane), at its upper side (the
/// highest) and their mean over the planes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcrossWidth {
    pub lower: f64,
    pub upper: f64,
    pub mean: f64,
}

/// A flow cross section by its area and the diameter of the circle of that area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowSection {
    /// Equivalent diameter √(4A/π), m.
    pub equivalent_diameter: f64,
    /// m²
    pub area: f64,
}

impl FlowSection {
    fn of(area: f64) -> Self {
        Self {
            equivalent_diameter: (4.0 * area / std::f64::consts::PI).sqrt(),
            area,
        }
    }
}

/// The values beside the cutwater that the design shows, from the planes and the flow
/// sections as built. The cutwater section is the start of the spiral: the cutwater
/// fills the space before it.
#[derive(Debug, Clone, PartialEq)]
pub struct CutwaterValues {
    /// Diameter of the nose circle in each plane, 2·nose distance, m.
    pub diameter: AcrossWidth,
    /// The least nose diameter over the planes, m: the effective cutwater diameter.
    pub least_diameter: f64,
    /// Wedge angle at the tip in each plane, rad.
    pub angle: AcrossWidth,
    /// Wrap angle of the tip in the midplane, rad (the cutwater position φC).
    pub position: f64,
    /// Start of the spiral, the cutwater section.
    pub start: FlowSection,
    /// Throat, the first section of the discharge diffuser.
    pub throat: FlowSection,
    /// Outlet of the discharge diffuser.
    pub outlet: FlowSection,
}

impl VoluteGeometry {
    /// The values beside the cutwater; `None` where the design has no planes or
    /// sections to take them from.
    pub fn cutwater_values(&self) -> Option<CutwaterValues> {
        let slices = &self.cutwater_slices;
        let lower = slices.iter().min_by(|a, b| a.z.total_cmp(&b.z))?;
        let upper = slices.iter().max_by(|a, b| a.z.total_cmp(&b.z))?;
        let mean =
            |f: fn(&CutwaterSlice) -> f64| slices.iter().map(f).sum::<f64>() / slices.len() as f64;
        let across = |f: fn(&CutwaterSlice) -> f64| AcrossWidth {
            lower: f(lower),
            upper: f(upper),
            mean: mean(f),
        };
        let least = slices
            .iter()
            .map(|s| s.nose_distance)
            .fold(f64::INFINITY, f64::min);
        Some(CutwaterValues {
            diameter: across(|s| 2.0 * s.nose_distance),
            least_diameter: 2.0 * least,
            angle: across(|s| s.wedge),
            position: self.cutwater.angle,
            start: FlowSection::of(self.sections.first()?.size.area),
            throat: FlowSection::of(self.nozzle.first()?.area),
            outlet: FlowSection::of(self.nozzle.last()?.area),
        })
    }

    /// Smooth outline in the midplane of the material a rounded cutwater adds to the
    /// sharp wedge: from where it joins the spiral wall, round the nose, to where it
    /// joins the diffuser wall, the chord back closing it inside the wedge. Empty unless
    /// rounded.
    pub fn cutwater_outline(&self) -> &[DVec3] {
        let zm = self.basis.midplane;
        self.cutwater_slices
            .iter()
            .find(|s| (s.z - zm).abs() < 1e-12)
            .map_or(&[], |s| s.outline.as_slice())
    }

    /// Outlines of the body of a rounded cutwater from −z to +z, between its planes
    /// too and widening into the side walls (see [`super::cutwater::body_outlines`]), for
    /// the meshes and the CAD model. Empty unless rounded.
    pub fn cutwater_body(&self) -> Vec<Vec<DVec3>> {
        self.cutwater_body.clone()
    }

    /// The arcs that round the tip of a fillet from plane to plane, between the planes
    /// too: the surface of its nose, which runs on into the walls. Empty for any other
    /// cutwater.
    pub fn fillet_nose(&self) -> Vec<Vec<DVec3>> {
        if !self.design.cutwater_shape.is_fillet() {
            return Vec::new();
        }
        self.cutwater_body
            .iter()
            .map(|o| super::cutwater::arc_of(o).to_vec())
            .filter(|a| !a.is_empty())
            .collect()
    }

    /// The outlines of the body in the planes of the cutwater, as built: with the side
    /// endings where a plane lies close to a side wall. Empty unless rounded.
    pub fn cutwater_planes(&self) -> Vec<Vec<DVec3>> {
        self.cutwater_slices
            .iter()
            .filter_map(|s| {
                self.cutwater_body
                    .iter()
                    .find(|o| o.first().is_some_and(|p| (p.z - s.z).abs() < 1e-9))
                    .cloned()
            })
            .collect()
    }

    /// The pieces of the flow channel, each a sequence of outlines in 3D to loft
    /// through, for surface meshes. A single volute is one piece from the start of the
    /// spiral to the diffuser outlet. A double volute has three: the first partial
    /// volute with the outer channel, the inner volute, and the discharge diffuser; the
    /// first two reach a little into the third. With a rib that stops short of the throat
    /// there is one more, the whole section from the end of the rib, before the diffuser.
    pub fn channels(&self) -> Vec<Vec<Vec<DVec3>>> {
        // An internal spiral starts inside the diffuser: they cut each other.
        let joined =
            matches!(self.design.cutwater_shape, CutwaterShape::Tangent) && self.bend.is_none();
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
        // A double volute of sections the rib cuts is one channel with the rib in it, as
        // the CAD model builds it.
        if self.design.free_double().is_some() {
            return self.lofts();
        }
        // The throat of the first partial volute and the start of the outer channel
        // share the angle of the rib nose: the channel takes over there. A rib that
        // stops short of the throat lets the two channels run into the whole section.
        let mut pieces = vec![
            self.extended(&self.followed_by(&self.outer_channel)),
            self.extended(&self.inner_sections),
        ];
        if !self.merged.is_empty() {
            pieces.push(self.extended(&self.merged));
        }
        pieces.push(self.nozzle_rings());
        pieces
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

    /// Indices where the pieces of every outline start: the inlet points, the top and
    /// the corners the cross sections share ([`CrossSections::loft_breaks`]). The
    /// outline is smooth inside a piece and may have a corner between pieces.
    pub fn ring_breaks(&self) -> Vec<usize> {
        self.design.cross_sections.loft_breaks()
    }

    /// Points of every outline of the spiral.
    pub fn outline_points(&self) -> usize {
        self.design.cross_sections.loft_points()
    }

    /// A point (x, y) of the plane of the design, x toward the cutwater and wrap angles
    /// counter-clockwise, in the midplane of the turned volute: the circular arcs of the
    /// spiral are given in that plane.
    pub fn plane_point(&self, p: DVec2) -> DVec3 {
        let q = DVec2::new(p.x, self.basis.rotation_sign * p.y);
        let (s, c) = self.rotation.sin_cos();
        DVec3::new(c * q.x - s * q.y, s * q.x + c * q.y, self.basis.midplane)
    }

    /// Inner and outer radius of the vaneless ring between impeller and volute.
    pub fn inlet_ring(&self) -> (f64, f64) {
        (
            self.basis.impeller_diameter / 2.0,
            self.design.stagnation_radius,
        )
    }

    /// (r, z) of a point (s, h) in the plane of a section.
    fn meridional(&self, p: DVec2) -> DVec2 {
        DVec2::new(
            self.design.stagnation_radius + p.y,
            self.basis.midplane + p.x,
        )
    }

    /// The profile in the meridional plane (r, z) to revolve into the fluid between the
    /// impeller and the spiral, as pieces that are smooth inside and closed together.
    ///
    /// For a volute outside r'_z it is the vaneless ring from the impeller outlet to a
    /// little past r'_z, so that it overlaps the spiral, and a hair narrower than b₃, so
    /// that its sides do not run on the flat side walls of rectangular sections. An
    /// internal volute adds its bend and neck. Their end reaches a little into the
    /// spiral, a hair lower than the neck at the face, so that no wall of the neck runs
    /// on the flat top of the spiral.
    pub fn inlet_profile(&self) -> Vec<Vec<DVec2>> {
        let (inner, outer) = self.inlet_ring();
        let b3 = self.design.inlet_width;
        let at = |p: DVec2| self.meridional(p);
        let Some(bend) = &self.bend else {
            let half = 0.995 * b3 / 2.0;
            let h = (outer + 0.02 * b3) - self.design.stagnation_radius;
            let low = inner - self.design.stagnation_radius;
            let corners = [
                DVec2::new(-half, low),
                DVec2::new(-half, h),
                DVec2::new(half, h),
                DVec2::new(half, low),
            ];
            return (0..4)
                .map(|i| vec![at(corners[i]), at(corners[(i + 1) % 4])])
                .collect();
        };
        let low = inner - self.design.stagnation_radius;
        let neck = bend.top - bend.floor;
        // Toward the spiral: the sign of the face.
        let away = bend.face.signum();
        let (lip, overlap, hair) = (0.02 * neck, 0.02 * neck, 0.005 * neck);
        let end = |wall: &[Vec<DVec2>]| -> Vec<Vec<DVec2>> {
            // The wall with its last piece, along the neck, stopped short of the face.
            let mut pieces = wall.to_vec();
            if let Some(last) = pieces.last_mut() {
                let n = last.len();
                last[n - 1].x = bend.face - away * lip;
            }
            pieces
        };
        let mut pieces: Vec<Vec<DVec2>> = Vec::new();
        // Up the vaneless ring on the side of the outer wall, round the bend and along
        // the top of the neck.
        let outer_start = bend.outer[0][0];
        pieces.push(vec![DVec2::new(outer_start.x, low), outer_start]);
        pieces.extend(end(&bend.outer));
        let top_end = DVec2::new(bend.face - away * lip, bend.top);
        let floor_end = DVec2::new(bend.face - away * lip, bend.floor);
        let top_in = DVec2::new(bend.face + away * overlap, bend.top - hair);
        let floor_in = DVec2::new(bend.face + away * overlap, bend.floor + hair);
        pieces.push(vec![top_end, top_in]);
        pieces.push(vec![top_in, floor_in]);
        pieces.push(vec![floor_in, floor_end]);
        // Back along the floor of the neck, round the inner wall to the inlet and down
        // the vaneless ring.
        for piece in end(&bend.inner).iter().rev() {
            pieces.push(piece.iter().rev().copied().collect());
        }
        let inner_start = bend.inner[0][0];
        pieces.push(vec![inner_start, DVec2::new(inner_start.x, low)]);
        pieces.push(vec![
            DVec2::new(inner_start.x, low),
            DVec2::new(outer_start.x, low),
        ]);
        pieces
            .into_iter()
            .map(|piece| piece.into_iter().map(at).collect())
            .collect()
    }

    /// For an internal volute, the outline (r, z) of the casing round its bend and neck:
    /// their region offset outward by the wall thickness.
    pub fn bend_casing_profile(&self) -> Option<Vec<DVec2>> {
        let bend = self.bend.as_ref()?;
        let ring: Vec<DVec3> = bend
            .region()
            .into_iter()
            .map(|p| {
                let m = self.meridional(p);
                DVec3::new(m.x, 0.0, m.y)
            })
            .collect();
        Some(
            offset_ring(&ring, self.design.wall_thickness)
                .into_iter()
                .map(|p| DVec2::new(p.x, p.z))
                .collect(),
        )
    }

    /// The axial range (z) of the space of the impeller, which the casing leaves open
    /// inside r₂: all of it for a volute outside r'_z, and only the side away from an
    /// internal spiral, which lies beside the impeller inside r₂.
    pub fn impeller_band(&self) -> (f64, f64) {
        match &self.bend {
            None => (f64::NEG_INFINITY, f64::INFINITY),
            Some(bend) => {
                let face = self.basis.midplane + bend.face;
                if bend.face > 0.0 {
                    (f64::NEG_INFINITY, face)
                } else {
                    (face, f64::INFINITY)
                }
            }
        }
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

    /// Angle by which the mean direction of the two walls at the cutwater tip lies above
    /// the flow angle α₃' the spiral delivers at the design point, rad: the incidence
    /// of the bare wedge, positive when the walls turn further from the circumference
    /// than the flow. 0 without a wedge. A sharp cutwater is flagged beyond ±3°; a
    /// rounded one follows α₃B with the camber of its body, so there it only shows how
    /// far the diffuser pulls the walls away.
    pub fn wall_misalignment(&self) -> f64 {
        if self.cutwater.wedge <= 0.0 {
            return 0.0;
        }
        self.cutwater.mean_angle() - self.design.cutwater_flow_angle
    }

    /// The height range (z_min, z_max), m, of the whole section of a double volute at the
    /// point of wrap angle `angle` from the cutwater (rad, 0 to 2π) and radius `r` from
    /// the pump axis: the channel the rib sits in. The sections are blended between their
    /// angles; before the rib nose it is the first one. `None` for a single volute.
    pub fn envelope_z_range(&self, angle: f64, r: f64) -> Option<(f64, f64)> {
        let at = |s: &VoluteSection| -> Option<(f64, f64)> {
            let mut range: Option<(f64, f64)> = None;
            let n = s.outline.len();
            // A point a hair off the section in radius (the tip of the nose of the rib
            // lies just inside the base circle) is taken at its edge.
            let (r_min, r_max) = s
                .outline
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p.x), hi.max(p.x))
                });
            let r = r.clamp(r_min, r_max);
            for i in 0..n {
                let (a, b) = (s.outline[i], s.outline[(i + 1) % n]);
                if (a.x - r) * (b.x - r) > 0.0 || (b.x - a.x).abs() < 1e-15 {
                    continue;
                }
                let z = a.y + (b.y - a.y) * (r - a.x) / (b.x - a.x);
                range = Some(range.map_or((z, z), |(lo, hi)| (lo.min(z), hi.max(z))));
            }
            range
        };
        let sections = &self.envelope;
        let (first, last) = (sections.first()?, sections.last()?);
        if angle <= first.angle {
            return at(first);
        }
        if angle >= last.angle {
            return at(last);
        }
        let k = sections
            .partition_point(|s| s.angle < angle)
            .clamp(1, sections.len() - 1);
        let (a, b) = (&sections[k - 1], &sections[k]);
        let w = ((angle - a.angle) / (b.angle - a.angle)).clamp(0.0, 1.0);
        match (at(a), at(b)) {
            (Some((l0, h0)), Some((l1, h1))) => Some((l0 + (l1 - l0) * w, h0 + (h1 - h0) * w)),
            (one, other) => one.or(other),
        }
    }

    /// Distance C from the pump axis to the axis of the outlet, m: 0 when the outlet
    /// stands on the line through the pump axis.
    pub fn outlet_offset(&self) -> f64 {
        let last = self.nozzle.last().expect("a diffuser");
        let axis = last.tangent.truncate().normalize();
        let c = axis.perp_dot(last.center.truncate()).abs();
        if c < 1e-9 { 0.0 } else { c }
    }

    /// Angle φ between the radial line through the throat and the line from the pump
    /// axis to the centre of the outlet, rad, positive in the direction of rotation.
    pub fn outlet_angle(&self) -> f64 {
        let last = self.nozzle.last().expect("a diffuser");
        let sign = self.basis.rotation_sign;
        // The throat is at 2π for a single and a double volute alike (the sections of a
        // double volute stop at the rib nose).
        let throat = self.rotation + sign * TAU;
        let outlet = last.center.y.atan2(last.center.x);
        let turn = sign * (outlet - throat);
        turn.sin().atan2(turn.cos())
    }

    /// The break of the area law and of the path where the spiral ends and the diffuser
    /// starts; `None` for a double volute, whose throat is split by the rib, and for
    /// sections too close together to measure.
    pub fn throat_junction(&self) -> Option<ThroatJunction> {
        if self.design.rib.is_some() || self.sections.len() < 3 || self.nozzle.len() < 3 {
            return None;
        }
        let sign = self.basis.rotation_sign;
        // Centroid of the area of a planar outline, by a fan of triangles.
        let mean = |p: Vec<DVec3>| {
            let (mut sum, mut area) = (DVec3::ZERO, 0.0);
            for w in p[1..].windows(2) {
                let a = (w[0] - p[0]).cross(w[1] - p[0]).length();
                sum += a * (p[0] + w[0] + w[1]) / 3.0;
                area += a;
            }
            sum / area
        };
        let n = self.sections.len();
        let before: Vec<(DVec3, f64)> = self.sections[n - 3..]
            .iter()
            .map(|s| (mean(s.points(sign)), s.size.area))
            .collect();
        let after: Vec<(DVec3, f64)> = self.nozzle[..3]
            .iter()
            .map(|s| (mean(s.points.clone()), s.area))
            .collect();
        // Derivative of the parabola through three (path length, area) samples at `at`.
        let slope = |samples: &[(DVec3, f64)], at: usize| {
            let x: Vec<f64> = samples
                .iter()
                .map(|s| (s.0 - samples[0].0).length())
                .collect();
            (0..3)
                .map(|i| {
                    let (j, k) = ((i + 1) % 3, (i + 2) % 3);
                    let weight =
                        ((x[at] - x[j]) + (x[at] - x[k])) / ((x[i] - x[j]) * (x[i] - x[k]));
                    samples[i].1 * weight
                })
                .sum::<f64>()
        };
        // Curvature of the circle through three points.
        let curvature = |p: [DVec3; 3]| {
            let (u, v) = (p[1] - p[0], p[2] - p[1]);
            2.0 * u.cross(v).length() / (u.length() * v.length() * (p[2] - p[0]).length())
        };
        let (last, first) = (
            (before[2].0 - before[1].0).normalize(),
            (after[1].0 - after[0].0).normalize(),
        );
        Some(ThroatJunction {
            area_slope_ratio: slope(&after, 0) / slope(&before, 2),
            curvature_before: curvature([before[0].0, before[1].0, before[2].0]),
            curvature_after: curvature([after[0].0, after[1].0, after[2].0]),
            direction_turn: last.dot(first).clamp(-1.0, 1.0).acos(),
        })
    }
}

/// The warning for a sharp or rounded cutwater whose spiral, starting at `start`, wraps
/// less than [`MIN_WRAP`] to the throat.
fn short_wrap_warning(shape: CutwaterShape, start: f64) -> Option<String> {
    (!matches!(shape, CutwaterShape::Tangent) && TAU - start < MIN_WRAP * (1.0 - 1e-9)).then(|| {
        format!(
            "the spiral wraps {:.0}° from its start to the throat, less than the {:.0}° a \
             sharp or rounded cutwater needs: start the spiral earlier",
            (TAU - start).to_degrees(),
            MIN_WRAP.to_degrees()
        )
    })
}

/// The warning for a fillet that leaves the cutwater at the diameter `effective`, m,
/// below the least `minimum` of Gülich Table 10.2, for an impeller of diameter `d2`.
fn fillet_diameter_warning(effective: f64, minimum: f64, d2: f64) -> Option<String> {
    (effective < minimum * (1.0 - 1e-9)).then(|| {
        format!(
            "the fillet leaves an effective cutwater diameter d_z/d₂ = {:.3}, below the {:.3} \
             of Gülich Table 10.2: expect pressure pulsations and hydraulic excitation forces; \
             start the spiral later or use a smaller radius",
            effective / d2,
            minimum / d2
        )
    })
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

/// The lofted outline (r, z) of the section of `size` at `angle`, scaled a little so
/// that it encloses the area of the design section: sampling curved walls for the loft
/// removes some area, which a slightly larger height gives back.
fn fitted_outline(
    sections: &CrossSections,
    angle: f64,
    size: &SectionSize,
    midplane: f64,
) -> Vec<DVec2> {
    let mut h = size.height;
    let mut outline = sections.loft_outline_lifted(angle, h, size.lift);
    for _ in 0..40 {
        let ratio = size.area / outline.signed_area();
        if !ratio.is_finite() || (ratio - 1.0).abs() < 1e-10 {
            break;
        }
        // The area grows with a power of the height between 1 and 2.
        h *= ratio.powf(0.75);
        outline = sections.loft_outline_lifted(angle, h, size.lift);
    }
    outline.meridional(sections.stagnation_radius, midplane)
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
        // A spline has no straight part after a bend: the length that puts the outlet
        // face at H, by bisection.
        if matches!(first.design.diffuser.shape, DischargeShape::Spline { .. }) {
            let reach = |length: f64| -> Result<f64, DesignError> {
                let mut trial = free.clone();
                trial.diffuser.length = Param::Manual(length);
                Ok(Self::build_with_length(basis, &trial)?.outlet_height)
            };
            let base = first.design.diffuser.length;
            let (mut lo, mut hi) = (0.2 * base, 8.0 * base);
            let (at_lo, at_hi) = (reach(lo)?, reach(hi)?);
            if !((at_lo.min(at_hi))..=(at_lo.max(at_hi))).contains(&height) {
                return Err(DesignError::InvalidInput(format!(
                    "the flange height H = {:.1} mm cannot be reached with a spline diffuser: \
                     between {:.1} mm and {:.1} mm",
                    height * 1e3,
                    at_lo.min(at_hi) * 1e3,
                    at_lo.max(at_hi) * 1e3
                )));
            }
            let rising = at_hi > at_lo;
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if (reach(mid)? < height) == rising {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            free.diffuser.length = Param::Manual(0.5 * (lo + hi));
            return Self::build_with_length(basis, &free);
        }
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
        let make = |angle: f64| -> Result<VoluteSection, DesignError> {
            let size = design.section(angle)?;
            Ok(VoluteSection {
                angle,
                size,
                outline: fitted_outline(&design.cross_sections, angle, &size, zm),
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
        let straight = design.shape();
        let base_width = |height: f64| straight.top_width(height);
        let inner_height = |angle: f64| -> Result<f64, DesignError> { design.rib_height(angle) };
        // The outer channel: the part of the whole section beyond the rib, the same
        // straight walls on a wider base.
        let channel = |angle: f64| -> Result<VoluteSection, DesignError> {
            let rib = design.rib.as_ref().expect("a double volute");
            let a = inner_height(angle)?;
            let base = base_width(a + rib.thickness);
            let rz = design.stagnation_radius + a + rib.thickness;
            let area = rib.channel_area(angle);
            let part = super::Shape::closed(straight.slope, base, rz);
            let size = part.size(part.height_for_area(area));
            let sections = design.cross_sections.with_inlet(base, rz);
            Ok(VoluteSection {
                angle,
                size,
                outline: fitted_outline(&sections, angle, &size, zm),
                rotation: 0.0,
            })
        };
        // The whole section of a double volute: the inner volute, the rib and the outer
        // channel. At the rib nose it is the throat of the first partial volute.
        let whole = |angle: f64| -> Result<VoluteSection, DesignError> {
            let rib = design.rib.as_ref().expect("a double volute");
            let size = match design.free_double() {
                // The whole section is the one the rib was sized on.
                Some(table) => super::size_of(
                    &design
                        .cross_sections
                        .properties(angle, table.row_at(angle).whole_height)?,
                ),
                None => {
                    let area = straight.area(inner_height(angle)? + rib.thickness)
                        + rib.channel_area(angle);
                    straight.size(straight.height_for_area(area))
                }
            };
            Ok(VoluteSection {
                angle,
                size,
                outline: fitted_outline(&design.cross_sections, angle, &size, zm),
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
        let bend = design.cross_sections.bend();
        // A spiral that wraps less than 360° starts that much past the cutwater.
        let first = design.development.start;
        let mut start = first + 2f64.to_radians();
        while bend.is_none() && (!clear(start)? || !clear(start + 0.05)?) {
            start += 1f64.to_radians();
            if start > first + 90f64.to_radians() {
                return Err(DesignError::Infeasible(
                    "the start of the spiral collides with the discharge diffuser".into(),
                ));
            }
        }
        let clear_start = start + 2f64.to_radians();
        // Growth of the area of the spiral per metre of the centroid path at the throat.
        let spiral_slope = (throat.size.area - before.area)
            / (position(TAU, &throat.size) - position(TAU - 0.02, &before)).length();
        let (nozzle, nozzle_bend, nozzle_length) =
            nozzle(&design, &throat, sign, axis, spiral_slope)?;
        let nozzle_axis = nozzle.last().map_or(axis, |n| n.tangent);
        // A sharp or rounded cutwater: the spiral starts at the cutwater and cuts the
        // inner wall of the diffuser. When the wall turns away instead (a radial
        // discharge), the spiral starts where it clears the wall and the cutwater sits
        // there.
        let mut start_range = None;
        let mut intersection_warning: Option<String> = None;
        let (start, cutwater) = match design.cutwater_shape {
            // An internal spiral starts beside the face of the neck, inside the diffuser:
            // the cutwater is where the two cut each other, with no nose of its own.
            _ if bend.is_some() => {
                let b = bend.as_ref().expect("an internal volute");
                let r = design.stagnation_radius + b.top;
                let tip = DVec3::new(r, 0.0, zm + b.face);
                (
                    CUTWATER_START,
                    CutwaterGeometry {
                        tip,
                        angle: 0.0,
                        wedge: 0.0,
                        radius: 0.0,
                        nose_center: tip,
                        nose_distance: r,
                        spiral_angle: 0.0,
                        diffuser_angle: 0.0,
                    },
                )
            }
            // A shorter spiral leaves the casing between the throat and its start as the
            // cutwater, its face on r'_z.
            _ if first > 0.0 => {
                let tip = DVec3::new(design.stagnation_radius, 0.0, zm);
                (
                    start,
                    CutwaterGeometry {
                        tip,
                        angle: 0.0,
                        wedge: 0.0,
                        radius: 0.0,
                        nose_center: tip,
                        nose_distance: design.stagnation_radius,
                        spiral_angle: 0.0,
                        diffuser_angle: 0.0,
                    },
                )
            }
            CutwaterShape::Tangent => (
                clear_start,
                cutwater(&design, &nozzle, sign, zm, clear_start)?,
            ),
            _ => {
                // Where the walls cross in each plane across the inlet width, with the
                // spiral from its earliest start: its sections do not depend on where it
                // starts. The spiral must start before every crossing. Near the side walls
                // of round sections the walls may not cross at all, at any start; there
                // the cutwater has no wedge, as at a tangent one. The midplane needs a true
                // wedge, and so does every plane where a sharp cutwater crosses; the body
                // of a rounded one fills the planes where the walls barely cross.
                let early: Vec<(f64, Vec<DVec2>)> = (0..=CUTWATER_SPAN.to_degrees() as usize)
                    .filter_map(|i| {
                        let angle = MIN_START + (i as f64).to_radians();
                        make(angle).ok().map(|s| (angle, s.outline))
                    })
                    .collect();
                let crossings = super::cutwater::crossings(&design, &early, &nozzle, sign, zm);
                let counts = super::cutwater::crossing_counts(&design, &early, &nozzle, sign, zm);
                let several = counts.iter().filter(|c| **c > 1).count();
                if several > 0 {
                    intersection_warning = Some(format!(
                        "the spiral wall crosses the diffuser wall more than once in {several} of \
                         {} planes, so there are several intersection curves: move the start of \
                         the spiral, reduce the height offset or raise the base form factor",
                        counts.len()
                    ));
                }
                let sharp = matches!(design.cutwater_shape, CutwaterShape::Sharp);
                let wedged = |c: &Option<(f64, f64)>| c.is_some_and(|(_, w)| w > MIN_WEDGE);
                let crosses = wedged(&crossings[crossings.len() / 2])
                    && (!sharp || crossings.iter().all(|c| c.is_none() || wedged(c)));
                let latest = crossings
                    .iter()
                    .flatten()
                    .map(|(angle, _)| *angle)
                    .fold(f64::INFINITY, f64::min)
                    - START_MARGIN;
                let earliest = early.first().map_or(MIN_START, |(angle, _)| *angle);
                start_range = (crosses && latest > earliest).then_some((earliest, latest));
                let chosen = match (input.cutwater.spiral_start.manual_value(), start_range) {
                    (Some(s), Some((lo, hi))) if (lo..=hi).contains(&s) => s,
                    (Some(s), Some((lo, hi))) => {
                        return Err(DesignError::InvalidInput(format!(
                            "the spiral must start between {:.1}° and {:.1}° for its wall to \
                             cross the diffuser's in every plane, got {:.1}°",
                            lo.to_degrees(),
                            hi.to_degrees(),
                            s.to_degrees()
                        )));
                    }
                    (Some(_), None) => {
                        return Err(DesignError::InvalidInput(
                            "the walls of the spiral and the diffuser do not cross in every \
                             plane: leave the start of the spiral automatic"
                                .into(),
                        ));
                    }
                    (None, Some((lo, hi))) => CUTWATER_START.clamp(lo, hi),
                    // They part at once (a radial discharge): where the spiral clears the
                    // diffuser.
                    (None, None) => clear_start,
                };
                (chosen, cutwater(&design, &nozzle, sign, zm, chosen)?)
            }
        };

        // The fitted outlines of the spiral every degree or less from its start: the
        // outer wall for drawing, and the spiral wall in the planes of the cutwater.
        let steps = (((TAU - start) / 1f64.to_radians()).ceil() as usize).max(2);
        let wall_outlines = (0..=steps)
            .map(|i| {
                let angle = start + (TAU - start) * i as f64 / steps as f64;
                let section = match &design.rib {
                    Some(rib) if angle > rib.start => whole(angle)?,
                    _ => make(angle)?,
                };
                Ok((angle, section.outline))
            })
            .collect::<Result<Vec<_>, DesignError>>()?;

        // A sharp or rounded cutwater that cuts the diffuser wall, in planes across the
        // inlet width.
        let mut cutwater = cutwater;
        let cuts_the_diffuser = bend.is_none()
            && first <= 0.0
            && !matches!(design.cutwater_shape, CutwaterShape::Tangent);
        let (cutwater_slices, cutwater_body) = if cuts_the_diffuser {
            let end = (start + CUTWATER_SPAN).min(design.rib.as_ref().map_or(TAU, |r| r.start));
            let near = wall_outlines.partition_point(|(angle, _)| *angle <= end);
            let walls = &wall_outlines[..near];
            let (slices, centre) = super::cutwater::slices(&design, walls, &nozzle, sign, zm);
            let e = 2.0 * design.nose_radius;
            let oblique = design.cutwater_obliquity;
            if centre.is_some() && oblique != 0.0 && !super::cutwater::sound(&slices, e) {
                // The most it can lean, by bisection.
                let leans = |obliquity: f64| {
                    let mut leaning = design.clone();
                    leaning.cutwater_obliquity = obliquity;
                    let (slices, _) = super::cutwater::slices(&leaning, walls, &nozzle, sign, zm);
                    super::cutwater::sound(&slices, e)
                };
                let (mut lo, mut hi) = (0.0, oblique);
                for _ in 0..12 {
                    let mid = 0.5 * (lo + hi);
                    if leans(mid) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                return Err(DesignError::Infeasible(format!(
                    "the cutwater cannot lean {:.0}°: past about {:.0}° it gets thinner than \
                     its nose in some plane, where the walls leave it no room",
                    oblique.to_degrees(),
                    lo.to_degrees().trunc()
                )));
            }
            if let Some(centre) = centre {
                let mid = slices
                    .iter()
                    .find(|s| (s.z - zm).abs() < 1e-12 && !s.outline.is_empty())
                    .ok_or_else(|| {
                        DesignError::Infeasible(
                            "the rounded cutwater does not reach the walls in the midplane: use \
                             a sharp or tangent cutwater"
                                .into(),
                        )
                    })?;
                cutwater.nose_center = DVec3::new(centre.x, centre.y, zm);
                cutwater.nose_distance = mid.nose_distance;
            }
            // The tip and the wedge where the walls really cross in the midplane.
            if let Some(mid) = slices.iter().find(|s| (s.z - zm).abs() < 1e-12) {
                cutwater.tip = mid.tip;
                cutwater.angle = (sign * mid.tip.y.atan2(mid.tip.x)).rem_euclid(TAU);
                cutwater.wedge = mid.wedge;
                cutwater.spiral_angle = mid.spiral_angle;
                cutwater.diffuser_angle = mid.diffuser_angle;
                if cutwater.radius <= 0.0 {
                    cutwater.nose_center = mid.tip;
                    cutwater.nose_distance = mid.nose_distance;
                }
            }
            if design.cutwater_shape.is_fillet() {
                let mid = slices
                    .iter()
                    .find(|s| (s.z - zm).abs() < 1e-12 && !s.outline.is_empty())
                    .ok_or_else(|| {
                        DesignError::Infeasible(format!(
                            "a fillet of {:.1} mm does not fit between the walls of the \
                             spiral and the diffuser in the midplane: use a smaller radius or \
                             a sharp cutwater",
                            design.nose_radius * 1e3
                        ))
                    })?;
                cutwater.radius = design.nose_radius;
                cutwater.nose_center = mid.nose_centre;
                cutwater.nose_distance = mid.nose_distance;
            }
            let body = super::cutwater::body_outlines(
                &design,
                &slices,
                (walls, &nozzle),
                sign,
                zm,
                if design.cutwater_shape.is_fillet() {
                    0.0
                } else {
                    design.cutwater_side_radius
                },
                BODY_STEPS,
            );
            (slices, body)
        } else {
            (Vec::new(), Vec::new())
        };

        // Sections spread over their wrap in proportion to the single spiral.
        let spread = |from: f64, to: f64| -> Vec<f64> {
            let n = ((f64::from(count) * (to - from) / (TAU - start)).round() as u32).max(4);
            (0..=n)
                .map(|i| from + (to - from) * f64::from(i) / f64::from(n))
                .collect()
        };
        let (sections, inner_sections, outer_channel, envelope, merged) = match &design.rib {
            None => (
                spread(start, TAU)
                    .into_iter()
                    .map(make)
                    .collect::<Result<Vec<_>, _>>()?,
                Vec::new(),
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
                    // Sections the rib cuts have no sections of their own for the inner
                    // volute and the outer channel: they are parts of the whole one.
                    if design.free_double().is_some() {
                        Vec::new()
                    } else {
                        spread(rib.start + CUTWATER_START, rib.end())
                            .into_iter()
                            .map(make)
                            .collect::<Result<Vec<_>, _>>()?
                    },
                    if design.free_double().is_some() {
                        Vec::new()
                    } else {
                        spread(rib.start, rib.end())
                            .into_iter()
                            .map(channel)
                            .collect::<Result<Vec<_>, _>>()?
                    },
                    spread(rib.start, TAU)
                        .into_iter()
                        .map(whole)
                        .collect::<Result<Vec<_>, _>>()?,
                    if rib.is_short() {
                        spread(rib.end(), TAU)
                            .into_iter()
                            .map(whole)
                            .collect::<Result<Vec<_>, _>>()?
                    } else {
                        Vec::new()
                    },
                )
            }
        };
        // The rib across the height of the sections.
        let heights = wall_outlines
            .iter()
            .flat_map(|(_, o)| o.iter().map(|p| p.y));
        let height = heights.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        });
        let rib = rib(&design, sign, zm, height)?;

        // The outer wall for drawing: the outermost point of each fitted outline.
        let outer_wall: Vec<DVec3> = wall_outlines
            .iter()
            .map(|(angle, outline)| {
                let r = outline
                    .iter()
                    .map(|p| p.x)
                    .fold(f64::NEG_INFINITY, f64::max);
                let theta = sign * angle;
                DVec3::new(r * theta.cos(), r * theta.sin(), zm)
            })
            .collect();
        let inner_wall = match design.development.compensation {
            None => Vec::new(),
            Some(c) => {
                let from = first + c.start;
                let steps = (((TAU - from) / 1f64.to_radians()).ceil() as usize).max(2);
                (0..=steps)
                    .map(|i| {
                        let angle = from + (TAU - from) * i as f64 / steps as f64;
                        let dev = &design.development;
                        let r = design.stagnation_radius + dev.lift_at(dev.phi(angle));
                        let theta = sign * angle;
                        DVec3::new(r * theta.cos(), r * theta.sin(), zm)
                    })
                    .collect()
            }
        };
        let outlet_height = nozzle.last().map_or(0.0, |n| {
            n.center
                .truncate()
                .dot(n.tangent.truncate().normalize_or_zero())
        });

        // Turn the whole volute about the pump axis, by default so that the discharge
        // points vertically up. Wrap angles stay measured from the cutwater.
        let rotation = input
            .diffuser
            .rotation
            .resolve(|| std::f64::consts::FRAC_PI_2 - nozzle_axis.y.atan2(nozzle_axis.x));
        let rotation = (rotation + PI).rem_euclid(TAU) - PI;
        let (rs, rc) = rotation.sin_cos();
        let turn = |p: DVec3| DVec3::new(rc * p.x - rs * p.y, rs * p.x + rc * p.y, p.z);
        let (mut sections, mut inner_sections, mut outer_channel, mut envelope, mut merged) =
            (sections, inner_sections, outer_channel, envelope, merged);
        for list in [
            &mut sections,
            &mut inner_sections,
            &mut outer_channel,
            &mut envelope,
            &mut merged,
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
        let cutwater_slices: Vec<CutwaterSlice> = cutwater_slices
            .into_iter()
            .map(|s| CutwaterSlice {
                tip: turn(s.tip),
                outline: s.outline.into_iter().map(turn).collect(),
                spiral_wall: s.spiral_wall.into_iter().map(turn).collect(),
                diffuser_wall: s.diffuser_wall.into_iter().map(turn).collect(),
                nose_centre: turn(s.nose_centre),
                removed: s.removed.into_iter().map(turn).collect(),
                joins: s.joins.map(|(a, b)| (turn(a), turn(b))),
                ..s
            })
            .collect();
        let cutwater_body: Vec<Vec<DVec3>> = cutwater_body
            .into_iter()
            .map(|o| o.into_iter().map(turn).collect())
            .collect();
        let rib = rib.map(|r| RibGeometry {
            inner_face: r.inner_face.into_iter().map(turn).collect(),
            outer_face: r.outer_face.into_iter().map(turn).collect(),
            nose: r.nose.into_iter().map(turn).collect(),
            tail: r.tail.into_iter().map(turn).collect(),
            nose_tip: turn(r.nose_tip),
            planes: r
                .planes
                .into_iter()
                .map(|p| RibPlane {
                    inner_face: p.inner_face.into_iter().map(turn).collect(),
                    outer_face: p.outer_face.into_iter().map(turn).collect(),
                    nose: p.nose.into_iter().map(turn).collect(),
                    tail: p.tail.into_iter().map(turn).collect(),
                    ..p
                })
                .collect(),
            ..r
        });
        let outer_wall: Vec<DVec3> = outer_wall.into_iter().map(turn).collect();
        let inner_wall: Vec<DVec3> = inner_wall.into_iter().map(turn).collect();

        let mut warnings = design.warnings.clone();
        warnings.extend(intersection_warning);
        warnings.extend(short_wrap_warning(design.cutwater_shape, start));
        if design.cutwater_shape.is_fillet() {
            // A fillet sets the diameter at which the cutwater stands: the nearest point of
            // its nose to the axis in any plane.
            let nearest = cutwater_slices
                .iter()
                .map(|s| s.nose_distance)
                .fold(f64::INFINITY, f64::min);
            warnings.extend(fillet_diameter_warning(
                2.0 * nearest,
                design.min_cutwater_diameter,
                basis.impeller_diameter,
            ));
        }
        if design.diffuser.z_bend.is_some() {
            warnings.push(
                "the CAD kernel builds the flow domain of a diffuser with an axial bend but \
                 not yet its casing"
                    .into(),
            );
        }
        if matches!(design.diffuser.shape, DischargeShape::Spline { .. }) {
            warnings.push(
                "the CAD kernel is slow and unreliable on a spline diffuser (minutes to \
                 half an hour, depending on the turn): export the CAD solids with a radial \
                 or tangential discharge"
                    .into(),
            );
        }
        if nozzle_length > design.diffuser.length * (1.0 + 1e-9) {
            warnings.push(format!(
                "the bend of the radial discharge ({:.0} mm) is longer than the diffuser \
                 ({:.0} mm): the diffuser ends with the bend",
                nozzle_length * 1e3,
                design.diffuser.length * 1e3
            ));
        }
        let mut volute = Self {
            basis: *basis,
            design,
            start_angle: start,
            start_range,
            sections,
            inner_sections,
            outer_channel,
            envelope,
            merged,
            rib,
            nozzle,
            nozzle_axis,
            nozzle_bend,
            nozzle_length,
            outlet_height,
            rotation,
            outer_wall,
            inner_wall,
            wall_stress: WallStress::default(),
            cutwater,
            cutwater_slices,
            cutwater_body,
            bend,
            warnings,
        };
        let stress = WallStress::new(&volute, &input.wall.pressure, &input.wall.allowable_stress)?;
        if stress.hoop > stress.allowable {
            volute.warnings.push(format!(
                "the casing wall carries {:.1} MPa, above the allowable {:.1} MPa: make it at \
                 least {:.1} mm thick",
                stress.hoop * 1e-6,
                stress.allowable * 1e-6,
                stress.required_thickness * 1e3
            ));
        }
        volute.wall_stress = stress;
        // Only the bare wedge of a sharp cutwater is the cutwater: a rounded one follows
        // the flow with the camber of its body, so there the angle is only reported.
        let off = volute.wall_misalignment();
        if matches!(volute.design.cutwater_shape, CutwaterShape::Sharp)
            && off.abs() > WALL_ALIGNMENT_BAND + 1e-12
        {
            volute.warnings.push(format!(
                "the walls at the cutwater tip point {:.1}° from the circumference, {:.1}° off \
                 the flow angle α₃' of {:.1}° (beyond ±3°): the flow meets the wedge at an \
                 angle; turn the diffuser or move the start of the spiral",
                volute.cutwater.mean_angle().to_degrees(),
                off.to_degrees(),
                volute.design.cutwater_flow_angle.to_degrees()
            ));
        }
        Ok(volute)
    }
}

/// The rib of a double volute. Its inner face follows the outer wall of the inner volute
/// from r'_z at the start of the inner volute, its outer face lies one rib thickness
/// beyond. The nose and the tail are half ellipses on the conjugate semi-diameters
/// across the rib and along its mid line, so they meet both faces tangentially: the
/// leading edge points along the inner volute's wall, which leaves r'_z at the flow
/// angle α₃' (Gülich Chap. 7.8.1, item 7, asks the cutwater to match the approach
/// flow). It is the same in the planes across the height `height` (from −z to +z) but
/// for the obliquity of the cutwater, which turns its nose about the pump axis from
/// plane to plane as the cutwater's ([`super::cutwater::oblique`]).
fn rib(
    design: &VoluteDesign,
    sign: f64,
    zm: f64,
    height: (f64, f64),
) -> Result<Option<RibGeometry>, DesignError> {
    let Some(rib) = design.rib else {
        return Ok(None);
    };
    let mid = rib_plane(design, &rib, sign, zm, rib.start)?;
    // The nose turns on the circle through its middle.
    let r = design.stagnation_radius + rib.thickness / 2.0;
    let planes = (0..RIB_PLANES)
        .map(|k| {
            let z = height.0 + (height.1 - height.0) * k as f64 / (RIB_PLANES - 1) as f64;
            let lean = (z - zm) * design.cutwater_obliquity.tan() / r;
            rib_plane(design, &rib, sign, z, rib.start + lean)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let nose_tip = mid.nose[RIB_NOSE_POINTS / 2];
    let nose_distance = mid
        .nose
        .iter()
        .chain(&mid.inner_face)
        .map(|p| p.truncate().length())
        .fold(f64::INFINITY, f64::min);
    let nose_angle = (sign * nose_tip.y.atan2(nose_tip.x)).rem_euclid(TAU);
    Ok(Some(RibGeometry {
        inner_face: mid.inner_face,
        outer_face: mid.outer_face,
        nose: mid.nose,
        tail: mid.tail,
        nose_tip,
        nose_angle,
        nose_distance,
        planes,
    }))
}

/// The rib in the plane `z`, its faces from the wrap angle `start` to its end.
fn rib_plane(
    design: &VoluteDesign,
    rib: &super::Rib,
    sign: f64,
    z: f64,
    start: f64,
) -> Result<RibPlane, DesignError> {
    let t = rib.thickness;
    let inner = |angle: f64| -> Result<f64, DesignError> {
        Ok(design.stagnation_radius + design.rib_height(angle)?)
    };
    let at = |r: f64, angle: f64| {
        let theta = sign * angle;
        DVec3::new(r * theta.cos(), r * theta.sin(), z)
    };
    let n = RIB_FACE_POINTS;
    let (mut inner_face, mut outer_face) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
    let end = rib.end();
    for i in 0..=n {
        let angle = start + (end - start) * i as f64 / n as f64;
        let r = inner(angle)?;
        inner_face.push(at(r, angle));
        outer_face.push(at(r + t, angle));
    }
    // Downstream direction of the mid line at both ends.
    let mid = |angle: f64| -> Result<DVec3, DesignError> { Ok(at(inner(angle)? + t / 2.0, angle)) };
    let step = 1e-4;
    let lead = (mid(start + step)? - mid(start)?).normalize();
    let trail = (mid(end)? - mid(end - step)?).normalize();
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
        points
    };
    let nose = half_ellipse(inner_face[0], outer_face[0], -lead, 0.0, PI);
    let tail = half_ellipse(inner_face[n], outer_face[n], trail, PI, 0.0);
    Ok(RibPlane {
        z,
        inner_face,
        outer_face,
        nose,
        tail,
    })
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
            spiral_angle: 0.0,
            diffuser_angle: 0.0,
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
    // Angles of the two walls from the circumferential direction at the tip.
    let radial = tip.truncate().normalize();
    let circumferential = DVec2::new(-radial.y, radial.x) * sign;
    let from_circle = |d: DVec3| {
        d.truncate()
            .dot(radial)
            .atan2(d.truncate().dot(circumferential))
    };
    // A rounded nose of radius ρ whose innermost point lies on the cutwater circle d_z
    // (Gülich Fig. 7.41b: the flow stagnates on the nose at r'_z = r_z + e₃/2 when
    // ρ = e₃/2), joined to the tip by its tangents.
    let radius = if design.cutwater_shape.is_fillet() {
        0.0
    } else {
        design.nose_radius
    };
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
        spiral_angle: from_circle(d_spiral),
        diffuser_angle: from_circle(d_wall),
    })
}

/// A point of the diffuser centerline and its unit tangent.
type PathPoint = (DVec3, DVec3);

/// The axial profile z(s) of a z-bend along the horizontal length of the diffuser, and
/// its slope: flat for `flat` from the throat, then the round of radius R through the
/// angle α, then straight at that angle. The flat run is whatever makes the extension
/// at the outlet, `length` along, what was asked for.
struct ZProfile {
    flat: f64,
    radius: f64,
    angle: f64,
}

impl ZProfile {
    fn new(bend: ZBend, length: f64) -> Result<Self, DesignError> {
        let (r, a) = (bend.radius, bend.angle);
        let round_run = r * a.sin();
        if length < round_run {
            return Err(DesignError::Infeasible(format!(
                "the round of the z-bend ({:.1} mm long) does not fit in the diffuser \
                 ({:.1} mm)",
                round_run * 1e3,
                length * 1e3
            )));
        }
        let at = |flat: f64| Self {
            flat,
            radius: r,
            angle: a,
        };
        let (low, high) = (at(length - round_run).z(length), at(0.0).z(length));
        if !(low..=high).contains(&bend.extent) {
            return Err(DesignError::Infeasible(format!(
                "the axial extension of the z-bend must lie between {:.1} mm and {:.1} mm for \
                 this radius, angle and length, got {:.1} mm",
                low * 1e3,
                high * 1e3,
                bend.extent * 1e3
            )));
        }
        // z at the outlet falls as the flat run grows: bisection.
        let (mut lo, mut hi) = (0.0, length - round_run);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if at(mid).z(length) > bend.extent {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Ok(at(0.5 * (lo + hi)))
    }

    fn z(&self, s: f64) -> f64 {
        let x = s - self.flat;
        let run = self.radius * self.angle.sin();
        if x <= 0.0 {
            0.0
        } else if x < run {
            self.radius - (self.radius * self.radius - x * x).sqrt()
        } else {
            self.radius * (1.0 - self.angle.cos()) + (x - run) * self.angle.tan()
        }
    }

    fn slope(&self, s: f64) -> f64 {
        let x = s - self.flat;
        let run = self.radius * self.angle.sin();
        if x <= 0.0 {
            0.0
        } else if x < run {
            x / (self.radius * self.radius - x * x).sqrt()
        } else {
            self.angle.tan()
        }
    }
}

/// A centerline of a fourth-degree Bézier curve from `start` along `tangent` that turns
/// `turn` toward `outward`, scaled to `length`, at `count + 1` equal steps of its length.
/// The control points: 0 and 4 the ends, 2 where the two straight legs of equal length
/// meet, 1 and 3 at the shares of the way from the ends to it.
fn spline_path(
    start: DVec3,
    tangent: DVec3,
    outward: DVec3,
    turn: f64,
    shares: [f64; 2],
    length: f64,
    count: usize,
) -> Vec<PathPoint> {
    // In the plane (along the tangent, outward) with legs of length 1.
    let (p0, d_end) = (DVec2::ZERO, DVec2::new(turn.cos(), turn.sin()));
    let p2 = DVec2::new(1.0, 0.0);
    let p4 = p2 + d_end;
    let p = [p0, p0.lerp(p2, shares[0]), p2, p4.lerp(p2, shares[1]), p4];
    let at = |t: f64| -> (DVec2, DVec2) {
        let u = 1.0 - t;
        let b = [
            u.powi(4),
            4.0 * u.powi(3) * t,
            6.0 * u * u * t * t,
            4.0 * u * t.powi(3),
            t.powi(4),
        ];
        let db = [
            -4.0 * u.powi(3),
            4.0 * u.powi(3) - 12.0 * u * u * t,
            12.0 * u * u * t - 12.0 * u * t * t,
            12.0 * u * t * t - 4.0 * t.powi(3),
            4.0 * t.powi(3),
        ];
        let position = p.iter().zip(b).map(|(q, w)| *q * w).sum::<DVec2>();
        let slope = p.iter().zip(db).map(|(q, w)| *q * w).sum::<DVec2>();
        (position, slope.normalize())
    };
    // Arc length by fine steps, then each equal step by interpolation.
    const FINE: usize = 2000;
    let samples: Vec<(DVec2, DVec2)> = (0..=FINE).map(|i| at(i as f64 / FINE as f64)).collect();
    let mut cumulative = vec![0.0];
    for w in samples.windows(2) {
        cumulative.push(cumulative.last().copied().unwrap_or(0.0) + (w[1].0 - w[0].0).length());
    }
    let scale = length / cumulative.last().copied().unwrap_or(1.0);
    (0..=count)
        .map(|k| {
            let s = cumulative.last().copied().unwrap_or(0.0) * k as f64 / count as f64;
            let i = cumulative.partition_point(|c| *c < s).clamp(1, FINE);
            let w = ((s - cumulative[i - 1]) / (cumulative[i] - cumulative[i - 1]).max(1e-300))
                .clamp(0.0, 1.0);
            let (q, d) = (
                samples[i - 1].0.lerp(samples[i].0, w),
                samples[i - 1].1.lerp(samples[i].1, w).normalize(),
            );
            (
                start + (tangent * q.x + outward * q.y) * scale,
                tangent * d.x + outward * d.y,
            )
        })
        .collect()
}

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
    if let DischargeShape::Spline {
        turn,
        start_share,
        end_share,
    } = design.diffuser.shape
    {
        let path = spline_path(
            start,
            tangent,
            outward,
            turn,
            [start_share, end_share],
            total,
            count,
        );
        return Ok((lifted(design, path, total)?, turn, total));
    }
    let (bend, radius) = match design.diffuser.shape {
        DischargeShape::Tangential { .. } | DischargeShape::Spline { .. } => (0.0, 0.0),
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
    Ok((lifted(design, points, length)?, bend, length))
}

/// The centerline `path` of the horizontal plane carried along the pump axis by the
/// z-bend of the design, if it has one, with the tangents turned to follow it.
fn lifted(
    design: &VoluteDesign,
    path: Vec<PathPoint>,
    length: f64,
) -> Result<Vec<PathPoint>, DesignError> {
    let Some(bend) = design.diffuser.z_bend else {
        return Ok(path);
    };
    let profile = ZProfile::new(bend, length)?;
    let count = path.len() - 1;
    Ok(path
        .into_iter()
        .enumerate()
        .map(|(k, (position, tangent))| {
            let s = length * k as f64 / count as f64;
            (
                position + DVec3::Z * profile.z(s),
                (tangent + DVec3::Z * profile.slope(s)).normalize(),
            )
        })
        .collect())
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
    spiral_slope: f64,
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
    let across = |t: DVec3| t.cross(DVec3::Z).normalize() * side;
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
    let (x_lo, x_hi) = throat_points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
    // The outline the sections tend to, point for point along the rays of the throat: the
    // circle, or a rectangle of the same area with its corners on the nearest rays.
    let circle: Vec<DVec2> = match design.diffuser.outlet_shape {
        OutletShape::Round => throat_points
            .iter()
            .map(|p| r1 * DVec2::new(p.x, p.y).normalize())
            .collect(),
        OutletShape::Rectangular { aspect } => {
            let area = std::f64::consts::PI * r1 * r1;
            let (hw, hh) = (0.5 * (area / aspect).sqrt(), 0.5 * (area * aspect).sqrt());
            let mut points: Vec<DVec2> = throat_points
                .iter()
                .map(|p| {
                    let d = p.normalize();
                    d * (hw / d.x.abs()).min(hh / d.y.abs())
                })
                .collect();
            for corner in [(hw, hh), (-hw, hh), (-hw, -hh), (hw, -hh)] {
                let corner = DVec2::new(corner.0, corner.1);
                let nearest = throat_points
                    .iter()
                    .enumerate()
                    .max_by(|a, b| {
                        a.1.normalize()
                            .dot(corner.normalize())
                            .total_cmp(&b.1.normalize().dot(corner.normalize()))
                    })
                    .map(|(k, _)| k)
                    .expect("a throat outline");
                points[nearest] = corner;
            }
            points
        }
    };
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
            // The base side, toward the pump axis, rounds sooner by the base form factor:
            // full weight at its edge, none over the far half, building up from the throat.
            let reach_b = (s / BASE_REACH).min(1.0);
            let base = design.diffuser.base_form * reach_b * reach_b * (3.0 - 2.0 * reach_b);
            let blend: Vec<DVec2> = throat_points
                .iter()
                .zip(&circle)
                .map(|(t, c)| {
                    let near = ((x_hi - t.x) / (x_hi - x_lo) * 2.0 - 1.0).clamp(0.0, 1.0);
                    let weight = near * near * (3.0 - 2.0 * near);
                    t.lerp(*c, w + (1.0 - w) * base * weight)
                })
                .collect();
            let radius = match design.diffuser.growth {
                DiffuserGrowth::Linear => r1 + (r2 - r1) * s,
                DiffuserGrowth::Matched => {
                    // Quadratic area A₀ + a·s + (A₁ − A₀ − a)·s², with the slope of the
                    // spiral at the throat, as far as the outlet area allows.
                    let (a0, a1) = (PI * r1 * r1, PI * r2 * r2);
                    let a = (spiral_slope * length).clamp(0.0, (a1 - a0).max(0.0));
                    ((a0 + a * s + (a1 - a0 - a) * s * s) / PI).sqrt()
                }
            };
            let scale = radius / r1 * (circle_area / polygon_area(&blend).abs()).sqrt();
            let lowest = blend.iter().map(|p| p.x).fold(f64::INFINITY, f64::min) * scale;
            let highest = blend.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max) * scale;
            let u = across(tangent);
            // Up across the section: the pump axis made perpendicular to the centerline.
            let v = (DVec3::Z - tangent * DVec3::Z.dot(tangent)).normalize();
            let e = design.diffuser.eccentricity;
            let shift = match design.diffuser.shape {
                DischargeShape::Tangential { .. } if e < 0.0 => -e * (inner - lowest),
                DischargeShape::Tangential { .. } => e * (outer - highest),
                DischargeShape::Radial { .. } | DischargeShape::Spline { .. } => 0.0,
            };
            // The height offset grows from nothing at the throat, with no slope there so
            // that the diffuser leaves along the spiral, to its full value over the first
            // OFFSET_REACH of the length: it is room for the cutwater, which sits at the
            // start.
            let reach = (s / OFFSET_REACH).min(1.0);
            let ramp = reach * reach * (3.0 - 2.0 * reach);
            let mut center = position + u * (shift + design.diffuser.height_offset * ramp);
            center.z += match design.diffuser.z_alignment {
                ZAlignment::Centric => 0.0,
                ZAlignment::Manual { offset, quadratic } => {
                    offset * if quadratic { s * s } else { s }
                }
            };
            NozzleSection {
                center,
                tangent,
                points: blend
                    .iter()
                    .map(|p| center + u * (p.x * scale) + v * (p.y * scale))
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
    use crate::volute::{AreaLaw, CrossSectionInput, SectionShape, SideShape, StraightWall};

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
        input.cross_section = CrossSectionInput::single(shape);
        VoluteGeometry::build(&reference_basis(), &input).unwrap()
    }

    #[test]
    fn outlines_have_a_fixed_point_count() {
        for shape in [
            SectionShape::rectangular(),
            SectionShape::round(),
            SectionShape::default(),
            SectionShape::symmetric(SideShape::RadiusBased(Default::default())),
            SectionShape::RoundExternal(Default::default()),
        ] {
            let g = geometry(shape);
            assert_eq!(g.sections.len(), 37);
            for ring in g.rings() {
                assert_eq!(ring.len(), g.outline_points());
            }
            assert_eq!(g.nozzle.len(), NOZZLE_SECTIONS + 1);
        }
    }

    /// The cases the junction is measured on: three section shapes, a straight and a
    /// radial discharge.
    fn junction_cases() -> Vec<(String, VoluteGeometry)> {
        let shapes = [
            ("trapezoid", SectionShape::default()),
            ("rectangle", SectionShape::rectangular()),
            ("round", SectionShape::round()),
        ];
        let mut cases = Vec::new();
        for (name, shape) in shapes {
            for radial in [false, true] {
                let mut input = unturned();
                input.cross_section = CrossSectionInput::single(shape.clone());
                if radial {
                    input.diffuser.shape = DischargeShape::default();
                }
                let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
                cases.push((
                    format!("{name} {}", if radial { "radial" } else { "tangential" }),
                    g,
                ));
            }
        }
        cases
    }

    #[test]
    fn a_fillet_below_the_table_10_2_diameter_is_warned_of() {
        let warning = fillet_diameter_warning(0.180, 0.190, 0.172).expect("below the minimum");
        assert!(
            warning.contains("1.047") && warning.contains("1.105"),
            "{warning}"
        );
        assert!(fillet_diameter_warning(0.195, 0.190, 0.172).is_none());
        assert!(fillet_diameter_warning(0.190, 0.190, 0.172).is_none());
    }

    #[test]
    fn a_spiral_that_wraps_less_than_330_degrees_is_warned_of() {
        let early = 3f64.to_radians();
        let late = 40f64.to_radians();
        for shape in [CutwaterShape::Sharp, CutwaterShape::default()] {
            assert!(short_wrap_warning(shape, early).is_none());
            let warning = short_wrap_warning(shape, late).expect("a short wrap");
            assert!(
                warning.contains("320°") && warning.contains("330°"),
                "{warning}"
            );
        }
        // A tangent cutwater has no wedge to build.
        assert!(short_wrap_warning(CutwaterShape::Tangent, late).is_none());
        // The default design wraps enough.
        let g = geometry(SectionShape::default());
        assert!(g.warnings.iter().all(|w| !w.contains("wraps")));
    }

    #[test]
    fn the_throat_junction_is_measured_on_every_case() {
        // Prints the table of the bench: `cargo test -p tp-design throat_junction -- --nocapture`.
        for (name, g) in junction_cases() {
            let j = g.throat_junction().expect("a single volute");
            println!(
                "{name:22} area slope ×{:6.2}  curvature {:6.2} → {:6.2} 1/m  turn {:5.2}°",
                j.area_slope_ratio,
                j.curvature_before,
                j.curvature_after,
                j.direction_turn.to_degrees()
            );
            assert!(j.area_slope_ratio.is_finite() && j.area_slope_ratio > 0.0);
            assert!(j.curvature_before.is_finite() && j.curvature_after.is_finite());
        }
    }

    #[test]
    fn the_throat_faces_of_spiral_and_diffuser_are_compared() {
        // Prints how much of the last section of the spiral lies outside the first
        // section of the diffuser, in the plane of the spiral's: `-- --nocapture`.
        for (name, g) in junction_cases() {
            let sign = g.basis.rotation_sign;
            let spiral = g.sections.last().unwrap().points(sign);
            let nozzle = &g.nozzle[0];
            let mean = |p: &[DVec3]| p.iter().copied().sum::<DVec3>() / p.len() as f64;
            let c = mean(&spiral);
            let normal = (spiral[0] - c)
                .cross(spiral[spiral.len() / 3] - c)
                .normalize();
            let u = (spiral[0] - c).reject_from(normal).normalize();
            let v = normal.cross(u);
            let flat = |p: &[DVec3]| -> Vec<DVec2> {
                p.iter()
                    .map(|q| DVec2::new((*q - c).dot(u), (*q - c).dot(v)))
                    .collect()
            };
            let (a, b) = (flat(&spiral), flat(&nozzle.points));
            let inside = |poly: &[DVec2], q: DVec2| {
                let mut hit = false;
                for i in 0..poly.len() {
                    let (p, r) = (poly[i], poly[(i + 1) % poly.len()]);
                    if (p.y > q.y) != (r.y > q.y)
                        && q.x < p.x + (q.y - p.y) / (r.y - p.y) * (r.x - p.x)
                    {
                        hit = !hit;
                    }
                }
                hit
            };
            let (lo, hi) = a
                .iter()
                .fold((DVec2::splat(1e9), DVec2::splat(-1e9)), |(l, h), q| {
                    (l.min(*q), h.max(*q))
                });
            let (mut n_a, mut n_out) = (0usize, 0usize);
            for i in 0..80 {
                for j in 0..80 {
                    let q = lo
                        + (hi - lo) * DVec2::new((i as f64 + 0.5) / 80.0, (j as f64 + 0.5) / 80.0);
                    if inside(&a, q) {
                        n_a += 1;
                        if !inside(&b, q) {
                            n_out += 1;
                        }
                    }
                }
            }
            println!(
                "{name:22} centre gap {:6.2} mm  normal vs tangent {:5.1}°  area {:7.1} → {:7.1} mm²  spiral face outside diffuser {:5.1} %",
                (mean(&nozzle.points) - c).length() * 1e3,
                normal.dot(nozzle.tangent).abs().acos().to_degrees(),
                g.sections.last().unwrap().size.area * 1e6,
                nozzle.area * 1e6,
                100.0 * n_out as f64 / n_a as f64
            );
        }
    }

    /// The cases of `junction_cases` with the given growth law of the diffuser.
    fn junction_cases_with(growth: DiffuserGrowth) -> Vec<(String, VoluteGeometry)> {
        let mut cases = Vec::new();
        for (name, shape) in [
            ("trapezoid", SectionShape::default()),
            ("rectangle", SectionShape::rectangular()),
            ("round", SectionShape::round()),
        ] {
            for radial in [false, true] {
                let mut input = unturned();
                input.cross_section = CrossSectionInput::single(shape.clone());
                input.diffuser.growth = growth;
                if radial {
                    input.diffuser.shape = DischargeShape::default();
                }
                let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
                cases.push((
                    format!("{name} {}", if radial { "radial" } else { "tangential" }),
                    g,
                ));
            }
        }
        cases
    }

    #[test]
    fn the_matched_law_has_no_break_in_the_area_at_the_throat() {
        for (name, g) in junction_cases_with(DiffuserGrowth::Matched) {
            let j = g.throat_junction().expect("a single volute");
            // The break of the linear law is ×4.9 to ×5.5.
            assert!(
                (0.9..=1.1).contains(&j.area_slope_ratio),
                "{name}: the area slope jumps ×{:.2} at the throat",
                j.area_slope_ratio
            );
        }
    }

    #[test]
    fn both_laws_start_at_the_throat_and_end_at_the_outlet_area() {
        let linear = junction_cases_with(DiffuserGrowth::Linear);
        let matched = junction_cases_with(DiffuserGrowth::Matched);
        for ((name, a), (_, b)) in linear.iter().zip(&matched) {
            for g in [a, b] {
                let (first, last) = (&g.nozzle[0], g.nozzle.last().unwrap());
                assert_relative_eq!(first.area, g.design.throat.area, max_relative = 1e-9);
                let outlet = PI * g.design.diffuser.outlet_diameter.powi(2) / 4.0;
                assert_relative_eq!(last.area, outlet, max_relative = 1e-9);
                assert!(
                    g.nozzle.windows(2).all(|w| w[1].area > w[0].area),
                    "{name}: the area must grow along the diffuser"
                );
            }
            // Same inlet, same outlet, same length: only the law between them differs.
            assert_relative_eq!(a.nozzle_length, b.nozzle_length, max_relative = 1e-12);
            assert_eq!(a.nozzle.len(), b.nozzle.len());
        }
    }

    #[test]
    fn the_default_law_is_the_linear_one() {
        let default = geometry(SectionShape::default());
        let linear = junction_cases_with(DiffuserGrowth::Linear).remove(0).1;
        for (a, b) in default.nozzle.iter().zip(&linear.nozzle) {
            assert_eq!(a.points, b.points);
        }
    }

    #[test]
    fn the_wedge_is_the_difference_of_the_wall_angles() {
        for shape in [CutwaterShape::Sharp, CutwaterShape::default()] {
            let mut input = unturned();
            input.cutwater.shape = shape;
            let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
            let c = g.cutwater;
            assert_relative_eq!(
                c.wedge,
                (c.diffuser_angle - c.spiral_angle).abs(),
                epsilon = 1e-9
            );
            // The wall of the spiral runs nearly along the circle; that of the diffuser
            // leaves it outward.
            assert!(c.spiral_angle.abs() < 0.3, "{c:?}");
            assert!(c.mean_angle() >= c.spiral_angle.min(c.diffuser_angle));
            assert!(c.mean_angle() <= c.spiral_angle.max(c.diffuser_angle));
        }
        // No wedge to speak of for a tangent cutwater.
        let mut input = unturned();
        input.cutwater.shape = CutwaterShape::Tangent;
        let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        assert_eq!(
            (g.cutwater.spiral_angle, g.cutwater.diffuser_angle),
            (0.0, 0.0)
        );
    }

    #[test]
    fn one_intersection_curve_gives_no_warning() {
        type Edit = fn(&mut crate::volute::DischargeInput);
        let edits: [Edit; 2] = [
            |_| {},
            |d| {
                d.height_offset = Param::Manual(0.01);
                d.base_form = Param::Manual(0.75);
            },
        ];
        for edit in edits {
            for shape in [CutwaterShape::Sharp, CutwaterShape::default()] {
                let mut input = unturned();
                input.cutwater.shape = shape;
                edit(&mut input.diffuser);
                let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
                assert!(
                    !g.warnings.iter().any(|w| w.contains("intersection curves")),
                    "{:?}",
                    g.warnings
                );
            }
        }
    }

    #[test]
    fn the_base_form_factor_rounds_the_base_side_only() {
        let plain = with_diffuser(|_| {});
        let round = with_diffuser(|d| d.base_form = Param::Manual(0.75));
        // The throat stays, and the area law does not change.
        for (a, b) in plain.nozzle[0].points.iter().zip(&round.nozzle[0].points) {
            assert!(a.distance(*b) < 1e-12);
        }
        for (a, b) in plain.nozzle.iter().zip(&round.nozzle) {
            assert_relative_eq!(a.area, b.area, epsilon = 1e-12);
        }
        // Past the first steps the outline differs, more on the base side (toward the
        // axis, small radius) than on the far side.
        let k = 4;
        let radius = |p: &DVec3| p.truncate().length();
        let moved: Vec<f64> = plain.nozzle[k]
            .points
            .iter()
            .zip(&round.nozzle[k].points)
            .map(|(a, b)| a.distance(*b))
            .collect();
        assert!(moved.iter().cloned().fold(0.0, f64::max) > 1e-3);
        let (near, far): (Vec<f64>, Vec<f64>) = {
            let mid = plain.nozzle[k].points.iter().map(radius).sum::<f64>()
                / plain.nozzle[k].points.len() as f64;
            let mut near = Vec::new();
            let mut far = Vec::new();
            for (p, d) in plain.nozzle[k].points.iter().zip(&moved) {
                if radius(p) < mid {
                    near.push(*d);
                } else {
                    far.push(*d);
                }
            }
            (near, far)
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        assert!(
            mean(&near) > mean(&far),
            "{} vs {}",
            mean(&near),
            mean(&far)
        );
        // Out of range is refused.
        let mut input = unturned();
        input.diffuser.base_form = Param::Manual(0.8);
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
    }

    #[test]
    fn the_walls_are_flagged_when_they_turn_away_from_the_flow() {
        let cases = [
            DischargeShape::Radial { axis_angle: 0.0 },
            DischargeShape::Radial {
                axis_angle: 30f64.to_radians(),
            },
            DischargeShape::Tangential {
                straight_wall: StraightWall::Centered,
            },
            DischargeShape::Tangential {
                straight_wall: StraightWall::Inner,
            },
            DischargeShape::Tangential {
                straight_wall: StraightWall::Outer,
            },
        ];
        let mut flagged = [0, 0];
        for shape in cases {
            let mut input = unturned();
            input.cutwater.shape = CutwaterShape::Sharp;
            input.diffuser.shape = shape;
            let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
            let off = g.wall_misalignment();
            assert_relative_eq!(
                off,
                g.cutwater.mean_angle() - g.design.cutwater_flow_angle,
                epsilon = 1e-12
            );
            let warned = g
                .warnings
                .iter()
                .any(|w| w.contains("walls at the cutwater tip"));
            assert_eq!(warned, off.abs() > 3f64.to_radians(), "{shape:?}");
            flagged[usize::from(warned)] += 1;
        }
        // Both outcomes occur among the discharges.
        assert!(flagged[0] > 0 && flagged[1] > 0, "{flagged:?}");
        // A rounded cutwater reports the angle but is not flagged for it.
        let mut input = unturned();
        input.cutwater.shape = CutwaterShape::default();
        let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        assert!(g.wall_misalignment().abs() > 3f64.to_radians());
        assert!(
            !g.warnings
                .iter()
                .any(|w| w.contains("walls at the cutwater tip"))
        );
        // A tangent cutwater has no wedge to judge.
        let mut input = unturned();
        input.cutwater.shape = CutwaterShape::Tangent;
        let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        assert_eq!(g.wall_misalignment(), 0.0);
        assert!(
            !g.warnings
                .iter()
                .any(|w| w.contains("walls at the cutwater tip"))
        );
    }

    /// The reference volute with the discharge diffuser changed.
    fn with_diffuser(edit: impl FnOnce(&mut crate::volute::DischargeInput)) -> VoluteGeometry {
        let mut input = unturned();
        edit(&mut input.diffuser);
        VoluteGeometry::build(&reference_basis(), &input).unwrap()
    }

    #[test]
    fn the_eccentricity_runs_between_the_three_straight_walls() {
        let tangential = |wall: StraightWall, e: Option<f64>| {
            with_diffuser(|d| {
                d.shape = DischargeShape::Tangential {
                    straight_wall: wall,
                };
                if let Some(e) = e {
                    d.eccentricity = Param::Manual(e);
                }
            })
        };
        let last = |g: &VoluteGeometry| g.nozzle.last().unwrap().center;
        // The manual values at the ends are the chosen walls.
        for (wall, e) in [
            (StraightWall::Inner, -1.0),
            (StraightWall::Centered, 0.0),
            (StraightWall::Outer, 1.0),
        ] {
            let by_wall = tangential(wall, None);
            let by_value = tangential(StraightWall::Centered, Some(e));
            for (a, b) in by_wall.nozzle.iter().zip(&by_value.nozzle) {
                for (p, q) in a.points.iter().zip(&b.points) {
                    assert!(p.distance(*q) < 1e-12, "{wall:?}");
                }
            }
        }
        // In between, the outlet sits in between.
        let inner = last(&tangential(StraightWall::Centered, Some(-1.0)));
        let middle = last(&tangential(StraightWall::Centered, Some(0.0)));
        let half = last(&tangential(StraightWall::Centered, Some(-0.5)));
        assert_relative_eq!(
            half.distance(middle),
            0.5 * inner.distance(middle),
            epsilon = 1e-9
        );
        // Out of range is refused.
        let mut input = unturned();
        input.diffuser.eccentricity = Param::Manual(1.2);
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
    }

    fn z_bend(extent_mm: f64, radius_mm: f64, angle_deg: f64) -> Option<ZBend> {
        Some(ZBend {
            extent: extent_mm * 1e-3,
            radius: radius_mm * 1e-3,
            angle: angle_deg.to_radians(),
        })
    }

    #[test]
    fn a_z_bend_carries_the_diffuser_along_the_pump_axis() {
        let plain = with_diffuser(|_| {});
        let bent = with_diffuser(|d| d.z_bend = z_bend(30.0, 80.0, 25.0));
        let (first, last) = (&bent.nozzle[0], bent.nozzle.last().unwrap());
        // The throat is the throat, and the centerline leaves it flat.
        for (a, b) in plain.nozzle[0].points.iter().zip(&first.points) {
            assert!(a.distance(*b) < 1e-12);
        }
        assert!(first.tangent.z.abs() < 1e-12);
        // It ends the extension asked for away from the plane of the volute, at the
        // angle asked for, with the horizontal path as it was.
        let end_plain = plain.nozzle.last().unwrap().center;
        assert_relative_eq!(last.center.z - end_plain.z, 0.03, epsilon = 1e-9);
        assert_relative_eq!(last.tangent.z.asin(), 25f64.to_radians(), epsilon = 1e-9);
        assert!(last.center.truncate().distance(end_plain.truncate()) < 1e-9);
        assert_relative_eq!(bent.outlet_height, plain.outlet_height, epsilon = 1e-9);
        // The sections stay circles of the same area, now square to the tilted centerline.
        for (k, (a, b)) in plain.nozzle.iter().zip(&bent.nozzle).enumerate() {
            assert_relative_eq!(a.area, b.area, epsilon = 1e-12);
            // The throat itself lies in the radial plane of the spiral.
            for p in b.points.iter().filter(|_| k > 0) {
                assert!((*p - b.center).dot(b.tangent).abs() < 1e-9);
            }
        }
        // It rises smoothly: no step in height between sections.
        let rise: Vec<f64> = bent
            .nozzle
            .windows(2)
            .map(|w| w[1].center.z - w[0].center.z)
            .collect();
        assert!(rise.iter().all(|r| *r >= -1e-12));
        assert!(rise.windows(2).all(|r| r[1] >= r[0] - 1e-9), "{rise:?}");
        // What cannot be built is refused: too much for the length, too little for the
        // round, a round longer than the diffuser.
        for (extent, radius, angle) in [(500.0, 80.0, 25.0), (1.0, 80.0, 25.0), (30.0, 900.0, 25.0)]
        {
            let mut input = unturned();
            input.diffuser.z_bend = z_bend(extent, radius, angle);
            assert!(
                VoluteGeometry::build(&reference_basis(), &input).is_err(),
                "{extent} {radius} {angle}"
            );
        }
    }

    fn spline(turn_deg: f64) -> DischargeShape {
        DischargeShape::Spline {
            turn: turn_deg.to_radians(),
            start_share: 0.5,
            end_share: 0.5,
        }
    }

    #[test]
    fn a_spline_diffuser_leaves_the_throat_along_the_flow_and_turns_as_asked() {
        let g = with_diffuser(|d| d.shape = spline(60.0));
        let (first, last) = (&g.nozzle[0], g.nozzle.last().unwrap());
        // It leaves the throat along the path of the spiral, as the other shapes do.
        let straight = with_discharge(DischargeShape::Tangential {
            straight_wall: StraightWall::Centered,
        });
        assert!(first.tangent.dot(straight.nozzle[0].tangent) > 1.0 - 1e-9);
        // It ends the turn further round, toward the outside of the volute.
        let turned = first.tangent.angle_between(last.tangent);
        assert_relative_eq!(turned, 60f64.to_radians(), epsilon = 1e-6);
        assert_relative_eq!(g.nozzle_bend, 60f64.to_radians(), epsilon = 1e-12);
        // Its length along the centerline is the length of the diffuser.
        let along: f64 = g
            .nozzle
            .windows(2)
            .map(|w| w[0].center.distance(w[1].center))
            .sum();
        assert!(
            along < g.nozzle_length && along > 0.97 * g.nozzle_length,
            "{along}"
        );
        // The steps are of equal length, and the tangent follows the path.
        let steps: Vec<f64> = g
            .nozzle
            .windows(2)
            .map(|w| w[0].center.distance(w[1].center))
            .collect();
        let mean = steps.iter().sum::<f64>() / steps.len() as f64;
        assert!(
            steps.iter().all(|s| (s / mean - 1.0).abs() < 0.05),
            "{steps:?}"
        );
        // The areas are the same as ever.
        let radial = with_discharge(DischargeShape::Radial { axis_angle: 0.0 });
        assert_relative_eq!(
            last.area,
            radial.nozzle.last().unwrap().area,
            epsilon = 1e-12
        );
        // Left to turn itself, the volute turns so that the outlet points vertically up.
        let mut input = VoluteInput::default();
        input.diffuser.shape = spline(60.0);
        let auto = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        assert!(auto.nozzle_axis.y > 0.99, "{:?}", auto.nozzle_axis);
    }

    #[test]
    fn the_shares_of_a_spline_change_its_shape_not_its_ends() {
        let a = with_diffuser(|d| d.shape = spline(60.0));
        let b = with_diffuser(|d| {
            d.shape = DischargeShape::Spline {
                turn: 60f64.to_radians(),
                start_share: 0.2,
                end_share: 0.8,
            }
        });
        let k = a.nozzle.len() / 2;
        assert!(a.nozzle[k].center.distance(b.nozzle[k].center) > 2e-4);
        assert_relative_eq!(a.nozzle_length, b.nozzle_length, epsilon = 1e-12);
        let turn = |g: &VoluteGeometry| {
            g.nozzle[0]
                .tangent
                .angle_between(g.nozzle.last().unwrap().tangent)
        };
        assert_relative_eq!(turn(&a), turn(&b), epsilon = 1e-6);
        // Out of range is refused.
        let mut input = unturned();
        input.diffuser.shape = spline(200.0);
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
        input.diffuser.shape = DischargeShape::Spline {
            turn: 1.0,
            start_share: 0.0,
            end_share: 0.5,
        };
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
    }

    #[test]
    fn a_flange_height_sets_the_length_of_a_spline_diffuser() {
        let free = with_diffuser(|d| d.shape = spline(60.0));
        let wanted = free.outlet_height + 0.04;
        let g = with_diffuser(|d| {
            d.shape = spline(60.0);
            d.flange_height = Param::Manual(wanted);
        });
        assert_relative_eq!(g.outlet_height, wanted, epsilon = 1e-6);
        assert!(g.nozzle_length > free.nozzle_length);
        // Out of reach is refused.
        let mut input = unturned();
        input.diffuser.shape = spline(60.0);
        input.diffuser.flange_height = Param::Manual(100.0);
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
    }

    #[test]
    fn a_rectangular_outlet_has_the_area_and_proportions_asked_for() {
        use crate::volute::OutletShape;
        let round = with_diffuser(|_| {});
        let aspect = 2.0;
        let rect = with_diffuser(|d| d.outlet_shape = OutletShape::Rectangular { aspect });
        // The throat and the area law are the same.
        for (a, b) in round.nozzle[0].points.iter().zip(&rect.nozzle[0].points) {
            assert!(a.distance(*b) < 1e-12);
        }
        let (a, b) = (round.nozzle.last().unwrap(), rect.nozzle.last().unwrap());
        assert_relative_eq!(a.area, b.area, epsilon = 1e-12);
        // The outlet outline is a rectangle with that aspect: its extent along the axis
        // over its extent across, and its corners among the points.
        let (lo, hi) = b.points.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p - b.center), hi.max(*p - b.center)),
        );
        let height = hi.z - lo.z;
        let width =
            (b.points.iter().map(|p| (*p - b.center).truncate().length())).fold(0.0, f64::max);
        assert!(width > 0.0);
        let across = b
            .points
            .iter()
            .map(|p| {
                let d = (*p - b.center).truncate();
                d.dot(b.tangent.truncate().perp()).abs()
            })
            .fold(0.0, f64::max)
            * 2.0;
        assert_relative_eq!(height / across, aspect, max_relative = 1e-6);
        // The polygon area is that of the circle it replaces, within the sampling.
        let polygon: f64 = {
            let n = b.points.len();
            0.5 * (0..n)
                .map(|i| {
                    let (p, q) = (b.points[i] - b.center, b.points[(i + 1) % n] - b.center);
                    p.cross(q).length()
                })
                .sum::<f64>()
        };
        assert_relative_eq!(polygon, b.area, max_relative = 0.02);
        // A flange is not drawn on it, and says so; out of range is refused.
        let mut input = unturned();
        input.diffuser.outlet_shape = OutletShape::Rectangular { aspect };
        input.wall.flange.enabled = true;
        let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        assert!(g.design.flange.is_none());
        assert!(g.warnings.iter().any(|w| w.contains("rectangular outlet")));
        input.diffuser.outlet_shape = OutletShape::Rectangular { aspect: 10.0 };
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
    }

    #[test]
    fn no_offsets_leave_the_diffuser_as_it_was() {
        let plain = with_diffuser(|_| {});
        let zero = with_diffuser(|d| {
            d.height_offset = Param::Manual(0.0);
            d.z_alignment = ZAlignment::Manual {
                offset: 0.0,
                quadratic: false,
            };
        });
        for (a, b) in plain.nozzle.iter().zip(&zero.nozzle) {
            for (p, q) in a.points.iter().zip(&b.points) {
                assert!(p.distance(*q) < 1e-12);
            }
        }
    }

    #[test]
    fn the_height_offset_moves_the_diffuser_away_from_the_spiral() {
        let plain = with_diffuser(|_| {});
        let offset = 0.01;
        let moved = with_diffuser(|d| d.height_offset = Param::Manual(offset));
        // The throat stays, the outlet is shifted by the whole offset across its axis.
        for (a, b) in plain.nozzle[0].points.iter().zip(&moved.nozzle[0].points) {
            assert!(a.distance(*b) < 1e-12);
        }
        let (a, b) = (plain.nozzle.last().unwrap(), moved.nozzle.last().unwrap());
        assert_relative_eq!(a.center.distance(b.center), offset, epsilon = 1e-9);
        // The first step has hardly left: the shift starts with no slope.
        let first = plain.nozzle[1].center.distance(moved.nozzle[1].center);
        assert!(first < 0.2 * offset, "{first}");
        // It is complete after 40 % of the length.
        let at = |g: &VoluteGeometry, k: usize| g.nozzle[k].center;
        assert_relative_eq!(
            at(&plain, 4).distance(at(&moved, 4)),
            offset,
            epsilon = 1e-9
        );
        // The outlet axis stands that much further from the pump axis (C).
        assert_relative_eq!(
            moved.outlet_offset() - plain.outlet_offset(),
            offset,
            epsilon = 1e-9
        );
        // Too much is refused.
        let mut input = unturned();
        input.diffuser.height_offset = Param::Manual(1.0);
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
    }

    #[test]
    fn a_manual_axial_offset_reaches_its_value_at_the_outlet() {
        let linear = with_diffuser(|d| {
            d.z_alignment = ZAlignment::Manual {
                offset: 0.02,
                quadratic: false,
            }
        });
        let quadratic = with_diffuser(|d| {
            d.z_alignment = ZAlignment::Manual {
                offset: 0.02,
                quadratic: true,
            }
        });
        let centric = with_diffuser(|_| {});
        for g in [&linear, &quadratic] {
            let last = g.nozzle.len() - 1;
            assert_relative_eq!(
                g.nozzle[last].center.z - centric.nozzle[last].center.z,
                0.02,
                epsilon = 1e-9
            );
        }
        // Halfway: half of it, or a quarter.
        let mid = linear.nozzle.len() / 2;
        let share =
            |g: &VoluteGeometry| (g.nozzle[mid].center.z - centric.nozzle[mid].center.z) / 0.02;
        assert_relative_eq!(share(&linear), 0.5, epsilon = 1e-9);
        assert_relative_eq!(share(&quadratic), 0.25, epsilon = 1e-9);
    }

    #[test]
    fn a_double_volute_measures_the_outlet_angle_from_its_throat_at_2pi() {
        let single = VoluteGeometry::build(&reference_basis(), &unturned()).unwrap();
        let mut input = unturned();
        input.setup.kind = crate::volute::VoluteKind::Double;
        let double = VoluteGeometry::build(&reference_basis(), &input).unwrap();
        // Same discharge, so the same angle, whatever the sections before the throat.
        assert_relative_eq!(double.outlet_angle(), single.outlet_angle(), epsilon = 0.05);
    }

    #[test]
    fn the_rib_nose_of_a_double_volute_is_180_degrees_from_the_cutwater() {
        for blades in [5, 6] {
            let mut basis = reference_basis();
            basis.impeller_blades = blades;
            let mut input = unturned();
            input.setup.kind = crate::volute::VoluteKind::Double;
            let g = VoluteGeometry::build(&basis, &input).unwrap();
            let rib = g.rib.as_ref().unwrap();
            assert_relative_eq!(rib.nose_angle, PI, epsilon = 0.03);
            // The inner volute runs from there to the throat, toward the discharge.
            assert_relative_eq!(g.design.rib.unwrap().wrap, PI, epsilon = 1e-12);
            assert!(g.inner_sections.iter().all(|s| s.angle > PI - 1e-9));
        }
    }

    #[test]
    fn the_outlet_offset_and_angle_follow_the_discharge() {
        // A radial discharge on the pump centerline: no offset, and it lies at the
        // angle between the throat and the vertical.
        let radial = with_discharge(DischargeShape::Radial { axis_angle: 0.0 });
        assert!(radial.outlet_offset() < 1e-6, "{}", radial.outlet_offset());
        let last = radial.nozzle.last().unwrap().center;
        let towards = radial.outlet_angle();
        let expected = last.y.atan2(last.x) - 0.0;
        assert_relative_eq!(
            towards * radial.basis.rotation_sign,
            expected,
            epsilon = 1e-9
        );
        // A tilted one stands off the line through the axis; a straight one too.
        let tilted = with_discharge(DischargeShape::Radial {
            axis_angle: 20f64.to_radians(),
        });
        assert!(tilted.outlet_offset() > 1e-3);
        let straight = with_discharge(DischargeShape::Tangential {
            straight_wall: StraightWall::Centered,
        });
        let r = straight.sections.last().unwrap().outline[0].x;
        assert!(
            straight.outlet_offset() > 0.5 * r,
            "{}",
            straight.outlet_offset()
        );
    }

    #[test]
    fn outline_areas_match_the_design_sections() {
        let g = geometry(SectionShape::default());
        for s in &g.sections {
            let polygon = polygon_area(&s.outline);
            assert!(polygon > 0.0, "outlines turn counter-clockwise");
            assert_relative_eq!(polygon, s.size.area, max_relative = 1e-6);
        }
        let circles = geometry(SectionShape::round());
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
            input.setup.rib_length = crate::Param::Manual(w);
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
            assert_eq!(ring.len(), g.outline_points());
        }
        // The diffuser starts from both channels and the rib between them.
        let first = &g.nozzle[0];
        let outer = g.outer_channel.last().unwrap();
        assert!(first.area > g.design.throat.area + outer.size.area);
        assert_eq!(g.nozzle.len(), NOZZLE_SECTIONS + 1);
    }

    #[test]
    fn the_rib_nose_leans_with_the_cutwater_across_its_height() {
        let double = |obliquity: f64| {
            let mut input = unturned();
            input.setup.kind = crate::volute::VoluteKind::Double;
            input.cutwater.obliquity = crate::Param::Manual(obliquity);
            VoluteGeometry::build(&reference_basis(), &input).unwrap()
        };
        let straight = double(0.0);
        let r = straight.rib.as_ref().unwrap();
        assert_eq!(r.planes.len(), RIB_PLANES);
        // Without obliquity every plane is the midplane outline at its own height.
        let mid = r.outline();
        for p in &r.planes {
            for (a, b) in p.outline().iter().zip(&mid) {
                assert!(a.truncate().distance(b.truncate()) < 1e-9);
                assert!((a.z - p.z).abs() < 1e-12);
            }
        }
        // Across the height of the sections.
        let (lo, hi) = (r.planes[0].z, r.planes[RIB_PLANES - 1].z);
        assert!(hi - lo > straight.design.inlet_width);
        // Leaning, the nose turns downstream toward +z by the obliquity unrolled onto
        // its circle, and the tail stays.
        let obliquity = 30f64.to_radians();
        let g = double(obliquity);
        let r = g.rib.as_ref().unwrap();
        let sign = g.basis.rotation_sign;
        let angle = |p: &DVec3| sign * p.y.atan2(p.x);
        let tip = |p: &RibPlane| p.nose[RIB_NOSE_POINTS / 2];
        let (first, last) = (&r.planes[0], &r.planes[RIB_PLANES - 1]);
        let radius = g.design.stagnation_radius + g.design.rib.unwrap().thickness / 2.0;
        let turn = (angle(&tip(last)) - angle(&tip(first)) + PI).rem_euclid(TAU) - PI;
        let expected = (last.z - first.z) * obliquity.tan() / radius;
        assert!((turn / expected - 1.0).abs() < 0.05, "{turn} vs {expected}");
        assert!(
            first
                .tail
                .iter()
                .zip(&last.tail)
                .all(|(a, b)| a.truncate().distance(b.truncate()) < 1e-9)
        );
        assert!(r.planes.iter().all(|p| p.outline().len() == mid.len()));
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
        for shape in [SectionShape::rectangular(), SectionShape::default()] {
            let mut input = unturned();
            input.setup.kind = crate::volute::VoluteKind::Double;
            input.cross_section = CrossSectionInput::single(shape);
            let g = VoluteGeometry::build(&reference_basis(), &input).unwrap();
            assert!(!g.inner_sections.is_empty() && !g.outer_channel.is_empty());
            assert_eq!(g.envelope.len(), g.outer_channel.len());
        }
    }

    /// Radius based, line segment and Bézier sections, which a double volute splits with
    /// its rib by their outlines.
    fn free_shapes() -> Vec<(&'static str, SectionShape)> {
        use crate::volute::{BezierBase, BezierSide, RadiusSide};
        vec![
            (
                "radius based",
                SectionShape::symmetric(SideShape::RadiusBased(RadiusSide::default())),
            ),
            (
                "line segments",
                SectionShape::symmetric(SideShape::LineSegments {
                    points: crate::volute::section::default_segments(),
                }),
            ),
            (
                "Bézier trapezoid",
                SectionShape::symmetric(SideShape::Bezier(BezierSide {
                    base: BezierBase::Trapezoid,
                    ..BezierSide::default()
                })),
            ),
            (
                "Bézier rectangle",
                SectionShape::symmetric(SideShape::Bezier(BezierSide::default())),
            ),
        ]
    }

    fn free_double_geometry(shape: SectionShape) -> VoluteGeometry {
        let mut input = unturned();
        input.setup.kind = crate::volute::VoluteKind::Double;
        input.cross_section = CrossSectionInput::single(shape);
        VoluteGeometry::build(&reference_basis(), &input).unwrap()
    }

    /// ∫ w(r) f(r) dr over the whole section of the envelope at `angle` between the
    /// radii `from` and `to`, with the axial width w of its outline.
    fn envelope_integral(
        g: &VoluteGeometry,
        angle: f64,
        from: f64,
        to: f64,
        f: impl Fn(f64) -> f64,
    ) -> f64 {
        let n = 400;
        (0..n)
            .map(|k| {
                let r = from + (to - from) * (f64::from(k) + 0.5) / f64::from(n);
                let (lo, hi) = g.envelope_z_range(angle, r).unwrap_or((0.0, 0.0));
                (hi - lo) * f(r) * (to - from) / f64::from(n)
            })
            .sum()
    }

    #[test]
    fn a_double_volute_splits_radius_segment_and_bezier_sections_with_its_rib() {
        for (name, shape) in free_shapes() {
            let g = free_double_geometry(shape);
            let rib = g.design.rib.unwrap();
            assert!(g.design.free_double().is_some(), "{name}");
            // One channel with the rib in it, as the CAD builds it.
            assert!(
                g.inner_sections.is_empty() && g.outer_channel.is_empty(),
                "{name}"
            );
            assert_eq!(g.channels().len(), 2, "{name}");
            let rz = g.design.stagnation_radius;
            let reference = g.basis.impeller_diameter / 2.0;
            let mut ratios = Vec::new();
            for sec in g.envelope.iter().filter(|s| s.angle > rib.start + 0.2) {
                let r_max = sec
                    .outline
                    .iter()
                    .map(|p| p.x)
                    .fold(f64::NEG_INFINITY, f64::max);
                let a = g.design.rib_height(sec.angle).unwrap();
                let cut = rz + a + rib.thickness;
                // The outer channel has the area the design gives it, beyond the rib.
                let outer = envelope_integral(&g, sec.angle, cut, r_max, |_| 1.0);
                assert_relative_eq!(outer, rib.channel_area(sec.angle), max_relative = 0.02);
                // The inner volute holds the angular momentum of the flow of its wrap.
                let momentum = envelope_integral(&g, sec.angle, rz, rz + a, |r| reference / r);
                ratios.push((sec.angle, momentum));
            }
            let (last_angle, last) = *ratios.last().unwrap();
            assert_relative_eq!(last_angle, TAU, epsilon = 1e-9);
            for (angle, m) in &ratios {
                let share = (angle - rib.start) / (TAU - rib.start);
                // Up to the throat correction, which makes the inner volute a little
                // larger toward the throat.
                assert_relative_eq!(m / last, share, max_relative = 0.05, epsilon = 0.01);
            }
        }
    }

    #[test]
    fn a_short_rib_keeps_its_nose_and_ends_before_the_throat() {
        let full = double(None);
        let short = double(Some(130f64.to_radians()));
        let (rf, rs) = (full.design.rib.unwrap(), short.design.rib.unwrap());
        // The nose does not move: it is 180° from the cutwater either way.
        assert_relative_eq!(rs.start, rf.start);
        assert_relative_eq!(short.rib.as_ref().unwrap().nose_angle, PI, epsilon = 0.03);
        let end = PI + 130f64.to_radians();
        assert_relative_eq!(rs.end(), end);
        // The inner volute and the outer channel stop where the rib does, and the whole
        // section takes over to the throat.
        assert_relative_eq!(short.inner_sections.last().unwrap().angle, end);
        assert_relative_eq!(short.outer_channel.last().unwrap().angle, end);
        assert_relative_eq!(short.merged.first().unwrap().angle, end);
        assert_relative_eq!(short.merged.last().unwrap().angle, TAU);
        assert!(full.merged.is_empty());
        // The sections of the whole channel, turned with the rest of the volute, lie where
        // their angles say: the cutwater turned off the x axis by the discharge.
        for list in [&short.merged, &short.envelope, &short.inner_sections] {
            for sec in list {
                let (sin, cos) = (sec.rotation + short.basis.rotation_sign * sec.angle).sin_cos();
                let p = sec.points(short.basis.rotation_sign)[0];
                assert_relative_eq!(p.y * cos - p.x * sin, 0.0, epsilon = 1e-9);
            }
        }
        assert_relative_eq!(short.merged[0].rotation, short.rotation);
        // The whole section is the same in both: the rib only divides it.
        assert_eq!(short.envelope.len(), full.envelope.len());
        for (a, b) in short.envelope.iter().zip(&full.envelope) {
            assert_relative_eq!(a.size.area, b.size.area, epsilon = 1e-12);
        }
        // Four pieces of channel, the third running into the diffuser; three with the
        // whole rib.
        assert_eq!(short.channels().len(), 4);
        assert_eq!(full.channels().len(), 3);
        // The faces of the rib end at the end of the rib, with the tail beyond: the
        // outer face at that angle is the same distance out as the outer channel's wall.
        let rib = short.rib.as_ref().unwrap();
        let last = rib.outer_face.last().unwrap();
        let angle =
            (short.basis.rotation_sign * (last.y.atan2(last.x) - short.rotation)).rem_euclid(TAU);
        assert_relative_eq!(angle, end, epsilon = 1e-9);
        // The tail is shorter than the whole rib's: less rib, less band.
        assert!(rib.inner_face.len() == full.rib.as_ref().unwrap().inner_face.len());
        let reach = |g: &VoluteGeometry| {
            g.rib
                .as_ref()
                .unwrap()
                .tail
                .iter()
                .map(|p| (g.basis.rotation_sign * (p.y.atan2(p.x) - g.rotation)).rem_euclid(TAU))
                .fold(0.0, f64::max)
        };
        assert!(reach(&short) < PI + 140f64.to_radians());
    }

    fn internal(round: crate::volute::InternalRound) -> VoluteGeometry {
        let mut input = unturned();
        input.cross_section = CrossSectionInput::single(SectionShape::RoundInternal(round));
        VoluteGeometry::build(&reference_basis(), &input).unwrap()
    }

    #[test]
    fn an_internal_volute_starts_inside_its_diffuser() {
        use crate::volute::InternalRound;
        for open_to_right in [true, false] {
            let g = internal(InternalRound {
                open_to_right,
                ..InternalRound::default()
            });
            let bend = g.bend.as_ref().unwrap();
            assert_relative_eq!(g.start_angle, CUTWATER_START);
            assert_eq!(g.channels().len(), 2);
            assert!(g.warnings.iter().any(|w| w.contains("internal volute")));
            // Every section lies beyond the face, away from the impeller, and reaches
            // inside r'_z.
            let zm = reference_basis().midplane;
            let face = zm + bend.face;
            let away = bend.face.signum();
            for s in &g.sections {
                for p in &s.outline {
                    assert!(away * (p.y - face) >= -1e-9, "{p:?}");
                }
            }
            let throat = g.sections.last().unwrap();
            let lowest = throat.outline.iter().map(|p| p.x).fold(f64::MAX, f64::min);
            assert!(lowest < g.design.stagnation_radius - 0.01, "{lowest}");
            // The fluid of the inlet: one closed loop of pieces over the bend and neck,
            // reaching past the face.
            let profile = g.inlet_profile();
            for w in profile.windows(2) {
                assert!(w[0].last().unwrap().distance(w[1][0]) < 1e-12);
            }
            assert!(profile[0][0].distance(*profile.last().unwrap().last().unwrap()) < 1e-12);
            let reach = profile
                .iter()
                .flatten()
                .map(|p| away * (p.y - face))
                .fold(f64::MIN, f64::max);
            assert!(
                reach > 0.0 && reach < 0.05 * g.design.inlet_width,
                "{reach}"
            );
            // The casing round the bend, and the impeller left open on the other side.
            let casing = g.bend_casing_profile().unwrap();
            assert_eq!(casing.len(), bend.region().len());
            let (lo, hi) = g.impeller_band();
            if open_to_right {
                assert!(lo.is_infinite() && (hi - face).abs() < 1e-12);
            } else {
                assert!(hi.is_infinite() && (lo - face).abs() < 1e-12);
            }
        }
        // A volute outside r'_z keeps its ring and the whole impeller band.
        let g = VoluteGeometry::build(&reference_basis(), &unturned()).unwrap();
        assert!(g.bend.is_none() && g.bend_casing_profile().is_none());
        assert_eq!(g.inlet_profile().len(), 4);
        assert!(g.impeller_band().0.is_infinite() && g.impeller_band().1.is_infinite());
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
        assert_relative_eq!(c.nose_distance, d.cutwater_diameter / 2.0, epsilon = 1e-9);
        // The nose circle moves off r'_z only as far as its innermost point needs to
        // reach d_z.
        assert_relative_eq!(c.nose_center.y, 0.0, epsilon = 1e-12);
        assert!((c.nose_center.x - d.stagnation_radius).abs() < 0.1 * c.radius);
        // The spiral starts at the cutwater and leaves the diffuser past it, at the
        // tip of the sharp wedge, a few millimetres outside r'_z.
        assert!(g.start_angle < 5f64.to_radians());
        assert_eq!(c.tip, sharp.cutwater.tip);
        assert!(c.angle > 5f64.to_radians() && c.angle < 30f64.to_radians());
        let tip = c.tip.truncate().length();
        assert!(tip > d.stagnation_radius && tip < 1.08 * d.stagnation_radius);
        assert!(c.wedge > 1f64.to_radians() && c.wedge < 20f64.to_radians());
        // The body of the nose in the midplane; a sharp cutwater adds none.
        assert!(!g.cutwater_outline().is_empty());
        assert!(sharp.cutwater_outline().is_empty());
        assert_eq!(sharp.cutwater_slices.len(), super::super::cutwater::SLICES);
        // Too large a radius reaches the tip.
        input.cutwater.shape = CutwaterShape::Rounded {
            radius: crate::Param::Manual(0.02),
        };
        assert!(VoluteGeometry::build(&reference_basis(), &input).is_err());
        // Two pieces now: the spiral reaches into the diffuser.
        assert_eq!(g.channels().len(), 2);
    }
}
