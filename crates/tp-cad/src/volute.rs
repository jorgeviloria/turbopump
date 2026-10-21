//! CAD solid of the volute flow domain: the vaneless ring after the impeller, the
//! spiral channel and the discharge diffuser. A double volute lofts its two channels
//! and the diffuser apart and joins them.

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

/// Builds the volute channel and its flow domain from the geometry.
pub fn build_volute(g: &VoluteGeometry) -> Result<VoluteCad, CadError> {
    let breaks = g.ring_breaks();
    let mut channel: Option<Shape> = None;
    for rings in g.channels() {
        let wires = rings
            .iter()
            .map(|ring| section_wire(ring, &breaks))
            .collect::<Result<Vec<_>, _>>()?;
        let piece = occ::loft(&wires)?;
        channel = Some(match channel {
            None => piece,
            Some(c) => c.fuse(&piece)?,
        });
    }
    let channel = channel.expect("a volute has at least one channel");

    // Ring between the impeller outlet and the cutwater circle, as wide as the volute
    // inlet. It reaches slightly into the channel so the two solids overlap.
    let (inner, outer) = g.inlet_ring();
    let b3 = g.design.inlet_width;
    let zm = g.basis.midplane;
    let overlap = 0.02 * b3;
    let corner = |r: f64, z: f64| DVec3::new(r * MM, 0.0, z * MM);
    let (r0, r1) = (inner, outer + overlap);
    let (z0, z1) = (zm - b3 / 2.0, zm + b3 / 2.0);
    let edges = [
        occ::line_edge(corner(r0, z0), corner(r1, z0))?,
        occ::line_edge(corner(r1, z0), corner(r1, z1))?,
        occ::line_edge(corner(r1, z1), corner(r0, z1))?,
        occ::line_edge(corner(r0, z1), corner(r0, z0))?,
    ];
    let ring = occ::revolve_z(&occ::planar_face(&occ::wire(&edges)?)?)?;
    let fluid = ring.fuse(&channel)?;
    Ok(VoluteCad { channel, fluid })
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
    fn expected_channel_volume(g: &VoluteGeometry) -> f64 {
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
        let mut input = VoluteInput::default();
        input.setup.kind = tp_design::volute::VoluteKind::Double;
        let g = VoluteGeometry::build(&basis(), &input).unwrap();
        let cad = build_volute(&g).unwrap();
        for s in [&cad.channel, &cad.fluid] {
            assert!(s.is_valid());
            assert_eq!(s.solid_count(), 1);
        }
        let channel = cad.channel.volume_with_tolerance(1e-3) / MM.powi(3);
        let expected = expected_channel_volume(&g);
        assert!(
            (channel / expected - 1.0).abs() < 0.05,
            "{channel} vs {expected}"
        );
    }
}
