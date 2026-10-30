//! CAD solids of the volute: the flow domain (the vaneless ring after the impeller, the
//! spiral channel and the discharge diffuser) and the casing around it. A double volute
//! lofts its whole sections as one spiral and cuts the rib out of it. A rounded cutwater
//! is the union of the spiral and the diffuser with their intersection edge rounded.
//!
//! The casing ([`build_volute_casing`]) lofts the outlines of the flow domain offset by
//! the wall thickness ([`VoluteGeometry::casing_lofts`]) and cuts the flow domain and the
//! space of the impeller (r < r₂) out of them. The cutwater, the rib of a double volute and the
//! walls beside the vaneless ring stay as material; the casing ends flat at the outlet
//! of the diffuser and is open toward the impeller.

use tp_design::volute::{CutwaterShape, VoluteGeometry};
use tp_math::{DVec2, DVec3};

use crate::CadError;
use crate::occ::{self, Shape};

/// Millimetres per metre: the CAD model is built in millimetres.
pub(crate) const MM: f64 = 1000.0;

/// CAD solids of one volute, in millimetres.
#[derive(Debug)]
pub struct VoluteCad {
    /// Spiral channel and discharge diffuser. The body of the cutwater may cut the start
    /// of a spiral that starts small off as a solid of its own.
    pub channel: Shape,
    /// The channel plus the ring between impeller outlet and cutwater circle.
    pub fluid: Shape,
    /// What the CAD kernel could not build as designed.
    pub warnings: Vec<String>,
}

impl VoluteCad {
    /// Volume of the flow domain, m³.
    pub fn fluid_volume(&self) -> f64 {
        self.fluid.volume() / MM.powi(3)
    }
}

/// The casing of one volute, in millimetres: the material around the flow domain, as
/// thick as the wall, with the flange at the outlet.
#[derive(Debug)]
pub struct VoluteCasing {
    pub solid: Shape,
    /// Whether the flange is on the casing.
    pub flange: bool,
    /// What could not be built as asked.
    pub warnings: Vec<String>,
}

impl VoluteCasing {
    /// Volume of the material, m³.
    pub fn volume(&self) -> f64 {
        self.solid.volume() / MM.powi(3)
    }
}

/// Closed wire through one section: one edge per smooth piece of the outline.
pub(crate) fn section_wire(ring: &[DVec3], breaks: &[usize]) -> Result<Shape, CadError> {
    let points: Vec<DVec3> = ring.iter().map(|p| *p * MM).collect();
    if breaks.len() < 2 {
        return occ::wire(&[occ::spline_edge(&points, true)?]);
    }
    let n = points.len();
    let edges = breaks
        .iter()
        .enumerate()
        .map(|(k, &start)| {
            let end = breaks.get(k + 1).copied().unwrap_or(n);
            // Each piece runs to the first point of the next one.
            let piece: Vec<DVec3> = (start..=end).map(|i| points[i % n]).collect();
            if piece.len() == 2 {
                occ::line_edge(piece[0], piece[1])
            } else {
                occ::spline_edge(&piece, false)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    occ::wire(&edges)
}

/// Closed wire through a meridional profile (r, z) in the plane y = 0: a line for each
/// piece of two points, a spline through the others.
fn profile_wire(pieces: &[Vec<DVec2>]) -> Result<Shape, CadError> {
    let at = |p: &DVec2| DVec3::new(p.x * MM, 0.0, p.y * MM);
    let edges = pieces
        .iter()
        .map(|piece| {
            let points: Vec<DVec3> = piece.iter().map(at).collect();
            if points.len() == 2 {
                occ::line_edge(points[0], points[1])
            } else {
                occ::spline_edge(&points, false)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    occ::wire(&edges)
}

/// The closed polygon `points` split at its corners, where it turns by more than 10°:
/// straight runs of two points and curved runs for splines, so that a revolved profile
/// has a few faces instead of one per segment.
fn smooth_pieces(points: &[DVec2]) -> Vec<Vec<DVec2>> {
    let n = points.len();
    let corner = |i: usize| {
        let a = (points[i] - points[(i + n - 1) % n]).normalize_or_zero();
        let b = (points[(i + 1) % n] - points[i]).normalize_or_zero();
        a.dot(b) < 10f64.to_radians().cos()
    };
    let corners: Vec<usize> = (0..n).filter(|&i| corner(i)).collect();
    if corners.is_empty() {
        let mut all = points.to_vec();
        all.push(points[0]);
        return vec![all];
    }
    corners
        .iter()
        .enumerate()
        .map(|(k, &start)| {
            let end = corners[(k + 1) % corners.len()];
            let span = match (end + n - start) % n {
                0 => n,
                span => span,
            };
            (0..=span).map(|j| points[(start + j) % n]).collect()
        })
        .collect()
}

/// Gaps in millimetres the boolean operations try in turn, until the flow domain comes
/// out as one valid solid: first the exact algorithm, then fuzzy ones. Where the spiral
/// leaves the throat, the diffuser and the start of the spiral share their side walls
/// for a stretch, and lofted walls run nearly coincident with each other or with the
/// inlet ring.
pub(crate) const FUZZY_GAPS: [f64; 5] = [0.0, 1e-3, 1e-4, 1e-2, 1e-1];

/// One solid lofted through each sequence of outlines.
fn loft_pieces(pieces: &[Vec<Vec<DVec3>>], breaks: &[usize]) -> Result<Vec<Shape>, CadError> {
    pieces
        .iter()
        .map(|rings| {
            let wires = rings
                .iter()
                .map(|ring| section_wire(ring, breaks))
                .collect::<Result<Vec<_>, _>>()?;
            occ::loft(&wires)
        })
        .collect()
}

/// The first attempt over [`FUZZY_GAPS`] whose result is `sound`, or turns sound once
/// `repair`ed, else the first that built at all. Repairing a nearly valid result by
/// shape healing costs far less than a boolean operation with the next gap.
pub(crate) fn first_sound<T>(
    attempt: impl Fn(f64) -> Result<T, CadError>,
    sound: impl Fn(&T) -> bool,
    repair: impl Fn(&T) -> Option<T>,
) -> Result<T, CadError> {
    first_sound_over(&FUZZY_GAPS, attempt, sound, repair)
}

/// As [`first_sound`], over the gaps `gaps`.
fn first_sound_over<T>(
    gaps: &[f64],
    attempt: impl Fn(f64) -> Result<T, CadError>,
    sound: impl Fn(&T) -> bool,
    repair: impl Fn(&T) -> Option<T>,
) -> Result<T, CadError> {
    let mut fallback = None;
    let mut error = None;
    for &fuzzy in gaps {
        match attempt(fuzzy) {
            Ok(result) if sound(&result) => return Ok(result),
            Ok(result) => {
                if let Some(fixed) = repair(&result).filter(|r| sound(r)) {
                    return Ok(fixed);
                }
                fallback.get_or_insert(result);
            }
            Err(e) => {
                error.get_or_insert(e);
            }
        }
    }
    fallback.map_or_else(|| Err(error.expect("at least one attempt")), Ok)
}

/// Least share of the channel's volume a rounded channel must keep: the fillet of the
/// cutwater edge takes a sliver away, never a piece.
const FILLET_KEEPS: f64 = 0.97;

/// Volume of the flow domain the design describes, m³, without the CAD: the spiral
/// sections swept about the axis (Pappus), the straight diffuser and the ring between
/// d₂ and the cutwater circle, with the bend and neck of an internal volute. The two
/// overlap a little at the cutwater, so it is a check on the solid, good to a few
/// percent, not its exact volume.
pub fn expected_fluid_volume(g: &VoluteGeometry) -> f64 {
    let swept = |sections: &[tp_design::volute::VoluteSection]| -> f64 {
        sections
            .windows(2)
            .map(|w| {
                let a = 0.5 * (w[0].size.area + w[1].size.area);
                let r = 0.5 * (w[0].size.centroid_radius + w[1].size.centroid_radius);
                a * r * (w[1].angle - w[0].angle)
            })
            .sum()
    };
    let spiral = if g.design.free_double().is_some() {
        // One channel of whole sections from the rib nose on, with the rib carved out of
        // it, as the CAD builds it: the rib's plan area over its height.
        let rib = g.rib.as_ref().map_or(0.0, |rib| {
            let area = |plane: &tp_design::volute::RibPlane| {
                let o = plane.outline();
                let n = o.len();
                0.5 * (0..n)
                    .map(|i| o[i].x * o[(i + 1) % n].y - o[(i + 1) % n].x * o[i].y)
                    .sum::<f64>()
                    .abs()
            };
            rib.planes
                .windows(2)
                .map(|w| 0.5 * (area(&w[0]) + area(&w[1])) * (w[1].z - w[0].z).abs())
                .sum()
        });
        let first = &g.sections[..g.sections.len().saturating_sub(1)];
        swept(first) + swept(&g.envelope) - rib
    } else {
        // A rib that stops short of the throat leaves whole sections after it.
        swept(&g.sections) + swept(&g.inner_sections) + swept(&g.outer_channel) + swept(&g.merged)
    };
    let nozzle: f64 = g
        .nozzle
        .windows(2)
        .map(|w| 0.5 * (w[0].area + w[1].area) * w[0].center.distance(w[1].center))
        .sum();
    // The ring the CAD revolves, with the bend and neck of an internal volute (Pappus).
    let mut profile: Vec<tp_math::DVec2> = Vec::new();
    for point in g.inlet_profile().into_iter().flatten() {
        if profile
            .last()
            .is_none_or(|last| last.distance(point) > 1e-12)
        {
            profile.push(point);
        }
    }
    let n = profile.len();
    // ∬ r dA over the profile, from its edges.
    let moment: f64 = (0..n)
        .map(|i| {
            let (a, b) = (profile[i], profile[(i + 1) % n]);
            (a.x + b.x) * (a.x * b.y - b.x * a.y) / 6.0
        })
        .sum();
    let ring = std::f64::consts::TAU * moment.abs();
    spiral + nozzle + ring
}

/// Builds the volute channel and its flow domain from the geometry.
pub fn build_volute(g: &VoluteGeometry) -> Result<VoluteCad, CadError> {
    let result = crate::profile::stage("volute flow domain", || flow_domain(g, true))
        .and_then(|cad| holds_the_design(g, &cad.fluid).map(|()| cad));
    // A rounded cutwater that does not cut cleanly leaves the flow domain unsound: build
    // it again with the edge sharp.
    let sound = |c: &VoluteCad| c.fluid.is_valid() && c.fluid.solid_count() == 1;
    let result = if g.design.cutwater_shape.is_fillet() && !result.as_ref().is_ok_and(sound) {
        // The design of a sharp one: the same spiral and diffuser, with no round to cut.
        let mut sharp_design = g.clone();
        sharp_design.design.cutwater_shape = CutwaterShape::Sharp;
        match crate::profile::stage("volute flow domain, sharp", || {
            flow_domain(&sharp_design, false)
                .and_then(|cad| holds_the_design(g, &cad.fluid).map(|()| cad))
        }) {
            Ok(mut sharp) if sound(&sharp) => {
                sharp.warnings.push(
                    "the CAD kernel could not cut the rounded cutwater cleanly: the CAD keeps \
                     it sharp"
                        .to_owned(),
                );
                Ok(sharp)
            }
            _ => result,
        }
    } else {
        result
    };
    crate::profile::report("the volute flow domain");
    result
}

/// Largest share by which the volume of a flow domain may part from the one the
/// design describes ([`expected_fluid_volume`]). A boolean that drops a piece (the
/// whole diffuser, on rectangular sections whose side walls lie in its planes) can
/// still leave one valid solid; the designs that build differ from it by under 2.5 %.
const VOLUME_SHARE: f64 = 0.05;

/// Whether `fluid` (mm) holds about the volume the design `g` describes.
pub(crate) fn holds_the_design(g: &VoluteGeometry, fluid: &Shape) -> Result<(), CadError> {
    let expected = expected_fluid_volume(g) * MM.powi(3);
    let off = fluid.volume_with_tolerance(0.0) / expected - 1.0;
    if off.abs() > VOLUME_SHARE {
        return Err(CadError::Operation(format!(
            "the flow domain came out {:+.0} % off the volume of the design: the CAD kernel \
             lost or added a piece",
            off * 100.0
        )));
    }
    Ok(())
}

/// The flow domain of the volute, with the cutwater rounded when it is a fillet and
/// `round`.
fn flow_domain(g: &VoluteGeometry, round: bool) -> Result<VoluteCad, CadError> {
    let breaks = g.ring_breaks();
    let lofts = crate::profile::stage("lofts", || loft_pieces(&g.lofts(), &breaks))?;

    // Ring between the impeller outlet and the cutwater circle, as wide as the volute
    // inlet, with the bend and neck of an internal volute. It reaches slightly into the
    // channel so the two solids overlap.
    let ring = occ::revolve_z(&occ::planar_face(&profile_wire(&g.inlet_profile())?)?)?;

    // A failed fusion may still give one valid solid that lost a piece: the channel must
    // keep about the volume of its pieces, which only overlap where the spiral reaches
    // into the diffuser, and the flow domain most of the ring besides (quick estimates).
    // The channel and the flow domain may each need their own gap.
    let pieces: f64 = lofts.iter().map(|l| l.volume_with_tolerance(0.0)).sum();
    let ring_volume = ring.volume_with_tolerance(0.0);
    let whole = |channel: &Shape| channel.volume_with_tolerance(0.0) > 0.9 * pieces;
    let mut joined = crate::profile::stage("channel fuse", || {
        first_sound(
            |fuzzy| occ::fuse_all_fuzzy(&lofts, fuzzy),
            |channel| channel.is_valid() && channel.solid_count() == 1 && whole(channel),
            |channel| channel.fixed().ok(),
        )
    })?;

    // A rounded cutwater is the union as it is, its intersection edge rounded by the
    // kernel (a fillet) or by the sliver the design's closing cuts off; where neither
    // works, the edge stays sharp.
    let mut notes = Vec::new();
    // Sections of radii and segments are left sharp: their casing does not come out of a
    // rounded flow domain.
    if g.design.cutwater_shape.is_fillet() && g.has_polyline_sides() {
        notes.push("the CAD keeps the cutwater of sections of radii and segments sharp".to_owned());
    } else if g.design.cutwater_shape.is_fillet() && round {
        let radius = g.design.nose_radius * MM;
        // A fillet only takes a sliver away: a result that lost more lost a piece (the
        // kernel has returned the spiral without the diffuser as one valid solid on some
        // platforms), and does not count as rounded.
        let whole_volume = joined.volume_with_tolerance(0.0);
        let keeps_whole = |shape: &Shape| {
            shape.is_valid()
                && shape.solid_count() == 1
                && shape.volume_with_tolerance(0.0) > FILLET_KEEPS * whole_volume
        };
        // The kernel's fillet, for an arc; else adding the sliver the closing cuts off.
        let kernel = if g.design.cutwater_shape.is_simple() {
            None
        } else {
            crate::profile::stage("fillet tip", || fillet_tip(&joined, &lofts, g, radius))
                .filter(|(shape, _)| keeps_whole(shape))
        };
        let rounded = kernel.or_else(|| {
            crate::profile::stage("round tip", || {
                crate::rounding::round_tip(&joined, g).map(|rounded| (rounded, radius))
            })
        });
        match rounded {
            Some((rounded, r)) => {
                joined = rounded;
                if r < radius * (1.0 - 1e-9) {
                    notes.push(format!(
                        "the cutwater is rounded with {r:.1} mm, the most the CAD kernel takes \
                         there"
                    ));
                }
            }
            None => notes.push(
                "the CAD kernel could not round the cutwater edge: the CAD keeps it sharp"
                    .to_owned(),
            ),
        }
    }

    // The rib of a double volute is carved out of the flow domain across the whole
    // height of the volute, lofted through its outlines plane by plane.
    let (z_lo, z_hi) = tool_span(g);
    // Each tool with the points its cut must leave on the surface of the channel and
    // of the ring.
    let mut tools: Vec<(Shape, Vec<DVec3>, Vec<DVec3>)> = Vec::new();
    let rib = rib_body(g, z_lo, z_hi)?;
    if let Some(rib) = &rib {
        tools.push((rib.copied()?, rib_probes(g), Vec::new()));
    }

    // The cutwater and the rib come out of the channel and of the ring apart, before
    // they are joined: the ring needs a gap to join some channels (external round
    // sections), and the body of the cutwater no longer cuts cleanly out of that
    // union, while it does out of each piece. Each tool finds its own gap. A cut that
    // fails may still give one valid solid, even one a little smaller: the body's cut
    // must leave its points on the surface (the faces on the channel, the nose on the
    // ring, both on the flow domain), the rib's take something away. The body may cut
    // the start of a spiral that starts small off the rest of the channel: the ring
    // joins them again.
    #[derive(Clone, Copy, PartialEq)]
    enum Part {
        Channel,
        Ring,
        Domain,
    }
    let carve = |shape: &Shape, part: Part| -> Result<Shape, CadError> {
        let mut carved = shape.copied()?;
        for (tool, faces, noses) in &tools {
            let probes: Vec<DVec3> = match part {
                Part::Channel => faces.clone(),
                Part::Ring => noses.clone(),
                Part::Domain => faces.iter().chain(noses).copied().collect(),
            };
            let before = carved.volume_with_tolerance(0.0);
            let next = first_sound(
                |fuzzy| carved.cut_fuzzy(tool, fuzzy),
                |cut| {
                    cut.is_valid()
                        && (cut.solid_count() == 1
                            || part == Part::Channel && cut.solid_count() > 1)
                        && if probes.is_empty() {
                            cut.volume_with_tolerance(0.0) < before
                        } else {
                            on_surface(cut, &probes)
                        }
                },
                |cut| cut.fixed().ok(),
            )?;
            carved = next;
        }
        Ok(carved)
    };
    let channel = crate::profile::stage("carve channel", || carve(&joined, Part::Channel))?;
    let (channel, corners_rounded) = match &rib {
        Some(rib) => crate::profile::stage("rib corners", || {
            round_rib_corners(g, channel, rib, &mut notes)
        }),
        None => (channel, false),
    };
    let inlet = crate::profile::stage("carve ring", || carve(&ring, Part::Ring))?;
    // The flow domain is what gets exported. A fuzzy fusion may move the cut body
    // again (a tapered nose, a compensated spiral), so its points must still lie on the
    // surface.
    let probes: Vec<DVec3> = tools
        .iter()
        .flat_map(|(_, faces, noses)| faces.iter().chain(noses))
        .copied()
        .collect();
    let domain = |fluid: &Shape| {
        fluid.is_valid()
            && fluid.solid_count() == 1
            && fluid.volume_with_tolerance(0.0)
                > channel.volume_with_tolerance(0.0) + 0.5 * ring_volume
    };
    // The first fusion that keeps the cut on the surface; else the first that is
    // only a sound flow domain, and the first that built at all.
    let (mut plain, mut built) = (None, None);
    for fuzzy in FUZZY_GAPS {
        let Ok(fused) = inlet.fuse_fuzzy(&channel, fuzzy) else {
            continue;
        };
        for fluid in std::iter::once(fused.copied()?).chain(fused.fixed().ok()) {
            if domain(&fluid) {
                if on_surface(&fluid, &probes) {
                    return Ok(VoluteCad {
                        channel,
                        fluid,
                        warnings: notes,
                    });
                }
                plain.get_or_insert(fluid);
            } else {
                built.get_or_insert(fluid);
            }
        }
    }
    // Else the tools come out of the union of the uncut ring and channel, which works
    // where the carved pieces do not join.
    let united = first_sound(
        |fuzzy| ring.fuse_fuzzy(&joined, fuzzy),
        |union| {
            union.is_valid()
                && union.solid_count() == 1
                && union.volume_with_tolerance(0.0) > 0.9 * pieces + 0.5 * ring_volume
        },
        |union| union.fixed().ok(),
    );
    if corners_rounded {
        notes.push(
            "the corners of the rib are left sharp: the inlet ring joins only the channel \
             before the rib comes out"
                .to_owned(),
        );
    }
    if let Ok(fluid) = united.and_then(|union| carve(&union, Part::Domain))
        && domain(&fluid)
    {
        if on_surface(&fluid, &probes) {
            return Ok(VoluteCad {
                channel,
                fluid,
                warnings: notes,
            });
        }
        plain.get_or_insert(fluid);
    }
    // Else a sound domain whose cut lies off the design, saying by how much, or the
    // first that built at all.
    let warning = if let Some(fluid) = &plain {
        let off = fluid
            .boundary_distances(&probes)
            .map_or(f64::INFINITY, |d| {
                d.into_iter()
                    .map(|d| if d.is_nan() { f64::INFINITY } else { d })
                    .fold(0.0, f64::max)
            });
        format!(
            "the cutwater of the CAD solid lies up to {off:.1} mm off its design: the CAD \
             kernel could not cut it cleanly"
        )
    } else {
        "the flow domain of the CAD is not one valid solid: the CAD kernel could not cut \
         the cutwater cleanly"
            .to_owned()
    };
    let fluid = plain
        .or(built)
        .ok_or_else(|| CadError::Operation("the flow domain of the volute failed".into()))?;
    notes.push(warning);
    let warnings = notes;
    Ok(VoluteCad {
        channel,
        fluid,
        warnings,
    })
}

/// The height the bodies carved out of the flow domain span, mm: the volute's, with a
/// tenth of it to spare on each side.
fn tool_span(g: &VoluteGeometry) -> (f64, f64) {
    let zs = g.rings().into_iter().flatten().map(|p| p.z);
    let (lo, hi) = zs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
        (lo.min(z), hi.max(z))
    });
    let margin = 0.1 * (hi - lo);
    ((lo - margin) * MM, (hi + margin) * MM)
}

/// Points sampled along each edge when looking for the corners of the rib.
const CORNER_SAMPLES: usize = 7;

/// How far from the rib the points of one of its corner edges may lie, mm.
const CORNER_TOLERANCE: f64 = 0.05;

/// Shares of the corner radius of the rib tried, largest first.
const CORNER_SHARES: [f64; 4] = [1.0, 0.75, 0.5, 0.25];

/// The edges of the flow domain `fluid` where the rib meets the walls: those lying on
/// the rib's surface that run along it, not across the height like the seams between
/// its faces.
fn rib_corner_edges(g: &VoluteGeometry, fluid: &Shape, rib: &Shape) -> Vec<usize> {
    let Some(geometry) = &g.rib else {
        return Vec::new();
    };
    // A quick sieve first: within 2 mm of the outline of the rib in its plane closest
    // in height.
    let near = |p: DVec3| {
        let z = p.z / MM;
        let Some(plane) = geometry
            .planes
            .iter()
            .min_by(|a, b| (a.z - z).abs().total_cmp(&(b.z - z).abs()))
        else {
            return false;
        };
        let q = (p / MM).truncate();
        let outline: Vec<DVec3> = plane.outline();
        let closed: Vec<DVec3> = outline.iter().chain(outline.first()).copied().collect();
        closed.windows(2).any(|w| {
            let (a, b) = (w[0].truncate(), w[1].truncate());
            let ab = b - a;
            let s = ((q - a).dot(ab) / ab.length_squared().max(1e-30)).clamp(0.0, 1.0);
            (a + ab * s).distance(q) < 2e-3
        })
    };
    fluid
        .edge_samples(CORNER_SAMPLES)
        .iter()
        .enumerate()
        .filter(|(_, points)| points.len() > 2)
        .filter(|(_, points)| {
            let (lo, hi) = points.iter().fold(
                (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            );
            let span = hi - lo;
            span.truncate().length() > span.z && points.iter().all(|p| near(*p))
        })
        .filter(|(_, points)| {
            rib.boundary_distances(points)
                .is_ok_and(|d| d.iter().all(|d| *d < CORNER_TOLERANCE))
        })
        .map(|(i, _)| i)
        .collect()
}

/// The channel `channel` with the corners where the rib `rib` of a double volute meets
/// its walls rounded with the corner radius of the rib, or with as much of it as the CAD
/// kernel takes, and whether they are rounded; sharp, with a note in `notes`, where it
/// takes none. The channel is rounded before the inlet ring joins it: the ring leaves
/// steps of hundredths of a millimetre by the rib nose that no fillet runs over.
fn round_rib_corners(
    g: &VoluteGeometry,
    channel: Shape,
    rib: &Shape,
    notes: &mut Vec<String>,
) -> (Shape, bool) {
    let Some(radius) = g
        .design
        .rib
        .map(|r| r.corner_radius * MM)
        .filter(|r| *r > 0.0)
    else {
        return (channel, false);
    };
    let edges = rib_corner_edges(g, &channel, rib);
    if edges.is_empty() {
        notes.push(
            "the corners of the rib are left sharp: the CAD found no edge where it meets \
             the walls"
                .to_owned(),
        );
        return (channel, false);
    }
    let volume = channel.volume_with_tolerance(0.0);
    for share in CORNER_SHARES {
        let r = share * radius;
        let Ok(rounded) = channel.filleted(&edges, r) else {
            continue;
        };
        // A concave fillet only takes fluid away, and little of it.
        let v = rounded.volume_with_tolerance(0.0);
        if rounded.is_valid() && rounded.solid_count() == 1 && v < volume && v > 0.99 * volume {
            if share < 1.0 {
                notes.push(format!(
                    "the corners of the rib are rounded with {r:.1} mm, the most the CAD \
                     kernel takes there"
                ));
            }
            return (rounded, true);
        }
    }
    notes.push(
        "the CAD kernel could not round the corners of the rib: the CAD keeps them sharp"
            .to_owned(),
    );
    (channel, false)
}

/// The rib of a double volute, mm: lofted smoothly through its planes, and carried
/// straight on from the two end planes to `z_lo` and `z_hi`. `None` for a single volute.
fn rib_body(g: &VoluteGeometry, z_lo: f64, z_hi: f64) -> Result<Option<Shape>, CadError> {
    let Some(rib) = &g.rib else {
        return Ok(None);
    };
    let wire = |p: &tp_design::volute::RibPlane, z: f64| -> Result<Shape, CadError> {
        let spline = |points: &mut dyn Iterator<Item = &DVec3>| {
            let points: Vec<DVec3> = points.map(|q| DVec3::new(q.x * MM, q.y * MM, z)).collect();
            occ::spline_edge(&points, false)
        };
        occ::wire(&[
            spline(&mut p.nose.iter())?,
            spline(&mut p.inner_face.iter())?,
            spline(&mut p.tail.iter())?,
            spline(&mut p.outer_face.iter().rev())?,
        ])
    };
    let (Some(first), Some(last)) = (rib.planes.first(), rib.planes.last()) else {
        return Ok(None);
    };
    let extend = |p: &tp_design::volute::RibPlane, to: f64| -> Result<Shape, CadError> {
        let from = p.z * MM;
        occ::prism_z(
            &occ::planar_face(&wire(p, from.min(to))?)?,
            (to - from).abs(),
        )
    };
    let wires = rib
        .planes
        .iter()
        .map(|p| wire(p, p.z * MM))
        .collect::<Result<Vec<_>, _>>()?;
    let below = extend(first, z_lo)?;
    let above = extend(last, z_hi)?;
    Ok(Some(occ::loft(&wires)?.fuse(&below)?.fuse(&above)?))
}

/// The tips of the rib nose in its planes across the inlet width, mm: where the sections
/// surely reach, so that a cut leaves them on the surface of the channel.
fn rib_probes(g: &VoluteGeometry) -> Vec<DVec3> {
    let (zm, half) = (g.basis.midplane, 0.45 * g.design.inlet_width);
    g.rib
        .iter()
        .flat_map(|r| &r.planes)
        .filter(|p| (p.z - zm).abs() < half)
        .map(|p| p.nose[p.nose.len() / 2] * MM)
        .collect()
}

/// How far from the surface of a carved shape the points of the body may lie, mm.
const PROBE_TOLERANCE: f64 = 0.02;

/// The outlines of the body in the planes of the cutwater clear of its side endings:
/// next to a side wall part of an outline may lie outside the flow, beyond the wall.
#[cfg(test)]
fn clear_planes(g: &VoluteGeometry) -> Vec<Vec<DVec3>> {
    let reach = 0.5 * g.design.inlet_width;
    let zm = g.basis.midplane;
    g.cutwater_planes()
        .into_iter()
        .filter(|o| {
            o.first()
                .is_some_and(|p| (p.z - zm).abs() <= reach.max(1e-9))
        })
        .collect()
}

/// Whether every one of `points` lies on the surface of `shape`.
fn on_surface(shape: &Shape, points: &[DVec3]) -> bool {
    points.is_empty()
        || shape
            .boundary_distances(points)
            .is_ok_and(|d| d.iter().all(|d| *d < PROBE_TOLERANCE))
}

/// Builds the casing around the flow domain `cad` of the volute `g`.
/// Sequences of outlines to loft through, one per piece.
type Lofts = Vec<Vec<Vec<DVec3>>>;

/// The outlines of the casing lofted, mm. The start of the spiral runs just inside its
/// throat, the cutwater between them: offset outward, the two would cut into each other
/// within one loft. Each piece is lofted in two halves, which meet on their middle
/// outline. `thin`ned, the outer wall leaves out the outlines and points it can
/// ([`CASING_TOLERANCE`]).
fn casing_pieces(
    g: &VoluteGeometry,
    overlap: f64,
    thin: bool,
) -> Result<(Vec<Shape>, Lofts), CadError> {
    let halves: Lofts = g
        .casing_lofts_with(overlap)
        .into_iter()
        .flat_map(|rings| {
            let mid = rings.len() / 2;
            if mid < 2 {
                vec![rings]
            } else {
                vec![rings[..=mid].to_vec(), rings[mid..].to_vec()]
            }
        })
        .collect();
    if !thin {
        return Ok((loft_pieces(&halves, &g.ring_breaks())?, halves));
    }
    // The outer wall only needs to keep the casing about as thick as designed: lofted
    // through fewer outlines and points its booleans run many times faster.
    let (thin, breaks) = crate::thin::thinned(&halves, &g.ring_breaks(), CASING_TOLERANCE);
    crate::profile::note(|| {
        let count = |p: &Lofts| (p.iter().map(Vec::len).sum::<usize>(), p[0][0].len());
        format!(
            "casing outlines x points: {:?} -> {:?}",
            count(&halves),
            count(&thin)
        )
    });
    Ok((loft_pieces(&thin, &breaks)?, halves))
}

/// How far the outer wall of a casing may stray from the outlines offset from the flow,
/// m: well within casting tolerances, and a small share of any wall.
const CASING_TOLERANCE: f64 = 1e-4;

/// The turns by which the pieces of spiral of a casing reach into the diffuser, rad. The
/// union of the pieces of a section with many corners (radius based, line segments) is a
/// solid at some of them and not at the ones between; these are the ones that gave a
/// solid, in order of preference.
const FREE_CASING_OVERLAPS: [f64; 2] = [
    6.0 * std::f64::consts::PI / 180.0,
    0.4 * std::f64::consts::PI / 180.0,
];

/// Builds the casing around the flow domain `cad` of the volute `g`.
pub fn build_volute_casing(g: &VoluteGeometry, cad: &VoluteCad) -> Result<VoluteCasing, CadError> {
    let result = crate::profile::stage("volute casing", || casing(g, cad));
    let sound = |c: &VoluteCasing| c.solid.is_valid() && c.solid.solid_count() == 1;
    // A casing that does not come out around a rounded cutwater is built around a sharp
    // one: the same spiral and diffuser, a sliver of flow less at the tip.
    let result = if g.design.cutwater_shape.is_fillet() && !result.as_ref().is_ok_and(sound) {
        let mut sharp_design = g.clone();
        sharp_design.design.cutwater_shape = CutwaterShape::Sharp;
        let sharp = crate::profile::stage("volute casing, sharp", || {
            let flow = flow_domain(&sharp_design, false)?;
            casing(&sharp_design, &flow)
        });
        match sharp {
            Ok(mut casing) if sound(&casing) => {
                casing.warnings.push(
                    "the CAD kernel could not build the casing around the rounded cutwater: \
                     the casing fits a sharp one"
                        .to_owned(),
                );
                Ok(casing)
            }
            _ => result,
        }
    } else {
        result
    };
    crate::profile::report("the volute casing");
    result
}

fn casing(g: &VoluteGeometry, cad: &VoluteCad) -> Result<VoluteCasing, CadError> {
    // The outer lofts of a diffuser tilted along the pump axis do not fuse into one
    // casing (tried from 10° to 25°: the casing came out in 8 pieces, with or without
    // the flange), so it is refused instead of returned broken.
    if g.design.diffuser.z_bend.is_some() {
        return Err(CadError::Input(
            "the casing of a diffuser with an axial bend cannot be built yet: the flow \
             domain can"
                .into(),
        ));
    }
    let overlaps: &[f64] = if g.has_polyline_sides() {
        &FREE_CASING_OVERLAPS
    } else {
        &[tp_design::volute::CHANNEL_OVERLAP]
    };
    // First the outer wall thinned, with the exact booleans only; where that is not a
    // sound casing (walls of the spiral and the diffuser that run together, thinned
    // apart), through every outline with every gap.
    if let Ok(body) = crate::profile::stage("thinned casing body", || {
        casing_body(g, cad, overlaps[0], true)
    }) && body.sound
    {
        return crate::profile::stage("flange", || finish_casing(g, body));
    }
    let mut fallback = None;
    let mut error = None;
    for &overlap in overlaps {
        match crate::profile::stage(&format!("casing body overlap {overlap:.4}"), || {
            casing_body(g, cad, overlap, false)
        }) {
            Ok(body) if body.sound => {
                return crate::profile::stage("flange", || finish_casing(g, body));
            }
            Ok(body) => {
                fallback.get_or_insert(body);
            }
            Err(e) => {
                error.get_or_insert(e);
            }
        }
    }
    match fallback {
        Some(body) => finish_casing(g, body),
        None => Err(error.expect("at least one attempt")),
    }
}

/// The casing before its flange, and whether it is a sound solid.
struct CasingBody {
    solid: Shape,
    rounded: Option<f64>,
    radius: f64,
    sound: bool,
}

/// The casing of the pieces of spiral that reach `overlap` rad into the diffuser; with a
/// `thin` outer wall, by the exact booleans only.
fn casing_body(
    g: &VoluteGeometry,
    cad: &VoluteCad,
    overlap: f64,
    thin: bool,
) -> Result<CasingBody, CadError> {
    let (mut outer, halves) =
        crate::profile::stage("casing lofts", || casing_pieces(g, overlap, thin))?;
    // The wall round the bend and neck of an internal volute.
    if let Some(profile) = g.bend_casing_profile() {
        let pieces = smooth_pieces(&profile);
        outer.push(occ::revolve_z(&occ::planar_face(&profile_wire(&pieces)?)?)?);
    }
    // The space of the impeller, through the height of the casing on its side.
    let zs = halves.iter().flatten().flatten().map(|p| p.z);
    let (z_lo, z_hi) = zs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
        (lo.min(z), hi.max(z))
    });
    let margin = 0.1 * (z_hi - z_lo);
    let (band_lo, band_hi) = g.impeller_band();
    let lo = band_lo.max(z_lo - margin) * MM;
    let hi = band_hi.min(z_hi + margin) * MM;
    let r2 = g.inlet_ring().0 * MM;
    let point = |r: f64, z: f64| DVec3::new(r, 0.0, z);
    let impeller = occ::revolve_z(&occ::planar_face(&occ::wire(&[
        occ::line_edge(point(0.0, lo), point(r2, lo))?,
        occ::line_edge(point(r2, lo), point(r2, hi))?,
        occ::line_edge(point(r2, hi), point(0.0, hi))?,
        occ::line_edge(point(0.0, hi), point(0.0, lo))?,
    ])?)?)?;
    // A failed boolean may still give one valid solid: the casing must also have lost
    // the channel out of its outer volume (quick estimates of the volumes).
    let channel = cad.channel.volume_with_tolerance(0.0);
    // The groove where the back of the start of the spiral runs into the diffuser's
    // casing, over the cutwater, filled and rounded before the flow domain comes out:
    // the fillet only adds material outside, so it cannot reach the flow.
    // The fillet of a section of radii or segments is skipped: the kernel does not come
    // back from rounding there.
    let radius = if g.has_polyline_sides() {
        0.0
    } else {
        g.design.cutwater_fillet * MM
    };
    let enclose = |fuzzy: f64| -> Result<(Shape, f64, Option<f64>), CadError> {
        let mut whole = crate::profile::stage(&format!("outer fuse {fuzzy}"), || {
            occ::fuse_all_fuzzy(&outer, fuzzy)
        })?;
        // The radius it took; 0 when asked for none.
        let mut rounded = Some(0.0);
        if radius > 0.0 && !g.cutwater_slices.is_empty() {
            rounded =
                crate::profile::stage("fill groove", || fill_groove(&whole, &outer, g, radius))
                    .map(|(filled, r)| {
                        whole = filled;
                        r
                    });
        }
        let gross = whole.volume_with_tolerance(0.0);
        // The flow and the space of the impeller come out together.
        let solid = crate::profile::stage(&format!("cut flow {fuzzy}"), || {
            whole.cut_all_fuzzy(&[&cad.fluid, &impeller], fuzzy)
        })?;
        Ok((solid, gross, rounded))
    };
    let sound = |(solid, gross, _): &(Shape, f64, Option<f64>)| {
        solid.is_valid()
            && solid.solid_count() == 1
            && solid.volume_with_tolerance(0.0) < gross - 0.8 * channel
    };
    let gaps: &[f64] = if thin { &[0.0] } else { &FUZZY_GAPS };
    let result = first_sound_over(gaps, enclose, sound, |(solid, gross, rounded)| {
        Some((solid.fixed().ok()?, *gross, *rounded))
    })?;
    Ok(CasingBody {
        sound: sound(&result),
        solid: result.0,
        rounded: result.2,
        radius,
    })
}

/// The casing `body` with the flange at the outlet of its diffuser.
fn finish_casing(g: &VoluteGeometry, body: CasingBody) -> Result<VoluteCasing, CadError> {
    let CasingBody {
        solid,
        rounded,
        radius,
        ..
    } = body;
    let mut warnings = Vec::new();
    match rounded {
        None => warnings
            .push("the groove over the cutwater could not be rounded: it stays sharp".to_owned()),
        Some(r) if radius > 0.0 && r < radius => warnings.push(format!(
            "the groove over the cutwater is rounded with {r:.1} mm, the most the CAD kernel \
             takes there"
        )),
        Some(_) => {}
    }
    if g.has_polyline_sides() && g.design.cutwater_fillet > 0.0 {
        warnings.push(
            "the groove over the cutwater stays sharp: sections of radii and segments are not \
             rounded"
                .to_owned(),
        );
    }
    let Some(f) = &g.design.flange else {
        return Ok(VoluteCasing {
            solid,
            flange: false,
            warnings,
        });
    };
    match flange(g, f).and_then(|disk| {
        let joined = solid.fuse(&disk)?;
        Ok(joined)
    }) {
        Ok(joined) if joined.is_valid() && joined.solid_count() == 1 => Ok(VoluteCasing {
            solid: joined,
            flange: true,
            warnings,
        }),
        result => {
            warnings.push(format!(
                "the flange could not be joined to the casing{}: the casing ends flat",
                result.err().map_or(String::new(), |e| format!(" ({e})"))
            ));
            Ok(VoluteCasing {
                solid,
                flange: false,
                warnings,
            })
        }
    }
}

/// The edges of `whole`, the fused outer lofts `pieces`, along the groove where the back
/// of the start of the spiral runs into the diffuser's casing, over the cutwater: edges
/// the fusion made (no loft has them), near the cutwater and farther from the axis than
/// its tip. All of them, and those that run across the height.
fn groove_edges(whole: &Shape, pieces: &[Shape], g: &VoluteGeometry) -> (Vec<usize>, Vec<usize>) {
    const SAMPLES: usize = 17;
    let lofted: Vec<Vec<DVec3>> = pieces
        .iter()
        .flat_map(|p| p.edge_samples(4 * SAMPLES))
        .filter(|e| !e.is_empty())
        .collect();
    // Distance from `p` to the nearest lofted edge, along its samples.
    let off_the_lofts = |p: DVec3| {
        lofted
            .iter()
            .flat_map(|e| e.windows(2))
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let t = ((p - a).dot(b - a) / (b - a).length_squared().max(1e-24)).clamp(0.0, 1.0);
                p.distance(a + (b - a) * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let tip = g.cutwater.tip * MM;
    let t = g.design.wall_thickness * MM;
    let mut all = Vec::new();
    let mut across = Vec::new();
    for (k, e) in whole.edge_samples(SAMPLES).iter().enumerate() {
        if e.is_empty() {
            continue;
        }
        let mid = e[SAMPLES / 2];
        let near = e
            .iter()
            .all(|p| p.truncate().distance(tip.truncate()) < 10.0 * t);
        let behind = mid.truncate().length() > tip.truncate().length() + 0.5 * t;
        if !(near && behind) || off_the_lofts(mid) < 0.01 {
            continue;
        }
        all.push(k);
        let length: f64 = e.windows(2).map(|w| w[0].distance(w[1])).sum();
        let (lo, hi) = e
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.z), hi.max(p.z))
            });
        if hi - lo > 0.5 * length {
            across.push(k);
        }
    }
    (all, across)
}

/// The edges of the flow domain `whole`, the fused lofts `pieces`, along the cutwater: the
/// edges the fusion made (no loft has them) within reach of the tip, where the outer
/// wall of the spiral meets the inner wall of the diffuser.
fn tip_edges(whole: &Shape, pieces: &[Shape], g: &VoluteGeometry) -> Vec<usize> {
    const SAMPLES: usize = 17;
    let lofted: Vec<Vec<DVec3>> = pieces
        .iter()
        .flat_map(|p| p.edge_samples(4 * SAMPLES))
        .filter(|e| !e.is_empty())
        .collect();
    let off_the_lofts = |p: DVec3| {
        lofted
            .iter()
            .flat_map(|e| e.windows(2))
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let t = ((p - a).dot(b - a) / (b - a).length_squared().max(1e-24)).clamp(0.0, 1.0);
                p.distance(a + (b - a) * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let tip = g.cutwater.tip * MM;
    let reach = 10.0 * g.design.wall_thickness * MM;
    whole
        .edge_samples(SAMPLES)
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            !e.is_empty()
                && e.iter()
                    .all(|p| p.truncate().distance(tip.truncate()) < reach)
                && off_the_lofts(e[SAMPLES / 2]) >= 0.01
        })
        .map(|(k, _)| k)
        .collect()
}

/// `channel` with its cutwater edge rounded by the kernel, and the radius it took, mm: the
/// edges of the tip that run across the height, with `radius` or a part of it if this
/// one is too much; `None` if the kernel builds nothing sound.
pub(crate) fn fillet_tip(
    channel: &Shape,
    pieces: &[Shape],
    g: &VoluteGeometry,
    radius: f64,
) -> Option<(Shape, f64)> {
    let samples = channel.edge_samples(17);
    let across: Vec<usize> = tip_edges(channel, pieces, g)
        .into_iter()
        .filter(|&k| {
            let e = &samples[k];
            let length: f64 = e.windows(2).map(|w| w[0].distance(w[1])).sum();
            let (lo, hi) = e
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p.z), hi.max(p.z))
                });
            hi - lo > 0.5 * length
        })
        .collect();
    if across.is_empty() || radius <= 0.0 {
        return None;
    }
    // With a rounded diffuser base the walls meet along a V whose point is the tip at
    // mid-height and whose arms run back to the corners, beside a corner edge of the
    // diffuser that also runs across the height: the arms alone, then.
    let arms: Vec<usize> = across
        .iter()
        .copied()
        .filter(|&k| {
            let e = &samples[k];
            e[0].truncate().distance(e[e.len() - 1].truncate()) > 1.0
        })
        .collect();
    let mut choices = vec![across.clone()];
    if !arms.is_empty() && arms != across {
        choices.push(arms);
    }
    let before = channel.volume_with_tolerance(0.0);
    let sound =
        |f: &Shape| f.is_valid() && f.solid_count() == 1 && f.volume_with_tolerance(0.0) > before;
    [1.0, 0.75, 0.5, 0.25].into_iter().find_map(|share| {
        let r = share * radius;
        choices.iter().find_map(|edges| {
            // The kernel may leave the faces round the ends of a rounded V out of
            // tolerance; shape healing sets them right.
            let rounded = channel.filleted(edges, r).ok()?;
            let rounded = if sound(&rounded) {
                rounded
            } else {
                rounded.fixed().ok().filter(|f| sound(f))?
            };
            Some((rounded, r))
        })
    })
}

/// `whole` with the groove over the cutwater filled and rounded with `radius`, mm, and
/// the radius it took: along all its edges if OCCT can, else along those across the
/// height, and with a smaller radius if neither takes this one (round sections take
/// less); `None` if none does.
fn fill_groove(
    whole: &Shape,
    pieces: &[Shape],
    g: &VoluteGeometry,
    radius: f64,
) -> Option<(Shape, f64)> {
    let (all, across) = groove_edges(whole, pieces, g);
    [1.0, 0.75, 0.5, 0.25].into_iter().find_map(|share| {
        let r = share * radius;
        [&all, &across]
            .into_iter()
            .filter(|edges| !edges.is_empty())
            .find_map(|edges| {
                whole
                    .filleted(edges, r)
                    .ok()
                    .filter(|f| f.is_valid() && f.solid_count() == 1)
            })
            .map(|filled| (filled, r))
    })
}

/// The flange at the outlet of the diffuser: a disk whose front face is the outlet face,
/// with the bore and the bolt holes through it. The bore is a little wider than the
/// flow so that it runs inside the casing wall instead of on the flow surface; the
/// bolt holes straddle the plane of the pump axis.
fn flange(g: &VoluteGeometry, f: &tp_design::volute::Flange) -> Result<Shape, CadError> {
    let outlet = g
        .nozzle
        .last()
        .ok_or_else(|| CadError::Input("the volute has no diffuser".into()))?;
    let axis = outlet.tangent.normalize();
    let face = outlet.center * MM;
    let thickness = f.thickness * MM;
    let back = face - axis * thickness;
    let disk = occ::cylinder(back, axis, f.outer_diameter / 2.0 * MM, thickness)?;
    let margin = thickness;
    let bore = occ::cylinder(
        back - axis * margin,
        axis,
        (f.bore / 2.0 + 0.5 * g.design.wall_thickness) * MM,
        thickness + 2.0 * margin,
    )?;
    // Square to the axis even when a z-bend tilts it.
    let u = (DVec3::Z - axis * DVec3::Z.dot(axis)).normalize();
    let v = axis.cross(u);
    let holes = (0..f.bolts)
        .map(|k| {
            let angle = std::f64::consts::TAU * (f64::from(k) + 0.5) / f64::from(f.bolts);
            let at = back - axis * margin
                + (u * angle.cos() + v * angle.sin()) * (f.bolt_circle / 2.0 * MM);
            occ::cylinder(at, axis, f.bolt_hole / 2.0 * MM, thickness + 2.0 * margin)
        })
        .collect::<Result<Vec<_>, _>>()?;
    disk.cut(&bore)?.cut(&occ::compound(&holes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{PI, TAU};
    use tp_design::outlet::ImpellerOutlet;
    use tp_design::volute::{CrossSectionInput, SectionShape, VoluteInput};

    fn basis() -> ImpellerOutlet {
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

    /// Volume of the channel by Pappus: spiral sections swept about the axis, plus the
    /// straight diffuser. A double volute adds its inner volute and outer channel.
    fn swept_volume(sections: &[tp_design::volute::VoluteSection]) -> f64 {
        sections
            .windows(2)
            .map(|w| {
                let a = 0.5 * (w[0].size.area + w[1].size.area);
                let r = 0.5 * (w[0].size.centroid_radius + w[1].size.centroid_radius);
                a * r * (w[1].angle - w[0].angle)
            })
            .sum()
    }

    fn expected_channel_volume(g: &VoluteGeometry) -> f64 {
        let swept = swept_volume;
        let spiral = swept(&g.sections) + swept(&g.inner_sections) + swept(&g.outer_channel);
        let nozzle: f64 = g
            .nozzle
            .windows(2)
            .map(|w| 0.5 * (w[0].area + w[1].area) * w[0].center.distance(w[1].center))
            .sum();
        spiral + nozzle
    }

    /// Every kind of cross section, and definitions that change round the spiral.
    fn every_cross_section() -> Vec<CrossSectionInput> {
        use tp_design::volute::{
            BezierSide, ExternalRound, RadiusSide, SectionDefinition, SideShape, section,
        };
        let single = |shape| CrossSectionInput::single(shape);
        vec![
            CrossSectionInput::default(),
            single(SectionShape::round()),
            single(SectionShape::symmetric(SideShape::RadiusBased(
                RadiusSide::default(),
            ))),
            single(SectionShape::symmetric(SideShape::Bezier(
                BezierSide::default(),
            ))),
            single(SectionShape::symmetric(SideShape::LineSegments {
                points: section::default_segments(),
            })),
            single(SectionShape::RoundExternal(ExternalRound::default())),
            CrossSectionInput {
                sections: vec![
                    SectionDefinition {
                        angle: PI / 2.0,
                        active: true,
                        shape: SectionShape::rectangular(),
                    },
                    SectionDefinition::at_throat(SectionShape::symmetric(SideShape::Round {
                        ellipse_ratio: 1.2,
                    })),
                ],
            },
        ]
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_fillet_that_crashes_the_kernel_only_fails_that_fillet() {
        let _slot = crate::test_slot::take();
        // Filling the groove over this cutwater makes OCCT read a null pointer while it
        // rounds the corners; the casing falls back instead of the process dying.
        let mut input = VoluteInput::default();
        input.diffuser.shape = tp_design::volute::DischargeShape::Tangential {
            straight_wall: tp_design::volute::StraightWall::Inner,
        };
        input.diffuser.base_form = tp_design::Param::Manual(0.5);
        input.cutwater.shape = tp_design::volute::CutwaterShape::Sharp;
        let mut g = VoluteGeometry::build(&basis(), &input).unwrap();
        g.design.flange = None;
        let cad = build_volute(&g).unwrap();
        let casing = build_volute_casing(&g, &cad).unwrap();
        assert!(casing.solid.is_valid());
        assert_eq!(casing.solid.solid_count(), 1);
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn volute_channel_and_fluid_domain_are_valid_solids() {
        let _slot = crate::test_slot::take();
        for sections in every_cross_section() {
            let input = VoluteInput {
                cross_section: sections,
                ..VoluteInput::default()
            };
            let shape = &input.cross_section;
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            for s in [&cad.channel, &cad.fluid] {
                assert!(s.is_valid());
                assert_eq!(s.solid_count(), 1);
            }
            let channel = cad.channel.volume_with_tolerance(1e-3) / MM.powi(3);
            let expected = expected_channel_volume(&g);
            assert!(
                (channel / expected - 1.0).abs() < 0.03,
                "{shape:?}: {channel} vs {expected}"
            );
            // The fluid domain adds the ring between d₂ and the cutwater circle.
            let (inner, outer) = g.inlet_ring();
            let ring = PI * (outer * outer - inner * inner) * g.design.inlet_width;
            let fluid = cad.fluid.volume_with_tolerance(1e-3) / MM.powi(3);
            assert!(
                fluid > channel + 0.8 * ring && fluid < channel + 1.3 * ring,
                "{shape:?}: fluid {fluid}, channel {channel}, ring {ring}"
            );
            assert_matches_the_view(&g, &cad, &format!("{shape:?}"));
        }
    }

    /// Distance from `p` to the triangle `t` (Ericson, Real-Time Collision Detection,
    /// 5.1.5).
    fn distance_to_triangle(p: DVec3, [a, b, c]: [DVec3; 3]) -> f64 {
        let (ab, ac) = (b - a, c - a);
        let (d1, d2) = (ab.dot(p - a), ac.dot(p - a));
        if d1 <= 0.0 && d2 <= 0.0 {
            return p.distance(a);
        }
        let (d3, d4) = (ab.dot(p - b), ac.dot(p - b));
        if d3 >= 0.0 && d4 <= d3 {
            return p.distance(b);
        }
        let vc = d1 * d4 - d3 * d2;
        if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            return p.distance(a + ab * (d1 / (d1 - d3)));
        }
        let (d5, d6) = (ab.dot(p - c), ac.dot(p - c));
        if d6 >= 0.0 && d5 <= d6 {
            return p.distance(c);
        }
        let vb = d5 * d2 - d1 * d6;
        if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            return p.distance(a + ac * (d2 / (d2 - d6)));
        }
        let va = d3 * d6 - d5 * d4;
        if va <= 0.0 && d4 >= d3 && d5 >= d6 {
            return p.distance(b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6))));
        }
        let sum = va + vb + vc;
        p.distance(a + ab * (vb / sum) + ac * (vc / sum))
    }

    /// Triangles bucketed in cubes, to find the nearest ones to a point quickly.
    struct Grid {
        cells: std::collections::HashMap<[i64; 3], Vec<usize>>,
        triangles: Vec<[DVec3; 3]>,
    }

    impl Grid {
        /// Side of the cubes, mm.
        const CELL: f64 = 1.0;

        fn cell(p: DVec3) -> [i64; 3] {
            (p / Self::CELL).floor().as_i64vec3().to_array()
        }

        fn new(triangles: Vec<[DVec3; 3]>) -> Self {
            let mut cells = std::collections::HashMap::<[i64; 3], Vec<usize>>::new();
            for (k, t) in triangles.iter().enumerate() {
                let lo = Self::cell(t[0].min(t[1]).min(t[2]));
                let hi = Self::cell(t[0].max(t[1]).max(t[2]));
                for i in lo[0]..=hi[0] {
                    for j in lo[1]..=hi[1] {
                        for l in lo[2]..=hi[2] {
                            cells.entry([i, j, l]).or_default().push(k);
                        }
                    }
                }
            }
            Self { cells, triangles }
        }

        /// Distance from `p` to the nearest triangle, or `reach` if none is nearer.
        fn distance(&self, p: DVec3, reach: f64) -> f64 {
            let (lo, hi) = (
                Self::cell(p - DVec3::splat(reach)),
                Self::cell(p + DVec3::splat(reach)),
            );
            let mut nearest = reach;
            for i in lo[0]..=hi[0] {
                for j in lo[1]..=hi[1] {
                    for l in lo[2]..=hi[2] {
                        for &k in self.cells.get(&[i, j, l]).into_iter().flatten() {
                            nearest = nearest.min(distance_to_triangle(p, self.triangles[k]));
                        }
                    }
                }
            }
            nearest
        }
    }

    /// How the flow domain departs from what the viewer shows, mm.
    struct Departure {
        /// The farthest point of the body outlines from the CAD surface.
        outline: f64,
        /// The farthest point of the CAD surface within 2 mm of the body from the
        /// viewer's surfaces.
        solid: f64,
        /// The same on the far side of the spiral, away from the cutwater.
        walls: f64,
        /// Distance from the axis to the CAD surface on the body nearest it.
        nearest: f64,
    }

    /// How the flow domain `cad` departs from what the viewer shows around the body of
    /// the cutwater; `None` without a body.
    fn departure_from_the_view(g: &VoluteGeometry, cad: &VoluteCad) -> Option<Departure> {
        let outlines: Vec<DVec3> = clear_planes(g)
            .iter()
            .filter(|o| o.len() > 2)
            .flat_map(|o| o.iter().map(|p| *p * MM))
            .collect();
        // Between the end planes of the body, side endings included.
        let (lo, hi) = g
            .cutwater_planes()
            .iter()
            .flatten()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.z * MM), hi.max(p.z * MM))
            });
        if outlines.is_empty() {
            return None;
        }
        // Only the triangles near the body count.
        let (low, high) = outlines.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(low, high), p| (low.min(*p), high.max(*p)),
        );
        let near = |t: &[DVec3; 3]| {
            t.iter().any(|p| {
                p.cmpge(low - DVec3::splat(3.0)).all() && p.cmple(high + DVec3::splat(3.0)).all()
            })
        };
        let grid =
            |triangles: &[[DVec3; 3]]| Grid::new(triangles.iter().copied().filter(near).collect());
        let in_mm = |t: &[DVec3; 3]| t.map(|p| p * MM);
        let body_mesh: Vec<[DVec3; 3]> = tp_io::mesh::cutwater_triangles(g)
            .iter()
            .map(in_mm)
            .collect();
        let triangles = cad.fluid.triangles(0.02).unwrap();
        let solid = grid(&triangles);
        let body = grid(&body_mesh);
        let view = grid(
            &tp_io::mesh::volute_view_triangles(g)
                .iter()
                .map(in_mm)
                .chain(body_mesh)
                .collect::<Vec<_>>(),
        );
        // Against the triangles, and exactly for the points far from them: OCCT may
        // leave a face without triangles, or with coarse ones.
        let (close, far): (Vec<DVec3>, Vec<DVec3>) = outlines
            .iter()
            .partition(|p| solid.distance(**p, 1.0) < 0.05);
        let outline_off = close
            .iter()
            .map(|p| solid.distance(*p, 1.0))
            .chain(cad.fluid.boundary_distances(&far).unwrap())
            .fold(0.0, f64::max);
        let around: Vec<DVec3> = solid
            .triangles
            .iter()
            .flatten()
            .copied()
            .filter(|p| p.z >= lo && p.z <= hi && body.distance(*p, 2.0) < 2.0)
            .collect();
        let solid_off = around
            .iter()
            .map(|p| view.distance(*p, 1.0))
            .fold(0.0, f64::max);
        let nearest = around
            .iter()
            .filter(|p| body.distance(**p, 0.05) < 0.05)
            .map(|p| p.truncate().length())
            .fold(f64::INFINITY, f64::min);
        // The walls on the far side of the spiral, within 5° of the opposite of the
        // tip and clear of the inlet ring, against the volute as drawn.
        let away = -g.cutwater_slices[0].tip.truncate().normalize();
        let clear = g.design.stagnation_radius * MM + 2.0;
        let opposite = |p: &DVec3| {
            p.truncate().length() > clear
                && p.truncate().normalize().dot(away) > 5f64.to_radians().cos()
        };
        let drawn = Grid::new(
            tp_io::mesh::volute_triangles(g)
                .iter()
                .map(in_mm)
                .filter(|t| t.iter().any(opposite))
                .collect(),
        );
        let walls = triangles
            .iter()
            .flatten()
            .filter(|p| opposite(p))
            .map(|p| drawn.distance(*p, 2.0))
            .fold(0.0, f64::max);
        Some(Departure {
            outline: outline_off,
            solid: solid_off,
            walls,
            nearest,
        })
    }

    /// The flow domain carves the body of the cutwater the viewer shows. Its outlines lie
    /// on the CAD surface within 0.1 mm, and no point of the body comes nearer the axis
    /// than its nose in the model. Around it, the CAD surface departs from what the
    /// viewer shows no more than the walls do away from the cutwater, plus 0.2 mm: no
    /// sliver of wall or of fluid is left. The viewer draws the walls straight between
    /// the 64 points of each section and from one section to the next, where the CAD
    /// lofts smooth surfaces through them.
    fn assert_matches_the_view(g: &VoluteGeometry, cad: &VoluteCad, case: &str) {
        // A cutwater the CAD keeps sharp, or rounds with less than the design, is not the
        // one the view shows.
        if cad
            .warnings
            .iter()
            .any(|w| w.contains("sharp") || w.contains("the most the CAD kernel takes"))
        {
            return;
        }
        let Some(d) = departure_from_the_view(g, cad) else {
            return;
        };
        let nose = g
            .cutwater_slices
            .iter()
            .map(|s| s.nose_distance * MM)
            .fold(f64::INFINITY, f64::min);
        // Some of it is the arc of the sliver that rounds a cutwater the kernel cannot (0.15
        // mm bigger than the round).
        assert!(
            d.outline < 0.3,
            "{case}: the outlines {} mm off the CAD",
            d.outline
        );
        assert!(
            d.solid < d.walls + 0.2,
            "{case}: the CAD {} mm off the view, the walls {} mm",
            d.solid,
            d.walls
        );
        // Within the deflection of the triangles, and of the splines where the side
        // endings ride the cutwater circle: they kink onto it, and a spline dips some
        // hundredths of a millimetre past the kink (0.03 mm in round sections).
        assert!(
            d.nearest > nose - 0.05,
            "{case}: {} vs {nose} mm",
            d.nearest
        );
    }

    /// Wetted outline times wall thickness, along the path of the outline centroids.
    fn shell_estimate(g: &VoluteGeometry) -> f64 {
        let t = g.design.wall_thickness;
        let perimeter = |ring: &[DVec3]| -> f64 {
            (0..ring.len())
                .map(|i| ring[i].distance(ring[(i + 1) % ring.len()]))
                .sum()
        };
        let centroid = |ring: &[DVec3]| ring.iter().copied().sum::<DVec3>() / ring.len() as f64;
        g.lofts()
            .iter()
            .flat_map(|rings| {
                rings.windows(2).map(|w| {
                    0.5 * (perimeter(&w[0]) + perimeter(&w[1]))
                        * t
                        * centroid(&w[0]).distance(centroid(&w[1]))
                })
            })
            .sum()
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_casing_wraps_the_flow_domain_in_one_solid() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::VoluteKind;
        // The trapezoidal default, rectangles whose flat sides meet in the throat, and
        // a double volute.
        let edits: [&dyn Fn(&mut VoluteInput); 3] = [
            &|_| {},
            &|v| v.cross_section = CrossSectionInput::single(SectionShape::rectangular()),
            &|v| v.setup.kind = VoluteKind::Double,
        ];
        for (case, edit) in edits.into_iter().enumerate() {
            let mut input = VoluteInput::default();
            edit(&mut input);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            let casing = build_volute_casing(&g, &cad).unwrap();
            assert!(
                casing.solid.is_valid() && casing.solid.solid_count() == 1,
                "case {case}: {:?} {:?}",
                cad.warnings,
                casing.warnings
            );
            assert!(casing.flange, "{:?}", casing.warnings);
            // The groove over the cutwater rounded with the radius asked for; or the casing
            // fitted to a sharp cutwater where the kernel cannot build it round one.
            assert!(
                casing
                    .warnings
                    .iter()
                    .all(|w| w.contains("the casing fits a sharp one")),
                "{:?}",
                casing.warnings
            );
            // The shell around the channel, plus the cutwater, the walls beside the
            // inlet ring and the flange beyond the wall, less its bolt holes.
            let f = g.design.flange.unwrap();
            let t = g.design.wall_thickness;
            let ring = |d: f64| PI / 4.0 * d * d * f.thickness;
            let flange = ring(f.outer_diameter)
                - ring(f.bore + 2.0 * t)
                - f64::from(f.bolts) * ring(f.bolt_hole);
            let volume = casing.solid.volume_with_tolerance(0.0) / MM.powi(3) - flange;
            let shell = shell_estimate(&g);
            assert!(
                volume > 0.9 * shell && volume < 1.6 * shell,
                "{:?}: {volume} vs {shell}",
                input.cross_section
            );
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_outer_wall_of_a_thinned_casing_passes_by_every_outline() {
        let _slot = crate::test_slot::take();
        for shape in [SectionShape::default(), SectionShape::round()] {
            let input = VoluteInput {
                cross_section: CrossSectionInput::single(shape.clone()),
                ..VoluteInput::default()
            };
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let (lofts, outlines) =
                casing_pieces(&g, tp_design::volute::CHANNEL_OVERLAP, true).unwrap();
            for (loft, rings) in lofts.iter().zip(&outlines) {
                let points: Vec<DVec3> = rings
                    .iter()
                    .step_by(2)
                    .flat_map(|r| r.iter().step_by(3).map(|p| *p * MM))
                    .collect();
                let off = loft
                    .boundary_distances(&points)
                    .unwrap()
                    .into_iter()
                    .fold(0.0, f64::max);
                // The loft's own approximation adds a little to the spline's.
                assert!(off < 2.0 * CASING_TOLERANCE * MM, "{shape:?}: {off} mm");
            }
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_casing_of_radius_and_segment_sections_is_one_solid() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::{RadiusSide, SideShape, VoluteKind, section};
        // Their groove over the cutwater is not rounded (the kernel hangs there), and
        // the pieces of their casing fuse only at some of the overlaps tried.
        let shapes = [
            SectionShape::symmetric(SideShape::RadiusBased(RadiusSide::default())),
            SectionShape::symmetric(SideShape::LineSegments {
                points: section::default_segments(),
            }),
        ];
        for shape in shapes {
            for kind in [VoluteKind::Single, VoluteKind::Double] {
                let mut input = VoluteInput::default();
                input.setup.kind = kind;
                input.cross_section = CrossSectionInput::single(shape.clone());
                let g = VoluteGeometry::build(&basis(), &input).unwrap();
                let cad = build_volute(&g).unwrap();
                let casing = build_volute_casing(&g, &cad).unwrap();
                let case = format!("{shape:?} {kind:?}");
                assert!(
                    casing.solid.is_valid() && casing.solid.solid_count() == 1,
                    "{case}: {:?}",
                    casing.warnings
                );
                assert!(
                    casing.warnings.iter().any(|w| w.contains("stays sharp")),
                    "{case}: {:?}",
                    casing.warnings
                );
                // Besides the groove, only the flange may give way (it ends flat on some
                // platforms), and it says so.
                assert!(
                    casing
                        .warnings
                        .iter()
                        .all(|w| w.contains("stays sharp") || w.contains("flange")),
                    "{case}: {:?}",
                    casing.warnings
                );
            }
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_casing_fills_and_rounds_the_groove_over_the_cutwater() {
        let _slot = crate::test_slot::take();
        // Round sections take a smaller radius there.
        for (shape, whole_radius) in [
            (SectionShape::default(), true),
            (SectionShape::rectangular(), true),
            (SectionShape::round(), false),
        ] {
            let input = VoluteInput {
                cross_section: CrossSectionInput::single(shape.clone()),
                ..VoluteInput::default()
            };
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let radius = g.design.cutwater_fillet * MM;
            approx::assert_relative_eq!(radius, g.design.wall_thickness * MM);
            let (pieces, _) = casing_pieces(&g, tp_design::volute::CHANNEL_OVERLAP, true).unwrap();
            // As the casing does: the first fusion that takes the fillet.
            let (whole, (filled, took)) = FUZZY_GAPS
                .into_iter()
                .find_map(|fuzzy| {
                    let whole = occ::fuse_all_fuzzy(&pieces, fuzzy).ok()?;
                    let filled = fill_groove(&whole, &pieces, &g, radius)?;
                    Some((whole, filled))
                })
                .unwrap_or_else(|| panic!("{shape:?}: no fillet"));
            assert!(filled.is_valid() && filled.solid_count() == 1, "{shape:?}");
            assert!(
                if whole_radius {
                    (took - radius).abs() < 1e-9
                } else {
                    took < radius
                },
                "{shape:?}: {took} mm"
            );
            // The bottom of the groove ends up inside the material.
            let (_, across) = groove_edges(&whole, &pieces, &g);
            assert!(!across.is_empty(), "{shape:?}");
            let samples = whole.edge_samples(9);
            let bottom: Vec<DVec3> = across.iter().map(|&k| samples[k][4]).collect();
            for (p, d) in bottom
                .iter()
                .zip(filled.boundary_distances(&bottom).unwrap())
            {
                assert!(d > 0.2 * took, "{shape:?}: {p} only {d} mm deep");
            }
            // Material only added, a little of it.
            let added = filled.volume_with_tolerance(0.0) - whole.volume_with_tolerance(0.0);
            let height = (g.design.inlet_width + 2.0 * g.design.wall_thickness) * MM;
            assert!(
                added > 0.0 && added < 10.0 * took * took * height,
                "{shape:?}: {added}"
            );
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_tapered_rib_gets_its_corners_rounded() {
        let _slot = crate::test_slot::take();
        let build = |corner: f64| {
            let mut input = VoluteInput::default();
            input.setup.kind = tp_design::volute::VoluteKind::Double;
            input.setup.rib_middle_thickness = tp_design::Param::Manual(0.008);
            input.setup.rib_end_thickness = tp_design::Param::Manual(0.006);
            input.setup.rib_tail_ratio = tp_design::Param::Manual(2.0);
            input.setup.rib_corner_radius = tp_design::Param::Manual(corner);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            (build_volute(&g).unwrap(), g)
        };
        let (sharp, g) = build(0.0);
        let (z_lo, z_hi) = tool_span(&g);
        let rib = rib_body(&g, z_lo, z_hi).unwrap().unwrap();
        // Two corners on each side of the rib, along its inner and outer faces at least.
        assert!(rib_corner_edges(&g, &sharp.channel, &rib).len() >= 4);
        let (round, _) = build(0.002);
        assert!(round.fluid.is_valid());
        assert_eq!(round.fluid.solid_count(), 1);
        // The corners are rounded, with the whole radius or with what the kernel takes
        // (it differs between platforms); never left sharp.
        assert!(
            round
                .warnings
                .iter()
                .all(|w| !w.contains("rib") || w.contains("the most the CAD kernel takes")),
            "{:?}",
            round.warnings
        );
        // The fillets fill the corners: a little less fluid.
        let (v0, v1) = (
            sharp.fluid.volume_with_tolerance(0.0),
            round.fluid.volume_with_tolerance(0.0),
        );
        assert!(v1 < v0 && v1 > 0.995 * v0, "{v1} vs {v0}");
        // One fillet face at least for each corner.
        assert!(round.channel.face_count() >= sharp.channel.face_count() + 4);
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_double_volute_is_one_valid_solid() {
        let _slot = crate::test_slot::take();
        let double = |edit: &dyn Fn(&mut VoluteInput)| {
            let mut input = VoluteInput::default();
            input.setup.kind = tp_design::volute::VoluteKind::Double;
            edit(&mut input);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            for s in [&cad.channel, &cad.fluid] {
                assert!(s.is_valid());
                assert_eq!(s.solid_count(), 1);
            }
            // The rib nose is carved in every plane, as the cutwater.
            assert!(on_surface(&cad.fluid, &rib_probes(&g)));
            assert_matches_the_view(&g, &cad, "double volute");
            (g, cad.channel.volume_with_tolerance(1e-3) / MM.powi(3))
        };
        let (g, channel) = double(&|_| {});
        let expected = expected_channel_volume(&g);
        assert!(
            (channel / expected - 1.0).abs() < 0.03,
            "{channel} vs {expected}"
        );
        // The rib is cut out of the whole sections: about its band of b·t swept from
        // the nose to the throat.
        let rib = g.design.rib.unwrap();
        let whole = expected - swept_volume(&g.inner_sections) - swept_volume(&g.outer_channel)
            + swept_volume(&g.envelope);
        let band = g.design.inlet_width * rib.thickness * g.design.stagnation_radius * rib.wrap;
        assert!(
            channel < whole - 0.5 * band,
            "{channel} vs {whole} − {band}"
        );
        // A long elliptic nose, a thick rib, rectangles, a tangent cutwater, and the
        // cutwater and the rib nose leaning.
        let edits: [&dyn Fn(&mut VoluteInput); 5] = [
            &|v| v.setup.rib_nose_ratio = tp_design::Param::Manual(3.0),
            &|v| v.setup.rib_thickness = tp_design::Param::Manual(0.008),
            &|v| v.cross_section = CrossSectionInput::single(SectionShape::rectangular()),
            &|v| {
                v.cutwater.shape = tp_design::volute::CutwaterShape::Tangent;
                v.setup.rib_length = tp_design::Param::Manual(170f64.to_radians());
            },
            &|v| v.cutwater.obliquity = tp_design::Param::Manual(35f64.to_radians()),
        ];
        for edit in edits {
            double(edit);
        }
    }

    /// Volume of the revolved profile of the inlet by Pappus: 2π times its first moment
    /// about the axis.
    fn revolved_volume(pieces: &[Vec<DVec2>]) -> f64 {
        let mut points: Vec<DVec2> = pieces.iter().flatten().copied().collect();
        points.dedup_by(|a, b| a.distance(*b) < 1e-12);
        let n = points.len();
        let moment: f64 = (0..n)
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                // ∬ r dA = ∮ r²/2 dz.
                (b.y - a.y) * (a.x * a.x + a.x * b.x + b.x * b.x) / 6.0
            })
            .sum();
        2.0 * PI * moment.abs()
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn an_internal_volute_is_one_solid_with_its_bend() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::{InternalRound, SectionShape};
        for open_to_right in [true, false] {
            let input = VoluteInput {
                cross_section: CrossSectionInput::single(SectionShape::RoundInternal(
                    InternalRound {
                        open_to_right,
                        ..InternalRound::default()
                    },
                )),
                ..VoluteInput::default()
            };
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            for s in [&cad.channel, &cad.fluid] {
                assert!(s.is_valid());
                assert_eq!(s.solid_count(), 1);
            }
            // The spiral starts inside the diffuser, so the channel is a little less than
            // its two pieces; the fluid adds the vaneless ring, the bend and the neck.
            let pieces: f64 = loft_pieces(&g.lofts(), &g.ring_breaks())
                .unwrap()
                .iter()
                .map(|p| p.volume_with_tolerance(1e-3))
                .sum::<f64>()
                / MM.powi(3);
            let channel = cad.channel.volume_with_tolerance(1e-3) / MM.powi(3);
            assert!(
                channel < pieces && channel > 0.9 * pieces,
                "{channel} vs {pieces}"
            );
            let inlet = revolved_volume(&g.inlet_profile());
            let fluid = cad.fluid.volume_with_tolerance(1e-3) / MM.powi(3);
            assert!(
                (fluid - channel - inlet).abs() < 0.03 * fluid,
                "{fluid} vs {channel} + {inlet}"
            );
            // The casing wraps the bend too, and leaves the impeller open on its side.
            let casing = build_volute_casing(&g, &cad).unwrap();
            assert!(casing.solid.is_valid() && casing.solid.solid_count() == 1);
            assert!(casing.flange, "{:?}", casing.warnings);
        }
    }

    #[test]
    fn polygons_split_at_their_corners() {
        let _slot = crate::test_slot::take();
        // A unit square with the corner at (1, 1) rounded by a quarter circle of 12
        // segments: the rounded side, tangent at both ends, is one curved run.
        let mut points = vec![DVec2::new(0.0, 0.0), DVec2::new(1.0, 0.0)];
        for k in 0..=12 {
            let a = PI / 2.0 * f64::from(k) / 12.0;
            points.push(DVec2::new(0.8, 0.8) + 0.2 * DVec2::new(a.cos(), a.sin()));
        }
        points.push(DVec2::new(0.0, 1.0));
        let pieces = smooth_pieces(&points);
        assert_eq!(pieces.len(), 3, "{pieces:?}");
        assert_eq!(pieces.iter().filter(|p| p.len() == 2).count(), 2);
        assert_eq!(pieces[1].len(), points.len() - 1);
        // The pieces close the loop end to start.
        for (k, piece) in pieces.iter().enumerate() {
            let next = &pieces[(k + 1) % pieces.len()];
            assert_eq!(piece.last(), next.first());
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn common_volute_options_give_one_valid_flow_domain() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::{CutwaterShape, DischargeShape, StraightWall};
        for shape in [SectionShape::default(), SectionShape::rectangular()] {
            for discharge in [
                DischargeShape::default(),
                DischargeShape::Tangential {
                    straight_wall: StraightWall::Centered,
                },
                DischargeShape::Tangential {
                    straight_wall: StraightWall::Inner,
                },
            ] {
                for cutwater in [CutwaterShape::default(), CutwaterShape::Tangent] {
                    let mut input = VoluteInput {
                        cross_section: CrossSectionInput::single(shape.clone()),
                        ..VoluteInput::default()
                    };
                    input.diffuser.shape = discharge;
                    input.cutwater.shape = cutwater;
                    let g = VoluteGeometry::build(&basis(), &input).unwrap();
                    let cad = build_volute(&g).unwrap();
                    let case = format!("{shape:?} {discharge:?} {cutwater:?}");
                    assert!(
                        cad.fluid.is_valid() && cad.fluid.solid_count() == 1,
                        "{case}"
                    );
                    assert_matches_the_view(&g, &cad, &case);
                }
            }
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn spiral_development_options_give_one_valid_flow_domain() {
        let _slot = crate::test_slot::take();
        use tp_design::Param;
        use tp_design::volute::AreaLaw;
        type Setting = fn(&mut VoluteInput);
        let cases: [(&str, Setting); 4] = [
            ("compensation", |v| v.spiral.compensation.enabled = true),
            ("rectangle with compensation", |v| {
                v.cross_section = CrossSectionInput::single(SectionShape::rectangular());
                v.spiral.compensation.enabled = true;
            }),
            ("330° wrap", |v| {
                v.spiral.wrap = Param::Manual(330f64.to_radians())
            }),
            ("Stepanoff with arcs", |v| {
                v.spiral.law = AreaLaw::Stepanoff;
                v.spiral.arcs.enabled = true;
            }),
        ];
        for (name, set) in cases {
            let mut input = VoluteInput::default();
            set(&mut input);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(
                cad.fluid.is_valid() && cad.fluid.solid_count() == 1,
                "{name}"
            );
            assert!(cad.fluid_volume() > 0.0, "{name}");
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_matched_area_law_gives_the_same_valid_flow_domain() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::{DiffuserGrowth, DischargeShape, StraightWall};
        let fluid = |growth: DiffuserGrowth, discharge: DischargeShape| -> f64 {
            let mut input = VoluteInput::default();
            input.diffuser.growth = growth;
            input.diffuser.shape = discharge;
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(cad.fluid.is_valid(), "{growth:?} {discharge:?}");
            assert_eq!(cad.fluid.solid_count(), 1, "{growth:?} {discharge:?}");
            assert_eq!(cad.channel.solid_count(), 1, "{growth:?} {discharge:?}");
            assert!(cad.warnings.is_empty(), "{:?}", cad.warnings);
            cad.fluid_volume()
        };
        for discharge in [
            DischargeShape::Radial { axis_angle: 0.0 },
            DischargeShape::Tangential {
                straight_wall: StraightWall::Centered,
            },
        ] {
            let linear = fluid(DiffuserGrowth::Linear, discharge);
            let matched = fluid(DiffuserGrowth::Matched, discharge);
            // Same throat, same outlet, same length: only the law between them differs.
            assert!(
                (0.0001..0.03).contains(&(matched / linear - 1.0).abs()),
                "{discharge:?}: {matched} vs {linear}"
            );
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_diffuser_offsets_give_valid_flow_domains() {
        let _slot = crate::test_slot::take();
        use tp_design::Param;
        use tp_design::volute::{DischargeInput, ZAlignment};
        let fluid = |name: &str, edit: &dyn Fn(&mut DischargeInput)| -> f64 {
            let mut input = VoluteInput::default();
            edit(&mut input.diffuser);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(cad.fluid.is_valid(), "{name}");
            assert_eq!(cad.fluid.solid_count(), 1, "{name}");
            assert_eq!(cad.channel.solid_count(), 1, "{name}");
            assert!(cad.warnings.is_empty(), "{name}: {:?}", cad.warnings);
            cad.fluid_volume()
        };
        let plain = fluid("plain", &|_| {});
        let offset = fluid("offset", &|d| d.height_offset = Param::Manual(0.01));
        let rounded_base = fluid("rounded base", &|d| d.base_form = Param::Manual(0.75));
        let manual = fluid("manual", &|d| {
            d.z_alignment = ZAlignment::Manual {
                offset: 0.01,
                quadratic: true,
            }
        });
        // Same areas along the diffuser: the volume hardly changes, but it does.
        for (name, v) in [
            ("offset", offset),
            ("rounded base", rounded_base),
            ("manual", manual),
        ] {
            assert!((v / plain - 1.0).abs() < 0.03, "{name}: {v} vs {plain}");
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_double_volute_of_radius_segment_and_bezier_sections_gives_valid_solids() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::{
            BezierBase, BezierSide, RadiusSide, SideShape, VoluteKind, section,
        };
        let shapes = [
            (
                "radius based",
                SectionShape::symmetric(SideShape::RadiusBased(RadiusSide::default())),
            ),
            (
                "line segments",
                SectionShape::symmetric(SideShape::LineSegments {
                    points: section::default_segments(),
                }),
            ),
            (
                "Bézier trapezoid",
                SectionShape::symmetric(SideShape::Bezier(BezierSide {
                    base: BezierBase::Trapezoid,
                    ..BezierSide::default()
                })),
            ),
        ];
        for (name, shape) in shapes {
            let mut input = VoluteInput::default();
            input.setup.kind = VoluteKind::Double;
            input.cross_section = CrossSectionInput::single(shape);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(cad.fluid.is_valid(), "{name}");
            assert_eq!(cad.fluid.solid_count(), 1, "{name}");
            assert_eq!(cad.channel.solid_count(), 1, "{name}");
            // Sections of radii and segments keep their cutwater sharp, and say so.
            assert!(
                cad.warnings
                    .iter()
                    .all(|w| w.contains("of radii and segments sharp")),
                "{name}: {:?}",
                cad.warnings
            );
            // The whole sections swept round, less the band of the rib, and the diffuser.
            let rib = g.design.rib.unwrap();
            let rz = g.design.stagnation_radius;
            let steps = 200;
            let removed: f64 = (0..steps)
                .map(|k| {
                    let angle = rib.start
                        + (rib.end() - rib.start) * (f64::from(k) + 0.5) / f64::from(steps);
                    let r = rz + g.design.rib_height(angle).unwrap() + 0.5 * rib.thickness;
                    let (lo, hi) = g.envelope_z_range(angle, r).unwrap();
                    rib.thickness * (hi - lo) * r * (rib.end() - rib.start) / f64::from(steps)
                })
                .sum();
            let nozzle: f64 = g
                .nozzle
                .windows(2)
                .map(|w| 0.5 * (w[0].area + w[1].area) * w[0].center.distance(w[1].center))
                .sum();
            let expected = swept_volume(&g.sections) + swept_volume(&g.envelope) + nozzle - removed;
            let channel = cad.channel.volume_with_tolerance(0.0) / MM.powi(3);
            assert!(
                (channel / expected - 1.0).abs() < 0.04,
                "{name}: {channel} vs {expected}"
            );
            // The CAD kernel does not build the casing of radius based or line segment
            // sections, a single volute's either (an error and a loop in it).
            if name == "Bézier trapezoid" {
                let casing = build_volute_casing(&g, &cad).unwrap();
                assert!(
                    casing.solid.is_valid() && casing.solid.solid_count() == 1,
                    "{name}"
                );
            }
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_rib_that_stops_short_of_the_throat_gives_a_valid_flow_domain_and_casing() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::VoluteKind;
        let build = |length: Option<f64>| {
            let mut input = VoluteInput::default();
            input.setup.kind = VoluteKind::Double;
            if let Some(l) = length {
                input.setup.rib_length = tp_design::Param::Manual(l.to_radians());
            }
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(cad.fluid.is_valid(), "{length:?}");
            assert_eq!(cad.fluid.solid_count(), 1, "{length:?}");
            assert_eq!(cad.channel.solid_count(), 1, "{length:?}");
            assert!(cad.warnings.is_empty(), "{length:?}: {:?}", cad.warnings);
            (g, cad)
        };
        let (_, full) = build(None);
        let (g, short) = build(Some(120.0));
        // Less rib, more flow domain: by the band of the rib, its thickness over the
        // height of the channel at its radius, swept over the 60° it lacks.
        let rib = g.design.rib.unwrap();
        let steps = 200;
        let removed: f64 = (0..steps)
            .map(|k| {
                let angle = rib.end() + (TAU - rib.end()) * (f64::from(k) + 0.5) / f64::from(steps);
                let r = g.design.stagnation_radius
                    + g.design.section(angle).unwrap().height
                    + 0.5 * rib.thickness;
                let (lo, hi) = g.envelope_z_range(angle, r).unwrap();
                rib.thickness * (hi - lo) * r * (TAU - rib.end()) / f64::from(steps)
            })
            .sum();
        let gain = short.fluid_volume() - full.fluid_volume();
        assert!((gain / removed - 1.0).abs() < 0.1, "{gain} vs {removed}");
        let casing = build_volute_casing(&g, &short).unwrap();
        assert!(casing.solid.is_valid() && casing.solid.solid_count() == 1);
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_axial_bend_gives_a_valid_flow_domain_but_no_casing() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::ZBend;
        let build = |bend: Option<ZBend>| {
            let mut input = VoluteInput::default();
            input.diffuser.z_bend = bend;
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(cad.fluid.is_valid());
            assert_eq!(cad.fluid.solid_count(), 1);
            assert_eq!(cad.channel.solid_count(), 1);
            assert!(cad.warnings.is_empty(), "{:?}", cad.warnings);
            (g, cad)
        };
        let straight = build(None).1.fluid_volume();
        let (g, cad) = build(Some(ZBend {
            extent: 0.03,
            radius: 0.08,
            angle: 25f64.to_radians(),
        }));
        // The same areas along the same length: the volumes agree to a few per cent.
        assert!((cad.fluid_volume() / straight - 1.0).abs() < 0.03);
        // The casing is refused, with the reason, rather than returned in pieces.
        let error = build_volute_casing(&g, &cad).unwrap_err().to_string();
        assert!(error.contains("axial bend"), "{error}");
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_eccentric_and_rectangular_diffusers_give_valid_flow_domains() {
        let _slot = crate::test_slot::take();
        use tp_design::Param;
        use tp_design::volute::{DischargeInput, DischargeShape, OutletShape, StraightWall};
        let fluid = |edit: &dyn Fn(&mut DischargeInput)| -> f64 {
            let mut input = VoluteInput::default();
            edit(&mut input.diffuser);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(cad.fluid.is_valid());
            assert_eq!(cad.fluid.solid_count(), 1);
            assert_eq!(cad.channel.solid_count(), 1);
            assert!(cad.warnings.is_empty(), "{:?}", cad.warnings);
            cad.fluid_volume()
        };
        let tangential = |d: &mut DischargeInput| {
            d.shape = DischargeShape::Tangential {
                straight_wall: StraightWall::Centered,
            }
        };
        let centered = fluid(&tangential);
        let eccentric = fluid(&|d| {
            tangential(d);
            d.eccentricity = Param::Manual(-0.5);
        });
        let radial = fluid(&|_| {});
        let rectangular = fluid(&|d| d.outlet_shape = OutletShape::Rectangular { aspect: 1.5 });
        // The casing round a rectangular outlet closes flat, with no flange.
        let mut input = VoluteInput::default();
        input.diffuser.outlet_shape = OutletShape::Rectangular { aspect: 1.5 };
        let g = VoluteGeometry::build(&basis(), &input).unwrap();
        let cad = build_volute(&g).unwrap();
        let casing = build_volute_casing(&g, &cad).unwrap();
        assert!(casing.solid.is_valid() && casing.solid.solid_count() == 1);
        assert!(!casing.flange, "{:?}", casing.warnings);
        // The same areas along the same length: the volumes agree to a few per cent.
        assert!(
            (eccentric / centered - 1.0).abs() < 0.03,
            "{eccentric} vs {centered}"
        );
        assert!(
            (rectangular / radial - 1.0).abs() < 0.03,
            "{rectangular} vs {radial}"
        );
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn cutwater_and_discharge_shapes_give_valid_solids() {
        let _slot = crate::test_slot::take();
        use tp_design::Param;
        use tp_design::volute::{CutwaterShape, DischargeShape, StraightWall};
        let fluid = |cutwater: CutwaterShape, discharge: DischargeShape| -> f64 {
            let mut input = VoluteInput::default();
            input.cutwater.shape = cutwater;
            input.diffuser.shape = discharge;
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(cad.fluid.is_valid(), "{cutwater:?} {discharge:?}");
            assert_eq!(cad.fluid.solid_count(), 1, "{cutwater:?} {discharge:?}");
            cad.fluid_volume()
        };
        let straight = DischargeShape::Tangential {
            straight_wall: StraightWall::Centered,
        };
        let sharp = fluid(CutwaterShape::Sharp, straight);
        let rounded = fluid(
            CutwaterShape::Fillet {
                radius: Param::Auto,
            },
            straight,
        );
        // The round adds a little to the flow domain.
        assert!(
            rounded > sharp && rounded < 1.02 * sharp,
            "{rounded} vs {sharp}"
        );
        fluid(CutwaterShape::Tangent, straight);
        fluid(
            CutwaterShape::default(),
            DischargeShape::Radial { axis_angle: 0.0 },
        );
        fluid(
            CutwaterShape::default(),
            DischargeShape::Tangential {
                straight_wall: StraightWall::Centered,
            },
        );
    }
    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_fillet_cutwater_is_rounded_by_the_kernel_without_a_body() {
        use tp_design::volute::CutwaterShape;
        let _slot = crate::test_slot::take();
        let fluid = |shape: CutwaterShape| {
            let mut input = VoluteInput::default();
            input.cutwater.shape = shape;
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            build_volute(&g).unwrap()
        };
        let sharp = fluid(CutwaterShape::Sharp);
        let mut input = VoluteInput::default();
        input.cutwater.shape = CutwaterShape::Fillet {
            radius: tp_design::Param::Auto,
        };
        let g = VoluteGeometry::build(&basis(), &input).unwrap();
        let fillet = build_volute(&g).unwrap();
        // The casing round it is one valid solid too.
        let casing = build_volute_casing(&g, &fillet).unwrap();
        assert!(casing.solid.is_valid() && casing.solid.solid_count() == 1);
        // Rounded by the kernel as asked, with nothing to say.
        assert!(fillet.warnings.is_empty(), "{:?}", fillet.warnings);
        for s in [&fillet.channel, &fillet.fluid] {
            assert!(s.is_valid());
            assert_eq!(s.solid_count(), 1);
        }
        // The round adds a sliver of flow at the tip of the sharp edge.
        let (sharp, fillet) = (sharp.fluid_volume(), fillet.fluid_volume());
        assert!(
            fillet > sharp && fillet < sharp * 1.01,
            "{fillet} vs {sharp}"
        );
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_rounded_cutwater_is_rounded_in_round_sections_and_in_a_double_volute() {
        use tp_design::volute::{CutwaterShape, VoluteKind};
        let _slot = crate::test_slot::take();
        type Setting = fn(&mut VoluteInput);
        let cases: [(&str, Setting); 2] = [
            ("round", |v| {
                v.cross_section = CrossSectionInput::single(SectionShape::round());
            }),
            ("double", |v| v.setup.kind = VoluteKind::Double),
        ];
        let rounded_shapes = [
            (
                "fillet",
                CutwaterShape::Fillet {
                    radius: tp_design::Param::Auto,
                },
            ),
            (
                "simple",
                CutwaterShape::Simple {
                    cut: tp_design::Param::Auto,
                    handle: tp_design::Param::Auto,
                    measure: tp_design::volute::CutMeasure::Along,
                },
            ),
        ];
        for (name, edit) in cases {
            let build = |shape: CutwaterShape| {
                let mut input = VoluteInput::default();
                edit(&mut input);
                input.cutwater.shape = shape;
                let g = VoluteGeometry::build(&basis(), &input).unwrap();
                let cad = build_volute(&g).unwrap();
                (g, cad)
            };
            let (_, sharp) = build(CutwaterShape::Sharp);
            for (closing, shape) in rounded_shapes {
                let case = format!("{name} {closing}");
                let (g, rounded) = build(shape);
                assert!(
                    rounded.warnings.is_empty(),
                    "{case}: {:?}",
                    rounded.warnings
                );
                for s in [&rounded.channel, &rounded.fluid] {
                    assert!(s.is_valid(), "{case}");
                    assert_eq!(s.solid_count(), 1, "{case}");
                }
                // The closing adds a sliver of flow at the tip of the sharp edge.
                let (sharp, rounded_volume) = (sharp.fluid_volume(), rounded.fluid_volume());
                assert!(
                    rounded_volume > sharp && rounded_volume < sharp * 1.01,
                    "{case}: {rounded_volume} vs {sharp}"
                );
                // The casing round it is one valid solid too.
                let casing = build_volute_casing(&g, &rounded).unwrap();
                assert!(
                    casing.solid.is_valid() && casing.solid.solid_count() == 1,
                    "{case}"
                );
            }
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_single_volute_of_radius_and_segment_sections_gives_a_valid_flow_domain() {
        let _slot = crate::test_slot::take();
        use tp_design::volute::{RadiusSide, SideShape, section};
        for (name, shape) in [
            (
                "radius based",
                SectionShape::symmetric(SideShape::RadiusBased(RadiusSide::default())),
            ),
            (
                "line segments",
                SectionShape::symmetric(SideShape::LineSegments {
                    points: section::default_segments(),
                }),
            ),
        ] {
            let input = VoluteInput {
                cross_section: CrossSectionInput::single(shape),
                ..VoluteInput::default()
            };
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            assert!(cad.fluid.is_valid(), "{name}");
            assert_eq!(cad.fluid.solid_count(), 1, "{name}");
            assert_eq!(cad.channel.solid_count(), 1, "{name}");
            // Sections of radii and segments keep their cutwater sharp, and say so.
            assert!(
                cad.warnings
                    .iter()
                    .all(|w| w.contains("of radii and segments sharp")),
                "{name}: {:?}",
                cad.warnings
            );
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_rounded_diffuser_base_leaves_no_step_in_the_casing() {
        let _slot = crate::test_slot::take();
        // The bulged start of the diffuser's wall stood out of the spiral's at the
        // throat, two small end faces showing beside the ring.
        let mut input = VoluteInput::default();
        input.cutwater.shape = tp_design::volute::CutwaterShape::Fillet {
            radius: tp_design::Param::Auto,
        };
        input.diffuser.base_form = tp_design::Param::Manual(0.5);
        let mut g = VoluteGeometry::build(&basis(), &input).unwrap();
        g.design.flange = None;
        let (flow, _) = crate::build_volute_flow(&g).unwrap();
        let casing = build_volute_casing(&g, &flow).unwrap().solid;
        assert!(casing.is_valid() && casing.solid_count() == 1);
        // Upright planar faces of any size (mm²): the outlet face alone.
        let mut area = vec![0.0; casing.face_count()];
        let triangles = casing.triangles(0.5).unwrap();
        for (t, &f) in triangles.iter().zip(&casing.triangle_faces()) {
            area[f as usize] += 0.5 * (t[1] - t[0]).cross(t[2] - t[0]).length();
        }
        let upright: Vec<f64> = casing
            .face_planes()
            .iter()
            .zip(&area)
            .filter(|(plane, a)| plane.is_some_and(|(n, _)| n.z.abs() < 0.9) && **a > 1.0)
            .map(|(_, a)| *a)
            .collect();
        assert_eq!(upright.len(), 1, "{upright:?}");
    }
}
