//! CAD solid of the volute flow domain: the vaneless ring after the impeller, the
//! spiral channel and the discharge diffuser. A double volute lofts its whole sections
//! as one spiral and cuts the rib out of it.

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
const FUZZY_GAPS: [f64; 4] = [0.0, 1e-3, 1e-4, 1e-2];

/// Builds the volute channel and its flow domain from the geometry.
pub fn build_volute(g: &VoluteGeometry) -> Result<VoluteCad, CadError> {
    let breaks = g.ring_breaks();
    let lofts = g
        .lofts()
        .iter()
        .map(|rings| {
            let wires = rings
                .iter()
                .map(|ring| section_wire(ring, &breaks))
                .collect::<Result<Vec<_>, _>>()?;
            occ::loft(&wires)
        })
        .collect::<Result<Vec<_>, _>>()?;

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

    let assemble = |fuzzy: f64| -> Result<VoluteCad, CadError> {
        // A turn by zero copies the first piece for this attempt.
        let mut channel = lofts[0].rotated_z(0.0)?;
        for piece in &lofts[1..] {
            channel = channel.fuse_fuzzy(piece, fuzzy)?;
        }
        let mut fluid = ring.fuse_fuzzy(&channel, fuzzy)?;
        for tool in &tools {
            fluid = fluid.cut_fuzzy(tool, fuzzy)?;
            channel = channel.cut_fuzzy(tool, fuzzy)?;
        }
        Ok(VoluteCad { channel, fluid })
    };
    // The flow domain is what gets exported.
    let sound = |cad: &VoluteCad| cad.fluid.is_valid() && cad.fluid.solid_count() == 1;
    // The first sound attempt, else the first that built at all.
    let mut fallback = None;
    let mut error = None;
    for fuzzy in FUZZY_GAPS {
        match assemble(fuzzy) {
            Ok(cad) if sound(&cad) => return Ok(cad),
            Ok(cad) => {
                fallback.get_or_insert(cad);
            }
            Err(e) => {
                error.get_or_insert(e);
            }
        }
    }
    fallback.map_or_else(|| Err(error.expect("at least one attempt")), Ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
    use tp_design::outlet::ImpellerOutlet;
    use tp_design::volute::{SectionShape, VoluteInput};

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

    #[test]
    fn volute_channel_and_fluid_domain_are_valid_solids() {
        for shape in [SectionShape::default(), SectionShape::Circular] {
            let mut input = VoluteInput::default();
            input.cross_section.shape = shape;
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
            assert!(fluid > channel + 0.8 * ring && fluid < channel + 1.3 * ring);
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
            &|v| v.cross_section.shape = SectionShape::Rectangular,
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
        for shape in [SectionShape::default(), SectionShape::Rectangular] {
            for discharge in [
                DischargeShape::default(),
                DischargeShape::Tangential {
                    straight_wall: StraightWall::Inner,
                },
                DischargeShape::Radial { axis_angle: 0.0 },
            ] {
                for cutwater in [CutwaterShape::default(), CutwaterShape::Tangent] {
                    let mut input = VoluteInput::default();
                    input.cross_section.shape = shape;
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
        let straight = DischargeShape::default();
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
