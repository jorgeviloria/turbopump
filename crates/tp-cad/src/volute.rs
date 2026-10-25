//! CAD solids of the volute: the flow domain (the vaneless ring after the impeller, the
//! spiral channel and the discharge diffuser) and the casing around it. A double volute
//! lofts its whole sections as one spiral and cuts the rib out of it. The body of a
//! rounded cutwater is lofted through the outlines the viewer meshes
//! ([`VoluteGeometry::cutwater_body`]) and cut out of the flow domain.
//!
//! The casing ([`build_volute_casing`]) lofts the outlines of the flow domain offset by
//! the wall thickness ([`VoluteGeometry::casing_lofts`]) and cuts the flow domain and the
//! space of the impeller (r < r₂) out of them. The cutwater, the rib of a double volute and the
//! walls beside the vaneless ring stay as material; the casing ends flat at the outlet
//! of the diffuser and is open toward the impeller.

use tp_design::volute::VoluteGeometry;
use tp_math::{DVec2, DVec3};

use crate::CadError;
use crate::occ::{self, Shape};

/// Millimetres per metre: the CAD model is built in millimetres.
const MM: f64 = 1000.0;

/// CAD solids of one volute, in millimetres.
#[derive(Debug)]
pub struct VoluteCad {
    /// Spiral channel and discharge diffuser. The body of the cutwater may cut the start
    /// of a spiral that starts small off as a solid of its own.
    pub channel: Shape,
    /// The channel plus the ring between impeller outlet and cutwater circle.
    pub fluid: Shape,
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
fn section_wire(ring: &[DVec3], breaks: &[usize]) -> Result<Shape, CadError> {
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
const FUZZY_GAPS: [f64; 5] = [0.0, 1e-3, 1e-4, 1e-2, 1e-1];

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
fn first_sound<T>(
    attempt: impl Fn(f64) -> Result<T, CadError>,
    sound: impl Fn(&T) -> bool,
    repair: impl Fn(&T) -> Option<T>,
) -> Result<T, CadError> {
    let mut fallback = None;
    let mut error = None;
    for fuzzy in FUZZY_GAPS {
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

/// Builds the volute channel and its flow domain from the geometry.
pub fn build_volute(g: &VoluteGeometry) -> Result<VoluteCad, CadError> {
    let breaks = g.ring_breaks();
    let lofts = loft_pieces(&g.lofts(), &breaks)?;

    // Ring between the impeller outlet and the cutwater circle, as wide as the volute
    // inlet, with the bend and neck of an internal volute. It reaches slightly into the
    // channel so the two solids overlap.
    let ring = occ::revolve_z(&occ::planar_face(&profile_wire(&g.inlet_profile())?)?)?;

    // The body of a rounded cutwater and the rib of a double volute are carved out of
    // the flow domain across the whole height of the volute, each lofted through its
    // outlines plane by plane.
    let zs = g.rings().into_iter().flatten().map(|p| p.z);
    let (z_lo, z_hi) = zs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
        (lo.min(z), hi.max(z))
    });
    let margin = 0.1 * (z_hi - z_lo);
    // Each tool with the points its cut must leave on the surface of the channel and
    // of the ring.
    let mut tools: Vec<(Shape, Vec<DVec3>, Vec<DVec3>)> = Vec::new();
    if let Some(body) = cutwater_body(g, (z_lo - margin) * MM, (z_hi + margin) * MM)? {
        let (faces, noses) = cutwater_probes(g);
        tools.push((body, faces, noses));
    }
    if let Some(rib) = rib_body(g, (z_lo - margin) * MM, (z_hi + margin) * MM)? {
        tools.push((rib, rib_probes(g), Vec::new()));
    }

    // A failed fusion may still give one valid solid that lost a piece: the channel must
    // keep about the volume of its pieces, which only overlap where the spiral reaches
    // into the diffuser, and the flow domain most of the ring besides (quick estimates).
    // The channel and the flow domain may each need their own gap.
    let pieces: f64 = lofts.iter().map(|l| l.volume_with_tolerance(0.0)).sum();
    let ring_volume = ring.volume_with_tolerance(0.0);
    let whole = |channel: &Shape| channel.volume_with_tolerance(0.0) > 0.9 * pieces;
    let joined = first_sound(
        |fuzzy| fuse_all(&lofts, fuzzy),
        |channel| channel.is_valid() && channel.solid_count() == 1 && whole(channel),
        |channel| channel.fixed().ok(),
    )?;
    // The cutwater and the rib come out of the channel and of the ring apart, before
    // they are joined: the ring needs a gap to join some channels (external round
    // sections), and the body of the cutwater no longer cuts cleanly out of that
    // union, while it does out of each piece. Each tool finds its own gap. A cut that
    // fails may still give one valid solid, even one a little smaller: the body's cut
    // must leave its points on the surface (the faces on the channel, the nose on the
    // ring), the rib's take something away. The body may cut the start of a spiral
    // that starts small off the rest of the channel: the ring joins them again.
    let carve = |shape: &Shape, ring: bool| -> Result<Shape, CadError> {
        let mut carved = shape.rotated_z(0.0)?;
        for (tool, faces, noses) in &tools {
            let probes = if ring { noses } else { faces };
            let before = carved.volume_with_tolerance(0.0);
            let next = first_sound(
                |fuzzy| carved.cut_fuzzy(tool, fuzzy),
                |cut| {
                    cut.is_valid()
                        && (cut.solid_count() == 1 || !ring && cut.solid_count() > 1)
                        && if probes.is_empty() {
                            cut.volume_with_tolerance(0.0) < before
                        } else {
                            on_surface(cut, probes)
                        }
                },
                |cut| cut.fixed().ok(),
            )?;
            carved = next;
        }
        Ok(carved)
    };
    let channel = carve(&joined, false)?;
    let inlet = carve(&ring, true)?;
    // The flow domain is what gets exported.
    let fluid = first_sound(
        |fuzzy| inlet.fuse_fuzzy(&channel, fuzzy),
        |fluid| {
            fluid.is_valid()
                && fluid.solid_count() == 1
                && fluid.volume_with_tolerance(0.0)
                    > channel.volume_with_tolerance(0.0) + 0.5 * ring_volume
        },
        |fluid| fluid.fixed().ok(),
    )?;

    Ok(VoluteCad { channel, fluid })
}

/// Deviation from the outlines of the cutwater body that its splines may take, m.
const OUTLINE_TOLERANCE: f64 = 1e-5;

/// Longest gap between the points a spline through an outline of the body passes, m.
const OUTLINE_SPACING: f64 = 2e-3;

/// The points of the outlines `outlines`, which correspond one to one, that are needed
/// to follow every one of them within [`OUTLINE_TOLERANCE`] by straight segments
/// (Douglas–Peucker over all of them at once), no more than [`OUTLINE_SPACING`] apart. The outlines are sampled densely along
/// the walls; a spline through every point would make heavy surfaces and slow booleans,
/// and the same points in every plane keep the lofted surface light.
fn outline_points(outlines: &[&[DVec3]]) -> Vec<usize> {
    let n = outlines[0].len();
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut spans = vec![(0, n - 1)];
    while let Some((a, b)) = spans.pop() {
        // The point farthest from its chord, over every outline.
        let far = (a + 1..b)
            .map(|i| {
                let off = outlines
                    .iter()
                    .map(|o| {
                        let (p, q) = (o[a], o[b]);
                        let t = ((o[i] - p).dot(q - p) / (q - p).length_squared().max(1e-30))
                            .clamp(0.0, 1.0);
                        o[i].distance(p + (q - p) * t)
                    })
                    .fold(0.0, f64::max);
                (i, off)
            })
            .max_by(|x, y| x.1.total_cmp(&y.1));
        let chord = outlines
            .iter()
            .map(|o| o[a].distance(o[b]))
            .fold(0.0, f64::max);
        let split = match far {
            Some((i, off)) if off > OUTLINE_TOLERANCE => Some(i),
            // A long straight run would let the spline swing where it ends.
            Some(_) if chord > OUTLINE_SPACING => Some((a + b) / 2),
            _ => None,
        };
        if let Some(i) = split {
            keep[i] = true;
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    (0..n).filter(|&i| keep[i]).collect()
}

/// Closed wire through the points `points` of one outline of the cutwater body at the
/// height `z`, mm: its smooth outline and the chord that closes it inside the wedge.
fn cutwater_wire(outline: &[DVec3], points: &[usize], z: f64) -> Result<Shape, CadError> {
    let points: Vec<DVec3> = points
        .iter()
        .map(|&i| DVec3::new(outline[i].x * MM, outline[i].y * MM, z))
        .collect();
    let (first, last) = (points[0], points[points.len() - 1]);
    occ::wire(&[
        occ::spline_edge(&points, false)?,
        occ::line_edge(last, first)?,
    ])
}

/// The body of a rounded cutwater, mm: lofted smoothly through its outlines in the
/// planes across the inlet width (the outlines the viewer meshes), and carried
/// straight on from the two end planes to `z_lo` and `z_hi`, beyond the side walls.
/// `None` unless rounded.
fn cutwater_body(g: &VoluteGeometry, z_lo: f64, z_hi: f64) -> Result<Option<Shape>, CadError> {
    let body = g.cutwater_body();
    let (Some(first), Some(last)) = (body.first(), body.last()) else {
        return Ok(None);
    };
    if body.iter().any(|o| o.len() != first.len()) {
        return Err(CadError::Input(
            "the outlines of the cutwater do not correspond".into(),
        ));
    }
    let outlines: Vec<&[DVec3]> = body.iter().map(Vec::as_slice).collect();
    let points = outline_points(&outlines);
    let wires = body
        .iter()
        .map(|o| cutwater_wire(o, &points, o[0].z * MM))
        .collect::<Result<Vec<_>, _>>()?;
    let extend = |outline: &[DVec3], from: f64, to: f64| -> Result<Shape, CadError> {
        let face = occ::planar_face(&cutwater_wire(outline, &points, from.min(to))?)?;
        occ::prism_z(&face, (to - from).abs())
    };
    let below = extend(first, first[0].z * MM, z_lo)?;
    let above = extend(last, last[0].z * MM, z_hi)?;
    if wires.len() < 2 {
        return Ok(Some(below.fuse(&above)?));
    }
    Ok(Some(occ::loft(&wires)?.fuse(&below)?.fuse(&above)?))
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

/// Points of the body of a rounded cutwater in each of its planes that a cut leaves on
/// the surface, mm: the middle of each face beyond the inlet ring, on the channel, and
/// the innermost point of the nose, on the ring.
fn cutwater_probes(g: &VoluteGeometry) -> (Vec<DVec3>, Vec<DVec3>) {
    // Clear of the inlet ring, which reaches 2 % of b₃ past r'_z.
    let clear = g.design.stagnation_radius + 0.03 * g.design.inlet_width;
    let mut faces = Vec::new();
    let mut noses = Vec::new();
    for s in g.cutwater_slices.iter().filter(|s| s.outline.len() > 2) {
        let radius = |p: &DVec3| p.truncate().length();
        let Some((nose, _)) = s
            .outline
            .iter()
            .enumerate()
            .min_by(|a, b| radius(a.1).total_cmp(&radius(b.1)))
        else {
            continue;
        };
        noses.push(s.outline[nose] * MM);
        // Each face from its join to where it reaches the ring.
        let spiral = s.outline[..nose]
            .iter()
            .take_while(|p| radius(p) > clear)
            .count();
        let diffuser = s.outline[nose..]
            .iter()
            .rev()
            .take_while(|p| radius(p) > clear)
            .count();
        if spiral > 0 {
            faces.push(s.outline[spiral / 2] * MM);
        }
        if diffuser > 0 {
            faces.push(s.outline[s.outline.len() - 1 - diffuser / 2] * MM);
        }
    }
    (faces, noses)
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
/// outline.
fn casing_pieces(g: &VoluteGeometry) -> Result<(Vec<Shape>, Lofts), CadError> {
    let halves: Lofts = g
        .casing_lofts()
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
    Ok((loft_pieces(&halves, &g.ring_breaks())?, halves))
}

pub fn build_volute_casing(g: &VoluteGeometry, cad: &VoluteCad) -> Result<VoluteCasing, CadError> {
    let (mut outer, halves) = casing_pieces(g)?;
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
    let radius = g.design.cutwater_fillet * MM;
    let enclose = |fuzzy: f64| -> Result<(Shape, f64, Option<f64>), CadError> {
        let space = cad.fluid.fuse_fuzzy(&impeller, fuzzy)?;
        let mut whole = fuse_all(&outer, fuzzy)?;
        // The radius it took; 0 when asked for none.
        let mut rounded = Some(0.0);
        if radius > 0.0 && !g.cutwater_slices.is_empty() {
            rounded = fill_groove(&whole, &outer, g, radius).map(|(filled, r)| {
                whole = filled;
                r
            });
        }
        let gross = whole.volume_with_tolerance(0.0);
        Ok((whole.cut_fuzzy(&space, fuzzy)?, gross, rounded))
    };
    let sound = |(solid, gross, _): &(Shape, f64, Option<f64>)| {
        solid.is_valid()
            && solid.solid_count() == 1
            && solid.volume_with_tolerance(0.0) < gross - 0.8 * channel
    };
    let (solid, _, rounded) = first_sound(enclose, sound, |(solid, gross, rounded)| {
        Some((solid.fixed().ok()?, *gross, *rounded))
    })?;
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
    let (u, v) = (DVec3::Z, axis.cross(DVec3::Z));
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

/// The union of `pieces`.
fn fuse_all(pieces: &[Shape], fuzzy: f64) -> Result<Shape, CadError> {
    // A turn by zero copies the first piece for this attempt.
    let mut whole = pieces[0].rotated_z(0.0)?;
    for piece in &pieces[1..] {
        whole = whole.fuse_fuzzy(piece, fuzzy)?;
    }
    Ok(whole)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
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
    fn volute_channel_and_fluid_domain_are_valid_solids() {
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
        let outlines: Vec<DVec3> = g
            .cutwater_slices
            .iter()
            .filter(|s| s.outline.len() > 2)
            .flat_map(|s| s.outline.iter().map(|p| *p * MM))
            .collect();
        // Between the end planes of the body.
        let (lo, hi) = outlines
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.z), hi.max(p.z))
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
        let Some(d) = departure_from_the_view(g, cad) else {
            return;
        };
        let nose = g
            .cutwater_slices
            .iter()
            .map(|s| s.nose_distance * MM)
            .fold(f64::INFINITY, f64::min);
        assert!(
            d.outline < 0.1,
            "{case}: the outlines {} mm off the CAD",
            d.outline
        );
        assert!(
            d.solid < d.walls + 0.2,
            "{case}: the CAD {} mm off the view, the walls {} mm",
            d.solid,
            d.walls
        );
        assert!(
            d.nearest > nose - 0.01,
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
    fn the_casing_wraps_the_flow_domain_in_one_solid() {
        use tp_design::volute::VoluteKind;
        // The trapezoidal default, rectangles whose flat sides meet in the throat, and
        // a double volute.
        let edits: [&dyn Fn(&mut VoluteInput); 3] = [
            &|_| {},
            &|v| v.cross_section = CrossSectionInput::single(SectionShape::rectangular()),
            &|v| v.setup.kind = VoluteKind::Double,
        ];
        for edit in edits {
            let mut input = VoluteInput::default();
            edit(&mut input);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = build_volute(&g).unwrap();
            let casing = build_volute_casing(&g, &cad).unwrap();
            assert!(casing.solid.is_valid() && casing.solid.solid_count() == 1);
            assert!(casing.flange, "{:?}", casing.warnings);
            // The groove over the cutwater rounded with the radius asked for.
            assert!(casing.warnings.is_empty(), "{:?}", casing.warnings);
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
    fn the_casing_fills_and_rounds_the_groove_over_the_cutwater() {
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
            let (pieces, _) = casing_pieces(&g).unwrap();
            // As the casing does: the first fusion that takes the fillet.
            let (whole, (filled, took)) = FUZZY_GAPS
                .into_iter()
                .find_map(|fuzzy| {
                    let whole = fuse_all(&pieces, fuzzy).ok()?;
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
    fn a_double_volute_is_one_valid_solid() {
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
                v.setup.rib_wrap = tp_design::Param::Manual(170f64.to_radians());
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
    fn an_internal_volute_is_one_solid_with_its_bend() {
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
    fn common_volute_options_give_one_valid_flow_domain() {
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
    fn spiral_development_options_give_one_valid_flow_domain() {
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
    fn cutwater_and_discharge_shapes_give_valid_solids() {
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
            CutwaterShape::Rounded {
                radius: Param::Auto,
            },
            straight,
        );
        // The nose takes a little of the flow domain.
        assert!(
            rounded < sharp && rounded > 0.98 * sharp,
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
}
