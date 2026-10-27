//! The flow domain of a volute sewn from its surfaces (prototype).
//!
//! The spiral, the discharge diffuser and the vaneless ring are lofted and revolved as
//! open surfaces and sewn together, without boolean operations, except in a small zone
//! round the cutwater: there the start of the spiral runs inside the beginning of the
//! diffuser, and the three overlap. That zone is joined by a boolean union of three
//! small solids, its faces on the planes where it meets the rest are dropped, and the
//! rest of its faces are sewn with the others.
//!
//! The spiral ends on the very outline the diffuser starts with (the throat), so the
//! two meet edge to edge. The ring meets the spiral along the inlet points, at r'_z and
//! half the inlet width off the midplane, which every outline of the spiral starts and
//! ends its inlet chord on.

use std::f64::consts::TAU;

use tp_design::volute::{CutwaterShape, VoluteGeometry};
use tp_math::DVec3;

use crate::CadError;
use crate::occ::{self, Shape};
use crate::volute::{MM, VoluteCad, fillet_tip, section_wire};

/// Gap the sewing closes between the edges of neighbouring faces, mm.
const SEWING_TOLERANCE: f64 = 1e-2;

/// Turn past the beginning of the diffuser kept in the zone beyond the diffuser's
/// pieces, rad.
const ZONE_MARGIN: f64 = 5.0 * std::f64::consts::PI / 180.0;

/// Builds the flow domain of a single volute with a sharp or fillet cutwater by sewing
/// its surfaces.
pub fn build_volute_sewn(g: &VoluteGeometry) -> Result<VoluteCad, CadError> {
    if g.design.rib.is_some()
        || g.bend.is_some()
        || g.design.diffuser.z_bend.is_some()
        || !matches!(
            g.design.cutwater_shape,
            CutwaterShape::Sharp | CutwaterShape::Fillet { .. }
        )
    {
        return Err(CadError::Input(
            "the sewn flow domain takes single volutes with a sharp or fillet cutwater".into(),
        ));
    }
    let sign = g.basis.rotation_sign;
    let breaks = g.ring_breaks();
    let spiral: Vec<Vec<DVec3>> = g.sections.iter().map(|s| s.points(sign)).collect();
    let nozzle: Vec<Vec<DVec3>> = g.nozzle.iter().map(|n| n.points.clone()).collect();
    let chord = *breaks
        .last()
        .ok_or_else(|| CadError::Input("the outlines have no inlet chord".into()))?;
    let n = spiral[0].len();
    let throat = angle(g.nozzle[0].center);
    // Turn from the throat, in the sense of rotation, in [0, 2π) for the spiral and in
    // [-π, π) for the diffuser, which starts on it.
    let past = |p: DVec3| (sign * (angle(p) - throat)).rem_euclid(TAU);
    let beyond = |p: DVec3| (past(p) + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI;

    // The zone: the diffuser up to its first outline clear of the spiral, the spiral up
    // to its first outline past that piece of diffuser.
    // The spiral before its throat, which is the diffuser's first outline.
    let reach = |upto: f64| {
        spiral[..spiral.len() - 1]
            .iter()
            .flatten()
            .filter(|p| past(**p) <= upto)
            .map(|p| p.truncate().length())
            .fold(0.0, f64::max)
    };
    let clear = (1..nozzle.len())
        .find(|&j| {
            let ring = &nozzle[j];
            let far = ring.iter().map(|p| beyond(*p)).fold(0.0, f64::max);
            let near = ring
                .iter()
                .map(|p| p.truncate().length())
                .fold(f64::INFINITY, f64::min);
            near > reach(far + ZONE_MARGIN) + 1e-3
        })
        .ok_or_else(|| CadError::Input("the diffuser never clears the spiral".into()))?;
    let zone_turn = nozzle[..=clear]
        .iter()
        .flatten()
        .map(|p| beyond(*p))
        .fold(0.0, f64::max)
        + ZONE_MARGIN;
    let split = (1..spiral.len() - 1)
        .find(|&k| past(spiral[k][0]) > zone_turn)
        .ok_or_else(|| CadError::Input("the spiral is too short for the zone".into()))?;
    let split_angle = angle(spiral[split][0]);
    crate::profile::note(|| {
        format!("sewn zone: spiral 0..={split}, diffuser 0..={clear}, {zone_turn:.3} rad")
    });

    let wires = |rings: &[Vec<DVec3>]| {
        rings
            .iter()
            .map(|r| section_wire(r, &breaks))
            .collect::<Result<Vec<_>, _>>()
    };
    let (r2, r3) = g.inlet_ring();
    let half = g.design.inlet_width / 2.0;
    let zm = g.basis.midplane;
    let at = |r: f64, z: f64| DVec3::new(r * MM, 0.0, z * MM);
    // The ring swept from `from` through `turn` (rad) in the sense of rotation.
    let sweep = |shape: Shape, from: f64, turn: f64| -> Result<Shape, CadError> {
        let start = if sign > 0.0 { from } else { from - turn };
        occ::revolve_z_by(&shape.rotated_z(start)?, turn)
    };

    // The zone, joined by booleans: the start of the spiral and the whole diffuser (with
    // the cutwater rounded, for a fillet), then the ring, a little past r'_z so that it
    // overlaps the inlet chords. The diffuser is lofted whole, as one surface through
    // all its outlines: lofted in two pieces its walls part by up to 0.2 mm between them.
    let start = occ::loft(&wires(&spiral[..=split])?)?;
    let diffuser = occ::loft(&wires(&nozzle)?)?;
    let pieces = [start, diffuser];
    let mut channel = crate::volute::first_sound(
        |fuzzy| occ::fuse_all_fuzzy(&pieces, fuzzy),
        |c| c.is_valid() && c.solid_count() == 1,
        |c| c.fixed().ok(),
    )?;
    let mut warnings = Vec::new();
    if let CutwaterShape::Fillet { .. } = g.design.cutwater_shape {
        match fillet_tip(&channel, &pieces, g, g.design.nose_radius * MM) {
            Some((rounded, _)) => channel = rounded,
            None => warnings.push(
                "the CAD kernel could not round the cutwater edge: the CAD keeps it sharp"
                    .to_owned(),
            ),
        }
    }
    let lip = 0.02 * g.design.inlet_width;
    let rectangle = occ::planar_face(&occ::wire(&[
        occ::line_edge(at(r2, zm - half), at(r3 + lip, zm - half))?,
        occ::line_edge(at(r3 + lip, zm - half), at(r3 + lip, zm + half))?,
        occ::line_edge(at(r3 + lip, zm + half), at(r2, zm + half))?,
        occ::line_edge(at(r2, zm + half), at(r2, zm - half))?,
    ])?)?;
    let zone_sweep = past(spiral[split][0]);
    let ring_sector = sweep(rectangle, throat, zone_sweep)?;
    let zone = crate::volute::first_sound(
        |fuzzy| channel.fuse_fuzzy(&ring_sector, fuzzy),
        |z| z.is_valid() && z.solid_count() == 1,
        |z| z.fixed().ok(),
    )?;
    // Its faces on the planes where it meets the rest: the throat and the split outline
    // of the spiral.
    let meridional = |a: f64| {
        let normal = DVec3::new(-a.sin(), a.cos(), 0.0);
        move |(n, p): (DVec3, DVec3)| n.dot(normal).abs() > 1.0 - 1e-6 && n.dot(p).abs() < 1e-3
    };
    let on_throat = meridional(throat);
    let on_split = meridional(split_angle);
    let dropped: Vec<u32> = zone
        .face_planes()
        .into_iter()
        .enumerate()
        .filter_map(|(i, plane)| {
            let plane = plane?;
            (on_throat(plane) || on_split(plane)).then_some(i as u32)
        })
        .collect();
    let zone_faces = zone.faces_except(&dropped)?;

    // The rest, as open surfaces: the spiral from the split outline to the throat without
    // its inlet chords, the ring from the split outline round to the throat, open on
    // r'_z.
    let open_wire = |ring: &[DVec3]| -> Result<Shape, CadError> {
        let points: Vec<DVec3> = ring.iter().map(|p| *p * MM).collect();
        let edges = breaks
            .windows(2)
            .map(|w| {
                let piece = &points[w[0]..=w[1]];
                if piece.len() == 2 {
                    occ::line_edge(piece[0], piece[1])
                } else {
                    occ::spline_edge(piece, false)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        occ::wire(&edges)
    };
    debug_assert!(chord < n);
    let spiral_rest = occ::loft_shell(
        &spiral[split..]
            .iter()
            .map(|r| open_wire(r))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let ring_edges = occ::wire(&[
        occ::line_edge(at(r3, zm + half), at(r2, zm + half))?,
        occ::line_edge(at(r2, zm + half), at(r2, zm - half))?,
        occ::line_edge(at(r2, zm - half), at(r3, zm - half))?,
    ])?;
    let ring_rest = sweep(ring_edges, split_angle, TAU - zone_sweep)?;

    let sewn = occ::sew(&[&zone_faces, &spiral_rest, &ring_rest], SEWING_TOLERANCE)?;
    let open = sewn.free_edges(9)?;
    if !open.is_empty() {
        let at: Vec<String> = open
            .iter()
            .map(|e| {
                let (a, b, m) = (e[0], e[e.len() - 1], e[e.len() / 2]);
                format!(
                    "r {:.1} turn {:.3} z {:.1} (from turn {:.3} to {:.3})",
                    m.truncate().length(),
                    past(m),
                    m.z,
                    past(a),
                    past(b)
                )
            })
            .collect();
        return Err(CadError::Operation(format!(
            "the sewn flow domain stays open along {} edges: {}",
            open.len(),
            at.join("; ")
        )));
    }
    let fluid = sewn.solid()?;
    Ok(VoluteCad {
        channel: fluid.copied()?,
        fluid,
        warnings,
    })
}

/// Angle of `p` about the z axis, rad.
fn angle(p: DVec3) -> f64 {
    p.y.atan2(p.x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_design::Param;
    use tp_design::outlet::ImpellerOutlet;
    use tp_design::volute::{AreaLaw, DischargeShape, StraightWall, VoluteInput};

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
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_sewn_flow_domain_is_the_boolean_one() {
        let _slot = crate::test_slot::take();
        type Edit = fn(&mut VoluteInput);
        let cases: [(&str, Edit); 5] = [
            ("trapezoid", |_| {}),
            ("tangential", |v| {
                v.diffuser.shape = DischargeShape::Tangential {
                    straight_wall: StraightWall::Inner,
                }
            }),
            ("eccentric", |v| {
                v.diffuser.shape = DischargeShape::Tangential {
                    straight_wall: StraightWall::Centered,
                };
                v.diffuser.eccentricity = Param::Manual(-0.5);
            }),
            ("330° wrap", |v| {
                v.spiral.wrap = Param::Manual(330f64.to_radians())
            }),
            ("Stepanoff with arcs", |v| {
                v.spiral.law = AreaLaw::Stepanoff;
                v.spiral.arcs.enabled = true;
            }),
        ];
        for (name, edit) in cases {
            for shape in [
                CutwaterShape::Sharp,
                CutwaterShape::Fillet {
                    radius: Param::Manual(0.0015),
                },
            ] {
                let mut input = VoluteInput::default();
                edit(&mut input);
                input.cutwater.shape = shape;
                let g = VoluteGeometry::build(&basis(), &input).unwrap();
                let sewn = build_volute_sewn(&g).unwrap();
                let case = format!("{name} {shape:?}");
                assert!(
                    sewn.fluid.is_valid() && sewn.fluid.solid_count() == 1,
                    "{case}"
                );
                let boolean = crate::build_volute(&g).unwrap();
                // The boolean ring is 0.5 % narrower, so that its sides keep off flat
                // side walls: some 1 cm³ less.
                let (a, b) = (sewn.fluid.volume(), boolean.fluid.volume());
                assert!((a / b - 1.0).abs() < 1e-3, "{case}: {a} vs {b}");
                // The same walls: the boolean spiral is lofted 1.5° into the diffuser,
                // which bends its last stretch by a tenth of a millimetre.
                let points: Vec<DVec3> = sewn
                    .fluid
                    .triangles(0.5)
                    .unwrap()
                    .iter()
                    .step_by(40)
                    .map(|t| t[0])
                    .collect();
                let off = boolean
                    .fluid
                    .boundary_distances(&points)
                    .unwrap()
                    .into_iter()
                    .fold(0.0, f64::max);
                assert!(off < 0.2, "{case}: {off} mm");
            }
        }
    }
}
