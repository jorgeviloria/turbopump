//! The zone round the cutwater of a simple cutwater, trimmed instead of joined: each
//! wall cut along the rail where the other lies the gap away, with no boolean between
//! the walls of the spiral and of the diffuser, which meet almost tangentially near the
//! inlet corners. The idea of the volute tutorial of CAESES (Friendship Systems): trim
//! each surface with a ribbon, leave a gap, and close it with a surface of its own.

#[cfg(test)]
mod tests {
    use tp_design::Param;
    use tp_design::outlet::ImpellerOutlet;
    use tp_design::volute::{
        CutMeasure, CutwaterShape, DischargeShape, StraightWall, VoluteGeometry, VoluteInput,
    };
    use tp_math::DVec3;

    use crate::CadError;
    use crate::occ::{self, Shape};
    use crate::volute::{MM, section_wire};

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

    /// The walls the rails lie on, mm: the spiral lofted through its sections and the
    /// diffuser through its own.
    fn walls(g: &VoluteGeometry) -> Result<(Shape, Shape), CadError> {
        let sign = g.basis.rotation_sign;
        let breaks = g.ring_breaks();
        let wires = |rings: Vec<Vec<DVec3>>| {
            rings
                .iter()
                .map(|r| section_wire(r, &breaks))
                .collect::<Result<Vec<_>, _>>()
        };
        let spiral = occ::loft(&wires(g.sections.iter().map(|s| s.points(sign)).collect())?)?;
        let diffuser = occ::loft(&wires(g.nozzle.iter().map(|n| n.points.clone()).collect())?)?;
        Ok((spiral, diffuser))
    }

    #[test]
    fn the_rails_lie_on_the_walls_of_the_cad() {
        for shape in [
            DischargeShape::default(),
            DischargeShape::Tangential {
                straight_wall: StraightWall::Inner,
            },
        ] {
            let mut input = VoluteInput::default();
            input.diffuser.shape = shape;
            input.cutwater.shape = CutwaterShape::Simple {
                cut: Param::Manual(0.003),
                handle: Param::Auto,
                measure: CutMeasure::Across,
            };
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let rails = g.cutwater_rails(21).unwrap();
            let (spiral, diffuser) = walls(&g).unwrap();
            let worst = |wall: &Shape, rail: &[DVec3]| {
                let points: Vec<DVec3> = rail.iter().map(|p| *p * MM).collect();
                wall.boundary_distances(&points)
                    .unwrap()
                    .into_iter()
                    .fold(0.0, f64::max)
            };
            // The design follows the walls by splines through the points of the sections,
            // the CAD lofts between them: they part by microns, well under the sewing
            // tolerance, along the diffuser, whose sections lie far apart.
            let (s, d) = (
                worst(&spiral, &rails.spiral),
                worst(&diffuser, &rails.diffuser),
            );
            assert!(s < 1e-3, "{shape:?}: the spiral rail {s} mm off its wall");
            assert!(d < 1e-2, "{shape:?}: the diffuser rail {d} mm off its wall");
        }
    }
}
