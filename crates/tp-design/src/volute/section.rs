//! Shapes of the volute cross sections.
//!
//! Every section is drawn in its own radial plane with coordinates (s, h): s = z − z_m
//! across the volute (the width B, positive toward +z) and h = r − r'_z outward from the
//! stagnation circle (the height H). The inlet of width b₃ joins the vaneless ring at
//! h = 0 between the inlet points (∓b₃/2, 0).
//!
//! A cross-section definition gives the shape at one wrap angle φ; the spiral development
//! scales it to the height H its flow needs. Most sections are built from two sides, one
//! each side of the centre line s = 0, that run from an inlet point up to the top
//! (0, H): rectangular, trapezoidal, round (an elliptic arc), Bézier, line segments or
//! radius based. Each side may have its own shape. The asymmetric round section grows
//! toward one side as a whole. Between two definitions the shape blends smoothly; before
//! the first and after the last one it stays as defined.
//!
//! Every quantity the spiral needs comes from the outline polygon by Green's theorem:
//! the area, ∬ dA/r = ∮ ln r dz for the conservation of angular momentum (Gülich eq.
//! 7.24 for any shape), ∬ r dA = ∮ r²/2 dz for the centroid, and the wetted walls. The
//! sums are exact for polygons; curved walls are sampled finely.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde::{Deserialize, Serialize};
use tp_math::{DVec2, roots};

use crate::{DesignError, Param};

/// Default cone angle δ of trapezoidal sides, degrees.
pub const DEFAULT_CONE_ANGLE_DEG: f64 = 30.0;
/// Largest cone angle δ, degrees.
pub const MAX_CONE_ANGLE_DEG: f64 = 60.0;
/// Largest tangent angle λ of the top at the centre line, degrees.
pub const MAX_TANGENT_ANGLE_DEG: f64 = 45.0;

/// Largest angle between two samples of a curved wall, rad.
const ARC_STEP: f64 = 1.5 * PI / 180.0;
/// Samples of a Bézier side.
const BEZIER_POINTS: usize = 96;
/// Points of the walls of every lofted outline, spread over its runs.
const WALL_POINTS: usize = 72;
/// Fewest points of one run of a lofted outline.
const MIN_RUN_POINTS: usize = 12;
/// Points of the inlet of every lofted outline.
const INLET_POINTS: usize = 8;
/// Denser runs for the outlines blended between two definitions.
const DENSE_FACTOR: usize = 4;

/// How the inner points 1 and 3 of a Bézier side sit on their lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InnerPosition {
    /// A fraction of the way from the end point (0 or 4) to the corner point S.
    #[default]
    Relative,
    /// A distance back from S toward the end point, m.
    Absolute,
}

/// The straight shape a Bézier side rounds off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BezierBase {
    /// The side wall leaves the inlet radially.
    #[default]
    Rectangle,
    /// The side wall opens at the cone angle.
    Trapezoid,
}

/// A side drawn as a fourth-degree Bézier curve through control points 0 to 4. Point 0 is
/// the inlet point and point 4 the top on the centre line. Point 1 lies on the side wall
/// line (radial, or at the cone angle), point 3 on the top line (at the tangent angle);
/// both lines meet in the corner point S. Point 2 is free: S plus `point2[0]` times the
/// way from S to point 0 plus `point2[1]` times the way from S to point 4, so that it
/// scales with the section. The curve leaves the inlet along the wall and reaches the top
/// along the top line; the inner points round the corner like a casting radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BezierSide {
    pub base: BezierBase,
    /// Cone angle δ of a trapezoid base from the radial direction, rad.
    pub cone_angle: f64,
    /// Tangent angle λ of the top at the centre line, rad.
    pub tangent_angle: f64,
    pub position: InnerPosition,
    /// Point 1: a fraction (relative) or a distance in m (absolute).
    pub point1: f64,
    /// Point 3: a fraction (relative) or a distance in m (absolute).
    pub point3: f64,
    pub point2: [f64; 2],
}

impl Default for BezierSide {
    fn default() -> Self {
        Self {
            base: BezierBase::Rectangle,
            cone_angle: DEFAULT_CONE_ANGLE_DEG.to_radians(),
            tangent_angle: 0.0,
            position: InnerPosition::Relative,
            point1: 0.7,
            point3: 0.7,
            point2: [0.0, 0.0],
        }
    }
}

impl BezierSide {
    /// Cone angle of the side wall, rad: 0 for a rectangle base.
    pub fn wall_angle(&self) -> f64 {
        match self.base {
            BezierBase::Rectangle => 0.0,
            BezierBase::Trapezoid => self.cone_angle,
        }
    }
}

/// A corner of a line-segment side. Its position is measured from the inlet point in
/// units of the section height H, so that the side scales about the inlet point: `out`
/// away from the centre line and `up` outward from r'_z.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SegmentPoint {
    pub out: f64,
    pub up: f64,
    /// Radius of the rounding at this corner over H; 0 for a sharp corner.
    pub radius: f64,
}

/// Default line segments: a short radial base, then a trapezoid flank to a well rounded
/// corner level with the top.
pub fn default_segments() -> Vec<SegmentPoint> {
    vec![
        SegmentPoint {
            out: 0.0,
            up: 0.08,
            radius: 0.0,
        },
        SegmentPoint {
            out: 0.55,
            up: 1.0,
            radius: 0.4,
        },
    ]
}

/// A side of straight lines and circular arcs: a radial base of height h_B, a cone at the
/// opening angle δ and a main arc of radius R centred on the centre line through the top.
/// The base and the cone are rounded with R_B, the cone and the main arc with R_C, both
/// limited by the lengths they join. Every length is automatic by default, in proportion
/// to the inlet width b₃.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RadiusSide {
    /// h_B, m (automatic: 0.1·b₃).
    pub base_height: Param<f64>,
    /// R_B, m (automatic: 0.1·b₃).
    pub base_radius: Param<f64>,
    /// δ, rad.
    pub cone_angle: f64,
    /// R_C, m (automatic: 0.3·b₃).
    pub corner_radius: Param<f64>,
    /// R, m (automatic: b₃).
    pub main_radius: Param<f64>,
}

impl Default for RadiusSide {
    fn default() -> Self {
        Self {
            base_height: Param::Auto,
            base_radius: Param::Auto,
            cone_angle: DEFAULT_CONE_ANGLE_DEG.to_radians(),
            corner_radius: Param::Auto,
            main_radius: Param::Auto,
        }
    }
}

impl RadiusSide {
    /// (h_B, R_B, R_C, R) for an inlet of width `b3`, m.
    pub fn lengths(&self, b3: f64) -> (f64, f64, f64, f64) {
        (
            self.base_height.resolve(|| 0.1 * b3),
            self.base_radius.resolve(|| 0.1 * b3),
            self.corner_radius.resolve(|| 0.3 * b3),
            self.main_radius.resolve(|| b3),
        )
    }
}

/// The shape of one side of a section, from its inlet point to the top on the centre
/// line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SideShape {
    /// A radial wall and the top; the tangent angle λ (rad) tilts the top down from the
    /// centre line.
    Rectangular {
        tangent_angle: f64,
    },
    /// A wall opening at the cone angle δ from the radial direction and the top at λ, rad.
    Trapezoidal {
        cone_angle: f64,
        tangent_angle: f64,
    },
    /// An elliptic arc through the inlet point with its axis on the centre line, level at
    /// the top. `ellipse_ratio` is its axial over its radial semi-axis, kept as it grows:
    /// 1 for a circle.
    Round {
        ellipse_ratio: f64,
    },
    Bezier(BezierSide),
    /// A polyline of rounded corners from the inlet point to the top.
    LineSegments {
        points: Vec<SegmentPoint>,
    },
    RadiusBased(RadiusSide),
}

impl SideShape {
    pub fn trapezoidal(cone_angle: f64) -> Self {
        SideShape::Trapezoidal {
            cone_angle,
            tangent_angle: 0.0,
        }
    }

    /// Name of the kind of side, in English.
    pub fn name(&self) -> &'static str {
        match self {
            SideShape::Rectangular { .. } => "rectangular",
            SideShape::Trapezoidal { .. } => "trapezoidal",
            SideShape::Round { .. } => "round",
            SideShape::Bezier(b) => match b.base {
                BezierBase::Rectangle => "Bézier, rectangle type",
                BezierBase::Trapezoid => "Bézier, trapezoid type",
            },
            SideShape::LineSegments { .. } => "line segments",
            SideShape::RadiusBased(_) => "radius based",
        }
    }
}

/// A round section that grows toward one side: the ellipse touches the side wall on the
/// other, closed side at its inlet point and widens across the volute, a shape with a
/// more favourable secondary flow than the symmetric one (often used after mixed-flow
/// impellers). Small sections are symmetric caps until the ellipse is as wide as the
/// inlet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalRound {
    /// Axial over radial semi-axis, kept as it grows: 1 for a circle.
    pub ellipse_ratio: f64,
    /// Keep the section outside r'_z; otherwise it also grows inward beside the inlet,
    /// on the open side.
    pub strictly_external: bool,
    /// Grow toward +z; by default toward −z.
    pub open_to_right: bool,
    /// A square corner on the closed side of the top instead of the elliptic arc.
    pub square_on_top: bool,
}

impl Default for ExternalRound {
    fn default() -> Self {
        Self {
            ellipse_ratio: 1.0,
            strictly_external: true,
            open_to_right: false,
            square_on_top: false,
        }
    }
}

/// The shape of a wall of the bend of an internal section, where it turns from radial to
/// axial.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BendWall {
    /// A sharp corner.
    Sharp,
    /// A straight chamfer across the corner.
    Straight,
    /// A quarter ellipse.
    #[default]
    Round,
}

/// A round section beside the impeller that keeps within the radius of the volute inlet:
/// a bend turns the outflow of the inlet from radial to axial, a straight neck leads it
/// sideways, and the spiral grows from the end of the neck away from the impeller and
/// inward toward the axis, under the top wall of the neck.
///
/// The bend is shaped by its two walls. The inner one, on the side the flow turns to,
/// starts at that inlet point; rounded or chamfered, it spans half the inlet width across
/// and `bend_ratio` times that up. The outer one turns about the same centre from the other
/// inlet point to the top of the neck. The neck is `area_ratio`·b₃ high (the outlet of the
/// bend over its inlet, per unit length round the spiral) and `neck_width` long. All of it
/// is the same round the whole spiral, so every internal definition shares it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InternalRound {
    /// Axial over radial semi-axis of the spiral part, kept as it grows: 1 for a circle.
    pub ellipse_ratio: f64,
    /// Toward +z, the back of the impeller where its hub disk is; otherwise toward −z.
    pub open_to_right: bool,
    /// Length of the straight neck after the bend, m (automatic: 0.5·b₃).
    pub neck_width: Param<f64>,
    pub inner_bend: BendWall,
    pub outer_bend: BendWall,
    /// Radial over axial semi-axis of the inner wall of the bend.
    pub bend_ratio: f64,
    /// Height of the neck over the inlet width b₃.
    pub area_ratio: f64,
    /// Axial gap s_ax between the casing wall round the spiral and the impeller beside
    /// it, m (automatic: 0.065·r₂). The automatic neck grows to keep it.
    pub side_gap: Param<f64>,
}

impl Default for InternalRound {
    fn default() -> Self {
        Self {
            ellipse_ratio: 1.0,
            open_to_right: true,
            neck_width: Param::Auto,
            inner_bend: BendWall::Round,
            outer_bend: BendWall::Round,
            bend_ratio: 1.0,
            area_ratio: 1.0,
            side_gap: Param::Auto,
        }
    }
}

/// The bend and the neck of an internal section in the plane of the section (s, h). Each
/// wall is a list of pieces, smooth inside and joined at corners or tangent points, the
/// last piece ending on the face where the spiral starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Bend {
    /// From the inlet point on the side away from the spiral round the outside of the
    /// bend and along the top of the neck.
    pub outer: Vec<Vec<DVec2>>,
    /// From the other inlet point round the inside of the bend and along the floor of
    /// the neck.
    pub inner: Vec<Vec<DVec2>>,
    /// Axial position s of the face, m.
    pub face: f64,
    /// Heights h of the floor and the top of the neck, m.
    pub floor: f64,
    pub top: f64,
}

impl Bend {
    /// The region of the bend and the neck as one closed polygon, counter-clockwise in
    /// (r, z): the inlet, the outer wall, the face and the inner wall.
    pub fn region(&self) -> Vec<DVec2> {
        let mut points: Vec<DVec2> = self.outer.iter().flatten().copied().collect();
        points.extend(
            self.inner
                .iter()
                .rev()
                .flat_map(|p| p.iter().rev())
                .copied(),
        );
        points.dedup_by(|a, b| a.distance(*b) < 1e-12);
        orient(points)
    }
}

/// `points` turned counter-clockwise in (r, z), with (s, h) coordinates.
fn orient(mut points: Vec<DVec2>) -> Vec<DVec2> {
    let n = points.len();
    let area: f64 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.y * b.x - b.y * a.x
        })
        .sum();
    if area < 0.0 {
        points.reverse();
    }
    points
}

impl InternalRound {
    /// The bend and the neck for an inlet of width `b3`.
    pub fn bend(&self, b3: f64) -> Bend {
        let c = 0.5 * b3;
        let neck = self.area_ratio * b3;
        // Built toward −z: the flow turns from the inlet point at −c, the inner corner.
        let (a_inner, b_inner) = match self.inner_bend {
            BendWall::Sharp => (0.0, 0.0),
            _ => (c, self.bend_ratio * c),
        };
        let centre = DVec2::new(-c - a_inner, 0.0);
        let floor = b_inner;
        let top = floor + neck;
        let face = centre.x - self.neck_width.resolve(|| 0.5 * b3);
        let quarter = |a: f64, b: f64| -> Vec<DVec2> {
            let steps = (FRAC_PI_2 / ARC_STEP).ceil() as usize;
            (0..=steps)
                .map(|i| {
                    let t = FRAC_PI_2 * i as f64 / steps as f64;
                    centre + DVec2::new(a * t.cos(), b * t.sin())
                })
                .collect()
        };
        let corner = |a: f64, b: f64, wall: BendWall| -> Vec<Vec<DVec2>> {
            let (start, end) = (centre + DVec2::new(a, 0.0), centre + DVec2::new(0.0, b));
            match wall {
                BendWall::Sharp => vec![
                    vec![start, centre + DVec2::new(a, b)],
                    vec![centre + DVec2::new(a, b), end],
                ],
                BendWall::Straight => vec![vec![start, end]],
                BendWall::Round => vec![quarter(a, b)],
            }
        };
        let mut inner = if a_inner > 0.0 {
            corner(a_inner, b_inner, self.inner_bend)
        } else {
            Vec::new()
        };
        inner.push(vec![DVec2::new(centre.x, floor), DVec2::new(face, floor)]);
        let mut outer = corner(c - centre.x, top, self.outer_bend);
        outer.push(vec![DVec2::new(centre.x, top), DVec2::new(face, top)]);
        let mut bend = Bend {
            outer,
            inner,
            face,
            floor,
            top,
        };
        if self.open_to_right {
            let mirror = |pieces: &mut Vec<Vec<DVec2>>| {
                for p in pieces.iter_mut().flatten() {
                    p.x = -p.x;
                }
            };
            mirror(&mut bend.outer);
            mirror(&mut bend.inner);
            bend.face = -bend.face;
        }
        bend
    }
}

/// The spiral part of an internal section of depth `h` from the face of the neck: the
/// asymmetric round section built on the face instead of the inlet, closed on the side
/// of the top of the neck with a square corner, growing away from the impeller and
/// inward.
fn internal_round(b3: f64, h: f64, shape: &InternalRound) -> Outline {
    let bend = shape.bend(b3);
    let half = 0.5 * (bend.top - bend.floor);
    let middle = 0.5 * (bend.top + bend.floor);
    let on_face = external_round(
        half,
        h,
        &ExternalRound {
            ellipse_ratio: shape.ellipse_ratio,
            strictly_external: true,
            open_to_right: false,
            square_on_top: true,
        },
    );
    // Across the face toward the top, out from the face away from the impeller.
    let away = if shape.open_to_right { 1.0 } else { -1.0 };
    on_face.mapped(
        |p| DVec2::new(bend.face + away * p.y, middle + p.x),
        shape.open_to_right,
    )
}

/// The shape of a whole cross section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SectionShape {
    /// The −z side and, unless symmetric (`None`), its own +z side.
    Sides {
        left: SideShape,
        right: Option<SideShape>,
    },
    RoundExternal(ExternalRound),
    RoundInternal(InternalRound),
}

impl Default for SectionShape {
    fn default() -> Self {
        SectionShape::symmetric(SideShape::trapezoidal(DEFAULT_CONE_ANGLE_DEG.to_radians()))
    }
}

impl SectionShape {
    pub fn symmetric(side: SideShape) -> Self {
        SectionShape::Sides {
            left: side,
            right: None,
        }
    }

    pub fn rectangular() -> Self {
        SectionShape::symmetric(SideShape::Rectangular { tangent_angle: 0.0 })
    }

    pub fn trapezoidal(cone_angle: f64) -> Self {
        SectionShape::symmetric(SideShape::trapezoidal(cone_angle))
    }

    pub fn round() -> Self {
        SectionShape::symmetric(SideShape::Round { ellipse_ratio: 1.0 })
    }

    /// The +z side, which mirrors the −z side when the section is symmetric.
    pub fn right_side(&self) -> Option<&SideShape> {
        match self {
            SectionShape::Sides { left, right } => Some(right.as_ref().unwrap_or(left)),
            SectionShape::RoundExternal(_) | SectionShape::RoundInternal(_) => None,
        }
    }

    /// The cone angle of a section of straight walls (rectangular or trapezoidal on both
    /// sides, level at the top) when both sides share it: the shape a double volute needs.
    pub fn straight_cone_angle(&self) -> Option<f64> {
        let angle = |side: &SideShape| match *side {
            SideShape::Rectangular { tangent_angle: 0.0 } => Some(0.0),
            SideShape::Trapezoidal {
                cone_angle,
                tangent_angle: 0.0,
            } => Some(cone_angle),
            _ => None,
        };
        match self {
            SectionShape::Sides { left, right } => {
                let a = angle(left)?;
                match right {
                    Some(r) if (angle(r)? - a).abs() > 1e-12 => None,
                    _ => Some(a),
                }
            }
            SectionShape::RoundExternal(_) | SectionShape::RoundInternal(_) => None,
        }
    }

    /// The cone angle this shape opens at, if it has one: the angle a double volute keeps
    /// when it converts the shape.
    pub fn cone_angle(&self) -> Option<f64> {
        match self {
            SectionShape::Sides { left, .. } => match left {
                SideShape::Rectangular { .. } => Some(0.0),
                SideShape::Trapezoidal { cone_angle, .. } => Some(*cone_angle),
                SideShape::Bezier(b) => Some(b.wall_angle()),
                SideShape::RadiusBased(r) => Some(r.cone_angle),
                SideShape::Round { .. } | SideShape::LineSegments { .. } => None,
            },
            SectionShape::RoundExternal(_) | SectionShape::RoundInternal(_) => None,
        }
    }
}

/// The shape of the cross sections at one wrap angle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionDefinition {
    /// Wrap angle φ from the cutwater, rad, in (0, 2π]. The section at φ = 0 is always
    /// flat, so a definition there would mean nothing.
    pub angle: f64,
    /// Only active definitions shape the spiral; at least one must be.
    pub active: bool,
    pub shape: SectionShape,
}

impl SectionDefinition {
    /// An active definition of `shape` at the throat (φ = 360°).
    pub fn at_throat(shape: SectionShape) -> Self {
        Self {
            angle: TAU,
            active: true,
            shape,
        }
    }
}

/// Inputs of the cross-section step: the definitions, in any order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawCrossSectionInput")]
pub struct CrossSectionInput {
    pub sections: Vec<SectionDefinition>,
}

impl Default for CrossSectionInput {
    fn default() -> Self {
        Self::single(SectionShape::default())
    }
}

impl CrossSectionInput {
    /// One definition of `shape` for the whole spiral.
    pub fn single(shape: SectionShape) -> Self {
        Self {
            sections: vec![SectionDefinition::at_throat(shape)],
        }
    }
}

/// The cross-section step as stored: the definitions, or the single `shape` of project
/// files written before definitions existed.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawCrossSectionInput {
    sections: Option<Vec<SectionDefinition>>,
    shape: Option<LegacyShape>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LegacyShape {
    Rectangular,
    Trapezoidal { flank_angle: f64 },
    Circular,
}

impl From<RawCrossSectionInput> for CrossSectionInput {
    fn from(raw: RawCrossSectionInput) -> Self {
        if let Some(sections) = raw.sections {
            return Self { sections };
        }
        match raw.shape {
            Some(LegacyShape::Rectangular) => Self::single(SectionShape::rectangular()),
            Some(LegacyShape::Trapezoidal { flank_angle }) => {
                Self::single(SectionShape::trapezoidal(flank_angle))
            }
            Some(LegacyShape::Circular) => Self::single(SectionShape::round()),
            None => Self::default(),
        }
    }
}

/// Geometric properties of a section placed at r'_z.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionProperties {
    /// The size H the section was scaled to, m.
    pub height: f64,
    /// m²
    pub area: f64,
    /// ∬ dA/r = ∫ b/r dr, m.
    pub momentum: f64,
    /// Radius of the area centroid, m.
    pub centroid_radius: f64,
    /// Outermost radius, m.
    pub outer_radius: f64,
    /// Innermost radius, m.
    pub inner_radius: f64,
    /// Axial extent, m.
    pub width: f64,
    /// Length of the walls: the whole outline but the inlet, m.
    pub wetted_perimeter: f64,
    /// ∮ r⁻² dℓ along the walls, 1/m.
    pub wall_inverse_square: f64,
}

/// A section outline in its plane, (s, h). It runs from the −z end of the inlet along
/// the walls over the top to the +z end of the inlet, counter-clockwise in (r, z); the
/// inlet closes it, with or without points of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub points: Vec<DVec2>,
    /// Index of the top, where the two sides meet.
    pub top: usize,
    /// Index of the +z end of the inlet: the last wall point.
    pub inlet_end: usize,
    /// Indices that start a smooth run of the outline: the −z inlet point, the corners of
    /// the walls, the top and the +z inlet point.
    pub breaks: Vec<usize>,
}

/// Average of ln r over a straight edge from radius `a` to `b`.
fn mean_ln(a: f64, b: f64) -> f64 {
    let m = 0.5 * (a + b);
    let d = 0.5 * (b - a);
    if d.abs() < 1e-5 * m {
        let q = d / m;
        m.ln() - q * q / 6.0 - q.powi(4) / 20.0
    } else {
        (b * b.ln() - b - a * a.ln() + a) / (b - a)
    }
}

impl Outline {
    /// Properties of the outline placed with its inlet on radius `rz`, scaled to the
    /// size `height`.
    pub fn properties(&self, rz: f64, height: f64) -> Result<SectionProperties, DesignError> {
        let n = self.points.len();
        let (mut area, mut momentum, mut moment) = (0.0, 0.0, 0.0);
        let (mut perimeter, mut inverse) = (0.0, 0.0);
        let (mut h_lo, mut h_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut s_lo, mut s_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in 0..n {
            let (p, q) = (self.points[i], self.points[(i + 1) % n]);
            let (ra, rb) = (rz + p.y, rz + q.y);
            if ra <= 0.0 {
                return Err(DesignError::Infeasible(
                    "the volute cross section reaches the pump axis".into(),
                ));
            }
            let dz = q.x - p.x;
            area += 0.5 * (ra * q.x - rb * p.x);
            momentum += dz * mean_ln(ra, rb);
            moment += dz * (ra * ra + ra * rb + rb * rb) / 6.0;
            if i < self.inlet_end {
                let length = p.distance(q);
                perimeter += length;
                inverse += length / (ra * rb);
            }
            h_lo = h_lo.min(p.y);
            h_hi = h_hi.max(p.y);
            s_lo = s_lo.min(p.x);
            s_hi = s_hi.max(p.x);
        }
        Ok(SectionProperties {
            height,
            area,
            momentum,
            centroid_radius: if area > 0.0 { moment / area } else { rz },
            outer_radius: rz + h_hi,
            inner_radius: rz + h_lo,
            width: s_hi - s_lo,
            wetted_perimeter: perimeter,
            wall_inverse_square: inverse,
        })
    }

    /// The outline with every point moved by `f`. A map that `flips` the orientation, a
    /// mirror, is followed by a reversal that keeps the outline counter-clockwise and
    /// starting at an end of the inlet.
    fn mapped(self, f: impl Fn(DVec2) -> DVec2, flips: bool) -> Outline {
        let mut points: Vec<DVec2> = self.points.iter().map(|p| f(*p)).collect();
        if !flips {
            return Outline { points, ..self };
        }
        let last = self.inlet_end;
        points.reverse();
        let mut breaks: Vec<usize> = self.breaks.iter().map(|&b| last - b).collect();
        breaks.sort_unstable();
        Outline {
            points,
            top: last - self.top,
            inlet_end: last,
            breaks,
        }
    }

    /// Breaks of the three runs every outline has: the −z inlet point, the top and the
    /// +z inlet point.
    fn basic_breaks(&self) -> Vec<usize> {
        vec![0, self.top, self.inlet_end]
    }

    /// The outline resampled run by run, `counts[k]` points evenly spaced by length on
    /// run k, which starts at `breaks[k]`; the last run is the inlet back to the start.
    fn resample(&self, breaks: &[usize], counts: &[usize]) -> Outline {
        let n = self.points.len();
        let mut points = Vec::with_capacity(counts.iter().sum());
        let mut starts = Vec::with_capacity(breaks.len());
        for (k, &start) in breaks.iter().enumerate() {
            let end = breaks.get(k + 1).copied().unwrap_or(n);
            let run: Vec<DVec2> = (start..=end).map(|i| self.points[i % n]).collect();
            starts.push(points.len());
            sample_run(&run, counts[k], &mut points);
        }
        let top_run = breaks.iter().position(|&b| b == self.top).unwrap_or(1);
        let inlet_run = breaks.len() - 1;
        Outline {
            top: starts[top_run],
            inlet_end: starts[inlet_run],
            breaks: starts[..breaks.len()].to_vec(),
            points,
        }
    }

    /// The outline in the meridional plane (r, z) for an inlet on radius `rz` and a
    /// midplane at `zm`.
    pub fn meridional(&self, rz: f64, zm: f64) -> Vec<DVec2> {
        self.points
            .iter()
            .map(|p| DVec2::new(rz + p.y, zm + p.x))
            .collect()
    }

    /// Area enclosed, positive when the outline runs counter-clockwise in (r, z), m².
    pub fn signed_area(&self) -> f64 {
        let p = &self.points;
        let n = p.len();
        // (r, z) = (h, s): the shoelace sum of h·s' − h'·s.
        0.5 * (0..n)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % n]);
                a.y * b.x - b.y * a.x
            })
            .sum::<f64>()
    }

    /// Whether no two edges cross: the outline bounds one region.
    pub fn is_simple(&self) -> bool {
        let p = &self.points;
        let n = p.len();
        let cross = |a: DVec2, b: DVec2, c: DVec2| (b - a).perp_dot(c - a);
        for i in 0..n {
            let (a, b) = (p[i], p[(i + 1) % n]);
            for j in i + 2..n {
                if (j + 1) % n == i {
                    continue;
                }
                let (c, d) = (p[j], p[(j + 1) % n]);
                let (d1, d2) = (cross(a, b, c), cross(a, b, d));
                let (d3, d4) = (cross(c, d, a), cross(c, d, b));
                if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                    return false;
                }
            }
        }
        true
    }
}

/// `count` points evenly spaced by length along the polyline `run`, its first point
/// included and its last one left for the next run.
fn sample_run(run: &[DVec2], count: usize, out: &mut Vec<DVec2>) {
    let mut lengths = Vec::with_capacity(run.len());
    let mut total = 0.0;
    lengths.push(0.0);
    for w in run.windows(2) {
        total += w[0].distance(w[1]);
        lengths.push(total);
    }
    let mut j = 0;
    for k in 0..count {
        let target = total * k as f64 / count as f64;
        while j + 2 < lengths.len() && lengths[j + 1] < target {
            j += 1;
        }
        let span = lengths[j + 1] - lengths[j];
        let t = if span > 0.0 {
            ((target - lengths[j]) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(run[j].lerp(run[j + 1], t));
    }
}

/// A side path in its own frame (x, y): x the distance from the centre line, y the height
/// above r'_z. It runs from the inlet point (b₃/2, 0) to the top (0, H).
#[derive(Debug, Clone)]
struct Path {
    points: Vec<DVec2>,
    /// Indices of the sharp corners between the ends.
    corners: Vec<usize>,
    /// Points closer than this are merged, m.
    tolerance: f64,
}

impl Path {
    fn new(start: DVec2, scale: f64) -> Self {
        Self {
            points: vec![start],
            corners: Vec::new(),
            tolerance: 1e-10 * scale,
        }
    }

    fn last(&self) -> DVec2 {
        self.points[self.points.len() - 1]
    }

    fn push(&mut self, p: DVec2) {
        if self.last().distance(p) > self.tolerance {
            self.points.push(p);
        }
    }

    /// Marks the last point as a sharp corner.
    fn corner(&mut self) {
        self.corners.push(self.points.len() - 1);
    }

    /// Arc about `center` from angle `from` to `to` (rad), its start already in place.
    fn arc(&mut self, center: DVec2, radius: f64, from: f64, to: f64) {
        self.ellipse(center, radius, radius, from, to);
    }

    fn ellipse(&mut self, center: DVec2, ax: f64, ay: f64, from: f64, to: f64) {
        let steps = ((to - from).abs() / ARC_STEP).ceil().max(1.0) as usize;
        for i in 1..=steps {
            let t = from + (to - from) * i as f64 / steps as f64;
            self.push(center + DVec2::new(ax * t.cos(), ay * t.sin()));
        }
    }

    /// Ends the path exactly at the top.
    fn finish(mut self, top: DVec2) -> Self {
        if self.last().distance(top) > self.tolerance {
            self.points.push(top);
        } else {
            let n = self.points.len();
            self.points[n - 1] = top;
        }
        let last = self.points.len() - 1;
        self.corners.retain(|&i| i > 0 && i < last);
        self
    }
}

fn angle_of(v: DVec2) -> f64 {
    v.y.atan2(v.x)
}

/// The corner point S where the side wall from the inlet point (c, 0) at cone angle
/// `cone` meets the top line through (0, H) at tangent angle `tangent`. The tangent angle
/// is reduced on low sections so that S stays above a tenth of the height.
fn corner_point(c: f64, h: f64, cone: f64, tangent: f64) -> DVec2 {
    let t = cone.tan();
    let lambda = tangent.tan().min(0.9 * h / (c + 0.1 * h * t)).atan();
    let u = (c + h * t) / (lambda.cos() + lambda.sin() * t);
    DVec2::new(u * lambda.cos(), h - u * lambda.sin())
}

fn straight_side(c: f64, h: f64, cone: f64, tangent: f64) -> Path {
    let mut path = Path::new(DVec2::new(c, 0.0), c + h);
    path.push(corner_point(c, h, cone, tangent));
    path.corner();
    path.finish(DVec2::new(0.0, h))
}

/// Elliptic arc of axis ratio `k` with its axis on the centre line, through (c, 0) and
/// level at the top (0, H).
fn round_side(c: f64, h: f64, k: f64) -> Path {
    // (c/(k·a))² + ((H − a)/a)² = 1 with the radial semi-axis a.
    let a = (h * h + (c / k).powi(2)) / (2.0 * h);
    let centre = DVec2::new(0.0, h - a);
    let from = (-centre.y).atan2(c / k);
    let mut path = Path::new(DVec2::new(c, 0.0), c + h);
    path.ellipse(centre, k * a, a, from, FRAC_PI_2);
    path.finish(DVec2::new(0.0, h))
}

impl BezierSide {
    /// Control points 0 to 4 and the corner point S of the side scaled to `height` on an
    /// inlet of width `b3`, in the frame of the side: x from the centre line, y above
    /// r'_z.
    pub fn control_points(&self, b3: f64, height: f64) -> [DVec2; 6] {
        let c = 0.5 * b3;
        let p0 = DVec2::new(c, 0.0);
        let p4 = DVec2::new(0.0, height);
        let s = corner_point(c, height, self.wall_angle(), self.tangent_angle);
        let (leg0, leg4) = (s - p0, s - p4);
        let place = |end: DVec2, leg: DVec2, value: f64| match self.position {
            InnerPosition::Relative => end + leg * value,
            InnerPosition::Absolute => {
                let length = leg.length();
                s - leg * (value.min(length) / length)
            }
        };
        let p1 = place(p0, leg0, self.point1);
        let p3 = place(p4, leg4, self.point3);
        let p2 = s - leg0 * self.point2[0] - leg4 * self.point2[1];
        [p0, p1, p2, p3, p4, s]
    }

    /// Moves inner point `index` (1, 2 or 3) as close to `target` as its line allows, for
    /// the side scaled to `height` on an inlet of width `b3`.
    pub fn drag(&mut self, index: usize, target: DVec2, b3: f64, height: f64) {
        let [p0, _, _, _, p4, s] = self.control_points(b3, height);
        // Fraction of the way from `end` toward S, or the distance back from S.
        let along = |end: DVec2| {
            let leg = s - end;
            let t = ((target - end).dot(leg) / leg.length_squared()).clamp(0.0, 1.0);
            match self.position {
                InnerPosition::Relative => t,
                InnerPosition::Absolute => (1.0 - t) * leg.length(),
            }
        };
        match index {
            1 => self.point1 = along(p0),
            3 => self.point3 = along(p4),
            2 => {
                // target = S − α·(S − P0) − β·(S − P4).
                let (a, b) = (s - p0, s - p4);
                let rhs = s - target;
                let det = a.perp_dot(b);
                if det.abs() > 1e-15 {
                    let alpha = rhs.perp_dot(b) / det;
                    let beta = a.perp_dot(rhs) / det;
                    self.point2 = [alpha.clamp(-1.0, 2.0), beta.clamp(-1.0, 2.0)];
                }
            }
            _ => {}
        }
    }
}

/// The corners of a line-segment side scaled to `height` on an inlet of width `b3`, in
/// the frame of the side: the inlet point, each corner and the top.
pub fn segment_vertices(points: &[SegmentPoint], b3: f64, height: f64) -> Vec<DVec2> {
    let c = 0.5 * b3;
    let mut vertices = vec![DVec2::new(c, 0.0)];
    vertices.extend(
        points
            .iter()
            .map(|p| DVec2::new((c + height * p.out).max(0.0), height * p.up)),
    );
    vertices.push(DVec2::new(0.0, height));
    vertices
}

impl SegmentPoint {
    /// The corner at `position` in the frame of a side scaled to `height` on an inlet of
    /// width `b3`.
    pub fn at(position: DVec2, b3: f64, height: f64, radius: f64) -> Self {
        Self {
            out: (position.x - 0.5 * b3) / height,
            up: position.y / height,
            radius,
        }
    }
}

fn bezier_side(c: f64, h: f64, side: &BezierSide) -> Path {
    let [p0, p1, p2, p3, p4, _] = side.control_points(2.0 * c, h);
    let control = [p0, p1, p2, p3, p4];
    let mut path = Path::new(p0, c + h);
    for i in 1..BEZIER_POINTS {
        let t = i as f64 / BEZIER_POINTS as f64;
        // de Casteljau on the five control points.
        let mut q = control;
        for level in (1..5).rev() {
            for j in 0..level {
                q[j] = q[j].lerp(q[j + 1], t);
            }
        }
        path.push(q[0]);
    }
    path.finish(p4)
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

/// Arc of a fillet about `center` from `a` to `b`, turning by less than π.
fn fillet_arc(path: &mut Path, center: DVec2, a: DVec2, b: DVec2) {
    let from = angle_of(a - center);
    let mut sweep = angle_of(b - center) - from;
    if sweep > PI {
        sweep -= TAU;
    } else if sweep < -PI {
        sweep += TAU;
    }
    path.arc(center, a.distance(center), from, from + sweep);
    path.push(b);
}

fn segment_side(c: f64, h: f64, points: &[SegmentPoint]) -> Path {
    let p0 = DVec2::new(c, 0.0);
    let top = DVec2::new(0.0, h);
    let vertices = segment_vertices(points, 2.0 * c, h);
    let radii: Vec<f64> = points.iter().map(|p| (h * p.radius).max(0.0)).collect();
    let mut path = Path::new(p0, c + h);
    // Tangent length each corner may use: all of an edge toward an end point, half of
    // one shared with another rounded corner.
    let n = vertices.len();
    for k in 1..n - 1 {
        let (prev, v, next) = (vertices[k - 1], vertices[k], vertices[k + 1]);
        let r = radii[k - 1];
        let (d1, d2) = (prev.distance(v), next.distance(v));
        let turn = (v - prev)
            .normalize_or_zero()
            .dot((next - v).normalize_or_zero());
        if r <= 0.0 || d1 == 0.0 || d2 == 0.0 || turn > 1.0 - 1e-12 {
            path.push(v);
            if r <= 0.0 {
                path.corner();
            }
            continue;
        }
        let share = |j: usize| {
            if j == 0 || j == n - 1 || radii[j - 1] <= 0.0 {
                1.0
            } else {
                0.5
            }
        };
        let available = (share(k - 1) * d1).min(share(k + 1) * d2);
        let half = 0.5 * (prev - v).normalize().dot((next - v).normalize()).acos();
        let radius = r.min(available * half.tan());
        let (a, b, centre) = fillet(prev, v, next, radius);
        path.push(a);
        fillet_arc(&mut path, centre, a, b);
    }
    path.finish(top)
}

/// Upper intersection, as the distance along the ray from `origin` in unit direction `d`,
/// of the ray with the circle about `centre` of `radius`; `None` when they miss.
fn ray_circle(origin: DVec2, d: DVec2, centre: DVec2, radius: f64) -> Option<f64> {
    let m = origin - centre;
    let b = d.dot(m);
    let disc = b * b - m.length_squared() + radius * radius;
    (disc >= 0.0).then(|| -b + disc.sqrt())
}

fn radius_side(c: f64, h: f64, side: &RadiusSide, b3: f64) -> Path {
    let (hb, rb, rc, r) = side.lengths(b3);
    let delta = side.cone_angle;
    let p0 = DVec2::new(c, 0.0);
    let top = DVec2::new(0.0, h);
    let base_top = DVec2::new(c, hb);
    let d = DVec2::new(delta.sin(), delta.cos());
    // Toward the inside of the section, normal to the cone.
    let n = DVec2::new(-delta.cos(), delta.sin());
    let centre = |radius: f64| DVec2::new(0.0, h - radius);
    let mut path = Path::new(p0, c + h);

    // Where the main arc meets the straight part, and from which line: on the cone at
    // `t` from the top of the base, or on the base at height `y`.
    enum Join {
        Cone { t: f64, radius: f64, tangent: bool },
        Base { y: f64, radius: f64 },
        Inlet { radius: f64 },
    }
    // Height at which the arc of radius R crosses the line of the base, x = c.
    let on_base = (r >= c).then(|| h - r + (r * r - c * c).sqrt());
    let on_cone = if h > hb {
        ray_circle(base_top, d, centre(r), r).filter(|&t| t >= 0.0)
    } else {
        None
    };
    let join = if let Some(t) = on_cone {
        Join::Cone {
            t,
            radius: r,
            tangent: false,
        }
    } else if let Some(y) = on_base.filter(|&y| y > 0.0 && y <= hb) {
        Join::Base { y, radius: r }
    } else if h > hb && on_base.is_some_and(|y| y > hb) || h > hb && r < c {
        // The arc stays inside the cone line: enlarge it to touch the cone, or else to
        // pass through the top of the base.
        let enlarged = (c * delta.cos() + delta.sin() * (h - hb)) / (1.0 + delta.sin());
        let foot = centre(enlarged) - n * enlarged;
        let t = (foot - base_top).dot(d);
        if t >= 0.0 {
            Join::Cone {
                t,
                radius: enlarged,
                tangent: true,
            }
        } else {
            let radius = (c * c + (h - hb).powi(2)) / (2.0 * (h - hb));
            Join::Base { y: hb, radius }
        }
    } else {
        // So low that the arc only spans the inlet: through the inlet point.
        Join::Inlet {
            radius: (h * h + c * c) / (2.0 * h),
        }
    };

    match join {
        Join::Inlet { radius } => {
            let m = centre(radius);
            path.arc(m, radius, angle_of(p0 - m), FRAC_PI_2);
        }
        Join::Base { y, radius } => {
            let m = centre(radius);
            let joint = DVec2::new(c, y);
            // Fillet R_C between the base line x = c and the arc, inside both: its
            // centre is R_C in from the line and R − R_C from the arc's centre.
            let fit = |f: f64| -> Option<(DVec2, DVec2, DVec2)> {
                if f <= 0.0 || f >= radius {
                    return None;
                }
                let x = c - f;
                let q = (radius - f).powi(2) - x * x;
                if q < 0.0 {
                    return None;
                }
                let centre_f = DVec2::new(x, m.y + q.sqrt());
                let on_line = DVec2::new(c, centre_f.y);
                let on_arc = m + (centre_f - m) * (radius / (radius - f));
                (on_line.y >= 0.0 && on_line.y <= y + 1e-12 && on_arc.x >= 0.0)
                    .then_some((on_line, on_arc, centre_f))
            };
            match largest_fillet(rc, fit) {
                Some((a, b, centre_f)) => {
                    path.push(a);
                    fillet_arc(&mut path, centre_f, a, b);
                    path.arc(m, radius, angle_of(b - m), FRAC_PI_2);
                }
                None => {
                    path.push(joint);
                    path.arc(m, radius, angle_of(joint - m), FRAC_PI_2);
                }
            }
        }
        Join::Cone { t, radius, tangent } => {
            let m = centre(radius);
            // Fillet R_C between the cone and the arc unless they already touch.
            let fit = |f: f64| -> Option<(DVec2, DVec2, DVec2)> {
                if f <= 0.0 || f >= radius {
                    return None;
                }
                let start = base_top + n * f;
                let s = ray_circle(start, d, m, radius - f)?;
                let centre_f = start + d * s;
                let on_arc = m + (centre_f - m) * (radius / (radius - f));
                (s >= 0.0 && s <= t + 1e-12 && on_arc.x >= 0.0).then_some((
                    base_top + d * s,
                    on_arc,
                    centre_f,
                ))
            };
            let corner = if tangent {
                None
            } else {
                largest_fillet(rc, fit)
            };
            let cone_end = corner.map_or(t, |(a, _, _)| (a - base_top).dot(d));
            // Fillet R_B between the base and the cone, within both.
            let turn_tan = (0.5 * delta).tan();
            let rb = if turn_tan > 1e-9 {
                rb.min(hb.min(cone_end) / turn_tan)
            } else {
                0.0
            };
            if rb > 0.0 {
                let (a, b, centre_b) = fillet(p0, base_top, base_top + d, rb);
                path.push(a);
                fillet_arc(&mut path, centre_b, a, b);
            } else {
                path.push(base_top);
            }
            match corner {
                Some((a, b, centre_f)) => {
                    path.push(a);
                    fillet_arc(&mut path, centre_f, a, b);
                    path.arc(m, radius, angle_of(b - m), FRAC_PI_2);
                }
                None => {
                    let joint = base_top + d * t;
                    path.push(joint);
                    path.arc(m, radius, angle_of(joint - m), FRAC_PI_2);
                }
            }
        }
    }
    path.finish(top)
}

/// The largest fillet radius up to `wanted` that `fit` accepts, with its tangent points
/// and centre.
fn largest_fillet(
    wanted: f64,
    fit: impl Fn(f64) -> Option<(DVec2, DVec2, DVec2)>,
) -> Option<(DVec2, DVec2, DVec2)> {
    if wanted <= 0.0 {
        return None;
    }
    if let Some(found) = fit(wanted) {
        return Some(found);
    }
    let (mut lo, mut hi) = (0.0, wanted);
    for _ in 0..50 {
        let mid = 0.5 * (lo + hi);
        if fit(mid).is_some() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    fit(lo)
}

fn side_path(c: f64, h: f64, side: &SideShape, b3: f64) -> Path {
    match side {
        SideShape::Rectangular { tangent_angle } => straight_side(c, h, 0.0, *tangent_angle),
        SideShape::Trapezoidal {
            cone_angle,
            tangent_angle,
        } => straight_side(c, h, *cone_angle, *tangent_angle),
        SideShape::Round { ellipse_ratio } => round_side(c, h, *ellipse_ratio),
        SideShape::Bezier(b) => bezier_side(c, h, b),
        SideShape::LineSegments { points } => segment_side(c, h, points),
        SideShape::RadiusBased(r) => radius_side(c, h, r, b3),
    }
}

/// The outline of two sides: the −z side from its inlet point to the top, then the +z
/// side back down.
fn join_sides(left: Path, right: Path) -> Outline {
    let mut points: Vec<DVec2> = left.points.iter().map(|p| DVec2::new(-p.x, p.y)).collect();
    let top = points.len() - 1;
    let m = right.points.len();
    points.extend(right.points.iter().rev().skip(1).copied());
    let mut breaks = vec![0];
    breaks.extend(&left.corners);
    breaks.push(top);
    let mut corners: Vec<usize> = right.corners.iter().map(|&i| top + (m - 1 - i)).collect();
    corners.sort_unstable();
    breaks.extend(corners);
    let inlet_end = points.len() - 1;
    breaks.push(inlet_end);
    Outline {
        points,
        top,
        inlet_end,
        breaks,
    }
}

/// The asymmetric round section, built growing toward −z and mirrored for +z.
fn external_round(c: f64, h: f64, shape: &ExternalRound) -> Outline {
    let k = shape.ellipse_ratio;
    let scale = c + h;
    // Growing toward −z, the closed side is +z: from (−c, 0) over the open side to the
    // top, then down the closed side to (c, 0).
    let mut path = Path::new(DVec2::new(-c, 0.0), scale);
    let (top, closed_from): (DVec2, Option<(DVec2, f64, f64, f64)>);
    if h <= c / k {
        // A symmetric cap through both inlet points until the ellipse spans the inlet.
        let a = (h * h + (c / k).powi(2)) / (2.0 * h);
        let centre = DVec2::new(0.0, h - a);
        let start = (-centre.y).atan2(-c / k);
        path.ellipse(centre, k * a, a, start, FRAC_PI_2);
        top = DVec2::new(0.0, h);
        closed_from = Some((centre, k * a, a, (-centre.y).atan2(c / k)));
    } else {
        // Half an ellipse on r'_z, touching the closed wall at its inlet point.
        let (ax, ay) = (k * h, h);
        let centre = DVec2::new(c - ax, 0.0);
        if shape.strictly_external {
            path.push(DVec2::new(centre.x - ax, 0.0));
            path.ellipse(centre, ax, ay, PI, FRAC_PI_2);
        } else {
            // Below r'_z on the open side, outside the inlet: down the side wall to the
            // lower half of the ellipse.
            let cos = ((-c - centre.x) / ax).clamp(-1.0, 1.0);
            let low = TAU - cos.acos();
            path.push(DVec2::new(-c, ay * low.sin()));
            path.ellipse(centre, ax, ay, low, PI);
            path.ellipse(centre, ax, ay, PI, FRAC_PI_2);
        }
        top = DVec2::new(centre.x, h);
        closed_from = Some((centre, ax, ay, 0.0));
    }
    let top_index = path.points.len() - 1;
    path.points[top_index] = top;
    let mut corners = Vec::new();
    if shape.square_on_top {
        path.push(DVec2::new(c, h));
        corners.push(path.points.len() - 1);
    } else if let Some((centre, ax, ay, end)) = closed_from {
        path.ellipse(centre, ax, ay, FRAC_PI_2, end);
    }
    path.push(DVec2::new(c, 0.0));
    let n = path.points.len();
    path.points[n - 1] = DVec2::new(c, 0.0);
    let mut outline = Outline {
        top: top_index,
        inlet_end: n - 1,
        breaks: [vec![0, top_index], corners, vec![n - 1]].concat(),
        points: path.points,
    };
    if shape.open_to_right {
        outline = outline.mapped(|p| DVec2::new(-p.x, p.y), true);
    }
    outline
}

impl SectionShape {
    /// The outline of this shape scaled to the height `height` for an inlet of width
    /// `b3`.
    pub fn outline(&self, b3: f64, height: f64) -> Outline {
        let c = 0.5 * b3;
        match self {
            SectionShape::Sides { left, right } => {
                let l = side_path(c, height, left, b3);
                let r = match right {
                    Some(side) => side_path(c, height, side, b3),
                    None => l.clone(),
                };
                join_sides(l, r)
            }
            SectionShape::RoundExternal(round) => external_round(c, height, round),
            SectionShape::RoundInternal(round) => internal_round(b3, height, round),
        }
    }

    /// Checks the parameters of the shape.
    fn validate(&self) -> Result<(), String> {
        let cone = |a: f64| -> Result<(), String> {
            if (0.0..=MAX_CONE_ANGLE_DEG.to_radians() + 1e-12).contains(&a) {
                Ok(())
            } else {
                Err(format!(
                    "the cone angle must lie between 0° and {MAX_CONE_ANGLE_DEG}°, got {:.1}°",
                    a.to_degrees()
                ))
            }
        };
        let tangent = |a: f64| -> Result<(), String> {
            if (0.0..=MAX_TANGENT_ANGLE_DEG.to_radians() + 1e-12).contains(&a) {
                Ok(())
            } else {
                Err(format!(
                    "the tangent angle must lie between 0° and {MAX_TANGENT_ANGLE_DEG}°, got \
                     {:.1}°",
                    a.to_degrees()
                ))
            }
        };
        let ratio = |k: f64| -> Result<(), String> {
            if (0.2..=5.0).contains(&k) {
                Ok(())
            } else {
                Err(format!(
                    "the ellipse ratio must lie between 0.2 and 5, got {k}"
                ))
            }
        };
        let side = |s: &SideShape| -> Result<(), String> {
            match s {
                SideShape::Rectangular { tangent_angle } => tangent(*tangent_angle),
                SideShape::Trapezoidal {
                    cone_angle,
                    tangent_angle,
                } => {
                    cone(*cone_angle)?;
                    tangent(*tangent_angle)
                }
                SideShape::Round { ellipse_ratio } => ratio(*ellipse_ratio),
                SideShape::Bezier(b) => {
                    cone(b.wall_angle())?;
                    tangent(b.tangent_angle)?;
                    let inner = |v: f64| match b.position {
                        InnerPosition::Relative if !(0.0..=1.0).contains(&v) => Err(format!(
                            "relative Bézier points must lie between 0 and 1, got {v}"
                        )),
                        InnerPosition::Absolute if v < 0.0 => Err(format!(
                            "absolute Bézier points cannot lie beyond S, got {v}"
                        )),
                        _ => Ok(()),
                    };
                    inner(b.point1)?;
                    inner(b.point3)?;
                    if b.point2.iter().any(|v| !(-1.0..=2.0).contains(v)) {
                        return Err("Bézier point 2 lies too far from S".into());
                    }
                    Ok(())
                }
                SideShape::LineSegments { points } => {
                    for p in points {
                        if !(p.up > 0.0 && p.up <= 3.0 && (-1.0..=3.0).contains(&p.out)) {
                            return Err(format!(
                                "a line-segment corner lies out of range: {:.2}·H out, {:.2}·H \
                                 up",
                                p.out, p.up
                            ));
                        }
                        if !(0.0..=1.0).contains(&p.radius) {
                            return Err(format!(
                                "a corner radius must lie between 0 and H, got {:.2}·H",
                                p.radius
                            ));
                        }
                    }
                    Ok(())
                }
                SideShape::RadiusBased(r) => {
                    cone(r.cone_angle)?;
                    for (name, value) in [
                        ("base height", r.base_height),
                        ("base radius", r.base_radius),
                        ("corner radius", r.corner_radius),
                    ] {
                        if value.manual_value().is_some_and(|v| v < 0.0) {
                            return Err(format!("the {name} cannot be negative"));
                        }
                    }
                    if r.main_radius.manual_value().is_some_and(|v| v <= 0.0) {
                        return Err("the main radius must be positive".into());
                    }
                    Ok(())
                }
            }
        };
        match self {
            SectionShape::Sides { left, right } => {
                side(left)?;
                if let Some(r) = right {
                    side(r)?;
                }
                Ok(())
            }
            SectionShape::RoundExternal(round) => ratio(round.ellipse_ratio),
            SectionShape::RoundInternal(round) => {
                ratio(round.ellipse_ratio)?;
                if !(0.2..=5.0).contains(&round.bend_ratio) {
                    return Err(format!(
                        "the bend ratio must lie between 0.2 and 5, got {}",
                        round.bend_ratio
                    ));
                }
                if !(0.3..=3.0).contains(&round.area_ratio) {
                    return Err(format!(
                        "the area ratio of the bend must lie between 0.3 and 3, got {}",
                        round.area_ratio
                    ));
                }
                if round.neck_width.manual_value().is_some_and(|w| w < 0.0) {
                    return Err("the neck width cannot be negative".into());
                }
                if round.side_gap.manual_value().is_some_and(|g| g < 0.0) {
                    return Err("the side gap cannot be negative".into());
                }
                Ok(())
            }
        }
    }
}

/// The cross sections of one volute, ready to scale: the active definitions in order of
/// wrap angle on an inlet of width b₃ at r'_z.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossSections {
    definitions: Vec<SectionDefinition>,
    pub inlet_width: f64,
    pub stagnation_radius: f64,
    /// Break indices of a definition's outline every definition shares; `None` when they
    /// differ and only the inlet points and the top are kept.
    shared_breaks: Option<(usize, usize)>,
}

impl CrossSections {
    /// Checks and orders the active definitions of `input` for an inlet of width `b3` on
    /// radius `rz`.
    pub fn new(input: &CrossSectionInput, b3: f64, rz: f64) -> Result<Self, DesignError> {
        let invalid = |m: String| DesignError::InvalidInput(m);
        let mut definitions: Vec<SectionDefinition> = input
            .sections
            .iter()
            .filter(|d| d.active)
            .cloned()
            .collect();
        if definitions.is_empty() {
            return Err(invalid("at least one cross section must be active".into()));
        }
        definitions.sort_by(|a, b| a.angle.total_cmp(&b.angle));
        for d in &definitions {
            if !(d.angle > 0.0 && d.angle <= TAU + 1e-9) {
                return Err(invalid(format!(
                    "cross sections are defined between 0° and 360°, got {:.1}°",
                    d.angle.to_degrees()
                )));
            }
            d.shape.validate().map_err(|e| {
                invalid(format!(
                    "the cross section at {:.0}°: {e}",
                    d.angle.to_degrees()
                ))
            })?;
        }
        for w in definitions.windows(2) {
            if w[1].angle - w[0].angle < 0.5f64.to_radians() {
                return Err(invalid(format!(
                    "two active cross sections share the angle {:.1}°",
                    w[0].angle.to_degrees()
                )));
            }
        }
        // The bend and the neck of internal sections run all round the spiral.
        let internal: Vec<InternalRound> = definitions
            .iter()
            .filter_map(|d| match &d.shape {
                SectionShape::RoundInternal(r) => Some(InternalRound {
                    ellipse_ratio: 1.0,
                    ..*r
                }),
                _ => None,
            })
            .collect();
        if !internal.is_empty() {
            if internal.len() != definitions.len() {
                return Err(invalid(
                    "internal cross sections cannot be mixed with other shapes: their bend \
                     and neck run all round the spiral"
                        .into(),
                ));
            }
            if internal.iter().any(|r| *r != internal[0]) {
                return Err(invalid(
                    "every internal cross section must share one bend and neck: only the \
                     ellipse ratio may change"
                        .into(),
                ));
            }
        }
        // Each shape must bound one region at any size.
        let mut signatures = Vec::new();
        for d in &definitions {
            for size in [0.05, 0.3, 1.0, 3.0, 10.0] {
                let o = d.shape.outline(b3, size * b3);
                if o.signed_area() <= 0.0 || !o.is_simple() {
                    return Err(invalid(format!(
                        "the cross section at {:.0}° crosses itself at H = {:.2}·b₃",
                        d.angle.to_degrees(),
                        size
                    )));
                }
            }
            let o = d.shape.outline(b3, b3);
            let top = o.breaks.iter().position(|&b| b == o.top).unwrap_or(1);
            signatures.push((o.breaks.len(), top));
        }
        let shared_breaks = signatures
            .iter()
            .all(|s| *s == signatures[0])
            .then_some(signatures[0]);
        Ok(Self {
            definitions,
            inlet_width: b3,
            stagnation_radius: rz,
            shared_breaks,
        })
    }

    /// The active definitions in order of wrap angle.
    pub fn definitions(&self) -> &[SectionDefinition] {
        &self.definitions
    }

    /// The internal shape of these sections, if they are internal: they all share its
    /// bend and neck.
    pub fn internal(&self) -> Option<&InternalRound> {
        match &self.definitions[0].shape {
            SectionShape::RoundInternal(r) => Some(r),
            _ => None,
        }
    }

    /// The bend and the neck of internal sections.
    pub fn bend(&self) -> Option<Bend> {
        self.internal().map(|r| r.bend(self.inlet_width))
    }

    /// The same shapes on an inlet of width `b3` at radius `rz`, as the outer channel of a
    /// double volute takes them beyond its rib.
    pub fn with_inlet(&self, b3: f64, rz: f64) -> Self {
        Self {
            inlet_width: b3,
            stagnation_radius: rz,
            ..self.clone()
        }
    }

    /// Points per run of the lofted outlines, the inlet last.
    fn counts(&self, factor: usize) -> Vec<usize> {
        let breaks = self.shared_breaks.map_or(3, |(n, _)| n);
        let wall = (WALL_POINTS / (breaks - 1)).max(MIN_RUN_POINTS);
        let mut counts = vec![wall * factor; breaks - 1];
        counts.push(INLET_POINTS * factor);
        counts
    }

    fn breaks_of(&self, outline: &Outline) -> Vec<usize> {
        match self.shared_breaks {
            Some(_) => outline.breaks.clone(),
            None => outline.basic_breaks(),
        }
    }

    /// The definitions shaping the section at wrap angle `angle` and the weight of the
    /// second, eased in between the two.
    fn neighbours(&self, angle: f64) -> (&SectionShape, &SectionShape, f64) {
        let defs = &self.definitions;
        let first = &defs[0];
        let last = &defs[defs.len() - 1];
        if angle <= first.angle {
            return (&first.shape, &first.shape, 0.0);
        }
        if angle >= last.angle {
            return (&last.shape, &last.shape, 0.0);
        }
        let i = defs.iter().rposition(|d| d.angle <= angle).unwrap_or(0);
        let (a, b) = (&defs[i], &defs[i + 1]);
        let x = (angle - a.angle) / (b.angle - a.angle);
        (&a.shape, &b.shape, x * x * (3.0 - 2.0 * x))
    }

    /// The outline at wrap angle `angle` scaled to `height`.
    pub fn outline(&self, angle: f64, height: f64) -> Outline {
        let b3 = self.inlet_width;
        let (a, b, w) = self.neighbours(angle);
        if w == 0.0 || a == b {
            return a.outline(b3, height);
        }
        let (oa, ob) = (a.outline(b3, height), b.outline(b3, height));
        let counts = self.counts(DENSE_FACTOR);
        let ra = oa.resample(&self.breaks_of(&oa), &counts);
        let rb = ob.resample(&self.breaks_of(&ob), &counts);
        Outline {
            points: ra
                .points
                .iter()
                .zip(&rb.points)
                .map(|(p, q)| p.lerp(*q, w))
                .collect(),
            ..ra
        }
    }

    /// The outline at `angle` and `height` resampled for lofting: the same number of
    /// points and runs for every section of the volute ([`Self::loft_breaks`]).
    pub fn loft_outline(&self, angle: f64, height: f64) -> Outline {
        let o = self.outline(angle, height);
        o.resample(&self.breaks_of(&o), &self.counts(1))
    }

    /// Indices where the runs of every lofted outline start.
    pub fn loft_breaks(&self) -> Vec<usize> {
        let mut at = 0;
        self.counts(1)
            .iter()
            .map(|c| {
                let start = at;
                at += c;
                start
            })
            .collect()
    }

    /// Points of every lofted outline.
    pub fn loft_points(&self) -> usize {
        self.counts(1).iter().sum()
    }

    pub fn properties(&self, angle: f64, height: f64) -> Result<SectionProperties, DesignError> {
        if height <= 0.0 {
            let rz = self.stagnation_radius;
            return Ok(SectionProperties {
                height: 0.0,
                area: 0.0,
                momentum: 0.0,
                centroid_radius: rz,
                outer_radius: rz,
                inner_radius: rz,
                width: self.inlet_width,
                wetted_perimeter: 0.0,
                wall_inverse_square: 0.0,
            });
        }
        self.outline(angle, height)
            .properties(self.stagnation_radius, height)
    }

    /// The height at `angle` whose `measure` reaches `target`; the measure grows with the
    /// height.
    fn solve(
        &self,
        angle: f64,
        target: f64,
        measure: impl Fn(&SectionProperties) -> f64,
    ) -> Result<f64, DesignError> {
        if target <= 0.0 {
            return Ok(0.0);
        }
        let failure = std::cell::RefCell::new(None);
        let f = |h: f64| match self.properties(angle, h) {
            Ok(p) => measure(&p) - target,
            Err(e) => {
                failure.borrow_mut().get_or_insert(e);
                f64::NAN
            }
        };
        let scale = self.stagnation_radius.max(self.inlet_width);
        let mut hi = 0.1 * self.inlet_width;
        loop {
            let value = f(hi);
            if let Some(e) = failure.borrow_mut().take() {
                return Err(e);
            }
            if value >= 0.0 {
                break;
            }
            hi *= 2.0;
            if hi > 100.0 * scale {
                return Err(DesignError::Infeasible(
                    "the volute cross section does not converge".into(),
                ));
            }
        }
        let h = roots::brent(f, 0.0, hi, 1e-14 * hi)
            .map_err(|e| DesignError::Infeasible(e.to_string()))?;
        match failure.into_inner() {
            Some(e) => Err(e),
            None => Ok(h),
        }
    }

    /// Height at `angle` whose ∫ b/r dr is `k` (Gülich eq. 7.24).
    pub fn height_for_momentum(&self, angle: f64, k: f64) -> Result<f64, DesignError> {
        self.solve(angle, k, |p| p.momentum)
    }

    /// Height at `angle` of the section of area `area`.
    pub fn height_for_area(&self, angle: f64, area: f64) -> Result<f64, DesignError> {
        self.solve(angle, area, |p| p.area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use tp_math::quad::gauss_legendre;

    const B3: f64 = 0.03;
    const RZ: f64 = 0.1;

    fn props(shape: &SectionShape, h: f64) -> SectionProperties {
        shape.outline(B3, h).properties(RZ, h).unwrap()
    }

    fn every_shape() -> Vec<SectionShape> {
        let sides = [
            SideShape::Rectangular { tangent_angle: 0.0 },
            SideShape::Rectangular {
                tangent_angle: 10f64.to_radians(),
            },
            SideShape::trapezoidal(30f64.to_radians()),
            SideShape::Trapezoidal {
                cone_angle: 20f64.to_radians(),
                tangent_angle: 5f64.to_radians(),
            },
            SideShape::Round { ellipse_ratio: 1.0 },
            SideShape::Round { ellipse_ratio: 0.6 },
            SideShape::Bezier(BezierSide::default()),
            SideShape::Bezier(BezierSide {
                base: BezierBase::Trapezoid,
                position: InnerPosition::Absolute,
                point1: 0.005,
                point3: 0.005,
                point2: [0.1, 0.2],
                ..BezierSide::default()
            }),
            SideShape::LineSegments {
                points: default_segments(),
            },
            SideShape::RadiusBased(RadiusSide::default()),
        ];
        let mut shapes: Vec<SectionShape> =
            sides.iter().cloned().map(SectionShape::symmetric).collect();
        shapes.push(SectionShape::Sides {
            left: sides[2].clone(),
            right: Some(sides[0].clone()),
        });
        for strictly_external in [true, false] {
            for open_to_right in [false, true] {
                for square_on_top in [false, true] {
                    shapes.push(SectionShape::RoundExternal(ExternalRound {
                        ellipse_ratio: 1.2,
                        strictly_external,
                        open_to_right,
                        square_on_top,
                    }));
                }
            }
        }
        shapes
    }

    #[test]
    fn rectangles_and_trapezoids_are_exact() {
        let h = 0.04;
        let p = props(&SectionShape::rectangular(), h);
        assert_relative_eq!(p.area, B3 * h, max_relative = 1e-14);
        // Gülich eq. T7.7.8: ∫ b/r dr = b₃·ln(1 + a/r'_z).
        assert_relative_eq!(p.momentum, B3 * (1.0 + h / RZ).ln(), max_relative = 1e-14);
        assert_relative_eq!(p.centroid_radius, RZ + h / 2.0, max_relative = 1e-14);
        assert_relative_eq!(p.outer_radius, RZ + h);
        assert_relative_eq!(p.wetted_perimeter, B3 + 2.0 * h, max_relative = 1e-14);
        // Trapezoid: b = b₃ + 2t·(r − r'_z).
        let delta = 25f64.to_radians();
        let t = delta.tan();
        let p = props(&SectionShape::trapezoidal(delta), h);
        assert_relative_eq!(p.area, B3 * h + t * h * h, max_relative = 1e-13);
        let expected = (B3 - 2.0 * RZ * t) * ((RZ + h) / RZ).ln() + 2.0 * t * h;
        assert_relative_eq!(p.momentum, expected, max_relative = 1e-12);
        assert_relative_eq!(p.width, B3 + 2.0 * t * h, max_relative = 1e-13);
    }

    #[test]
    fn round_sections_pass_through_the_inlet_points() {
        let h = 0.05;
        for k in [1.0, 0.5, 2.0] {
            let shape = SectionShape::symmetric(SideShape::Round { ellipse_ratio: k });
            let o = shape.outline(B3, h);
            assert_eq!(o.points[0], DVec2::new(-B3 / 2.0, 0.0));
            assert_eq!(o.points[o.inlet_end], DVec2::new(B3 / 2.0, 0.0));
            assert_eq!(o.points[o.top], DVec2::new(0.0, h));
            // The elliptic segment above the chord, by integration of its width.
            let a = (h * h + (B3 / 2.0 / k).powi(2)) / (2.0 * h);
            let yc = h - a;
            let width = |y: f64| 2.0 * k * (a * a - (y - yc).powi(2)).max(0.0).sqrt();
            let area = gauss_legendre(width, 0.0, h, 400);
            let momentum = gauss_legendre(|y| width(y) / (RZ + y), 0.0, h, 400);
            let p = o.properties(RZ, h).unwrap();
            assert_relative_eq!(p.area, area, max_relative = 2e-4);
            assert_relative_eq!(p.momentum, momentum, max_relative = 2e-4);
        }
    }

    #[test]
    fn a_round_section_on_a_narrow_inlet_is_the_circle_of_eq_t7_7_7() {
        // With b₃ → 0 the section is a whole circle touching r'_z: ∫ b/r dr =
        // 2π·(R − √(R² − ρ²)) about its centre R = r'_z + ρ.
        let d = 0.04;
        let o = SectionShape::round().outline(1e-7, d);
        let p = o.properties(RZ, d).unwrap();
        let (rho, centre) = (d / 2.0, RZ + d / 2.0);
        assert_relative_eq!(
            p.momentum,
            TAU * (centre - (centre * centre - rho * rho).sqrt()),
            max_relative = 2e-4
        );
        assert_relative_eq!(p.area, PI * rho * rho, max_relative = 2e-4);
    }

    #[test]
    fn every_shape_is_one_region_that_grows_with_its_height() {
        for shape in every_shape() {
            let mut previous = (0.0, 0.0);
            // Up to 0.9·r'_z: the round section that also grows inward reaches as far
            // below r'_z as above it.
            for i in 1..=45 {
                let h = 0.002 * f64::from(i);
                let o = shape.outline(B3, h);
                assert!(o.is_simple(), "{shape:?} at {h}");
                assert_eq!(o.points[0], DVec2::new(-B3 / 2.0, 0.0), "{shape:?}");
                assert_eq!(o.points[o.inlet_end], DVec2::new(B3 / 2.0, 0.0));
                assert_eq!(o.inlet_end, o.points.len() - 1);
                let p = o.properties(RZ, h).unwrap();
                assert!(p.area > previous.0, "{shape:?} at {h}");
                assert!(p.momentum > previous.1, "{shape:?} at {h}");
                assert_relative_eq!(p.outer_radius - RZ, h, max_relative = 1e-9);
                previous = (p.area, p.momentum);
            }
        }
    }

    #[test]
    fn breaks_do_not_change_with_the_height() {
        for shape in every_shape() {
            let reference = shape.outline(B3, B3);
            for h in [0.001, 0.01, 0.1] {
                let o = shape.outline(B3, h);
                assert_eq!(o.breaks.len(), reference.breaks.len(), "{shape:?} at {h}");
                assert!(o.breaks.windows(2).all(|w| w[0] < w[1]));
            }
        }
    }

    #[test]
    fn sides_scale_about_their_inlet_points() {
        let shape = SectionShape::symmetric(SideShape::LineSegments {
            points: vec![SegmentPoint {
                out: 0.3,
                up: 0.6,
                radius: 0.0,
            }],
        });
        for h in [0.01, 0.05] {
            let o = shape.outline(B3, h);
            // The sharp corner at (b₃/2 + 0.3·H, 0.6·H) on both sides.
            assert_eq!(o.breaks.len(), 5);
            let corner = o.points[o.breaks[1]];
            assert_relative_eq!(corner.x, -(B3 / 2.0 + 0.3 * h), epsilon = 1e-15);
            assert_relative_eq!(corner.y, 0.6 * h, epsilon = 1e-15);
        }
    }

    #[test]
    fn dragging_moves_the_control_points() {
        let h = 0.05;
        for position in [InnerPosition::Relative, InnerPosition::Absolute] {
            let mut b = BezierSide {
                base: BezierBase::Trapezoid,
                tangent_angle: 5f64.to_radians(),
                position,
                point1: if position == InnerPosition::Relative {
                    0.4
                } else {
                    0.01
                },
                ..BezierSide::default()
            };
            let before = b.control_points(B3, h);
            // Onto its own line: point 1 stays put, point 3 goes where it is dragged.
            let original = b;
            b.drag(1, before[1] + DVec2::new(0.0, 0.0), B3, h);
            assert_relative_eq!(b.point1, original.point1, max_relative = 1e-9);
            let on_top_line = before[4].lerp(before[5], 0.25);
            let normal = (before[5] - before[4]).perp().normalize();
            b.drag(3, on_top_line + normal * 0.002, B3, h);
            assert!(b.control_points(B3, h)[3].distance(on_top_line) < 1e-12);
            // Point 2 anywhere near S.
            let target = before[5] + DVec2::new(0.004, -0.003);
            b.drag(2, target, B3, h);
            assert!(b.control_points(B3, h)[2].distance(target) < 1e-12);
        }
        // A line-segment corner put back where it was.
        let points = default_segments();
        let v = segment_vertices(&points, B3, h);
        let p = SegmentPoint::at(v[2], B3, h, points[1].radius);
        assert_relative_eq!(p.out, points[1].out, max_relative = 1e-12);
        assert_relative_eq!(p.up, points[1].up, max_relative = 1e-12);
    }

    #[test]
    fn a_tangent_angle_tilts_the_top() {
        let lambda = 10f64.to_radians();
        let h = 0.05;
        let o = SectionShape::symmetric(SideShape::Rectangular {
            tangent_angle: lambda,
        })
        .outline(B3, h);
        let corner = o.points[o.breaks[1]];
        assert_relative_eq!(corner.y, h - B3 / 2.0 * lambda.tan(), epsilon = 1e-15);
        // On a low section the corner stays above a tenth of the height.
        let o = SectionShape::symmetric(SideShape::Rectangular {
            tangent_angle: MAX_TANGENT_ANGLE_DEG.to_radians(),
        })
        .outline(B3, 0.002);
        assert!(o.points[o.breaks[1]].y >= 0.1 * 0.002 - 1e-15);
    }

    #[test]
    fn the_radius_based_side_has_its_base_cone_and_main_arc() {
        let side = RadiusSide {
            base_height: Param::Manual(0.003),
            base_radius: Param::Manual(0.002),
            cone_angle: 30f64.to_radians(),
            corner_radius: Param::Manual(0.006),
            main_radius: Param::Manual(0.03),
        };
        let shape = SectionShape::symmetric(SideShape::RadiusBased(side));
        let h = 0.06;
        let o = shape.outline(B3, h);
        let left: Vec<DVec2> = o.points[..=o.top].to_vec();
        // The radial base up to its fillet.
        assert_relative_eq!(left[1].x, -B3 / 2.0, epsilon = 1e-15);
        // Points near the top lie on the main arc about (0, H − R).
        let m = DVec2::new(0.0, h - 0.03);
        for p in &left[left.len() - 5..] {
            assert_relative_eq!(p.distance(m), 0.03, max_relative = 1e-9);
        }
        // Tangent continuity: no turn sharper than the sampling step.
        for w in left.windows(3) {
            let (a, b) = ((w[1] - w[0]).normalize(), (w[2] - w[1]).normalize());
            assert!(a.dot(b) > (3.0 * ARC_STEP).cos(), "{w:?}");
        }
        // Its area changes smoothly through the changes of construction: each step adds
        // about the width of the top times the step.
        let dh = 0.00025;
        let mut previous = props(&shape, dh).area;
        for i in 2..400 {
            let h = dh * f64::from(i);
            let area = props(&shape, h).area;
            let step = area - previous;
            assert!(
                step > 0.0 && step < 2.0 * dh * (B3 + 2.0 * h),
                "{h}: {step}"
            );
            previous = area;
        }
    }

    #[test]
    fn the_external_round_section_grows_toward_its_open_side() {
        let round = ExternalRound {
            ellipse_ratio: 1.0,
            ..ExternalRound::default()
        };
        let h = 0.06;
        let o = SectionShape::RoundExternal(round).outline(B3, h);
        let (lo, hi) = o.points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
        // It touches the closed wall at +b₃/2 and grows 2·k·H toward −z.
        assert_relative_eq!(hi, B3 / 2.0, epsilon = 1e-12);
        assert_relative_eq!(lo, B3 / 2.0 - 2.0 * h, epsilon = 1e-9);
        assert!(o.points.iter().all(|p| p.y >= -1e-15));
        assert_relative_eq!(
            props(&SectionShape::RoundExternal(round), h).area,
            PI * h * h / 2.0,
            max_relative = 1e-3
        );
        // Mirrored toward +z.
        let right = SectionShape::RoundExternal(ExternalRound {
            open_to_right: true,
            ..round
        });
        let m = right.outline(B3, h);
        let hi = m.points.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        assert_relative_eq!(hi, -B3 / 2.0 + 2.0 * h, epsilon = 1e-9);
        assert_relative_eq!(
            props(&right, h).area,
            props(&SectionShape::RoundExternal(round), h).area,
            max_relative = 1e-12
        );
        // Not strictly external: it reaches below r'_z beside the inlet.
        let low = SectionShape::RoundExternal(ExternalRound {
            strictly_external: false,
            ..round
        });
        let o = low.outline(B3, h);
        let lowest = o.points.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        assert!(lowest < -0.5 * h, "{lowest}");
        for p in o.points.iter().filter(|p| p.y < -1e-12) {
            assert!(p.x <= -B3 / 2.0 + 1e-12, "{p:?}");
        }
        // A square top corner adds area on the closed side.
        let square = SectionShape::RoundExternal(ExternalRound {
            square_on_top: true,
            ..round
        });
        let extra = (1.0 - PI / 4.0) * h * h;
        assert_relative_eq!(
            props(&square, h).area - props(&SectionShape::RoundExternal(round), h).area,
            extra,
            max_relative = 1e-3
        );
    }

    fn internal_shapes() -> Vec<InternalRound> {
        let mut out = Vec::new();
        for inner_bend in [BendWall::Sharp, BendWall::Straight, BendWall::Round] {
            for outer_bend in [BendWall::Sharp, BendWall::Straight, BendWall::Round] {
                for open_to_right in [true, false] {
                    out.push(InternalRound {
                        inner_bend,
                        outer_bend,
                        open_to_right,
                        ellipse_ratio: 0.8,
                        bend_ratio: 1.5,
                        area_ratio: 0.8,
                        neck_width: Param::Manual(0.01),
                        side_gap: Param::Auto,
                    });
                }
            }
        }
        out
    }

    #[test]
    fn the_bend_turns_the_inlet_to_the_neck() {
        let c = B3 / 2.0;
        for round in internal_shapes() {
            let b = round.bend(B3);
            let side = if round.open_to_right { 1.0 } else { -1.0 };
            // The walls start at the inlet points and end on the face, a neck high.
            let (outer, inner) = (&b.outer, &b.inner);
            assert_eq!(outer[0][0], DVec2::new(-side * c, 0.0), "{round:?}");
            assert_eq!(inner[0][0], DVec2::new(side * c, 0.0), "{round:?}");
            let end = |w: &Vec<Vec<DVec2>>| *w.last().unwrap().last().unwrap();
            assert!(end(outer).distance(DVec2::new(b.face, b.top)) < 1e-15);
            assert!(end(inner).distance(DVec2::new(b.face, b.floor)) < 1e-15);
            assert_relative_eq!(b.top - b.floor, 0.8 * B3, epsilon = 1e-15);
            // The face lies beyond the bend, a neck width past it.
            assert!(side * b.face >= c + 0.01 - 1e-15, "{round:?}");
            let floor = match round.inner_bend {
                BendWall::Sharp => 0.0,
                _ => 1.5 * c,
            };
            assert_relative_eq!(b.floor, floor, epsilon = 1e-15);
            let region = b.region();
            let o = Outline {
                top: 1,
                inlet_end: region.len() - 1,
                breaks: vec![0, 1, region.len() - 1],
                points: region,
            };
            assert!(o.is_simple() && o.signed_area() > 0.0, "{round:?}");
        }
    }

    #[test]
    fn the_internal_spiral_grows_from_the_face_inward() {
        for round in internal_shapes() {
            let shape = SectionShape::RoundInternal(round);
            let b = round.bend(B3);
            let away = if round.open_to_right { 1.0 } else { -1.0 };
            let mut previous = 0.0;
            for i in 1..=30 {
                let h = 0.002 * f64::from(i);
                let o = shape.outline(B3, h);
                assert!(o.is_simple(), "{round:?} at {h}");
                // It starts and ends on the face, between the floor and the top of the neck.
                let (first, last) = (o.points[0], o.points[o.inlet_end]);
                assert_relative_eq!(first.x, b.face, epsilon = 1e-12);
                assert_relative_eq!(last.x, b.face, epsilon = 1e-12);
                let mut ends = [first.y, last.y];
                ends.sort_by(f64::total_cmp);
                assert_relative_eq!(ends[0], b.floor, epsilon = 1e-12);
                assert_relative_eq!(ends[1], b.top, epsilon = 1e-12);
                for p in &o.points {
                    // Away from the impeller and never above the top of the neck.
                    assert!(away * (p.x - b.face) >= -1e-12, "{round:?} at {h}: {p:?}");
                    assert!(p.y <= b.top + 1e-12);
                }
                // It reaches h out from the face.
                let depth = o
                    .points
                    .iter()
                    .map(|p| away * (p.x - b.face))
                    .fold(0.0, f64::max);
                assert_relative_eq!(depth, h, max_relative = 1e-9);
                let p = o.properties(RZ, h).unwrap();
                assert!(p.area > previous, "{round:?} at {h}");
                assert_relative_eq!(p.outer_radius, RZ + b.top, epsilon = 1e-12);
                previous = p.area;
            }
            // Large sections reach inward, below the inlet radius.
            let deep = shape.outline(B3, 0.05).properties(RZ, 0.05).unwrap();
            assert!(deep.inner_radius < RZ - 0.02, "{round:?}");
        }
    }

    #[test]
    fn internal_sections_share_one_bend() {
        let round = InternalRound::default();
        let def = |angle: f64, shape: SectionShape| SectionDefinition {
            angle,
            active: true,
            shape,
        };
        let with = |sections| CrossSections::new(&CrossSectionInput { sections }, B3, RZ);
        let ok = with(vec![
            def(
                PI,
                SectionShape::RoundInternal(InternalRound {
                    ellipse_ratio: 0.7,
                    ..round
                }),
            ),
            def(TAU, SectionShape::RoundInternal(round)),
        ])
        .unwrap();
        assert_eq!(ok.bend(), Some(round.bend(B3)));
        assert!(ok.internal().is_some());
        assert!(
            with(vec![
                def(PI, SectionShape::default()),
                def(TAU, SectionShape::RoundInternal(round)),
            ])
            .is_err()
        );
        assert!(
            with(vec![
                def(
                    PI,
                    SectionShape::RoundInternal(InternalRound {
                        area_ratio: 1.2,
                        ..round
                    })
                ),
                def(TAU, SectionShape::RoundInternal(round)),
            ])
            .is_err()
        );
        assert!(
            with(vec![def(
                TAU,
                SectionShape::RoundInternal(InternalRound {
                    bend_ratio: 9.0,
                    ..round
                })
            )])
            .is_err()
        );
        // The automatic neck is half the inlet wide.
        let b = round.bend(B3);
        assert_relative_eq!(b.face, B3 / 2.0 + B3 / 2.0 + 0.5 * B3, epsilon = 1e-15);
    }

    #[test]
    fn definitions_blend_between_their_angles() {
        let input = CrossSectionInput {
            sections: vec![
                SectionDefinition {
                    angle: PI,
                    active: true,
                    shape: SectionShape::rectangular(),
                },
                SectionDefinition {
                    angle: 1.5 * PI,
                    active: true,
                    shape: SectionShape::trapezoidal(40f64.to_radians()),
                },
                SectionDefinition {
                    angle: TAU,
                    active: false,
                    shape: SectionShape::round(),
                },
            ],
        };
        let c = CrossSections::new(&input, B3, RZ).unwrap();
        assert_eq!(c.definitions().len(), 2);
        let h = 0.04;
        let area = |angle: f64| c.properties(angle, h).unwrap().area;
        let rect = props(&SectionShape::rectangular(), h).area;
        let trap = props(&SectionShape::trapezoidal(40f64.to_radians()), h).area;
        assert_relative_eq!(area(0.5), rect, max_relative = 1e-12);
        assert_relative_eq!(area(PI), rect, max_relative = 1e-12);
        assert_relative_eq!(area(TAU), trap, max_relative = 1e-12);
        // Halfway the straight walls blend corner to corner: the mean area.
        assert_relative_eq!(area(1.25 * PI), 0.5 * (rect + trap), max_relative = 1e-9);
        let mid = area(1.1 * PI);
        assert!(mid > rect && mid < trap);
        // Both share their corners, so the lofts keep them.
        assert_eq!(c.loft_breaks().len(), 5);
        let lofted = c.loft_outline(1.3 * PI, h);
        assert_eq!(lofted.points.len(), c.loft_points());
        assert_eq!(lofted.breaks, c.loft_breaks());
        // A round section among them keeps only the inlet points and the top.
        let mut mixed = input.clone();
        mixed.sections[2].active = true;
        let c = CrossSections::new(&mixed, B3, RZ).unwrap();
        assert_eq!(c.loft_breaks().len(), 3);
        let o = c.outline(1.75 * PI, h);
        assert!(o.is_simple());
    }

    #[test]
    fn heights_follow_the_momentum_and_the_area() {
        let c = CrossSections::new(&CrossSectionInput::default(), B3, RZ).unwrap();
        let k = 0.004;
        let h = c.height_for_momentum(1.0, k).unwrap();
        assert_relative_eq!(
            c.properties(1.0, h).unwrap().momentum,
            k,
            max_relative = 1e-12
        );
        let h = c.height_for_area(1.0, 1e-3).unwrap();
        assert_relative_eq!(
            c.properties(1.0, h).unwrap().area,
            1e-3,
            max_relative = 1e-12
        );
        assert_eq!(c.height_for_area(1.0, 0.0).unwrap(), 0.0);
    }

    #[test]
    fn rejects_impossible_definitions() {
        let with = |sections: Vec<SectionDefinition>| {
            CrossSections::new(&CrossSectionInput { sections }, B3, RZ)
        };
        let def = |angle: f64, active: bool, shape: SectionShape| SectionDefinition {
            angle,
            active,
            shape,
        };
        assert!(with(vec![def(TAU, false, SectionShape::default())]).is_err());
        assert!(
            with(vec![
                def(PI, true, SectionShape::default()),
                def(PI, true, SectionShape::round()),
            ])
            .is_err()
        );
        assert!(with(vec![def(7.0, true, SectionShape::default())]).is_err());
        assert!(with(vec![def(TAU, true, SectionShape::trapezoidal(1.2))]).is_err());
        // Corners that loop back across each other.
        let crossing = SectionShape::symmetric(SideShape::LineSegments {
            points: vec![
                SegmentPoint {
                    out: 1.0,
                    up: 0.8,
                    radius: 0.0,
                },
                SegmentPoint {
                    out: -0.5,
                    up: 0.2,
                    radius: 0.0,
                },
                SegmentPoint {
                    out: 1.2,
                    up: 0.5,
                    radius: 0.0,
                },
            ],
        });
        assert!(with(vec![def(TAU, true, crossing)]).is_err());
    }

    #[test]
    fn old_project_files_keep_their_shape() {
        let old: CrossSectionInput =
            serde_json::from_str(r#"{"shape":{"type":"trapezoidal","flank_angle":0.4}}"#).unwrap();
        assert_eq!(
            old,
            CrossSectionInput::single(SectionShape::trapezoidal(0.4))
        );
        let old: CrossSectionInput =
            serde_json::from_str(r#"{"shape":{"type":"circular"}}"#).unwrap();
        assert_eq!(old, CrossSectionInput::single(SectionShape::round()));
        let empty: CrossSectionInput = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, CrossSectionInput::default());
        let input = CrossSectionInput {
            sections: vec![
                SectionDefinition {
                    angle: 1.0,
                    active: true,
                    shape: SectionShape::symmetric(SideShape::RadiusBased(RadiusSide {
                        main_radius: Param::Manual(0.02),
                        ..RadiusSide::default()
                    })),
                },
                SectionDefinition::at_throat(SectionShape::RoundExternal(ExternalRound::default())),
            ],
        };
        let text = serde_json::to_string(&input).unwrap();
        assert_eq!(
            serde_json::from_str::<CrossSectionInput>(&text).unwrap(),
            input
        );
    }
}
