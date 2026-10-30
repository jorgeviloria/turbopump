//! The volute as curves and surfaces, for modelling the solid in another CAD system.
//! With a sharp or fillet cutwater the surfaces are the walls of the flow domain: the
//! spiral and the diffuser trimmed where they meet, or where the fillet touches them,
//! and sewn to it, without the faces the flow comes in and goes out through. Where
//! that domain does not build, and for the other cutwaters, each surface is lofted,
//! revolved or extruded on its own, without a boolean operation, and left untrimmed
//! where it meets the next: the cutwater is where they cross.

use tp_design::volute::{CutwaterShape, VoluteGeometry};
use tp_math::{DVec2, DVec3};

use crate::CadError;
use crate::occ::{self, Shape};
use crate::volute::{MM, section_wire};

/// Planes across the inlet width the walls at the cutwater are drawn in.
const WALL_PLANES: usize = 5;

/// Planes across the inlet width the cutwater tip line is sampled in.
const TIP_PLANES: usize = 41;

/// The volute as curves and surfaces, mm.
#[derive(Debug)]
pub struct VoluteSurfaces {
    /// The outline of every section of the spiral and the diffuser, the arcs of a fillet
    /// plane by plane, the walls at the cutwater in planes across the inlet width, the
    /// profile of the inlet ring and the line of the cutwater tip.
    pub curves: Shape,
    /// The walls of the flow domain, trimmed and sewn at the cutwater ([`Self::trimmed`]);
    /// else the walls lofted through the sections of each channel and of the diffuser,
    /// the ring revolved about the axis and the nose of a fillet, each a shell of its
    /// own, open at its ends.
    pub surfaces: Shape,
    /// The surfaces are the walls of the flow domain, trimmed and sewn at the cutwater.
    pub trimmed: bool,
    /// The parts that could not be built.
    pub warnings: Vec<String>,
}

impl VoluteSurfaces {
    /// Curves and surfaces together, for one file.
    pub fn both(&self) -> Result<Shape, CadError> {
        occ::compound(&[self.curves.copied()?, self.surfaces.copied()?])
    }
}

/// The curves and surfaces of the volute `g`. A part that fails is left out with a
/// warning; it fails only if nothing builds.
pub fn build_volute_surfaces(g: &VoluteGeometry) -> Result<VoluteSurfaces, CadError> {
    let loose = loose_surfaces(g)?;
    if !matches!(
        g.design.cutwater_shape,
        CutwaterShape::Sharp | CutwaterShape::Fillet { .. }
    ) {
        return Ok(loose);
    }
    let trimmed =
        crate::build_volute_flow(g).and_then(|(cad, _)| Ok((walls(g, &cad.fluid)?, cad.warnings)));
    match trimmed {
        Ok((walls, warnings)) => Ok(VoluteSurfaces {
            curves: loose.curves,
            surfaces: walls,
            trimmed: true,
            warnings,
        }),
        Err(e) => {
            let mut loose = loose;
            loose.warnings.push(format!(
                "the walls could not be trimmed and sewn at the cutwater ({e}): they come \
                 loose, crossing there"
            ));
            Ok(loose)
        }
    }
}

/// The walls of the flow domain `fluid` (mm) of the volute `g`: every face but the
/// cylinder at d₂ the flow comes in through and the outlet face.
fn walls(g: &VoluteGeometry, fluid: &Shape) -> Result<Shape, CadError> {
    let r2 = g.inlet_ring().0 * MM;
    let triangles = fluid.triangles(0.5)?;
    let mut off = vec![0.0f64; fluid.face_count()];
    for (t, &f) in triangles.iter().zip(&fluid.triangle_faces()) {
        for p in t {
            off[f as usize] = off[f as usize].max((p.truncate().length() - r2).abs());
        }
    }
    let outlet = g
        .nozzle
        .last()
        .ok_or_else(|| CadError::Input("the volute has no diffuser".into()))?;
    let (axis, centre) = (outlet.tangent.normalize(), outlet.center * MM);
    let ends: Vec<u32> = fluid
        .face_planes()
        .iter()
        .enumerate()
        .filter(|(i, plane)| {
            off[*i] < WALL_TOLERANCE
                || plane.is_some_and(|(n, p)| {
                    n.dot(axis).abs() > 0.999 && (p - centre).dot(axis).abs() < WALL_TOLERANCE
                })
        })
        .map(|(i, _)| i as u32)
        .collect();
    if ends.len() < 2 {
        return Err(CadError::Operation(
            "the inlet and outlet faces of the flow domain were not found".into(),
        ));
    }
    fluid.faces_except(&ends)
}

/// How far from d₂ or from the outlet plane a face may lie to count as one the flow
/// comes in or goes out through, mm.
const WALL_TOLERANCE: f64 = 0.05;

/// The curves and the surfaces lofted, revolved or extruded each on its own.
fn loose_surfaces(g: &VoluteGeometry) -> Result<VoluteSurfaces, CadError> {
    let breaks = g.ring_breaks();
    let mut curves = Vec::new();
    let mut surfaces = Vec::new();
    let mut warnings = Vec::new();
    let keep = |what: &str,
                built: Result<Shape, CadError>,
                into: &mut Vec<Shape>,
                warnings: &mut Vec<String>| match built {
        Ok(shape) => into.push(shape),
        Err(e) => warnings.push(format!("{what}: {e}")),
    };

    for (k, rings) in g.channels().iter().enumerate() {
        let wires: Vec<Shape> = rings
            .iter()
            .filter_map(|ring| section_wire(ring, &breaks).ok())
            .collect();
        for w in &wires {
            curves.push(w.copied()?);
        }
        keep(
            &format!("the walls of channel {}", k + 1),
            occ::loft_shell(&wires),
            &mut surfaces,
            &mut warnings,
        );
    }

    match ring_profile(g) {
        Ok(profile) => {
            curves.push(profile.copied()?);
            keep(
                "the inlet ring",
                occ::revolved_surfaces_z(&profile),
                &mut surfaces,
                &mut warnings,
            );
        }
        Err(e) => warnings.push(format!("the inlet ring: {e}")),
    }

    let nose = fillet_wires(g);
    for w in &nose {
        curves.push(w.copied()?);
    }
    if nose.len() >= 2 {
        keep(
            "the fillet of the cutwater",
            occ::loft_shell(&nose),
            &mut surfaces,
            &mut warnings,
        );
    }

    for wall in tongue_walls(g) {
        keep("a wall at the cutwater", wall, &mut curves, &mut warnings);
    }

    if let Some(tip) = tip_line(g) {
        keep("the cutwater tip line", tip, &mut curves, &mut warnings);
    }

    if curves.is_empty() && surfaces.is_empty() {
        return Err(CadError::Operation(
            "no curve or surface of the volute could be built".into(),
        ));
    }
    Ok(VoluteSurfaces {
        curves: occ::compound(&curves)?,
        surfaces: occ::compound(&surfaces)?,
        trimmed: false,
        warnings,
    })
}

/// The arcs that round the tip of a fillet cutwater from plane to plane, one wire each,
/// mm. Empty for any other cutwater.
fn fillet_wires(g: &VoluteGeometry) -> Vec<Shape> {
    g.fillet_nose()
        .iter()
        .filter(|arc| arc.len() >= 2)
        .filter_map(|arc| {
            let points: Vec<DVec3> = arc.iter().map(|&p| p * MM).collect();
            occ::spline_edge(&points, false)
                .and_then(|e| occ::wire(&[e]))
                .ok()
        })
        .collect()
}

/// The walls of the spiral and of the diffuser near the cutwater in planes across the
/// inlet width, mm: where each runs from the tip on downstream, the outline to trim the
/// two surfaces to.
fn tongue_walls(g: &VoluteGeometry) -> Vec<Result<Shape, CadError>> {
    let half = g.design.inlet_width / 2.0;
    let zm = g.basis.midplane;
    (0..WALL_PLANES)
        .filter_map(|k| {
            let z = zm - half + 2.0 * half * k as f64 / (WALL_PLANES - 1) as f64;
            g.tongue_cut(z)
        })
        .flat_map(|cut| [cut.spiral, cut.diffuser])
        .filter(|wall| wall.len() >= 2)
        .map(|wall| {
            let points: Vec<DVec3> = wall.iter().map(|&p| p * MM).collect();
            occ::spline_edge(&points, false)
        })
        .collect()
}

/// The profile of the inlet ring in the plane through the x axis, mm.
fn ring_profile(g: &VoluteGeometry) -> Result<Shape, CadError> {
    let at = |p: DVec2| DVec3::new(p.x * MM, 0.0, p.y * MM);
    let edges = g
        .inlet_profile()
        .iter()
        .filter(|piece| piece.len() >= 2)
        .map(|piece| {
            let points: Vec<DVec3> = piece.iter().map(|&p| at(p)).collect();
            if points.len() == 2 {
                occ::line_edge(points[0], points[1])
            } else {
                occ::spline_edge(&points, false)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    occ::wire(&edges)
}

/// The line of the cutwater tip across the inlet width, mm: where the walls of the
/// spiral and the diffuser meet, plane by plane. `None` without a tip.
fn tip_line(g: &VoluteGeometry) -> Option<Result<Shape, CadError>> {
    let half = g.design.inlet_width / 2.0;
    let zm = g.basis.midplane;
    let tips: Vec<DVec3> = (0..TIP_PLANES)
        .filter_map(|k| {
            let z = zm - half + 2.0 * half * k as f64 / (TIP_PLANES - 1) as f64;
            g.tongue_cut(z).and_then(|cut| cut.tip)
        })
        .map(|p| p * MM)
        .collect();
    (tips.len() >= 2).then(|| occ::spline_edge(&tips, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_design::Param;
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

    #[test]
    fn every_cutwater_gives_loose_curves_and_surfaces_without_warnings() {
        for shape in [
            CutwaterShape::Sharp,
            CutwaterShape::Fillet {
                radius: Param::Manual(0.0015),
            },
            CutwaterShape::Simple {
                cut: Param::Auto,
                handle: Param::Auto,
                measure: tp_design::volute::CutMeasure::Along,
            },
        ] {
            for section in [SectionShape::round(), SectionShape::rectangular()] {
                let mut input = VoluteInput {
                    cross_section: CrossSectionInput::single(section),
                    ..VoluteInput::default()
                };
                input.cutwater.shape = shape;
                input.diffuser.base_form = Param::Manual(0.5);
                let Ok(g) = VoluteGeometry::build(&basis(), &input) else {
                    continue;
                };
                let s = loose_surfaces(&g).unwrap();
                assert!(s.warnings.is_empty(), "{shape:?}: {:?}", s.warnings);
                assert!(s.surfaces.face_count() >= 3, "{shape:?}");
                assert!(
                    s.curves.edge_samples(2).len() > g.sections.len(),
                    "{shape:?}"
                );
                if shape.is_fillet() {
                    assert!(!g.fillet_nose().is_empty(), "{shape:?}");
                    let mut sharp = input.clone();
                    sharp.cutwater.shape = CutwaterShape::Sharp;
                    let sharp = VoluteGeometry::build(&basis(), &sharp).unwrap();
                    let bare = loose_surfaces(&sharp).unwrap();
                    assert!(s.surfaces.face_count() > bare.surfaces.face_count());
                }
            }
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn sharp_and_fillet_cutwaters_give_the_walls_trimmed_and_sewn() {
        let _slot = crate::test_slot::take();
        for (shape, base) in [
            (CutwaterShape::Sharp, 0.0),
            (
                CutwaterShape::Fillet {
                    radius: Param::Auto,
                },
                0.5,
            ),
        ] {
            let mut input = VoluteInput::default();
            input.cutwater.shape = shape;
            input.diffuser.base_form = Param::Manual(base);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let s = build_volute_surfaces(&g).unwrap();
            assert!(s.trimmed, "{shape:?}: {:?}", s.warnings);
            assert!(s.warnings.is_empty(), "{shape:?}: {:?}", s.warnings);
            // One sewn shell, open only where the flow comes in and goes out.
            assert_eq!(s.surfaces.solid_count(), 0);
            assert!(s.surfaces.face_count() > 10);
        }
    }
}
