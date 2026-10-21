//! CAD solids of the volute: the flow domain (the vaneless ring after the impeller, the
//! spiral channel and the discharge diffuser) and the casing around it. A double volute
//! lofts its whole sections as one spiral and cuts the rib out of it.
//!
//! The casing ([`build_volute_casing`]) lofts the outlines of the flow domain offset by
//! the wall thickness ([`VoluteGeometry::casing_lofts`]) and cuts the flow domain and the
//! space of the impeller (r < r₂) out of them. The cutwater, the rib of a double volute and the
//! walls beside the vaneless ring stay as material; the casing ends flat at the outlet
//! of the diffuser and is open toward the impeller.

use tp_design::volute::VoluteGeometry;
use tp_math::DVec3;

use crate::CadError;
use crate::occ::{self, Shape};

/// Millimetres per metre: the CAD model is built in millimetres.
const MM: f64 = 1000.0;

/// CAD solids of one volute, in millimetres.
#[derive(Debug)]
pub struct VoluteCad {
    /// Spiral channel and discharge diffuser.
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

/// The first attempt over [`FUZZY_GAPS`] whose result is `sound`, else the first that
/// built at all.
fn first_sound<T>(
    attempt: impl Fn(f64) -> Result<T, CadError>,
    sound: impl Fn(&T) -> bool,
) -> Result<T, CadError> {
    let mut fallback = None;
    let mut error = None;
    for fuzzy in FUZZY_GAPS {
        match attempt(fuzzy) {
            Ok(result) if sound(&result) => return Ok(result),
            Ok(result) => {
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
    // inlet. It reaches slightly into the channel so the two solids overlap.
    let (inner, outer) = g.inlet_ring();
    let b3 = g.design.inlet_width;
    let zm = g.basis.midplane;
    let overlap = 0.02 * b3;
    let corner = |r: f64, z: f64| DVec3::new(r * MM, 0.0, z * MM);
    let (r0, r1) = (inner, outer + overlap);
    // A hair narrower than b₃, so that its sides do not run on the flat side walls of
    // rectangular sections.
    let half = 0.995 * b3 / 2.0;
    let (z0, z1) = (zm - half, zm + half);
    let edges = [
        occ::line_edge(corner(r0, z0), corner(r1, z0))?,
        occ::line_edge(corner(r1, z0), corner(r1, z1))?,
        occ::line_edge(corner(r1, z1), corner(r0, z1))?,
        occ::line_edge(corner(r0, z1), corner(r0, z0))?,
    ];
    let ring = occ::revolve_z(&occ::planar_face(&occ::wire(&edges)?)?)?;

    // The rounded nose of the cutwater and the rib of a double volute are carved out of
    // the flow domain across the whole height of the volute.
    let zs = g.rings().into_iter().flatten().map(|p| p.z);
    let (z_lo, z_hi) = zs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
        (lo.min(z), hi.max(z))
    });
    let margin = 0.1 * (z_hi - z_lo);
    let at = |p: &DVec3| DVec3::new(p.x * MM, p.y * MM, (z_lo - margin) * MM);
    let extrude = |edges: &[Shape]| -> Result<Shape, CadError> {
        occ::prism_z(
            &occ::planar_face(&occ::wire(edges)?)?,
            (z_hi - z_lo + 2.0 * margin) * MM,
        )
    };
    let mut tools = Vec::new();
    let nose = g.cutwater.nose_outline();
    if !nose.is_empty() {
        let arc: Vec<DVec3> = nose[..nose.len() - 1].iter().map(at).collect();
        let tip = at(&nose[nose.len() - 1]);
        tools.push(extrude(&[
            occ::spline_edge(&arc, false)?,
            occ::line_edge(arc[arc.len() - 1], tip)?,
            occ::line_edge(tip, arc[0])?,
        ])?);
    }
    if let Some(rib) = &g.rib {
        let spline = |points: &mut dyn Iterator<Item = &DVec3>| {
            occ::spline_edge(&points.map(at).collect::<Vec<_>>(), false)
        };
        tools.push(extrude(&[
            spline(&mut rib.nose.iter())?,
            spline(&mut rib.inner_face.iter())?,
            spline(&mut rib.tail.iter())?,
            spline(&mut rib.outer_face.iter().rev())?,
        ])?);
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
    )?;
    let assemble = |fuzzy: f64| -> Result<(Shape, Shape), CadError> {
        // A turn by zero copies the channel for this attempt.
        let mut channel = joined.rotated_z(0.0)?;
        let mut fluid = ring.fuse_fuzzy(&channel, fuzzy)?;
        for tool in &tools {
            fluid = fluid.cut_fuzzy(tool, fuzzy)?;
            channel = channel.cut_fuzzy(tool, fuzzy)?;
        }
        Ok((channel, fluid))
    };
    // The flow domain is what gets exported.
    let (channel, fluid) = first_sound(assemble, |(channel, fluid)| {
        fluid.is_valid()
            && fluid.solid_count() == 1
            && whole(channel)
            && fluid.volume_with_tolerance(0.0)
                > channel.volume_with_tolerance(0.0) + 0.5 * ring_volume
    })?;

    Ok(VoluteCad { channel, fluid })
}

/// Builds the casing around the flow domain `cad` of the volute `g`.
pub fn build_volute_casing(g: &VoluteGeometry, cad: &VoluteCad) -> Result<VoluteCasing, CadError> {
    // The start of the spiral runs just inside its throat, the cutwater between them:
    // offset outward, the two would cut into each other within one loft. Each piece is
    // lofted in two halves, which meet on their middle outline.
    let halves: Vec<Vec<Vec<DVec3>>> = g
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
    let outer = loft_pieces(&halves, &g.ring_breaks())?;
    // The space of the impeller, through the whole height of the casing.
    let zs = halves.iter().flatten().flatten().map(|p| p.z);
    let (z_lo, z_hi) = zs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
        (lo.min(z), hi.max(z))
    });
    let margin = 0.1 * (z_hi - z_lo);
    let (lo, hi) = ((z_lo - margin) * MM, (z_hi + margin) * MM);
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
    let enclose = |fuzzy: f64| -> Result<(Shape, f64), CadError> {
        let space = cad.fluid.fuse_fuzzy(&impeller, fuzzy)?;
        let whole = fuse_all(&outer, fuzzy)?;
        let gross = whole.volume_with_tolerance(0.0);
        Ok((whole.cut_fuzzy(&space, fuzzy)?, gross))
    };
    let sound = |(solid, gross): &(Shape, f64)| {
        solid.is_valid()
            && solid.solid_count() == 1
            && solid.volume_with_tolerance(0.0) < gross - 0.8 * channel
    };
    let (solid, _) = first_sound(enclose, sound)?;
    let Some(f) = &g.design.flange else {
        return Ok(VoluteCasing {
            solid,
            flange: false,
            warnings: Vec::new(),
        });
    };
    match flange(g, f).and_then(|disk| {
        let joined = solid.fuse(&disk)?;
        Ok(joined)
    }) {
        Ok(joined) if joined.is_valid() && joined.solid_count() == 1 => Ok(VoluteCasing {
            solid: joined,
            flange: true,
            warnings: Vec::new(),
        }),
        result => Ok(VoluteCasing {
            solid,
            flange: false,
            warnings: vec![format!(
                "the flange could not be joined to the casing{}: the casing ends flat",
                result.err().map_or(String::new(), |e| format!(" ({e})"))
            )],
        }),
    }
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
        }
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
        // A long elliptic nose, a thick rib, rectangles and a tangent cutwater.
        let edits: [&dyn Fn(&mut VoluteInput); 4] = [
            &|v| v.setup.rib_nose_ratio = tp_design::Param::Manual(3.0),
            &|v| v.setup.rib_thickness = tp_design::Param::Manual(0.008),
            &|v| v.cross_section = CrossSectionInput::single(SectionShape::rectangular()),
            &|v| {
                v.cutwater.shape = tp_design::volute::CutwaterShape::Tangent;
                v.setup.rib_wrap = tp_design::Param::Manual(170f64.to_radians());
            },
        ];
        for edit in edits {
            double(edit);
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
                    assert!(
                        cad.fluid.is_valid() && cad.fluid.solid_count() == 1,
                        "{shape:?} {discharge:?} {cutwater:?}"
                    );
                }
            }
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
