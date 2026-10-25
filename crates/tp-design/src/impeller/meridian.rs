//! Meridional contour of a radial impeller and the quasi-one-dimensional flow through it.
//!
//! Coordinates in the meridional plane are `DVec2 { x: z, y: r }`: z along the axis in
//! the inflow direction, r the radius. The hub point of the inlet sits at z = z₀, 0
//! unless the meridian is placed elsewhere (see [`AxialReference`]); at the outlet
//! (r = d₂/2) the shroud sits at z₀ + L − b₂ and the hub at z₀ + L, L being the axial
//! length of the impeller. The inlet line may lean: its shroud point lies Δz
//! downstream of its hub point (see [`InletTilt`]).

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use tp_math::{Bezier, DVec2, Polyline};

use super::ImpellerBasis;
use super::center;
pub use super::center::DesignMode;
use super::chambers::{self, ChamberSettings, Chambers};
use super::crossings::{Crossing, crossings};
use super::duct::LegacyDuct;
use super::material::{self, Material, MaterialSettings};
use super::seals::{ResolvedSeals, Seals};
use super::wall::{SegmentInput, Wall};
use crate::{DesignError, Param};

/// Points used to turn each Bézier contour into an arc-length parameterized polyline.
const CONTOUR_SAMPLES: usize = 401;

/// Inputs of the meridional-contour step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeridionalInput {
    /// Axial length L from the inlet plane to the hub at the outlet, m.
    pub axial_length: Param<f64>,
    /// Bézier tangent lengths as fractions (0–1) of the distance from each end point
    /// to the corner where the inlet and outlet tangents meet.
    pub hub_inlet_tangent: Param<f64>,
    pub hub_outlet_tangent: Param<f64>,
    pub shroud_inlet_tangent: Param<f64>,
    pub shroud_outlet_tangent: Param<f64>,
    /// Leading-edge position on the hub, as a fraction (0–1) of the hub length.
    pub leading_edge_hub: Param<f64>,
    /// Leading-edge position on the shroud, as a fraction (0–1) of the shroud length.
    pub leading_edge_shroud: Param<f64>,
    /// Shape of the leading edge between its hub and shroud points.
    pub leading_edge_shape: LeadingEdgeShape,
    /// How the hub point of the leading edge follows the shroud point.
    pub leading_edge_extent: EdgeExtent,
    /// Hub drawn segment by segment; `None` keeps the automatic Bézier and its tangents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hub_segments: Option<Vec<SegmentInput>>,
    /// Shroud drawn segment by segment; `None` keeps the automatic Bézier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shroud_segments: Option<Vec<SegmentInput>>,
    /// Thickness, bore and boss of the automatic material.
    pub material: MaterialSettings,
    /// The annular seals at the eye and on the hub side.
    pub seals: Seals,
    /// Wire of the hub disk drawn segment by segment, from the hub end at the outlet
    /// back to the hub start; `None` keeps the automatic one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hub_material: Option<Vec<SegmentInput>>,
    /// Wire of the shroud cover drawn segment by segment, from the shroud end at the
    /// outlet back to the shroud start; `None` keeps the automatic one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shroud_material: Option<Vec<SegmentInput>>,
    /// The side chambers between the impeller and the casing.
    pub chambers: ChamberSettings,
    /// The inlet duct as older files keep it here; the project moves it to a component
    /// of its own as it loads them.
    #[serde(skip_serializing)]
    pub inlet_duct: LegacyDuct,
    /// Hub and shroud drawn, or following a center line.
    pub design_mode: DesignMode,
    /// Center line drawn segment by segment, from the middle of the inlet to the middle
    /// of the outlet; `None` keeps the automatic one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_line: Option<Vec<SegmentInput>>,
    /// Cross section along the center line, (x, A_rel) pairs; empty for linear.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub area_progression: Vec<[f64; 2]>,
    /// The point the axial position refers to.
    pub axial_reference: AxialReference,
    /// Axial position z of the reference point, m.
    pub axial_position: f64,
    /// How the inlet line leans.
    pub inlet_tilt: InletTilt,
}

/// The point of the center line whose axial position is given: the middle of the inlet
/// line or of the outlet line. The other end follows from the axial length.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxialReference {
    #[default]
    Inlet,
    Outlet,
}

/// How the inlet line, from the hub point to the shroud point, leans.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum InletTilt {
    /// Normal to the axis.
    #[default]
    Normal,
    /// The shroud point lies this far downstream of the hub point, m.
    Axial(f64),
    /// The line leans this much from the normal to the axis, rad, the shroud point
    /// downstream for a positive angle.
    Angle(f64),
}

impl InletTilt {
    /// The axial offset of the shroud point from the hub point for an inlet `height`
    /// high, m.
    pub fn offset(self, height: f64) -> f64 {
        match self {
            InletTilt::Normal => 0.0,
            InletTilt::Axial(dz) => dz,
            InletTilt::Angle(a) => height * a.tan(),
        }
    }
}

impl MeridionalInput {
    /// Moves every point stored in absolute coordinates by `dz` along the axis: drawn
    /// walls, center line, material wires and casing walls. Moved together with the
    /// axial position, the whole meridian shifts.
    pub fn shift(&mut self, dz: f64) {
        let by = DVec2::new(dz, 0.0);
        let wires = [
            &mut self.hub_segments,
            &mut self.shroud_segments,
            &mut self.center_line,
            &mut self.hub_material,
            &mut self.shroud_material,
            &mut self.chambers.front,
            &mut self.chambers.rear,
        ];
        for segments in wires.into_iter().flatten() {
            for s in segments {
                s.end += by;
                for c in &mut s.controls {
                    *c += by;
                }
            }
        }
    }
}

/// How the hub point of the leading edge sits relative to its shroud point.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum EdgeExtent {
    /// Each point has its own position along its wall.
    #[default]
    Independent,
    /// The hub point lies this far downstream of the shroud point, z_hub − z_shroud
    /// in m; 0 puts the edge in a plane normal to the axis.
    Axial(f64),
    /// The hub point lies this far above the shroud point, r_hub − r_shroud in m
    /// (negative below it); 0 makes the edge parallel to the axis, as on impellers of
    /// low n_q with blades curved in two dimensions.
    Radial(f64),
}

/// Fraction of the arc length of `line` at the first point, from the inlet, where
/// `coordinate` reaches `target`, interpolated between its vertices.
fn fraction_reaching(
    line: &Polyline<DVec2>,
    coordinate: impl Fn(DVec2) -> f64,
    target: f64,
) -> Option<f64> {
    let points = line.points();
    let total = line.length();
    let mut along = 0.0;
    for w in points.windows(2) {
        let (a, b) = (coordinate(w[0]), coordinate(w[1]));
        let step = w[0].distance(w[1]);
        if (a - target) * (b - target) <= 0.0 && (b - a).abs() > 0.0 {
            let k = ((target - a) / (b - a)).clamp(0.0, 1.0);
            return Some((along + k * step) / total);
        }
        along += step;
    }
    None
}

/// Shape of the leading edge in the meridional plane, from its hub point to its shroud
/// point.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum LeadingEdgeShape {
    /// Each stream line meets it at the same fraction of the way between the hub and
    /// shroud positions, measured along the channel.
    #[default]
    Channel,
    /// A straight line.
    Line,
    /// A cubic Bézier. Its inner control points are (along, across) the hub→shroud
    /// chord, in fractions of the chord length, so the curve keeps its shape when its
    /// ends slide along the walls.
    Bezier { controls: [DVec2; 2] },
}

impl LeadingEdgeShape {
    /// The Bézier from `hub` to `shroud` that passes through `a` and `b` at a third and
    /// two thirds of its parameter.
    pub fn bezier_through(hub: DVec2, a: DVec2, b: DVec2, shroud: DVec2) -> Self {
        // B(1/3) = a and B(2/3) = b solved for the two inner control points.
        let c1 = (hub * -5.0 + a * 18.0 - b * 9.0 + shroud * 2.0) / 6.0;
        let c2 = (hub * 2.0 - a * 9.0 + b * 18.0 - shroud * 5.0) / 6.0;
        Self::Bezier {
            controls: [
                chord_coordinates(hub, shroud, c1),
                chord_coordinates(hub, shroud, c2),
            ],
        }
    }

    /// The edge from `hub` to `shroud` through the points `inner` between them, in
    /// order from the hub (our construction):
    /// - none: a straight line;
    /// - one: the quadratic through it, raised to a cubic, which keeps its shape;
    /// - two or more: the cubic nearest to them in least squares.
    ///
    /// Each point starts at the parameter of its chord-length position along the
    /// polygon hub → points → shroud. With more than one point the fit then alternates
    /// with moving each parameter to the curve point nearest to its point (Hoschek's
    /// parameter correction, as in Piegl and Tiller, *The NURBS Book*, 2nd ed., §9.4.1).
    /// Two points are met exactly.
    pub fn through_points(hub: DVec2, inner: &[DVec2], shroud: DVec2) -> Self {
        let polygon: Vec<DVec2> = std::iter::once(hub)
            .chain(inner.iter().copied())
            .chain(std::iter::once(shroud))
            .collect();
        let mut walked = vec![0.0];
        for w in polygon.windows(2) {
            walked.push(walked.last().copied().unwrap_or(0.0) + w[0].distance(w[1]));
        }
        let total = walked.last().copied().unwrap_or(0.0);
        if inner.is_empty() || total <= 0.0 {
            return Self::Line;
        }
        let mut ts: Vec<f64> = walked[1..walked.len() - 1]
            .iter()
            .map(|s| (s / total).clamp(1e-6, 1.0 - 1e-6))
            .collect();
        let (c1, c2) = if let [m] = inner {
            // Quadratic hub, q, shroud through m at t, then its exact cubic form.
            let t = ts[0];
            let q = (*m - hub * (1.0 - t).powi(2) - shroud * t * t) / (2.0 * t * (1.0 - t));
            (
                hub + (q - hub) * (2.0 / 3.0),
                shroud + (q - shroud) * (2.0 / 3.0),
            )
        } else {
            let mut controls = None;
            for _ in 0..PARAMETER_CORRECTIONS {
                let Some((c1, c2)) = cubic_fit(hub, shroud, inner, &ts) else {
                    break;
                };
                controls = Some((c1, c2));
                let curve = [hub, c1, c2, shroud];
                for (t, p) in ts.iter_mut().zip(inner) {
                    *t = nearest_parameter(&curve, *p, *t);
                }
            }
            match controls {
                Some(c) => c,
                None => return Self::Line,
            }
        };
        Self::Bezier {
            controls: [
                chord_coordinates(hub, shroud, c1),
                chord_coordinates(hub, shroud, c2),
            ],
        }
    }
}

/// Fits and parameter corrections of [`LeadingEdgeShape::through_points`].
const PARAMETER_CORRECTIONS: usize = 50;

/// Inner control points of the cubic from `hub` to `shroud` nearest in least squares to
/// `points` at parameters `ts`: the normal equations of
/// Σ|b1·c1 + b2·c2 − (p − b0·hub − b3·shroud)|². `None` if they are singular.
fn cubic_fit(hub: DVec2, shroud: DVec2, points: &[DVec2], ts: &[f64]) -> Option<(DVec2, DVec2)> {
    let (mut a11, mut a12, mut a22) = (0.0, 0.0, 0.0);
    let (mut r1, mut r2) = (DVec2::ZERO, DVec2::ZERO);
    for (p, &t) in points.iter().zip(ts) {
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        let rest = *p - hub * b0 - shroud * b3;
        a11 += b1 * b1;
        a12 += b1 * b2;
        a22 += b2 * b2;
        r1 += rest * b1;
        r2 += rest * b2;
    }
    let det = a11 * a22 - a12 * a12;
    (det.abs() > 1e-12).then(|| ((r1 * a22 - r2 * a12) / det, (r2 * a11 - r1 * a12) / det))
}

/// Parameter of the point of the cubic Bézier `c` nearest to `p`, by Newton steps on
/// (B(t) − p)·B'(t) = 0 from `t`.
fn nearest_parameter(c: &[DVec2; 4], p: DVec2, mut t: f64) -> f64 {
    for _ in 0..8 {
        let s = 1.0 - t;
        let b = c[0] * (s * s * s)
            + c[1] * (3.0 * s * s * t)
            + c[2] * (3.0 * s * t * t)
            + c[3] * (t * t * t);
        let d1 = (c[1] - c[0]) * (3.0 * s * s)
            + (c[2] - c[1]) * (6.0 * s * t)
            + (c[3] - c[2]) * (3.0 * t * t);
        let d2 = (c[2] - c[1] * 2.0 + c[0]) * (6.0 * s) + (c[3] - c[2] * 2.0 + c[1]) * (6.0 * t);
        let e = b - p;
        let denominator = d1.dot(d1) + e.dot(d2);
        if denominator.abs() < 1e-30 {
            break;
        }
        t = (t - e.dot(d1) / denominator).clamp(1e-6, 1.0 - 1e-6);
    }
    t
}

impl MeridionalInput {
    /// Places the leading edge through `points` (z, r), m, given from the hub to the
    /// shroud. The first and last points go to the nearest points of the hub and shroud
    /// of `contour`. They set the leading-edge fractions on those walls, and the points
    /// between them set the shape ([`LeadingEdgeShape::through_points`]). Returns how
    /// far the end points lie from the walls, m.
    pub fn set_leading_edge_points(
        &mut self,
        contour: &MeridionalContour,
        points: &[DVec2],
    ) -> Result<f64, DesignError> {
        let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
            return Err(DesignError::InvalidInput(
                "the leading edge needs at least its hub and shroud points".into(),
            ));
        };
        if points.len() < 2 || points.iter().any(|p| !p.is_finite()) {
            return Err(DesignError::InvalidInput(
                "the leading edge needs at least its hub and shroud points".into(),
            ));
        }
        let (hub_t, hub_off) = nearest_fraction(&contour.hub_line, first);
        let (shroud_t, shroud_off) = nearest_fraction(&contour.shroud_line, last);
        if hub_t >= 1.0 || shroud_t >= 1.0 {
            return Err(DesignError::InvalidInput(
                "the leading edge must lie upstream of the outlet".into(),
            ));
        }
        let hub = contour.hub_at(hub_t);
        let shroud = contour.shroud_at(shroud_t);
        self.leading_edge_hub = Param::Manual(hub_t);
        self.leading_edge_shroud = Param::Manual(shroud_t);
        self.leading_edge_shape =
            LeadingEdgeShape::through_points(hub, &points[1..points.len() - 1], shroud);
        Ok(hub_off.max(shroud_off))
    }
}

/// Fraction of the length of `line` at its point nearest to `p`, and the distance.
fn nearest_fraction(line: &Polyline<DVec2>, p: DVec2) -> (f64, f64) {
    let points = line.points();
    let lengths = line.cumulative_lengths();
    let mut best = (0.0, f64::INFINITY);
    for (i, w) in points.windows(2).enumerate() {
        let d = w[1] - w[0];
        let l2 = d.length_squared();
        let t = if l2 > 0.0 {
            ((p - w[0]).dot(d) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let distance = (w[0] + d * t).distance(p);
        if distance < best.1 {
            best = (lengths[i] + t * (lengths[i + 1] - lengths[i]), distance);
        }
    }
    (best.0 / line.length(), best.1)
}

/// (along, across) coordinates of `p` on the chord from `a` to `b`, in chord lengths.
pub fn chord_coordinates(a: DVec2, b: DVec2, p: DVec2) -> DVec2 {
    let chord = b - a;
    let d = p - a;
    let l2 = chord.length_squared();
    DVec2::new(d.dot(chord) / l2, d.dot(chord.perp()) / l2)
}

/// The point at chord coordinates `uv` on the chord from `a` to `b`.
pub fn chord_point(a: DVec2, b: DVec2, uv: DVec2) -> DVec2 {
    let chord = b - a;
    a + chord * uv.x + chord.perp() * uv.y
}

/// Default tangent fraction: close to the 0.552 that makes a cubic Bézier match a
/// quarter ellipse inscribed in the corner.
pub const DEFAULT_TANGENT: f64 = 0.55;
pub const DEFAULT_LEADING_EDGE_HUB: f64 = 0.25;
pub const DEFAULT_LEADING_EDGE_SHROUD: f64 = 0.05;

/// Default axial length L = b₂ + (d₁ − d_n)/2 of a radial impeller: the outlet width
/// plus the radial height of the eye, which gives a channel of roughly constant
/// curvature. A semi-axial impeller keeps the shroud's axial run (d₁ − d_n)/2 before it
/// turns and then rises at ε_s to d₂a; L adds the offset of the hub end of the outlet
/// edge (our construction).
pub fn default_axial_length(basis: &ImpellerBasis) -> f64 {
    let eps = basis.outlet_inclination;
    let shroud_outlet = (basis.inlet_diameter - basis.hub_diameter) / 2.0
        + (basis.outlet_diameter - basis.inlet_diameter) / 2.0 * eps.tan();
    shroud_outlet + outlet_edge(basis).x
}

/// Hub end of the outlet edge relative to its shroud end, (Δz, Δr). The walls leave the
/// outlet in the direction ε_s from a normal to the axis, b₂ apart; the hub end lies on
/// the hub wall at d₂i/2. Radial impellers: (b₂, 0).
pub fn outlet_edge(basis: &ImpellerBasis) -> DVec2 {
    let eps = basis.outlet_inclination;
    let along = DVec2::new(eps.sin(), eps.cos());
    let across = DVec2::new(eps.cos(), -eps.sin());
    let b2 = basis.outlet_width;
    let r2 = basis.outlet_diameter / 2.0;
    let r2i = basis.outlet_hub_diameter / 2.0;
    let s = (r2i - r2 + b2 * eps.sin()) / eps.cos();
    across * b2 + along * s
}

/// Hub and shroud contours with the leading-edge position.
#[derive(Debug, Clone)]
pub struct MeridionalContour {
    pub hub: Wall,
    pub shroud: Wall,
    /// Wire of the hub disk, from the hub end at the outlet back to the hub start.
    pub hub_material: Wall,
    /// Wire of the shroud cover, from the shroud end at the outlet back to its start.
    pub shroud_material: Wall,
    /// Thickness, bore and boss as used for the material.
    pub material: Material,
    /// The seals as used.
    pub seals: ResolvedSeals,
    /// What of the material could not be built as asked.
    pub material_warnings: Vec<String>,
    /// The side chambers beside the material, built whether designed or not.
    pub chambers: Chambers,
    /// Where walls cross each other or themselves; the casing walls only when the
    /// chambers are designed.
    pub crossings: Vec<Crossing>,
    /// Center line of the channel, from the middle of the inlet to the middle of the
    /// outlet: the one the walls follow in [`DesignMode::CenterLine`], the automatic one
    /// otherwise.
    pub center_line: Wall,
    pub axial_length: f64,
    /// The point the axial position refers to, as given.
    pub axial_reference: AxialReference,
    /// Leading edge on the hub and on the shroud, as fractions of each contour length.
    pub leading_edge: (f64, f64),
    /// The leading edge as a curve, unless it follows the channel.
    pub leading_edge_shape: LeadingEdgeShape,
    hub_line: Polyline<DVec2>,
    shroud_line: Polyline<DVec2>,
}

fn fraction(name: &str, value: f64) -> Result<f64, DesignError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(DesignError::InvalidInput(format!(
            "{name} must lie between 0 and 1, got {value}"
        )))
    }
}

/// Cubic Bézier from `start` to `end` with tangents through `corner`.
fn corner_curve(start: DVec2, corner: DVec2, end: DVec2, t_in: f64, t_out: f64) -> Bezier<DVec2> {
    Bezier::new(vec![
        start,
        start + (corner - start) * t_in,
        end + (corner - end) * t_out,
        end,
    ])
    .expect("four control points")
}

impl MeridionalContour {
    /// The reference point of the axial position: the middle of the inlet line or of
    /// the outlet line.
    pub fn axial_reference_point(&self) -> DVec2 {
        match self.axial_reference {
            AxialReference::Inlet => self.hub.start().lerp(self.shroud.start(), 0.5),
            AxialReference::Outlet => self.hub.end().lerp(self.shroud.end(), 0.5),
        }
    }

    pub fn build(
        basis: &ImpellerBasis,
        input: &MeridionalInput,
        eyes: f64,
    ) -> Result<Self, DesignError> {
        let rn = basis.hub_diameter / 2.0;
        let r1 = basis.inlet_diameter / 2.0;
        let r2 = basis.outlet_diameter / 2.0;
        let b2 = basis.outlet_width;
        let length = input.axial_length.resolve(|| default_axial_length(basis));
        // Both walls run axially from the inlet, turn at a corner and leave the outlet at
        // ε_s; each corner sits where the axial line meets the outlet direction.
        let edge = outlet_edge(basis);
        // The hub point of the inlet: where the reference point puts it.
        let tilt = input.inlet_tilt.offset(r1 - rn);
        let z0 = match input.axial_reference {
            AxialReference::Inlet => input.axial_position - tilt / 2.0,
            AxialReference::Outlet => input.axial_position - (length - edge.x / 2.0),
        };
        let hub_end = DVec2::new(z0 + length, basis.outlet_hub_diameter / 2.0);
        let shroud_end = DVec2::new(z0 + length - edge.x, r2);
        let eps = basis.outlet_inclination;
        let corner = |end: DVec2, r: f64| DVec2::new(end.x - (end.y - r) * eps.tan(), r);
        let (hub_corner, shroud_corner) = (corner(hub_end, rn), corner(shroud_end, r1));
        // Only automatic walls need room to run axially before they turn.
        let room = |corner: DVec2, start: f64, drawn: bool| drawn || corner.x > start;
        if !(length.is_finite()
            && length > b2
            && room(hub_corner, z0, input.hub_segments.is_some())
            && room(shroud_corner, z0 + tilt, input.shroud_segments.is_some()))
        {
            return Err(DesignError::InvalidInput(format!(
                "axial length {length:.4} m is too short for the outlet: the hub and shroud \
                 must run axially before they turn"
            )));
        }
        let tangent = |name, p: &Param<f64>| fraction(name, p.resolve(|| DEFAULT_TANGENT));
        let (hub_start, shroud_start) = (DVec2::new(z0, rn), DVec2::new(z0 + tilt, r1));
        let hub = match &input.hub_segments {
            Some(segments) => Wall::build(hub_start, hub_end, segments)?,
            None => Wall::single(corner_curve(
                hub_start,
                hub_corner,
                hub_end,
                tangent("hub inlet tangent", &input.hub_inlet_tangent)?,
                tangent("hub outlet tangent", &input.hub_outlet_tangent)?,
            )),
        };
        let shroud = match &input.shroud_segments {
            Some(segments) => Wall::build(shroud_start, shroud_end, segments)?,
            None => Wall::single(corner_curve(
                shroud_start,
                shroud_corner,
                shroud_end,
                tangent("shroud inlet tangent", &input.shroud_inlet_tangent)?,
                tangent("shroud outlet tangent", &input.shroud_outlet_tangent)?,
            )),
        };
        // The center line, and the walls that follow it in its design mode.
        let (center_start, center_end) = (
            hub_start.lerp(shroud_start, 0.5),
            hub_end.lerp(shroud_end, 0.5),
        );
        let center_line = match &input.center_line {
            Some(segments) => Wall::build(center_start, center_end, segments)?,
            None => Wall::single(corner_curve(
                center_start,
                corner(center_end, center_start.y),
                center_end,
                DEFAULT_TANGENT,
                DEFAULT_TANGENT,
            )),
        };
        let (hub, shroud) = match input.design_mode {
            DesignMode::Walls => (hub, shroud),
            DesignMode::CenterLine => center::walls(
                &center_line,
                (hub_start, hub_end),
                (shroud_start, shroud_end),
                b2,
                &input.area_progression,
            )?,
        };
        let leading_edge = (
            fraction(
                "leading edge on the hub",
                input.leading_edge_hub.resolve(|| DEFAULT_LEADING_EDGE_HUB),
            )?,
            fraction(
                "leading edge on the shroud",
                input
                    .leading_edge_shroud
                    .resolve(|| DEFAULT_LEADING_EDGE_SHROUD),
            )?,
        );
        let line =
            |w: &Wall| Polyline::new(w.sample(CONTOUR_SAMPLES)).expect("sampled curve has points");
        let (hub_line, shroud_line) = (line(&hub), line(&shroud));
        // The hub point where the extent puts it, from the shroud point.
        let shroud_point = shroud_line.point_at_fraction(leading_edge.1);
        let follow = |coordinate: fn(DVec2) -> f64, offset: f64, name: &str| {
            fraction_reaching(&hub_line, coordinate, coordinate(shroud_point) + offset).ok_or_else(
                || {
                    DesignError::InvalidInput(format!(
                        "the hub never reaches the {name} the leading edge needs there; \
                         move its shroud point or change the offset"
                    ))
                },
            )
        };
        let leading_edge = match input.leading_edge_extent {
            EdgeExtent::Independent => leading_edge,
            EdgeExtent::Axial(dz) => (follow(|p| p.x, dz, "axial position")?, leading_edge.1),
            EdgeExtent::Radial(dr) => (follow(|p| p.y, dr, "radius")?, leading_edge.1),
        };
        if leading_edge.0 >= 1.0 || leading_edge.1 >= 1.0 {
            return Err(DesignError::InvalidInput(
                "the leading edge must lie upstream of the outlet".into(),
            ));
        }
        let settings = input.material.resolve(basis);
        let seals = input.seals.resolve(basis, settings.wall_thickness, eyes);
        let mut wires = material::wires(
            &hub,
            &shroud,
            basis,
            &settings,
            &seals,
            (
                input.hub_material.as_deref(),
                input.shroud_material.as_deref(),
            ),
        )?;
        wires.warnings.extend(seals.warnings.iter().cloned());
        let inside = hub_line
            .point_at_fraction(0.5)
            .lerp(shroud_line.point_at_fraction(0.5), 0.5);
        let side_chambers =
            chambers::build(&wires.shroud, &wires.hub, inside, basis, &input.chambers)?;
        // Walls dragged across each other.
        let mut walls: Vec<(&'static str, &Wall)> = vec![
            ("hub", &hub),
            ("shroud", &shroud),
            ("hub disk", &wires.hub),
            ("shroud cover", &wires.shroud),
        ];
        let mut pairs = vec![(0, 1), (2, 0), (2, 1), (3, 1), (3, 0), (2, 3)];
        if side_chambers.enabled {
            walls.push(("front casing wall", &side_chambers.front_casing));
            walls.push(("rear casing wall", &side_chambers.rear_casing));
            pairs.extend([(4, 3), (4, 1), (5, 2), (5, 0)]);
        }
        let found = crossings(&walls, &pairs);
        Ok(Self {
            hub_line,
            shroud_line,
            crossings: found,
            chambers: side_chambers,
            center_line,
            axial_reference: input.axial_reference,
            hub,
            shroud,
            hub_material: wires.hub,
            shroud_material: wires.shroud,
            material: settings,
            seals,
            material_warnings: wires.warnings,
            axial_length: length,
            leading_edge,
            leading_edge_shape: input.leading_edge_shape,
        })
    }

    /// The leading edge as a curve from the hub to the shroud, unless it follows the
    /// channel.
    pub fn leading_edge_curve(&self) -> Option<Bezier<DVec2>> {
        let hub = self.hub_at(self.leading_edge.0);
        let shroud = self.shroud_at(self.leading_edge.1);
        match self.leading_edge_shape {
            LeadingEdgeShape::Channel => None,
            LeadingEdgeShape::Line => Some(Bezier::line(hub, shroud)),
            LeadingEdgeShape::Bezier { controls } => Bezier::new(vec![
                hub,
                chord_point(hub, shroud, controls[0]),
                chord_point(hub, shroud, controls[1]),
                shroud,
            ])
            .ok(),
        }
    }

    /// Channel position (0–1) of the leading edge on the stream line at `fraction` of
    /// the flow from hub to shroud, given by one point per quasi-orthogonal. A curved
    /// edge is met where the stream line crosses it; where it does not, and along the
    /// channel, the hub and shroud positions are interpolated.
    pub fn leading_edge_on(&self, fraction: f64, points: &[DVec2]) -> f64 {
        let (hub, shroud) = self.leading_edge;
        let along = hub + fraction * (shroud - hub);
        let curve = match self.leading_edge_curve() {
            Some(c) if fraction > 0.0 && fraction < 1.0 && points.len() > 1 => c,
            _ => return along,
        };
        let edge = curve.sample(LEADING_EDGE_SAMPLES);
        let last = (points.len() - 1) as f64;
        for (i, pair) in points.windows(2).enumerate() {
            for e in edge.windows(2) {
                if let Some(w) = crossing(pair[0], pair[1], e[0], e[1]) {
                    return (i as f64 + w) / last;
                }
            }
        }
        along
    }

    /// Leading edge parallel to the axis, as cylindrical blades are usually laid out:
    /// at the radius where the shroud has turned 30° away from axial, and the hub point
    /// at the same radius. Fractions of the hub and the shroud length.
    pub fn axial_leading_edge(&self) -> (f64, f64) {
        const SAMPLES: usize = 400;
        let h = 0.5 / SAMPLES as f64;
        let at = |i: usize| i as f64 / SAMPLES as f64;
        let shroud = (1..SAMPLES)
            .map(at)
            .find(|&t| {
                let (a, b) = (self.shroud_at(t - h), self.shroud_at(t + h));
                (b.y - a.y) >= 0.5 * a.distance(b)
            })
            .unwrap_or(0.5);
        let r = self.shroud_at(shroud).y;
        let hub = (0..SAMPLES)
            .map(at)
            .find(|&t| self.hub_at(t).y >= r)
            .unwrap_or(0.5);
        (hub, shroud)
    }

    /// Hub point at fraction `t` of its arc length.
    pub fn hub_at(&self, t: f64) -> DVec2 {
        self.hub_line.point_at_fraction(t)
    }

    /// Shroud point at fraction `t` of its arc length.
    pub fn shroud_at(&self, t: f64) -> DVec2 {
        self.shroud_line.point_at_fraction(t)
    }

    pub fn hub_length(&self) -> f64 {
        self.hub_line.length()
    }

    pub fn shroud_length(&self) -> f64 {
        self.shroud_line.length()
    }
}

/// Points that turn a curved leading edge into segments to meet the stream lines.
const LEADING_EDGE_SAMPLES: usize = 200;

/// Where segment a→b crosses segment c→d, as a fraction of a→b.
fn crossing(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<f64> {
    let (r, q) = (b - a, d - c);
    let det = r.perp_dot(q);
    if det.abs() < 1e-30 {
        return None;
    }
    let w = (c - a).perp_dot(q) / det;
    let v = (c - a).perp_dot(r) / det;
    ((0.0..=1.0).contains(&w) && (0.0..=1.0).contains(&v)).then_some(w)
}

/// A straight line across the channel from hub to shroud.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct QuasiOrthogonal {
    /// Position along the channel: fraction of hub and shroud arc length.
    pub t: f64,
    pub hub: DVec2,
    pub shroud: DVec2,
    /// Flow area normal to the mean streamline, m².
    pub area: f64,
    /// Meridional velocity, uniform along the line, m/s.
    pub cm: f64,
}

/// A streamline in the meridional plane, one point per quasi-orthogonal.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpanLine {
    /// Fraction of the flow between the hub and this line (0 hub, 1 shroud).
    pub fraction: f64,
    pub points: Vec<DVec2>,
    /// Meridional velocity at each point, m/s.
    pub cm: Vec<f64>,
}

impl SpanLine {
    /// Meridional velocity at channel position `t` (its points are evenly spaced in t).
    pub fn cm_at(&self, t: f64) -> f64 {
        let n = self.cm.len() - 1;
        let x = t.clamp(0.0, 1.0) * n as f64;
        let i = (x.floor() as usize).min(n - 1);
        self.cm[i] + (x - i as f64) * (self.cm[i + 1] - self.cm[i])
    }
}

/// Quasi-one-dimensional meridional flow: uniform meridional velocity along each
/// quasi-orthogonal, and streamlines that split the flow into equal stream tubes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Channel {
    pub quasi_orthogonals: Vec<QuasiOrthogonal>,
    pub spans: Vec<SpanLine>,
}

/// Position λ ∈ [0, 1] along a hub→shroud line with radii `r_hub`, `r_shroud` that
/// encloses `fraction` of the annular area swept by revolving the line about the axis.
///
/// The swept area of the first λ of the line is π·(2r_h + λΔr)·λ·ℓ, so λ solves
/// Δr·λ² + 2r_h·λ − f·(2r_h + Δr) = 0, written without cancellation.
pub fn equal_area_position(r_hub: f64, r_shroud: f64, fraction: f64) -> f64 {
    let dr = r_shroud - r_hub;
    let rhs = fraction * (2.0 * r_hub + dr);
    let denominator = r_hub + (r_hub * r_hub + dr * rhs).max(0.0).sqrt();
    if denominator <= f64::EPSILON * r_shroud.abs().max(1.0) {
        fraction
    } else {
        rhs / denominator
    }
}

impl Channel {
    /// The same quasi-orthogonals with other stream surfaces, such as those of the
    /// potential flow, one point per quasi-orthogonal.
    ///
    /// # Panics
    /// If a span has not one point and one velocity per quasi-orthogonal: the channel
    /// position t of a span point is its index.
    pub fn with_spans(self, spans: Vec<SpanLine>) -> Self {
        let n = self.quasi_orthogonals.len();
        assert!(
            spans.iter().all(|s| s.points.len() == n && s.cm.len() == n),
            "stream surfaces need one point per quasi-orthogonal"
        );
        Self { spans, ..self }
    }

    /// `quasi_orthogonals` lines along the channel and `spans` streamlines (≥ 2).
    pub fn build(
        contour: &MeridionalContour,
        flow: f64,
        quasi_orthogonals: usize,
        spans: usize,
    ) -> Self {
        let n = quasi_orthogonals.max(3);
        let dt = 0.5 / (n - 1) as f64;
        let direction = |at: &dyn Fn(f64) -> DVec2, t: f64| {
            (at((t + dt).min(1.0)) - at((t - dt).max(0.0))).normalize()
        };
        let qos: Vec<QuasiOrthogonal> = (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                let hub = contour.hub_at(t);
                let shroud = contour.shroud_at(t);
                let across = shroud - hub;
                let streamwise = (direction(&|t| contour.hub_at(t), t)
                    + direction(&|t| contour.shroud_at(t), t))
                .normalize();
                let sin = across.normalize().perp_dot(streamwise).abs();
                let area = PI * (hub.y + shroud.y) * across.length() * sin;
                QuasiOrthogonal {
                    t,
                    hub,
                    shroud,
                    area,
                    cm: flow / area,
                }
            })
            .collect();

        let m = spans.max(2);
        let span_lines = (0..m)
            .map(|j| {
                let fraction = j as f64 / (m - 1) as f64;
                let points = qos
                    .iter()
                    .map(|q| {
                        let lambda = equal_area_position(q.hub.y, q.shroud.y, fraction);
                        q.hub.lerp(q.shroud, lambda)
                    })
                    .collect();
                let cm = qos.iter().map(|q| q.cm).collect();
                SpanLine {
                    fraction,
                    points,
                    cm,
                }
            })
            .collect();
        Self {
            quasi_orthogonals: qos,
            spans: span_lines,
        }
    }

    /// Meridional velocity at channel position `t`, interpolated between
    /// quasi-orthogonals.
    pub fn cm_at(&self, t: f64) -> f64 {
        let n = self.quasi_orthogonals.len();
        let x = t.clamp(0.0, 1.0) * (n - 1) as f64;
        let i = (x.floor() as usize).min(n - 2);
        let w = x - i as f64;
        let (a, b) = (&self.quasi_orthogonals[i], &self.quasi_orthogonals[i + 1]);
        a.cm + w * (b.cm - a.cm)
    }

    /// Largest local rise of the meridional velocity along the channel, as a ratio to
    /// its value upstream (0 when cm never rises).
    pub fn max_cm_rise(&self) -> f64 {
        let mut min_so_far = f64::INFINITY;
        let mut rise = 0.0_f64;
        for q in &self.quasi_orthogonals {
            min_so_far = min_so_far.min(q.cm);
            rise = rise.max(q.cm / min_so_far - 1.0);
        }
        rise
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use approx::assert_relative_eq;

    pub(crate) fn basis() -> ImpellerBasis {
        ImpellerBasis {
            hub_diameter: 0.0191,
            inlet_diameter: 0.0723,
            outlet_diameter: 0.1722,
            outlet_width: 0.0174,
            flow: 0.02864,
            omega: 303.69,
            blade_count: 7,
            blade_thickness: 0.003,
            outlet_blade_angle: 22.0f64.to_radians(),
            incidence: 0.0,
            rotation_sign: 1.0,
            pressure_coefficient: 0.9176,
            hydraulic_efficiency: 0.9065,
            nq: 35.92,
            outlet_relative_velocity: 13.27,
            outlet_hub_diameter: 0.1722,
            outlet_inclination: 0.0,
            slip_factor: 0.826,
            outlet_swirl: 13.0,
            outlet_meridional_velocity: 3.0,
        }
    }

    #[test]
    fn contour_end_points_and_tangents() {
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let l = default_axial_length(&b);
        assert_eq!(c.hub.start(), DVec2::new(0.0, b.hub_diameter / 2.0));
        assert_eq!(c.hub.end(), DVec2::new(l, b.outlet_diameter / 2.0));
        assert_eq!(c.shroud.start(), DVec2::new(0.0, b.inlet_diameter / 2.0));
        assert_eq!(
            c.shroud.end(),
            DVec2::new(l - b.outlet_width, b.outlet_diameter / 2.0)
        );
        // Axial inflow, radial outflow.
        let d_in = c.shroud.segments()[0].pieces()[0].derivative().eval(0.0);
        let d_out = c.hub.segments()[0].pieces()[0].derivative().eval(1.0);
        assert!(d_in.y.abs() < 1e-12 && d_in.x > 0.0);
        assert!(d_out.x.abs() < 1e-12 && d_out.y > 0.0);
    }

    #[test]
    fn inlet_and_outlet_areas_are_exact() {
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let ch = Channel::build(&c, b.flow, 41, 5);
        let inlet = ch.quasi_orthogonals.first().unwrap();
        let outlet = ch.quasi_orthogonals.last().unwrap();
        let (rn, r1, r2) = (
            b.hub_diameter / 2.0,
            b.inlet_diameter / 2.0,
            b.outlet_diameter / 2.0,
        );
        assert_relative_eq!(inlet.area, PI * (r1 * r1 - rn * rn), max_relative = 1e-3);
        assert_relative_eq!(
            outlet.area,
            2.0 * PI * r2 * b.outlet_width,
            max_relative = 1e-3
        );
        assert_relative_eq!(outlet.cm * outlet.area, b.flow, max_relative = 1e-12);
    }

    #[test]
    fn spans_split_flow_equally() {
        for (rh, rs) in [(0.01, 0.036), (0.086, 0.086), (0.0, 0.03), (0.05, 0.02)] {
            for f in [0.0, 0.25, 0.5, 1.0] {
                let lambda = equal_area_position(rh, rs, f);
                let swept = |l: f64| (2.0 * rh + l * (rs - rh)) * l;
                assert_relative_eq!(swept(lambda), f * swept(1.0), epsilon = 1e-12);
            }
        }
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let ch = Channel::build(&c, b.flow, 21, 5);
        assert_eq!(
            ch.spans.first().unwrap().points[10],
            ch.quasi_orthogonals[10].hub
        );
        assert_eq!(
            ch.spans.last().unwrap().points[10],
            ch.quasi_orthogonals[10].shroud
        );
    }

    #[test]
    fn reference_channel_decelerates_smoothly() {
        let b = basis();
        let c = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let ch = Channel::build(&c, b.flow, 61, 5);
        let first = ch.quasi_orthogonals.first().unwrap().cm;
        let last = ch.quasi_orthogonals.last().unwrap().cm;
        assert!(last < first, "cm1 = {first}, cm2 = {last}");
        assert!(ch.max_cm_rise() < 0.05, "rise = {}", ch.max_cm_rise());
        assert_relative_eq!(ch.cm_at(0.0), first);
    }

    #[test]
    fn drawn_walls_reproduce_the_automatic_contour_and_survive_a_round_trip() {
        let b = basis();
        let automatic = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let input = MeridionalInput {
            hub_segments: Some(super::super::wall::split(&automatic.hub, 0, 0.4)),
            shroud_segments: Some(automatic.shroud.to_input()),
            ..MeridionalInput::default()
        };
        let drawn = MeridionalContour::build(&b, &input, 1.0).unwrap();
        for t in [0.0, 0.3, 0.7, 1.0] {
            assert!(drawn.hub_at(t).distance(automatic.hub_at(t)) < 1e-6);
            assert!(drawn.shroud_at(t).distance(automatic.shroud_at(t)) < 1e-12);
        }
        let json = serde_json::to_string(&input).unwrap();
        let back: MeridionalInput = serde_json::from_str(&json).unwrap();
        let read = MeridionalContour::build(&b, &back, 1.0).unwrap();
        assert_eq!(read.hub.segments().len(), 2);
        assert_eq!(read.hub.smooth(), [true]);
        for t in [0.2, 0.5, 0.9] {
            assert!(read.hub_at(t).distance(drawn.hub_at(t)) < 1e-15);
        }
        // Projects saved before drawn walls existed keep the automatic contour.
        let old: MeridionalInput = serde_json::from_str("{}").unwrap();
        assert!(old.hub_segments.is_none() && old.shroud_segments.is_none());
        assert!(
            !serde_json::to_string(&MeridionalInput::default())
                .unwrap()
                .contains("segments")
        );
    }

    #[test]
    fn a_straight_leading_edge_meets_every_stream_line_on_the_line() {
        let b = basis();
        let input = MeridionalInput {
            leading_edge_shape: LeadingEdgeShape::Line,
            ..MeridionalInput::default()
        };
        let contour = MeridionalContour::build(&b, &input, 1.0).unwrap();
        let channel = Channel::build(&contour, b.flow, 41, 7);
        let (hub, shroud) = (
            contour.hub_at(contour.leading_edge.0),
            contour.shroud_at(contour.leading_edge.1),
        );
        let line = Polyline::new(vec![hub, shroud]).unwrap();
        for span in &channel.spans {
            let t = contour.leading_edge_on(span.fraction, &span.points);
            let x = t * (span.points.len() - 1) as f64;
            let i = (x.floor() as usize).min(span.points.len() - 2);
            let p = span.points[i].lerp(span.points[i + 1], x - i as f64);
            let off = (p - hub).perp_dot((shroud - hub).normalize()).abs();
            assert!(off < 1e-9, "span {}: {off} m off the line", span.fraction);
            assert!(line.length() > 0.0);
        }
        // Following the channel keeps the old interpolation.
        let channel_edge = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let (h, s) = channel_edge.leading_edge;
        assert_relative_eq!(
            channel_edge.leading_edge_on(0.5, &channel.spans[3].points),
            h + 0.5 * (s - h)
        );
    }

    #[test]
    fn a_bezier_leading_edge_passes_through_its_points_and_follows_its_ends() {
        let (hub, shroud) = (DVec2::new(0.01, 0.02), DVec2::new(0.0, 0.04));
        let (a, b) = (DVec2::new(0.012, 0.027), DVec2::new(0.009, 0.034));
        let LeadingEdgeShape::Bezier { controls } =
            LeadingEdgeShape::bezier_through(hub, a, b, shroud)
        else {
            panic!("a Bézier");
        };
        let curve = |hub: DVec2, shroud: DVec2| {
            Bezier::new(vec![
                hub,
                chord_point(hub, shroud, controls[0]),
                chord_point(hub, shroud, controls[1]),
                shroud,
            ])
            .unwrap()
        };
        let c = curve(hub, shroud);
        assert!(c.eval(1.0 / 3.0).distance(a) < 1e-15);
        assert!(c.eval(2.0 / 3.0).distance(b) < 1e-15);
        // Moving and turning the chord carries the curve with it.
        let moved = curve(
            hub + DVec2::new(0.002, 0.001),
            shroud + DVec2::new(0.002, 0.001),
        );
        assert!(
            moved
                .eval(0.5)
                .distance(c.eval(0.5) + DVec2::new(0.002, 0.001))
                < 1e-15
        );
        let p = DVec2::new(0.013, 0.031);
        assert!(chord_point(hub, shroud, chord_coordinates(hub, shroud, p)).distance(p) < 1e-15);
    }

    /// The Bézier a leading-edge shape stands for, between `hub` and `shroud`.
    #[test]
    fn the_meridian_moves_with_its_axial_position_and_reference() {
        let b = basis();
        let base = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let moved = |input: MeridionalInput| MeridionalContour::build(&b, &input, 1.0).unwrap();
        let dz = 0.05;
        let at_inlet = moved(MeridionalInput {
            axial_position: dz,
            ..MeridionalInput::default()
        });
        for (p, q) in base.hub.sample(50).iter().zip(at_inlet.hub.sample(50)) {
            assert!((q - *p - DVec2::new(dz, 0.0)).length() < 1e-12);
        }
        assert!((at_inlet.hub_material.end().x - dz).abs() < 1e-12);
        assert_eq!(at_inlet.axial_length, base.axial_length);
        // From the outlet: the middle of the outlet line sits at the position, wherever
        // the axial length puts the inlet.
        let at_outlet = moved(MeridionalInput {
            axial_reference: AxialReference::Outlet,
            axial_position: 0.2,
            axial_length: Param::Manual(0.06),
            ..MeridionalInput::default()
        });
        let middle = at_outlet.hub.end().lerp(at_outlet.shroud.end(), 0.5);
        assert!((middle.x - 0.2).abs() < 1e-12);
        assert!((at_outlet.hub.end().x - at_outlet.hub.start().x - 0.06).abs() < 1e-12);
        // The guide measures from the inlet: it does not move with the meridian.
        let (g0, g1) = (
            super::super::guide::guide(&base, &b),
            super::super::guide::guide(&at_inlet, &b),
        );
        assert!((g0.axial_extension.actual - g1.axial_extension.actual).abs() < 1e-12);
        assert!((g0.inlet_run.actual - g1.inlet_run.actual).abs() < 1e-9);

        // Drawn walls move along when shifted with the position.
        let mut input = MeridionalInput {
            hub_segments: Some(super::super::wall::split(&base.hub, 0, 0.4)),
            ..MeridionalInput::default()
        };
        let drawn = moved(input.clone());
        input.shift(dz);
        input.axial_position = dz;
        let shifted = moved(input);
        for (p, q) in drawn.hub.sample(50).iter().zip(shifted.hub.sample(50)) {
            assert!((q - *p - DVec2::new(dz, 0.0)).length() < 1e-12);
        }
    }

    #[test]
    fn the_inlet_line_leans_by_an_axial_distance_or_an_angle() {
        let b = basis();
        let height = (b.inlet_diameter - b.hub_diameter) / 2.0;
        for (tilt, dz) in [
            (InletTilt::Axial(0.004), 0.004),
            (
                InletTilt::Angle(20f64.to_radians()),
                height * 20f64.to_radians().tan(),
            ),
        ] {
            let c = MeridionalContour::build(
                &b,
                &MeridionalInput {
                    inlet_tilt: tilt,
                    ..MeridionalInput::default()
                },
                1.0,
            )
            .unwrap();
            assert!((c.shroud.start().x - c.hub.start().x - dz).abs() < 1e-12);
            // The middle of the inlet line stays at the axial position.
            let middle = c.hub.start().lerp(c.shroud.start(), 0.5);
            assert!(middle.x.abs() < 1e-12);
        }
    }

    #[test]
    fn the_hub_point_of_the_leading_edge_keeps_its_offset_from_the_shroud_point() {
        let b = basis();
        let shroud_at = |c: &MeridionalContour| c.shroud_at(c.leading_edge.1);
        let hub_at = |c: &MeridionalContour| c.hub_at(c.leading_edge.0);
        for shroud in [0.05, 0.2] {
            let mut input = MeridionalInput {
                leading_edge_shroud: Param::Manual(shroud),
                leading_edge_extent: EdgeExtent::Radial(0.0),
                ..MeridionalInput::default()
            };
            let c = MeridionalContour::build(&b, &input, 1.0).unwrap();
            // Parallel to the axis: the same radius on both walls.
            assert!((hub_at(&c).y - shroud_at(&c).y).abs() < 1e-6);
            input.leading_edge_extent = EdgeExtent::Axial(0.004);
            let c = MeridionalContour::build(&b, &input, 1.0).unwrap();
            assert!((hub_at(&c).x - shroud_at(&c).x - 0.004).abs() < 1e-6);
        }
        // An offset the hub cannot reach is an error, not a wrong edge.
        let input = MeridionalInput {
            leading_edge_extent: EdgeExtent::Radial(1.0),
            ..MeridionalInput::default()
        };
        assert!(MeridionalContour::build(&b, &input, 1.0).is_err());
    }

    fn edge_curve(shape: LeadingEdgeShape, hub: DVec2, shroud: DVec2) -> Bezier<DVec2> {
        match shape {
            LeadingEdgeShape::Bezier { controls } => Bezier::new(vec![
                hub,
                chord_point(hub, shroud, controls[0]),
                chord_point(hub, shroud, controls[1]),
                shroud,
            ])
            .unwrap(),
            LeadingEdgeShape::Line => Bezier::line(hub, shroud),
            LeadingEdgeShape::Channel => panic!("not a curve"),
        }
    }

    /// Smallest distance from `p` to `curve`.
    fn distance_to(curve: &Bezier<DVec2>, p: DVec2) -> f64 {
        curve
            .sample(20_001)
            .iter()
            .map(|q| q.distance(p))
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn a_leading_edge_through_points_meets_them() {
        let (hub, shroud) = (DVec2::new(0.038, 0.034), DVec2::new(0.004, 0.071));
        assert_eq!(
            LeadingEdgeShape::through_points(hub, &[], shroud),
            LeadingEdgeShape::Line
        );
        // One point: met exactly (within the 3 µm sampling of `distance_to`).
        let mid = DVec2::new(0.019, 0.051);
        let one = edge_curve(
            LeadingEdgeShape::through_points(hub, &[mid], shroud),
            hub,
            shroud,
        );
        assert!(distance_to(&one, mid) < 5e-6, "{}", distance_to(&one, mid));
        // Two points: met exactly.
        // A smooth edge bulging toward the outlet, as leading edges usually are.
        let cubic = Bezier::new(vec![
            hub,
            DVec2::new(0.028, 0.044),
            DVec2::new(0.016, 0.058),
            shroud,
        ])
        .unwrap();
        let (a, b) = (cubic.eval(0.3), cubic.eval(0.7));
        let two = edge_curve(
            LeadingEdgeShape::through_points(hub, &[a, b], shroud),
            hub,
            shroud,
        );
        assert!(distance_to(&two, a) < 5e-6 && distance_to(&two, b) < 5e-6);
        // More: a least-squares fit that stays close to points of a smooth curve.
        let many: Vec<DVec2> = (1..8).map(|i| cubic.eval(i as f64 / 8.0)).collect();
        let fit = edge_curve(
            LeadingEdgeShape::through_points(hub, &many, shroud),
            hub,
            shroud,
        );
        for p in &many {
            assert!(
                distance_to(&fit, *p) < 2e-4,
                "{p}: {}",
                distance_to(&fit, *p)
            );
        }
    }

    #[test]
    fn leading_edge_points_set_the_wall_fractions() {
        let b = basis();
        let contour = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let (hub, shroud) = (contour.hub_at(0.3), contour.shroud_at(0.1));
        let mid = (hub + shroud) * 0.5 + DVec2::new(0.002, 0.0);
        let mut input = MeridionalInput::default();
        // Points slightly off the walls land on them.
        let off = input
            .set_leading_edge_points(
                &contour,
                &[
                    hub + DVec2::new(0.0, -1e-4),
                    mid,
                    shroud + DVec2::new(0.0, 1e-4),
                ],
            )
            .unwrap();
        assert!(off < 1.1e-4, "{off}");
        assert_relative_eq!(
            input.leading_edge_hub.manual_value().unwrap(),
            0.3,
            epsilon = 2e-3
        );
        assert_relative_eq!(
            input.leading_edge_shroud.manual_value().unwrap(),
            0.1,
            epsilon = 2e-3
        );
        let built = MeridionalContour::build(&b, &input, 1.0).unwrap();
        let curve = built.leading_edge_curve().unwrap();
        assert!(distance_to(&curve, mid) < 1e-5);
        assert!(input.set_leading_edge_points(&contour, &[hub]).is_err());
    }

    #[test]
    fn rejects_bad_inputs() {
        let b = basis();
        let short = MeridionalInput {
            axial_length: Param::Manual(0.01),
            ..Default::default()
        };
        assert!(MeridionalContour::build(&b, &short, 1.0).is_err());
        let bad_le = MeridionalInput {
            leading_edge_hub: Param::Manual(1.2),
            ..Default::default()
        };
        assert!(MeridionalContour::build(&b, &bad_le, 1.0).is_err());
    }
}
