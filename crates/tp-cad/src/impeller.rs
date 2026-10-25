//! Solids of a radial impeller and its flow domain.
//!
//! - **Channel**: the meridional region between hub and shroud revolved through 360°.
//! - **Blade**: B-spline loft through the closed blade sections (pressure side, nose,
//!   suction side, trailing edge), extended slightly beyond hub and shroud so that
//!   boolean operations never meet coincident faces.
//! - **Flow domain**: channel minus all blades.
//! - **Impeller**: hub disk and shroud (one profile of revolution less the channel) ∪
//!   (blades ∩ channel), less the balance holes.
//! - **Fillets** where the blades meet hub and shroud, on the impeller and on the flow
//!   domain alike.
//! - **Hub disk and shroud**: the closed material contours of the meridional design
//!   (`tp_design::impeller::material`), automatic or drawn.
//!
//! Coordinates: z along the axis in the inflow direction, the meridional profile drawn
//! in the xz plane (x = r), all in millimetres.

use std::f64::consts::PI;

use tp_design::impeller::ImpellerGeometry;
use tp_design::impeller::back_vanes::{BackVanes, VaneShape};
use tp_design::impeller::blade::BladeSection;
use tp_design::impeller::duct::InletDuct;
use tp_design::impeller::material::collar_inner_radius;
use tp_design::impeller::seals::Seal;
use tp_design::impeller::wall::{SegmentKind, Wall};
use tp_math::{DVec2, DVec3};

use crate::CadError;
use crate::occ::{self, Shape};

/// Model units per metre.
const MM: f64 = 1000.0;
/// Below this distance (mm) two section points are taken as one (closed blade edge).
const CLOSED_EDGE: f64 = 1e-6;

/// Balance holes through the hub disk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceHoleOptions {
    pub count: u32,
    /// m
    pub diameter: f64,
    /// Diameter of the annular seal on the rear shroud d_s2, m: the holes lie inside it.
    pub seal_diameter: f64,
}

/// The balance holes drawn: `count` holes of `diameter` on `pitch_diameter`, m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceHoles {
    pub count: u32,
    pub diameter: f64,
    pub pitch_diameter: f64,
}

/// Options of the CAD model. The material of hub disk and shroud comes from the
/// meridional design.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CadOptions {
    /// Fillet radius where the blades meet hub and shroud, m. `None`: half the blade
    /// thickness; zero leaves the edges sharp.
    pub fillet_radius: Option<f64>,
    pub balance_holes: Option<BalanceHoleOptions>,
    /// Points used to interpolate hub and shroud contours.
    pub contour_points: usize,
    /// Extension of the blade beyond hub and shroud, as a fraction of the distance
    /// between neighbouring blade sections.
    pub blade_extension: f64,
}

impl Default for CadOptions {
    fn default() -> Self {
        Self {
            fillet_radius: None,
            balance_holes: None,
            contour_points: 41,
            blade_extension: 0.5,
        }
    }
}

/// The four replaceable wear rings of the annular seals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingKind {
    FrontImpeller,
    FrontCasing,
    RearImpeller,
    RearCasing,
}

impl RingKind {
    pub const ALL: [RingKind; 4] = [
        RingKind::FrontImpeller,
        RingKind::FrontCasing,
        RingKind::RearImpeller,
        RingKind::RearCasing,
    ];

    /// Name of the exported file, without extension.
    pub fn file_stem(self) -> &'static str {
        match self {
            RingKind::FrontImpeller => "front-impeller-ring",
            RingKind::FrontCasing => "front-casing-ring",
            RingKind::RearImpeller => "rear-impeller-ring",
            RingKind::RearCasing => "rear-casing-ring",
        }
    }

    /// English name for reports.
    pub fn label(self) -> &'static str {
        match self {
            RingKind::FrontImpeller => "Front impeller ring",
            RingKind::FrontCasing => "Front casing ring",
            RingKind::RearImpeller => "Rear impeller ring",
            RingKind::RearCasing => "Rear casing ring",
        }
    }
}

/// One wear ring: a rectangle of the meridional plane revolved about the axis.
#[derive(Debug)]
pub struct WearRing {
    pub kind: RingKind,
    pub solid: Shape,
}

/// The rectangle of a wear ring in the meridional plane, m: radii and axial span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RingSpan {
    pub kind: RingKind,
    pub inner: f64,
    pub outer: f64,
    pub z_low: f64,
    pub z_high: f64,
}

/// The axial run (low, high) of the line of `wire` on radius `seat` that is `length` long:
/// the seat of an impeller ring.
fn seat_run(wire: &Wall, seat: f64, length: f64) -> Option<(f64, f64)> {
    let tol = 1e-9 * length.max(seat);
    wire.segments().iter().find_map(|s| {
        let (a, b) = (s.start(), s.end());
        (s.kind() == SegmentKind::Line
            && (a.y - seat).abs() <= tol
            && (b.y - seat).abs() <= tol
            && ((a.x - b.x).abs() - length).abs() <= tol)
            .then(|| (a.x.min(b.x), a.x.max(b.x)))
    })
}

/// Axial position of the back face of the hub disk at radius `r`: the first curved
/// piece of `wire` that crosses it.
fn back_face_at(wire: &Wall, r: f64) -> Option<f64> {
    wire.segments()
        .iter()
        .filter(|s| s.kind() != SegmentKind::Line)
        .find_map(|s| {
            let points: Vec<DVec2> = (0..=400).map(|i| s.eval(f64::from(i) / 400.0)).collect();
            points.windows(2).find_map(|w| {
                ((w[0].y - r) * (w[1].y - r) <= 0.0 && (w[0].y - w[1].y).abs() > 0.0).then(|| {
                    let f = (r - w[0].y) / (w[1].y - w[0].y);
                    w[0].x + (w[1].x - w[0].x) * f
                })
            })
        })
}

/// Least radial or axial extent of a wear ring that can be revolved, m.
const MIN_RING_EXTENT: f64 = 1e-5;

/// The wear rings the design asks for, as rectangles of the meridional plane, with what
/// could not be placed. The rings of a wall drawn by hand are left out: its seat is not
/// ours to know.
pub(crate) fn ring_spans(g: &ImpellerGeometry) -> (Vec<RingSpan>, Vec<String>) {
    let c = &g.contour;
    let mut spans = Vec::new();
    let mut warnings = Vec::new();
    let skipped = |what: &str, wall: &str| {
        format!(
            "the {what} wear ring is left out: the {wall} wire is drawn by hand, so its seat is not known"
        )
    };
    let mut push = |kind, inner, outer, run: (f64, f64)| {
        spans.push(RingSpan {
            kind,
            inner,
            outer,
            z_low: run.0,
            z_high: run.1,
        });
    };
    let casing_ring = |seal: &Seal| {
        seal.casing_ring
            .map(|_| (seal.radius + seal.clearance, seal.casing_seat))
    };

    if let Some(seal) = &c.seals.front {
        let run = if c.shroud_material_drawn {
            None
        } else {
            seat_run(&c.shroud_material, seal.impeller_seat, seal.length)
        };
        if seal.impeller_ring.is_some() {
            match run {
                Some(run) => push(RingKind::FrontImpeller, seal.impeller_seat, seal.radius, run),
                None if c.shroud_material_drawn => warnings.push(skipped("front impeller", "shroud")),
                None => warnings.push(
                    "the front impeller wear ring is left out: the shroud wire has no seat on the ring radius"
                        .to_string(),
                ),
            }
        }
        if let Some((inner, outer)) = casing_ring(seal) {
            let values = &c.chambers.values;
            let measured = (c.chambers.enabled && values.seal_length > 0.0).then(|| {
                let end = c.chambers.front_casing.end().x;
                (end, end + values.seal_length)
            });
            if measured.is_none() && c.chambers.enabled && run.is_some() {
                warnings.push(
                    "the front casing ring follows the impeller seat run: the chamber wall does not run on the casing seat"
                        .to_string(),
                );
            }
            match measured.or(run) {
                Some(run) => push(RingKind::FrontCasing, inner, outer, run),
                None if c.shroud_material_drawn => warnings.push(skipped("front casing", "shroud")),
                None => warnings.push(
                    "the front casing wear ring is left out: the shroud wire has no seat on the ring radius"
                        .to_string(),
                ),
            }
        }
    }
    if let Some(seal) = &c.seals.rear {
        let run = if c.hub_material_drawn {
            None
        } else {
            seat_run(&c.hub_material, seal.impeller_seat, seal.length)
        };
        for (kind, name, ring) in [
            (
                RingKind::RearImpeller,
                "rear impeller",
                seal.impeller_ring
                    .map(|_| (seal.impeller_seat, seal.radius)),
            ),
            (RingKind::RearCasing, "rear casing", casing_ring(seal)),
        ] {
            let Some((inner, outer)) = ring else { continue };
            // The back face of the disk rises toward the rim: a ring on the collar starts
            // where the face reaches its outer radius, so it never enters the disk.
            let start = run.map(|(low, high)| match back_face_at(&c.hub_material, outer) {
                Some(face) => (low.max(face), high),
                None => {
                    warnings.push(format!(
                        "the back face of the disk does not reach the {name} wear ring radius: the ring spans the whole collar"
                    ));
                    (low, high)
                }
            });
            match start {
                Some((low, high)) if high - low <= 1e-9 * high.abs().max(1e-3) => {
                    warnings.push(format!(
                        "the {name} wear ring is left out: the back face of the disk leaves no room for it on the collar"
                    ))
                }
                Some(run) => push(kind, inner, outer, run),
                None if c.hub_material_drawn => warnings.push(skipped(name, "hub disk")),
                None => warnings.push(format!(
                    "the {name} wear ring is left out: the hub disk wire has no collar seat"
                )),
            }
        }
    }
    // A ring with no radial or axial extent cannot be drawn: it is left out.
    spans.retain(|span| {
        let thin = span.outer - span.inner < MIN_RING_EXTENT;
        let short = span.z_high - span.z_low < MIN_RING_EXTENT;
        if thin || short {
            warnings.push(format!(
                "the {} wear ring is left out: it has no {} extent",
                span.kind.label().to_lowercase().trim_end_matches(" ring"),
                if thin { "radial" } else { "axial" }
            ));
        }
        !(thin || short)
    });
    // The front casing ring (L − s_ax) and the rear rings (the slope of the back face) are
    // shorter than their seals by design; only a ring under half the seal is no longer a
    // working seal.
    for span in &spans {
        let seal = match span.kind {
            RingKind::FrontImpeller | RingKind::FrontCasing => c.seals.front,
            RingKind::RearImpeller | RingKind::RearCasing => c.seals.rear,
        };
        if let Some(seal) = seal {
            let length = span.z_high - span.z_low;
            if length < 0.5 * seal.length {
                warnings.push(format!(
                    "the {} is {:.1} mm long, shorter than the {:.1} mm of its seal",
                    span.kind.label().to_lowercase(),
                    length * 1e3,
                    seal.length * 1e3
                ));
            }
        }
    }
    (spans, warnings)
}

/// CAD solids of one impeller, in millimetres.
#[derive(Debug)]
pub struct ImpellerCad {
    /// One blade, extended beyond hub and shroud.
    pub blade: Shape,
    /// Revolved channel between hub and shroud.
    pub channel: Shape,
    /// Flow domain: channel minus blades.
    pub fluid: Shape,
    /// Blades trimmed to the channel.
    pub blades: Shape,
    /// Impeller material: hub disk, shroud and blades.
    pub impeller: Shape,
    /// Fluid of the side chambers between the impeller and the casing, when designed.
    /// The wear rings are taken out of it.
    pub side_chambers: Option<Shape>,
    /// The wear rings of the annular seals, each its own solid.
    pub rings: Vec<WearRing>,
    /// The back vanes on the rear face of the hub disk, already fused into `impeller`;
    /// `None` when the design has none or they have no room.
    pub back_vanes: Option<Shape>,
    /// Wall thickness used, m.
    pub wall_thickness: f64,
    /// Bore and boss diameters, m.
    pub bore_diameter: f64,
    pub boss_diameter: f64,
    /// Fillet radius at the blade roots, m; zero for sharp edges.
    pub fillet_radius: f64,
    pub balance_holes: Option<BalanceHoles>,
    /// What could not be built as asked.
    pub warnings: Vec<String>,
}

impl ImpellerCad {
    /// Volume of the flow domain, m³.
    pub fn fluid_volume(&self) -> f64 {
        self.fluid.volume() * 1e-9
    }

    /// Volume of the impeller material, m³.
    pub fn impeller_volume(&self) -> f64 {
        self.impeller.volume() * 1e-9
    }

    /// Volume of a wear ring, m³; `None` when the design has no such ring.
    pub fn ring_volume(&self, kind: RingKind) -> Option<f64> {
        self.rings
            .iter()
            .find(|r| r.kind == kind)
            .map(|r| r.solid.volume() * 1e-9)
    }
}

/// Meridional point (z, r) in metres to a 3D point of the xz plane in millimetres.
fn xz(p: DVec2) -> DVec3 {
    DVec3::new(p.y * MM, 0.0, p.x * MM)
}

/// Closed planar wire through a polygon of edges given as point chains: each chain with
/// more than two points becomes a spline, two points a straight line.
fn profile_face(chains: &[Vec<DVec3>]) -> Result<Shape, CadError> {
    let edges = chains
        .iter()
        .map(|c| {
            if c.len() == 2 {
                occ::line_edge(c[0], c[1])
            } else {
                occ::spline_edge(c, false)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    occ::planar_face(&occ::wire(&edges)?)
}

/// Section points extrapolated beyond `edge` away from `inner` by `factor`.
fn extrapolate(edge: &[DVec3], inner: &[DVec3], factor: f64) -> Vec<DVec3> {
    edge.iter()
        .zip(inner)
        .map(|(e, i)| *e + (*e - *i) * factor)
        .collect()
}

/// Closed wire of one blade section from its pressure and suction sides (leading to
/// trailing edge). Zero-thickness edges become a smooth nose of the spline; edges with
/// thickness are closed by straight lines.
fn section_wire(ps: &[DVec3], ss: &[DVec3]) -> Result<Shape, CadError> {
    let n = ps.len();
    let le_closed = ps[0].distance(ss[0]) < CLOSED_EDGE;
    let te_closed = ps[n - 1].distance(ss[n - 1]) < CLOSED_EDGE;
    let edges = match (le_closed, te_closed) {
        (true, true) => {
            // Periodic loop starting at the trailing edge: suction TE→LE, pressure LE→TE.
            let mut pts: Vec<DVec3> = ss.iter().rev().copied().collect();
            pts.extend_from_slice(&ps[1..n - 1]);
            vec![occ::spline_edge(&pts, true)?]
        }
        (true, false) => {
            let mut pts: Vec<DVec3> = ss.iter().rev().copied().collect();
            pts.extend_from_slice(&ps[1..]);
            vec![
                occ::spline_edge(&pts, false)?,
                occ::line_edge(ps[n - 1], ss[n - 1])?,
            ]
        }
        (false, true) => {
            let mut pts: Vec<DVec3> = ps.to_vec();
            pts.extend(ss[..n - 1].iter().rev());
            vec![
                occ::spline_edge(&pts, false)?,
                occ::line_edge(ss[0], ps[0])?,
            ]
        }
        (false, false) => vec![
            occ::spline_edge(ss, false)?,
            occ::line_edge(ss[n - 1], ps[n - 1])?,
            occ::spline_edge(&ps.iter().rev().copied().collect::<Vec<_>>(), false)?,
            occ::line_edge(ps[0], ss[0])?,
        ],
    };
    occ::wire(&edges)
}

fn scaled(points: Vec<DVec3>) -> Vec<DVec3> {
    points.into_iter().map(|p| p * MM).collect()
}

/// Lofted solid of one blade, extended beyond hub and shroud.
fn blade_solid(g: &ImpellerGeometry, extension: f64) -> Result<Shape, CadError> {
    let sign = g.blade.rotation_sign;
    let sides = |s: &BladeSection| (scaled(s.pressure_side(sign)), scaled(s.suction_side(sign)));
    let mut sections: Vec<(Vec<DVec3>, Vec<DVec3>)> = g.blade.sections.iter().map(sides).collect();
    let m = sections.len();
    if m < 2 {
        return Err(CadError::Input(
            "a blade needs at least two sections".into(),
        ));
    }
    let hub = (
        extrapolate(&sections[0].0, &sections[1].0, extension),
        extrapolate(&sections[0].1, &sections[1].1, extension),
    );
    let shroud = (
        extrapolate(&sections[m - 1].0, &sections[m - 2].0, extension),
        extrapolate(&sections[m - 1].1, &sections[m - 2].1, extension),
    );
    sections.insert(0, hub);
    sections.push(shroud);
    // A blunt trailing edge is a chord between two points at r₂, which would leave a
    // thin lens of fluid against the outlet surface. Extend blunt edges past r₂ along
    // the blade so that trimming with the channel cuts them exactly at d₂, as machining
    // the impeller to its outer diameter does.
    let reach = (g.basis.blade_thickness * MM).max(1.0);
    for (ps, ss) in &mut sections {
        let n = ps.len();
        if ps[n - 1].distance(ss[n - 1]) > CLOSED_EDGE {
            for side in [ps, ss] {
                let tip = side[n - 1];
                let direction = (tip - side[n - 2]).normalize();
                side.push(tip + direction * reach);
            }
        }
    }
    let wires = sections
        .iter()
        .map(|(ps, ss)| section_wire(ps, ss))
        .collect::<Result<Vec<_>, _>>()?;
    occ::loft(&wires)
}

/// The point chains of the edges of `wire`, `points` for each curved segment.
fn wire_chains(wire: &Wall, points: usize) -> Vec<Vec<DVec3>> {
    wire.segments()
        .iter()
        .map(|s| {
            if s.kind() == SegmentKind::Line {
                vec![xz(s.start()), xz(s.end())]
            } else {
                (0..points)
                    .map(|i| xz(s.eval(i as f64 / (points - 1) as f64)))
                    .collect()
            }
        })
        .collect()
}

/// A closed outline (z, r) as point chains split where it turns by more than 20°, so
/// that each smooth run becomes one spline and the corners stay sharp.
fn outline_chains(outline: &[DVec2]) -> Vec<Vec<DVec3>> {
    let mut points: Vec<DVec2> = Vec::with_capacity(outline.len());
    for &p in outline {
        if points.last().is_none_or(|q| q.distance(p) > 1e-9) {
            points.push(p);
        }
    }
    if points.len() > 1 && points[0].distance(points[points.len() - 1]) <= 1e-9 {
        points.pop();
    }
    let n = points.len();
    let corner = |i: usize| {
        let (a, b, c) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        (b - a).normalize_or_zero().dot((c - b).normalize_or_zero()) < 20f64.to_radians().cos()
    };
    let corners: Vec<usize> = (0..n).filter(|&i| corner(i)).collect();
    let starts = if corners.is_empty() { vec![0] } else { corners };
    starts
        .iter()
        .enumerate()
        .map(|(k, &from)| {
            let to = starts[(k + 1) % starts.len()];
            let mut chain = vec![xz(points[from])];
            let mut i = from;
            loop {
                i = (i + 1) % n;
                chain.push(xz(points[i]));
                if i == to {
                    break;
                }
            }
            chain
        })
        .collect()
}

/// Fractions of the requested fillet radius tried in turn.
const FILLET_RETRIES: [f64; 4] = [1.0, 0.75, 0.5, 0.25];

/// The fluid of inlet `duct`, revolved from its outline; an error that says why when the
/// duct could not be built.
pub fn build_inlet_duct(duct: &InletDuct) -> Result<Shape, CadError> {
    if duct.outline.is_empty() {
        return Err(CadError::Input(
            duct.warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "the inlet duct was not built".into()),
        ));
    }
    occ::revolve_z(&profile_face(&outline_chains(&duct.outline))?)
}

/// Fuzzy tolerance of the join of the vanes to the disk, mm: the revolved band and the
/// wall of the disk are both splines through sampled points, not the same surface.
const VANE_JOIN_TOLERANCE: f64 = 1e-3;

/// Margin of the vane strips beyond the band, mm: the band alone trims their ends.
const VANE_MARGIN: f64 = 1.0;

/// Plan outline of one back vane at angle zero, as point chains in the plane `z`, mm: a
/// straight strip of width `thickness` for radial vanes, a strip following the
/// logarithmic spiral θ(r) = ln(r/r_in)/tan(angle) for curved ones. The strip runs
/// beyond the radii of the band, which trims it.
fn vane_outline(vanes: &BackVanes, sign: f64, z: f64) -> Result<Vec<Vec<DVec3>>, CadError> {
    let half = 0.5 * vanes.thickness * MM;
    let (lo, hi) = (
        0.5 * vanes.inner_radius * MM,
        vanes.outer_radius * MM + VANE_MARGIN,
    );
    let point = |x: f64, y: f64| DVec3::new(x, y, z);
    let (side_a, side_b): (Vec<DVec3>, Vec<DVec3>) = match vanes.shape {
        VaneShape::Radial => (
            vec![point(lo, -half), point(hi, -half)],
            vec![point(lo, half), point(hi, half)],
        ),
        VaneShape::Curved { angle } => {
            let slope = angle.tan();
            if !(slope.is_finite()
                && slope.abs() > 1e-6
                && angle > 0.0
                && angle <= std::f64::consts::FRAC_PI_2 + 1e-9)
            {
                return Err(CadError::Input(
                    "the curved back vanes need an angle above 0 and up to 90 degrees".into(),
                ));
            }
            let r0 = vanes.inner_radius * MM;
            let steps = 24;
            let mut sides = (Vec::new(), Vec::new());
            for k in 0..=steps {
                let r = lo + (hi - lo) * f64::from(k) / f64::from(steps);
                let theta = -sign * (r / r0).ln() / slope;
                // Backward swept like the blades, whose angle falls with the radius for a
                // positive sense. The centre line and its tangent: dθ/dr = ∓1/(r·tan), so
                // the tangent is
                // (cos θ − r θ' sin θ, sin θ + r θ' cos θ) up to a factor.
                let (c, s) = (theta.cos(), theta.sin());
                let rate = -sign / slope;
                let tangent = DVec2::new(c - rate * s, s + rate * c).normalize();
                let normal = DVec2::new(-tangent.y, tangent.x) * half;
                let centre = DVec2::new(r * c, r * s);
                sides.0.push(centre - normal);
                sides.1.push(centre + normal);
            }
            let lift = |v: Vec<DVec2>| v.into_iter().map(|p| point(p.x, p.y)).collect();
            (lift(sides.0), lift(sides.1))
        }
    };
    let last = |v: &Vec<DVec3>| *v.last().expect("a side has points");
    let mut chains = vec![side_a.clone()];
    chains.push(vec![last(&side_a), last(&side_b)]);
    chains.push(side_b.iter().rev().copied().collect());
    chains.push(vec![side_b[0], side_a[0]]);
    Ok(chains)
}

/// The back vanes: the band revolved about the axis, trimmed by `count` strips laid out
/// in plan. `None` when the design has none or they have no room.
fn back_vane_solid(g: &ImpellerGeometry) -> Result<Option<Shape>, CadError> {
    let Some(vanes) = g.contour.back_vanes.as_ref().filter(|v| !v.band.is_empty()) else {
        return Ok(None);
    };
    let ring = occ::revolve_z(&profile_face(&outline_chains(&vanes.band))?)?;
    let (z_min, z_max) = vanes
        .band
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
    let z0 = z_min * MM - VANE_MARGIN;
    let height = (z_max - z_min) * MM + 2.0 * VANE_MARGIN;
    let chains = vane_outline(vanes, g.basis.rotation_sign, z0)?;
    let strip = occ::prism_z(&profile_face(&chains)?, height)?;
    let copies = (0..vanes.count)
        .map(|i| strip.rotated_z(2.0 * PI * f64::from(i) / f64::from(vanes.count)))
        .collect::<Result<Vec<_>, _>>()?;
    let solid = ring.common(&occ::compound(&copies)?)?;
    if solid.is_valid() && solid.volume() > 0.0 {
        Ok(Some(solid))
    } else {
        Err(CadError::Operation(
            "the back vanes are not a valid solid".into(),
        ))
    }
}

/// Builds blade, channel, flow domain and impeller solids from the geometry.
pub fn build_impeller(g: &ImpellerGeometry, options: &CadOptions) -> Result<ImpellerCad, CadError> {
    let b = &g.basis;
    let material = g.contour.material;
    let t = material.wall_thickness;
    let (rb, rboss) = (material.bore_diameter / 2.0, material.boss_diameter / 2.0);
    let k = options.contour_points.max(5);
    let hub: Vec<DVec2> = g.contour.hub.sample(k);
    let shroud: Vec<DVec2> = g.contour.shroud.sample(k);
    let chain = |pts: &[DVec2]| pts.iter().map(|&p| xz(p)).collect::<Vec<_>>();
    let rev = |pts: &[DVec2]| pts.iter().rev().map(|&p| xz(p)).collect::<Vec<_>>();

    // Channel: hub (inlet → outlet), outlet, shroud (outlet → inlet), inlet.
    let channel = occ::revolve_z(&profile_face(&[
        chain(&hub),
        vec![xz(hub[k - 1]), xz(shroud[k - 1])],
        rev(&shroud),
        vec![xz(shroud[0]), xz(hub[0])],
    ])?)?;

    let pattern = options
        .balance_holes
        .as_ref()
        .map(|holes| {
            hole_pattern(
                holes,
                rboss,
                collar_limit(g, holes),
                g.contour.seals.rear.is_some(),
            )
        })
        .transpose()?;

    // Everything the impeller fills when the flow domain is taken out, in one profile:
    // across the inlet, the shroud material back to the outlet, across the outlet and
    // the hub material back to the inlet. Cutting the (rounded) flow domain out of it
    // meets no wall face, only the inlet and the outlet.
    let (hub_wire, shroud_wire) = (&g.contour.hub_material, &g.contour.shroud_material);
    let mut profile = vec![vec![xz(hub[0]), xz(shroud[0])]];
    profile.extend(wire_chains(shroud_wire, k).into_iter().rev().map(|mut c| {
        c.reverse();
        c
    }));
    if shroud[k - 1].distance(hub[k - 1]) > 1e-9 {
        profile.push(vec![xz(shroud[k - 1]), xz(hub[k - 1])]);
    }
    profile.extend(wire_chains(hub_wire, k));
    let material_solid = occ::revolve_z(&profile_face(&profile)?)?;
    // The rear face of the hub disk, for the balance holes.
    let rear = hub_wire
        .sample(200)
        .iter()
        .map(|p| p.x)
        .fold(g.contour.hub.end().x, f64::max);

    // The wear rings, each a revolved rectangle.
    let (spans, mut warnings) = ring_spans(g);
    let rings = spans
        .iter()
        .map(|s| {
            let corner = |z, r| xz(DVec2::new(z, r));
            let chains = [
                vec![corner(s.z_low, s.inner), corner(s.z_high, s.inner)],
                vec![corner(s.z_high, s.inner), corner(s.z_high, s.outer)],
                vec![corner(s.z_high, s.outer), corner(s.z_low, s.outer)],
                vec![corner(s.z_low, s.outer), corner(s.z_low, s.inner)],
            ];
            Ok(WearRing {
                kind: s.kind,
                solid: occ::revolve_z(&profile_face(&chains)?)?,
            })
        })
        .collect::<Result<Vec<_>, CadError>>()?;

    // The back vanes; a failure leaves them out with a warning.
    let back_vanes = match back_vane_solid(g) {
        Ok(vanes) => vanes,
        Err(e) => {
            warnings.push(format!("the back vanes were not built: {e}"));
            None
        }
    };

    // The side chambers, revolved from their outlines, less the rings: the fluid and the
    // rings never share a volume.
    let chambers = &g.contour.chambers;
    let side_chambers = if chambers.enabled {
        let solids = [&chambers.front, &chambers.rear]
            .into_iter()
            .map(|outline| occ::revolve_z(&profile_face(&outline_chains(outline))?))
            .collect::<Result<Vec<_>, _>>()?;
        let mut fluid = occ::compound(&solids)?;
        for ring in &rings {
            match fluid.cut(&ring.solid) {
                Ok(cut) if cut.is_valid() && cut.solid_count() == fluid.solid_count() => {
                    fluid = cut
                }
                _ => warnings.push(format!(
                    "the {} could not be taken out of the side chambers",
                    ring.kind.label().to_lowercase()
                )),
            }
        }
        // The rear chamber fluid does not reach into the back vanes either.
        if let Some(vanes) = &back_vanes {
            match fluid.cut_fuzzy(vanes, VANE_JOIN_TOLERANCE) {
                Ok(cut) if cut.is_valid() && cut.solid_count() == fluid.solid_count() => {
                    fluid = cut
                }
                _ => warnings
                    .push("the back vanes could not be taken out of the side chambers".to_string()),
            }
        }
        Some(fluid)
    } else {
        None
    };

    // Blades.
    let blade = blade_solid(g, options.blade_extension)?;
    let z = g.blade.count;
    let copies = (0..z)
        .map(|i| blade.rotated_z(2.0 * PI * f64::from(i) / f64::from(z)))
        .collect::<Result<Vec<_>, _>>()?;
    let all_blades = occ::compound(&copies)?;
    let mut fluid = channel.cut(&all_blades)?;
    let blades = channel.common(&all_blades)?;
    // The walls are the profile less the channel: their faces are those of the channel,
    // which the trimmed blades share.
    let mut impeller = material_solid.cut(&channel)?.fuse(&blades)?;

    // Fillets where the blades meet hub and shroud: concave edges of the impeller and
    // convex ones of the flow domain, rounded on each with the same radius. (A boolean
    // between the two rounded solids would meet their common faces and fails.)
    let radius = options
        .fillet_radius
        .unwrap_or(0.5 * b.blade_thickness)
        .max(0.0);
    let mut fillet_radius = 0.0;
    if radius > 0.0 {
        let walls = [g.contour.hub.sample(400), g.contour.shroud.sample(400)];
        // A fillet wider than the corner it rounds fails, typically where a blade root
        // meets the outlet at d₂: try smaller radii before leaving the edges sharp.
        let mut failure = None;
        for scale in FILLET_RETRIES {
            let r = radius * scale;
            let attempt = |shape: &Shape| -> Result<Shape, CadError> {
                let rounded = shape.filleted(&root_edges(shape, &walls), r * MM)?;
                if rounded.is_valid() && rounded.solid_count() == 1 {
                    Ok(rounded)
                } else {
                    Err(CadError::Operation("no single valid solid".into()))
                }
            };
            match attempt(&impeller).and_then(|i| Ok((i, attempt(&fluid)?))) {
                Ok((rounded_impeller, rounded_fluid)) => {
                    impeller = rounded_impeller;
                    fluid = rounded_fluid;
                    fillet_radius = r;
                    break;
                }
                Err(e) => {
                    failure.get_or_insert(e);
                }
            }
        }
        match failure {
            Some(e) if fillet_radius == 0.0 => warnings.push(format!(
                "the {:.1} mm fillets at the blade roots failed ({e}); the edges stay sharp",
                radius * MM
            )),
            Some(_) => warnings.push(format!(
                "the {:.1} mm fillets at the blade roots did not fit; they were built with \
                 {:.2} mm",
                radius * MM,
                fillet_radius * MM
            )),
            None => {}
        }
    }

    // The back vanes stand on the rear face of the disk: one solid with the impeller.
    let back_vanes = match back_vanes {
        Some(vanes) => match impeller.fuse_fuzzy(&vanes, VANE_JOIN_TOLERANCE) {
            Ok(joined) if joined.is_valid() && joined.solid_count() == 1 => {
                // What the join adds: the vanes less the sliver the spline walls overlap.
                let own = vanes.volume();
                match joined.cut_fuzzy(&impeller, VANE_JOIN_TOLERANCE) {
                    Ok(added) if added.is_valid() && added.solid_count() == vanes.solid_count() => {
                        if added.volume() < 0.5 * own {
                            warnings.push(
                                "the back vanes left out: the join added less than half of their volume"
                                    .to_string(),
                            );
                            None
                        } else {
                            impeller = joined;
                            Some(added)
                        }
                    }
                    _ => {
                        warnings.push(
                            "the volume the back vanes add to the impeller could not be checked"
                                .to_string(),
                        );
                        impeller = joined;
                        Some(vanes)
                    }
                }
            }
            _ => {
                warnings.push("the back vanes could not be joined to the impeller".to_string());
                None
            }
        },
        None => None,
    };

    // Balance holes, parallel to the axis through the hub wall, midway between the
    // boss and the rear seal and between two blades (our layout).
    let balance_holes = match (&options.balance_holes, pattern) {
        (Some(holes), Some(pattern)) => {
            let r = pattern.pitch_diameter / 2.0;
            let radius = holes.diameter / 2.0;
            let start = hub_z_at(&hub, r - radius) - holes.diameter;
            let height = rear + t - start;
            let first = blade_angle_at(g, r) + PI / f64::from(z);
            let cylinders = (0..holes.count)
                .map(|i| {
                    let angle = first + 2.0 * PI * f64::from(i) / f64::from(holes.count);
                    let base = DVec3::new(r * angle.cos(), r * angle.sin(), start) * MM;
                    occ::cylinder(base, DVec3::Z, radius * MM, height * MM)
                })
                .collect::<Result<Vec<_>, _>>()?;
            impeller = impeller.cut(&occ::compound(&cylinders)?)?;
            Some(pattern)
        }
        _ => None,
    };

    Ok(ImpellerCad {
        blade,
        channel,
        fluid,
        blades,
        impeller,
        side_chambers,
        rings,
        back_vanes,
        wall_thickness: t,
        bore_diameter: 2.0 * rb,
        boss_diameter: 2.0 * rboss,
        fillet_radius,
        balance_holes,
        warnings,
    })
}

/// Indices of the edges of `shape` that run on one of the meridional `walls` (z, r in
/// m) and cross it both radially and around the axis: where the blades meet the wall.
/// The circles and seams of the walls themselves stay out.
fn root_edges(shape: &Shape, walls: &[Vec<DVec2>]) -> Vec<usize> {
    // Distance in mm from a meridional point to a wall polyline.
    let distance = |wall: &[DVec2], p: DVec2| -> f64 {
        wall.windows(2)
            .map(|w| {
                let (a, b) = (w[0] * MM, w[1] * MM);
                let t = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                p.distance(a + (b - a) * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    const TOLERANCE: f64 = 0.05;
    shape
        .edge_samples(9)
        .iter()
        .enumerate()
        .filter(|(_, points)| {
            if points.is_empty() {
                return false;
            }
            let meridional: Vec<DVec2> = points
                .iter()
                .map(|p| DVec2::new(p.z, p.x.hypot(p.y)))
                .collect();
            let on_wall = walls
                .iter()
                .any(|w| meridional.iter().all(|&m| distance(w, m) < TOLERANCE));
            let radii = meridional.iter().map(|m| m.y);
            let (lo, hi) = radii.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), r| {
                (lo.min(r), hi.max(r))
            });
            let around = points.windows(2).any(|w| {
                let turn = (w[0].y.atan2(w[0].x) - w[1].y.atan2(w[1].x)).abs();
                turn > 1e-4 && turn < PI
            });
            on_wall && hi - lo > TOLERANCE && around
        })
        .map(|(i, _)| i)
        .collect()
}

/// Radius the balance holes must stay inside, m: the inner radius of the collar of the
/// rear seal when the design has one, else the given seal diameter.
fn collar_limit(g: &ImpellerGeometry, holes: &BalanceHoleOptions) -> f64 {
    match &g.contour.seals.rear {
        Some(rear) => collar_inner_radius(rear, g.contour.material.wall_thickness),
        None => holes.seal_diameter / 2.0,
    }
}

/// The pitch circle of the balance holes: midway between the boss and the inside of the
/// collar of the rear seal (`limit`), where both leave room for a hole.
fn hole_pattern(
    holes: &BalanceHoleOptions,
    boss: f64,
    limit: f64,
    collar: bool,
) -> Result<BalanceHoles, CadError> {
    let radius = holes.diameter / 2.0;
    let pitch = 0.5 * (boss + limit);
    if holes.count == 0
        || radius.is_nan()
        || radius <= 0.0
        || pitch - radius < boss
        || pitch + radius > limit
    {
        return Err(CadError::Input(format!(
            "{} balance holes of {:.1} mm do not fit between the boss ({:.1} mm) and the \
             {} ({:.1} mm)",
            holes.count,
            holes.diameter * MM,
            2.0 * boss * MM,
            if collar { "rear seal collar" } else { "seal" },
            2.0 * limit * MM
        )));
    }
    Ok(BalanceHoles {
        count: holes.count,
        diameter: holes.diameter,
        pitch_diameter: 2.0 * pitch,
    })
}

/// Axial position z of the hub contour (z, r) at radius `r`, m; the eye below it.
fn hub_z_at(hub: &[DVec2], r: f64) -> f64 {
    if r <= hub[0].y {
        return hub[0].x;
    }
    for w in hub.windows(2) {
        if (w[0].y..=w[1].y).contains(&r) {
            let f = (r - w[0].y) / (w[1].y - w[0].y);
            return w[0].x + f * (w[1].x - w[0].x);
        }
    }
    hub[hub.len() - 1].x
}

/// Angle about the axis of the blade's hub section at radius `r`; its leading edge
/// below that radius.
fn blade_angle_at(g: &ImpellerGeometry, r: f64) -> f64 {
    let mean = g.blade.sections[0].mean_line(g.blade.rotation_sign);
    let closest = mean
        .iter()
        .min_by(|a, b| {
            let da = (a.x.hypot(a.y) - r).abs();
            let db = (b.x.hypot(b.y) - r).abs();
            da.total_cmp(&db)
        })
        .expect("a blade section has points");
    closest.y.atan2(closest.x)
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use tp_design::impeller::{self, ImpellerBasis, ImpellerGeometryInput};
    use tp_design::pump::{CentrifugalPumpDesign, CentrifugalPumpInput, PumpDuty, design};

    pub(crate) fn reference() -> ImpellerGeometry {
        reference_with(&ImpellerGeometryInput::default()).0
    }

    /// The reference impeller built from `geometry`, with its main dimensions and duty.
    pub(crate) fn reference_with(
        geometry: &ImpellerGeometryInput,
    ) -> (ImpellerGeometry, CentrifugalPumpDesign, PumpDuty) {
        let duty = PumpDuty {
            flow: 100.0 / 3600.0,
            head: 32.0,
            machine_head: 32.0,
            stages: 1,
            speed: 2900.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
            kinematic_viscosity: 1.0e-6,
        };
        let input = CentrifugalPumpInput::default();
        let main = design(&duty, &input).unwrap();
        let basis = ImpellerBasis::from_pump(&main, &input, 1.0);
        (impeller::build(&basis, geometry, 1.0).unwrap(), main, duty)
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::reference;
    use super::*;
    use approx::assert_relative_eq;

    /// Volume of revolution of the closed meridional polygon (z, r) by Pappus.
    fn revolved_volume(polygon: &[DVec2]) -> f64 {
        let n = polygon.len();
        let (mut area, mut moment) = (0.0, 0.0);
        for i in 0..n {
            let (p, q) = (polygon[i], polygon[(i + 1) % n]);
            let cross = p.x * q.y - q.x * p.y;
            area += cross / 2.0;
            moment += cross * (p.y + q.y) / 6.0;
        }
        2.0 * PI * (moment / area) * area.abs()
    }

    /// One full build of the reference impeller with sharp edges and without holes,
    /// checked several ways (booleans on seven B-spline blades take a few seconds, so
    /// the build is shared), then one finished with fillets and balance holes.
    #[test]
    fn reference_impeller_solids() {
        let _slot = crate::test_slot::take();
        let g = reference();
        let sharp = CadOptions {
            fillet_radius: Some(0.0),
            ..CadOptions::default()
        };
        let cad = build_impeller(&g, &sharp).unwrap();

        // Channel volume against Pappus on a fine polygon of the same contours.
        let mut polygon = g.contour.hub.sample(2000);
        polygon.extend(g.contour.shroud.sample(2000).into_iter().rev());
        let pappus = revolved_volume(&polygon);
        assert_relative_eq!(cad.channel.volume() * 1e-9, pappus, max_relative = 2e-3);

        // Booleans conserve volume: fluid + trimmed blades = channel.
        let (fluid, blades, channel) = (
            cad.fluid.volume(),
            cad.blades.volume(),
            cad.channel.volume(),
        );
        assert_relative_eq!(fluid + blades, channel, max_relative = 1e-4);

        // One connected flow domain, one solid per blade, one impeller part.
        assert_eq!(cad.fluid.solid_count(), 1);
        assert_eq!(cad.blades.solid_count(), g.blade.count as usize);
        assert_eq!(cad.impeller.solid_count(), 1);
        for shape in [&cad.blade, &cad.channel, &cad.fluid, &cad.impeller] {
            assert!(shape.is_valid());
        }

        // Each trimmed blade ≈ blade length × channel width × thickness.
        let one_blade = blades * 1e-9 / f64::from(g.blade.count);
        let s = &g.blade.sections[g.blade.sections.len() / 2];
        let le = s.leading_edge.position;
        let width_le = g
            .contour
            .shroud_at(g.contour.leading_edge.1)
            .distance(g.contour.hub_at(g.contour.leading_edge.0));
        let width = 0.5 * (width_le + g.basis.outlet_width);
        let estimate = s.mean_line_length() * width * g.basis.blade_thickness;
        assert!(
            (0.7..1.3).contains(&(one_blade / estimate)),
            "blade {one_blade:e} m³ vs estimate {estimate:e} m³ (LE at r = {})",
            le.y
        );

        // Impeller material: hub disk + shroud + blades, which only touch. The hub
        // disk is a wall with a boss around the bore.
        assert!(cad.impeller_volume() > blades * 1e-9);
        assert_relative_eq!(cad.bore_diameter, g.basis.hub_diameter);
        assert!(cad.boss_diameter > cad.bore_diameter && cad.fillet_radius == 0.0);

        // Fillets move material from the flow domain to the impeller: about
        // (1 − π/4)·r² on each side of every blade root, at hub and shroud.
        let holes = BalanceHoleOptions {
            count: 7,
            diameter: 0.0085,
            seal_diameter: 0.0841,
        };
        let finished = build_impeller(
            &g,
            &CadOptions {
                balance_holes: Some(holes),
                ..CadOptions::default()
            },
        )
        .unwrap();
        // The roots are rounded, with half the blade thickness or the largest radius
        // that fits the corner at the outlet, which is then reported.
        let wanted = 0.5 * g.basis.blade_thickness;
        assert!(finished.fillet_radius > 0.0 && finished.fillet_radius <= wanted + 1e-12);
        assert!(
            finished.warnings.iter().all(|w| w.contains("did not fit")),
            "{:?}",
            finished.warnings
        );
        assert_eq!(
            finished.warnings.is_empty(),
            (finished.fillet_radius - wanted).abs() < 1e-12
        );
        for shape in [&finished.fluid, &finished.impeller] {
            assert!(shape.is_valid() && shape.solid_count() == 1);
        }
        let r = finished.fillet_radius;
        let roots: f64 = [
            &g.blade.sections[0],
            &g.blade.sections[g.blade.sections.len() - 1],
        ]
        .iter()
        .map(|s| s.mean_line_length())
        .sum();
        let fillets = (1.0 - PI / 4.0) * r * r * 2.0 * roots * f64::from(g.blade.count);
        let lost = cad.fluid_volume() - finished.fluid_volume();
        assert!(
            (0.7..2.0).contains(&(lost / fillets)),
            "fillets {lost:e} m³ vs {fillets:e} m³"
        );
        // Each hole crosses the hub wall, a few wall thicknesses long at most.
        let gained = finished.impeller_volume() - cad.impeller_volume();
        let drilled = lost - gained;
        let hole = PI / 4.0 * holes.diameter * holes.diameter * cad.wall_thickness;
        assert!(
            (7.0 * hole..7.0 * 3.0 * hole).contains(&drilled),
            "drilled {drilled:e} m³ vs one hole through the wall {hole:e} m³"
        );
        let pattern = finished.balance_holes.unwrap();
        assert!(pattern.pitch_diameter > finished.boss_diameter && pattern.pitch_diameter < 0.0841);
    }

    #[test]
    fn the_material_contours_set_the_impeller_solid() {
        let _slot = crate::test_slot::take();
        // The material as Pappus sees it: hub disk and shroud regions revolved.
        let pappus = |g: &ImpellerGeometry| {
            let region = |wall: &tp_design::impeller::wall::Wall, wire: &Wall| {
                let mut polygon = wall.sample(2000);
                polygon.extend(wire.sample(2000));
                revolved_volume(&polygon)
            };
            let c = &g.contour;
            region(&c.hub, &c.hub_material) + region(&c.shroud, &c.shroud_material)
        };
        let sharp = CadOptions {
            fillet_radius: Some(0.0),
            ..CadOptions::default()
        };
        let g = reference();
        let cad = build_impeller(&g, &sharp).unwrap();
        let walls = cad.impeller_volume() - cad.blades.volume() * 1e-9;
        assert_relative_eq!(walls, pappus(&g), max_relative = 5e-3);

        // Thicker walls, set in the meridional design, make a heavier impeller.
        let mut input = tp_design::impeller::ImpellerGeometryInput::default();
        input.meridional.material.wall_thickness = tp_design::Param::Manual(0.006);
        let thick = tp_design::impeller::build(&g.basis, &input, 1.0).unwrap();
        let heavy = build_impeller(&thick, &sharp).unwrap();
        assert_relative_eq!(heavy.wall_thickness, 0.006);
        let walls = heavy.impeller_volume() - heavy.blades.volume() * 1e-9;
        assert_relative_eq!(walls, pappus(&thick), max_relative = 5e-3);
        assert!(heavy.impeller_volume() > cad.impeller_volume() * 1.1);
    }

    #[test]
    fn designed_side_chambers_are_revolved_into_their_fluid() {
        let _slot = crate::test_slot::take();
        let g = reference();
        assert!(
            build_impeller(&g, &CadOptions::default())
                .unwrap()
                .side_chambers
                .is_none()
        );
        let mut input = tp_design::impeller::ImpellerGeometryInput::default();
        input.meridional.chambers.enabled = true;
        let with = tp_design::impeller::build(&g.basis, &input, 1.0).unwrap();
        let sharp = CadOptions {
            fillet_radius: Some(0.0),
            ..CadOptions::default()
        };
        let cad = build_impeller(&with, &sharp).unwrap();
        let chambers = cad.side_chambers.as_ref().unwrap();
        assert!(chambers.is_valid());
        assert_eq!(chambers.solid_count(), 2);
        let c = &with.contour.chambers;
        let pappus = revolved_volume(&c.front) + revolved_volume(&c.rear);
        // The rear casing ring stands in the plain rear chamber: its fluid is taken out.
        let ring = cad.ring_volume(RingKind::RearCasing).unwrap();
        assert_relative_eq!(chambers.volume() * 1e-9, pappus - ring, max_relative = 5e-3);
    }

    #[test]
    fn the_inlet_duct_is_revolved_into_its_fluid() {
        let _slot = crate::test_slot::take();
        use tp_design::impeller::duct::{self, DuctInput};
        let mut input = tp_design::impeller::ImpellerGeometryInput::default();
        input.meridional.chambers.enabled = true;
        let g = reference();
        let with = tp_design::impeller::build(&g.basis, &input, 1.0).unwrap();
        let built = duct::build(&with.contour, &with.basis, &DuctInput::default());
        let shape = build_inlet_duct(&built).unwrap();
        assert!(shape.is_valid());
        assert_eq!(shape.solid_count(), 1);
        let pappus = revolved_volume(&built.outline);
        assert_relative_eq!(shape.volume() * 1e-9, pappus, max_relative = 5e-3);
        // One that could not be built says why.
        let broken = duct::build(
            &with.contour,
            &with.basis,
            &DuctInput {
                length: tp_design::Param::Manual(-1.0),
                ..DuctInput::default()
            },
        );
        let e = build_inlet_duct(&broken).unwrap_err().to_string();
        assert!(e.contains("length"), "{e}");
    }

    #[test]
    fn shaped_edges_loft_a_valid_blade() {
        let _slot = crate::test_slot::take();
        use tp_design::impeller::blade::EdgeShape;
        let plain = blade_solid(&reference(), 0.0).unwrap();
        let mut input = tp_design::impeller::ImpellerGeometryInput::default();
        input.edges.leading = EdgeShape::BEZIER;
        input.edges.leading_shroud = Some(EdgeShape::Bezier {
            start: [2.5, 1.6],
            pull: [0.7, 0.5],
            tip: 0.35,
            symmetric: false,
        });
        input.edges.trailing = EdgeShape::Taper {
            length: 4.0,
            nose: 0.5,
            ratio: 1.6,
            asymmetry: 0.3,
        };
        let g = super::tests_support::reference_with(&input).0;
        let blade = blade_solid(&g, 0.0).unwrap();
        assert!(blade.is_valid());
        assert_eq!(blade.solid_count(), 1);
        // The thinner trailing edge takes a little of the blade away.
        let (shaped, full) = (blade.volume(), plain.volume());
        assert!(shaped < full && shaped > 0.85 * full, "{shaped} vs {full}");
    }

    #[test]
    fn balance_holes_sit_inside_the_collar() {
        let _slot = crate::test_slot::take();
        let g = reference();
        let c = &g.contour;
        let rear = c.seals.rear.expect("the default design has a rear seal");
        let inner = collar_inner_radius(&rear, c.material.wall_thickness);
        let holes = BalanceHoleOptions {
            count: 7,
            diameter: 0.0085,
            // Not what bounds the holes when there is a collar.
            seal_diameter: 1.0,
        };
        let limit = collar_limit(&g, &holes);
        assert!((limit - inner).abs() < 1e-15);
        let boss = c.material.boss_diameter / 2.0;
        let pattern = hole_pattern(&holes, boss, limit, true).unwrap();
        assert!(pattern.pitch_diameter > 2.0 * boss);
        assert!(pattern.pitch_diameter + holes.diameter <= 2.0 * inner + 1e-12);
        // A hole that fits between boss and seal but not inside the collar is refused.
        let wide = BalanceHoleOptions {
            diameter: 2.0 * (inner - boss),
            ..holes
        };
        let Err(CadError::Input(with)) = hole_pattern(&wide, boss, limit, true) else {
            panic!("a hole too wide for the collar is refused");
        };
        assert!(with.contains("rear seal collar"), "{with}");
        let Err(CadError::Input(without)) = hole_pattern(&wide, boss, limit, false) else {
            panic!("a hole too wide for the seal is refused");
        };
        assert!(!without.contains("collar"), "{without}");
    }

    #[test]
    fn rejects_bad_options() {
        let _slot = crate::test_slot::take();
        let g = reference();
        // Holes that do not fit between boss and rear seal.
        let big_holes = CadOptions {
            balance_holes: Some(BalanceHoleOptions {
                count: 7,
                diameter: 0.04,
                seal_diameter: 0.0841,
            }),
            ..Default::default()
        };
        assert!(build_impeller(&g, &big_holes).is_err());
    }

    /// The reference geometry rebuilt with `change` applied to the meridional input.
    fn changed(
        change: impl FnOnce(&mut tp_design::impeller::ImpellerGeometryInput),
    ) -> ImpellerGeometry {
        let g = reference();
        let mut input = tp_design::impeller::ImpellerGeometryInput::default();
        change(&mut input);
        tp_design::impeller::build(&g.basis, &input, 1.0).unwrap()
    }

    fn sharp() -> CadOptions {
        CadOptions {
            fillet_radius: Some(0.0),
            ..CadOptions::default()
        }
    }

    #[test]
    fn four_rings_are_built_by_default() {
        let _slot = crate::test_slot::take();
        let cad = build_impeller(&reference(), &sharp()).unwrap();
        let kinds: Vec<RingKind> = cad.rings.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, RingKind::ALL);
        for ring in &cad.rings {
            assert!(ring.solid.is_valid() && ring.solid.solid_count() == 1);
        }
        let stems: Vec<&str> = RingKind::ALL.iter().map(|k| k.file_stem()).collect();
        assert_eq!(
            stems,
            [
                "front-impeller-ring",
                "front-casing-ring",
                "rear-impeller-ring",
                "rear-casing-ring"
            ]
        );
        // A default design builds without a warning.
        assert!(cad.warnings.is_empty(), "{:?}", cad.warnings);
    }

    #[test]
    fn the_ring_volume_is_its_annulus() {
        let _slot = crate::test_slot::take();
        let g = changed(|i| i.meridional.chambers.enabled = true);
        let cad = build_impeller(&g, &sharp()).unwrap();
        let (spans, _) = ring_spans(&g);
        assert_eq!(spans.len(), 4);
        for s in spans {
            let annulus = PI * (s.outer.powi(2) - s.inner.powi(2)) * (s.z_high - s.z_low);
            let volume = cad.ring_volume(s.kind).expect("the ring is built");
            assert_relative_eq!(volume, annulus, max_relative = 1e-6);
        }
        // The lengths come from the design, not from the spans themselves.
        let (spans, _) = ring_spans(&g);
        let length = |kind| {
            let s = spans.iter().find(|s| s.kind == kind).unwrap();
            s.z_high - s.z_low
        };
        let front = g.contour.seals.front.unwrap();
        assert_relative_eq!(
            length(RingKind::FrontImpeller),
            front.length,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            length(RingKind::FrontCasing),
            g.contour.chambers.values.seal_length,
            max_relative = 1e-9
        );
        // A ring the design does not ask for has no volume.
        let none = changed(|i| i.meridional.seals.rear.enabled = false);
        let cad = build_impeller(&none, &sharp()).unwrap();
        assert!(cad.ring_volume(RingKind::RearCasing).is_none());
        assert!(cad.ring_volume(RingKind::FrontCasing).is_some());
    }

    #[test]
    fn a_ring_trimmed_to_nothing_builds_without_that_ring() {
        let _slot = crate::test_slot::take();
        let base = reference();
        let wall = tp_design::impeller::default_wall_thickness(base.basis.outlet_diameter);
        let g = changed(|i| {
            i.meridional.seals.front.diameter =
                tp_design::Param::Manual(base.basis.inlet_diameter + 2.0 * wall);
        });
        assert!(g.contour.seals.front.unwrap().impeller_ring.is_none());
        let cad = build_impeller(&g, &sharp()).expect("the impeller builds");
        assert!(cad.ring_volume(RingKind::FrontImpeller).is_none());
    }

    #[test]
    fn the_rear_rings_sit_on_the_collar_and_clear_of_the_disk() {
        let _slot = crate::test_slot::take();
        let g = reference();
        let rear = g.contour.seals.rear.unwrap();
        let (spans, warnings) = ring_spans(&g);
        for kind in [RingKind::RearImpeller, RingKind::RearCasing] {
            let s = spans.iter().find(|s| s.kind == kind).unwrap();
            // The collar seat is the inner radius of the impeller ring.
            if kind == RingKind::RearImpeller {
                assert_relative_eq!(s.inner, rear.impeller_seat, max_relative = 1e-12);
            }
            // Never longer than the seal, and behind the back face at its outer radius.
            assert!(s.z_high - s.z_low <= rear.length * (1.0 + 1e-9));
            let face = back_face_at(&g.contour.hub_material, s.outer).unwrap();
            assert!(s.z_low >= face - 1e-12, "{kind:?}: {} < {face}", s.z_low);
            // The shortening is reported.
            let name = kind.label().to_lowercase();
            let short = s.z_high - s.z_low < 0.5 * rear.length;
            assert_eq!(
                warnings
                    .iter()
                    .any(|w| w.contains(&name) && w.contains("shorter")),
                short,
                "{kind:?}: {warnings:?}"
            );
        }
    }

    #[test]
    fn a_front_casing_ring_without_a_run_on_the_seat_says_so() {
        let _slot = crate::test_slot::take();
        use tp_design::impeller::wall::{SegmentInput, SegmentKind};
        // A casing wall drawn straight to its end, never running on the casing seat.
        let end = changed(|i| i.meridional.chambers.enabled = true)
            .contour
            .chambers
            .front_casing
            .end();
        let g = changed(|i| {
            i.meridional.chambers.enabled = true;
            i.meridional.chambers.front = Some(vec![SegmentInput {
                kind: SegmentKind::Line,
                end,
                ..SegmentInput::default()
            }]);
        });
        assert_eq!(g.contour.chambers.values.seal_length, 0.0);
        let (_, warnings) = ring_spans(&g);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("front casing ring follows the impeller seat run")),
            "{warnings:?}"
        );
    }

    #[test]
    fn rings_do_not_overlap_the_impeller() {
        let _slot = crate::test_slot::take();
        let cad = build_impeller(&reference(), &sharp()).unwrap();
        for ring in &cad.rings {
            let shared = cad
                .impeller
                .common(&ring.solid)
                .map_or(0.0, |s| s.volume_with_tolerance(0.0));
            assert!(
                shared < 1e-9 * ring.solid.volume_with_tolerance(0.0),
                "{:?} shares {shared} mm³ with the impeller",
                ring.kind
            );
        }
    }

    #[test]
    fn impeller_plus_its_rings_fills_the_solid_seat() {
        let _slot = crate::test_slot::take();
        use tp_design::Param;
        // The identity is exact at a seal radius where the shoulder stands on the face
        // for both seats (the 4 mm ring is trimmed to leave the neck a wall).
        let seal = |ring: bool| {
            changed(|i| {
                i.meridional.seals.front.diameter = Param::Manual(0.088);
                i.meridional.seals.front.impeller_ring.enabled = ring;
            })
        };
        let (with, without) = (seal(true), seal(false));
        let (a, b) = (
            build_impeller(&with, &sharp()).unwrap(),
            build_impeller(&without, &sharp()).unwrap(),
        );
        assert!(b.ring_volume(RingKind::FrontImpeller).is_none());
        let ring = a.ring_volume(RingKind::FrontImpeller).unwrap();
        assert_relative_eq!(
            a.impeller_volume() + ring,
            b.impeller_volume(),
            max_relative = 1e-4
        );

        // At the default diameter the flow wall holds the shoulder back differently
        // with and without the ring; the shift is tolerated (ruling R8).
        let default_seal = |ring: bool| {
            let d = 2.0 * changed(|_| {}).contour.seals.front.unwrap().radius;
            changed(|i| {
                i.meridional.seals.front.diameter = Param::Manual(d);
                i.meridional.seals.front.impeller_ring.enabled = ring;
            })
        };
        let (a, b) = (
            build_impeller(&default_seal(true), &sharp()).unwrap(),
            build_impeller(&default_seal(false), &sharp()).unwrap(),
        );
        let ring = a.ring_volume(RingKind::FrontImpeller).unwrap();
        assert_relative_eq!(
            a.impeller_volume() + ring,
            b.impeller_volume(),
            max_relative = 2e-3
        );
    }

    #[test]
    fn a_drawn_wire_leaves_its_rings_out_with_a_warning() {
        let _slot = crate::test_slot::take();
        let g = reference();
        let shroud = g.contour.shroud_material.to_input();
        let hub = g.contour.hub_material.to_input();
        let drawn = changed(|i| i.meridional.shroud_material = Some(shroud));
        assert!(drawn.contour.shroud_material_drawn && !drawn.contour.hub_material_drawn);
        let cad = build_impeller(&drawn, &sharp()).unwrap();
        assert!(cad.ring_volume(RingKind::FrontImpeller).is_none());
        assert!(cad.ring_volume(RingKind::RearImpeller).is_some());
        assert!(
            cad.warnings
                .iter()
                .any(|w| w.contains("front impeller wear ring")),
            "{:?}",
            cad.warnings
        );
        let drawn = changed(|i| i.meridional.hub_material = Some(hub));
        let cad = build_impeller(&drawn, &sharp()).unwrap();
        assert!(cad.ring_volume(RingKind::RearImpeller).is_none());
        assert!(cad.ring_volume(RingKind::FrontImpeller).is_some());
        assert!(
            cad.warnings
                .iter()
                .any(|w| w.contains("rear impeller wear ring")),
            "{:?}",
            cad.warnings
        );
    }

    #[test]
    fn the_chamber_fluid_and_the_rings_share_no_volume() {
        let _slot = crate::test_slot::take();
        let g = changed(|i| i.meridional.chambers.enabled = true);
        let cad = build_impeller(&g, &sharp()).unwrap();
        let chambers = cad.side_chambers.as_ref().unwrap();
        assert!(chambers.is_valid());
        assert_eq!(cad.rings.len(), 4);
        for ring in &cad.rings {
            let shared = chambers
                .common(&ring.solid)
                .map_or(0.0, |s| s.volume_with_tolerance(0.0));
            assert!(
                shared < 1e-9 * ring.solid.volume_with_tolerance(0.0),
                "{:?} shares {shared} mm³ with the chamber fluid",
                ring.kind
            );
        }
        // The front casing ring, which lies in the front chamber, is out of it too.
        assert!(cad.ring_volume(RingKind::FrontCasing).is_some());
        // The front casing ring spans the measured run on the casing seat.
        let (spans, _) = ring_spans(&g);
        let front = spans
            .iter()
            .find(|s| s.kind == RingKind::FrontCasing)
            .unwrap();
        let v = g.contour.chambers.values;
        assert_relative_eq!(
            front.z_high - front.z_low,
            v.seal_length,
            max_relative = 1e-12
        );
    }

    /// The reference impeller with back vanes of the given shape.
    fn with_vanes(
        shape: tp_design::impeller::back_vanes::VaneShape,
        chambers: bool,
    ) -> ImpellerGeometry {
        changed(|i| {
            i.meridional.back_vanes.enabled = true;
            i.meridional.back_vanes.shape = shape;
            i.meridional.chambers.enabled = chambers;
        })
    }

    /// Area of the closed meridional polygon, m².
    fn polygon_area(p: &[DVec2]) -> f64 {
        let n = p.len();
        (0..n)
            .map(|i| p[i].x * p[(i + 1) % n].y - p[(i + 1) % n].x * p[i].y)
            .sum::<f64>()
            .abs()
            / 2.0
    }

    #[test]
    fn radial_back_vanes_have_their_strip_volume() {
        let _slot = crate::test_slot::take();
        use tp_design::impeller::back_vanes::VaneShape;
        let g = with_vanes(VaneShape::Radial, false);
        let v = g.contour.back_vanes.as_ref().unwrap();
        assert!(!v.band.is_empty(), "{:?}", v.warnings);
        let cad = build_impeller(&g, &sharp()).unwrap();
        let vanes = cad
            .back_vanes
            .as_ref()
            .unwrap_or_else(|| panic!("the vanes are built: {:?}", cad.warnings));
        assert!(vanes.is_valid());
        assert_eq!(vanes.solid_count(), v.count as usize);
        let expected = f64::from(v.count) * v.thickness * polygon_area(&v.band);
        assert_relative_eq!(vanes.volume() * 1e-9, expected, max_relative = 0.02);
    }

    #[test]
    fn no_back_vanes_when_they_are_off() {
        let _slot = crate::test_slot::take();
        assert!(
            build_impeller(&reference(), &sharp())
                .unwrap()
                .back_vanes
                .is_none()
        );
    }

    #[test]
    fn back_vanes_join_the_impeller() {
        let _slot = crate::test_slot::take();
        use tp_design::impeller::back_vanes::VaneShape;
        let without = build_impeller(&reference(), &sharp()).unwrap();
        let cad = build_impeller(&with_vanes(VaneShape::Radial, false), &sharp()).unwrap();
        // A tight quadrature: the default one is off by more on the whole impeller than
        // the vanes' share of 1e-4.
        let exact = |s: &Shape| s.volume_with_tolerance(1e-7);
        let vanes = exact(cad.back_vanes.as_ref().unwrap());
        assert!(cad.impeller.is_valid() && cad.impeller.solid_count() == 1);
        assert_relative_eq!(
            exact(&cad.impeller) - exact(&without.impeller),
            vanes,
            max_relative = 1e-4
        );
    }

    #[test]
    fn curved_back_vanes_build() {
        let _slot = crate::test_slot::take();
        use tp_design::impeller::back_vanes::VaneShape;
        let radial = build_impeller(&with_vanes(VaneShape::Radial, false), &sharp()).unwrap();
        let curved = VaneShape::Curved {
            angle: 60f64.to_radians(),
        };
        let cad = build_impeller(&with_vanes(curved, false), &sharp()).unwrap();
        let vanes = cad
            .back_vanes
            .as_ref()
            .unwrap_or_else(|| panic!("the vanes are built: {:?}", cad.warnings));
        assert!(vanes.is_valid() && cad.impeller.is_valid());
        assert_eq!(cad.impeller.solid_count(), 1);
        // A spiral is longer than the straight strip across the same radii.
        let (a, b) = (vanes.volume(), radial.back_vanes.as_ref().unwrap().volume());
        assert!(
            a > b && a < 1.5 * b,
            "curved {a} mm³ against radial {b} mm³"
        );
    }

    #[test]
    fn the_chamber_fluid_and_the_back_vanes_share_no_volume() {
        let _slot = crate::test_slot::take();
        use tp_design::impeller::back_vanes::VaneShape;
        let cad = build_impeller(&with_vanes(VaneShape::Radial, true), &sharp()).unwrap();
        let chambers = cad.side_chambers.as_ref().unwrap();
        let vanes = cad.back_vanes.as_ref().unwrap();
        let shared = chambers
            .common(vanes)
            .expect("the common of the chamber fluid and the vanes")
            .volume_with_tolerance(1e-7);
        assert!(
            shared < 1e-9 * vanes.volume_with_tolerance(1e-7),
            "{shared} mm³ shared with the chamber fluid; {:?}",
            cad.warnings
        );
    }

    #[test]
    fn curved_back_vanes_lean_like_the_blades() {
        let _slot = crate::test_slot::take();
        use tp_design::impeller::back_vanes::VaneShape;
        let curved = VaneShape::Curved {
            angle: 40f64.to_radians(),
        };
        let g = with_vanes(curved, false);
        let vanes = g.contour.back_vanes.as_ref().unwrap();
        let sign = g.basis.rotation_sign;
        // Angle of the vane centre line at its two ends, wrapped to (-pi, pi].
        let sweep = |sign: f64| {
            let chains = vane_outline(vanes, sign, 0.0).unwrap();
            let (a, b) = (&chains[0], &chains[2]);
            let phi = |k: usize| {
                let mid = 0.5 * (a[k] + b[b.len() - 1 - k]);
                mid.y.atan2(mid.x)
            };
            let d = phi(a.len() - 1) - phi(0);
            d.sin().atan2(d.cos())
        };
        let (r_in, r_out) = (vanes.inner_radius, vanes.outer_radius);
        let d = blade_angle_at(&g, r_out) - blade_angle_at(&g, r_in);
        let blade = d.sin().atan2(d.cos());
        assert!(
            blade * sweep(sign) > 0.0,
            "blade {blade}, vane {}",
            sweep(sign)
        );
        assert!(
            sweep(sign) * sweep(-sign) < 0.0,
            "the mirror sense leans the other way"
        );
    }
}
