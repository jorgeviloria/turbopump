//! The flow domain of a volute sewn from its surfaces (prototype).
//!
//! The spiral and the vaneless ring are lofted and revolved as open surfaces and sewn
//! together, without boolean operations, except in a zone round the cutwater: there the
//! start of the spiral runs inside the beginning of the diffuser. That zone is the
//! boolean union of the start of the spiral, the diffuser and that sector of the ring;
//! its faces on the planes where it meets the rest are dropped, and the rest of its
//! faces are sewn with the others.
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
use crate::volute::{MM, VoluteCad, fillet_tip, holds_the_design, section_wire};

/// Gap the sewing closes between the edges of neighbouring faces, mm.
const SEWING_TOLERANCE: f64 = 1e-2;

/// How far from r'_z the points of a face of the zone may lie for it to count as an
/// inlet chord, mm.
const INLET_TOLERANCE: f64 = 1e-2;

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
    let radius = g.design.nose_radius * MM;
    if !g.design.cutwater_shape.is_fillet() || radius <= 0.0 {
        return sewn_with(g, 0.0);
    }
    // A large fillet reaches into the strip of the ring beside the cutwater, and the
    // ring no longer sews to the zone: a smaller one, said so, rather than none.
    let mut failed = None;
    for share in FILLET_SHARES {
        match sewn_with(g, share * radius) {
            Ok(mut cad) => {
                if share < 1.0 && !cad.warnings.iter().any(|w| w.contains("rounded with")) {
                    cad.warnings.push(format!(
                        "the cutwater is rounded with {:.1} mm, the most the CAD kernel takes \
                         there",
                        share * radius
                    ));
                }
                return Ok(cad);
            }
            Err(e) => {
                failed.get_or_insert(e);
            }
        }
    }
    Err(failed.expect("at least one radius"))
}

/// Shares of the fillet radius the sewn flow domain tries, largest first.
const FILLET_SHARES: [f64; 4] = [1.0, 0.75, 0.5, 0.25];

/// [`build_volute_sewn`] with the cutwater rounded with `radius`, mm (0: as designed
/// for a sharp cutwater).
fn sewn_with(g: &VoluteGeometry, radius: f64) -> Result<VoluteCad, CadError> {
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

    // The zone, joined by a boolean union: the start of the spiral and the whole diffuser
    // (with the cutwater rounded, for a fillet), the ring between the throat and the
    // split outline, and between the throat and the start of the spiral a sliver along
    // r'_z, half as thick as the lip by which the ring reaches past r'_z, where the
    // inner wall of the diffuser rises off r'_z. The diffuser is lofted whole, as one surface through all
    // its outlines: lofted in two pieces its walls part by up to 0.2 mm between them.
    let start = occ::loft(&wires(&spiral[..=split])?)?;
    let diffuser = occ::loft(&wires(&nozzle)?)?;
    let gap = past(spiral[0][0]);
    let lip = 0.02 * g.design.inlet_width;
    let sliver = {
        // Half the lip: a sliver as thick as the lip would run face to face with the
        // ring and leave its faces inside the union.
        let (inner, outer) = (r3, r3 + 0.5 * lip);
        let face = occ::planar_face(&occ::wire(&[
            occ::line_edge(at(inner, zm - half), at(outer, zm - half))?,
            occ::line_edge(at(outer, zm - half), at(outer, zm + half))?,
            occ::line_edge(at(outer, zm + half), at(inner, zm + half))?,
            occ::line_edge(at(inner, zm + half), at(inner, zm - half))?,
        ])?)?;
        let from = if sign > 0.0 { throat } else { throat - gap };
        occ::revolve_z_by(&face.rotated_z(from)?, gap)?
    };
    let channel = crate::volute::first_sound(
        |fuzzy| occ::fuse_all_fuzzy(&[start.copied()?, diffuser.copied()?], fuzzy),
        |c| c.is_valid() && c.solid_count() == 1,
        |c| c.fixed().ok(),
    )?;
    let pieces = [start, diffuser];
    let mut channel = channel;
    let mut warnings = Vec::new();
    if let CutwaterShape::Fillet { .. } = g.design.cutwater_shape {
        match fillet_tip(&channel, &pieces, g, radius) {
            Some((rounded, r)) => {
                channel = rounded;
                if r < radius * (1.0 - 1e-9) {
                    warnings.push(format!(
                        "the cutwater is rounded with {r:.1} mm, the most the CAD kernel takes \
                         there"
                    ));
                }
            }
            None => warnings.push(
                "the CAD kernel could not round the cutwater edge: the CAD keeps it sharp"
                    .to_owned(),
            ),
        }
    }
    let channel = crate::volute::first_sound(
        |fuzzy| channel.fuse_fuzzy(&sliver, fuzzy),
        |z| z.is_valid() && z.solid_count() == 1,
        |z| z.fixed().ok(),
    )?;
    // The ring between the throat and the split outline, reaching a little past r'_z so
    // that it overlaps the inlet chords, less what of that lip would stand out of the
    // channel (through the walls of a small round section, beside the inner wall of
    // the diffuser where it runs wider than the inlet).
    let sector = |inner: f64, outer: f64, low: f64, high: f64| -> Result<Shape, CadError> {
        let face = occ::planar_face(&occ::wire(&[
            occ::line_edge(at(inner, low), at(outer, low))?,
            occ::line_edge(at(outer, low), at(outer, high))?,
            occ::line_edge(at(outer, high), at(inner, high))?,
            occ::line_edge(at(inner, high), at(inner, low))?,
        ])?)?;
        let turn = past(spiral[split][0]);
        let from = if sign > 0.0 { throat } else { throat - turn };
        occ::revolve_z_by(&face.rotated_z(from)?, turn)
    };
    let ring_sector = sector(r2, r3 + lip, zm - half, zm + half)?;
    let inside = sector(0.5 * r2, r3, zm - 2.0 * half, zm + 2.0 * half)?;
    let poking = ring_sector.cut_all_fuzzy(&[&channel, &inside], 0.0)?;
    let ring_sector = if poking.volume_with_tolerance(0.0) > 1e-6 {
        ring_sector.cut(&poking)?
    } else {
        ring_sector
    };
    let zone = crate::volute::first_sound(
        |fuzzy| channel.fuse_fuzzy(&ring_sector, fuzzy),
        |z| z.is_valid() && z.solid_count() == 1,
        |z| z.fixed().ok(),
    )?;
    // Its faces on the planes where it meets the rest (the throat and the split outline
    // of the spiral), and any inlet chord the union left on r'_z inside the ring.
    let meridional = |a: f64| {
        let normal = DVec3::new(-a.sin(), a.cos(), 0.0);
        move |(n, p): (DVec3, DVec3)| n.dot(normal).abs() > 1.0 - 1e-6 && n.dot(p).abs() < 1e-3
    };
    let on_throat = meridional(throat);
    let on_split = meridional(split_angle);
    let mut dropped: Vec<u32> = zone
        .face_planes()
        .into_iter()
        .enumerate()
        .filter_map(|(i, plane)| {
            let plane = plane?;
            (on_throat(plane) || on_split(plane)).then_some(i as u32)
        })
        .collect();
    let triangles = zone.triangles(0.2)?;
    let faces = zone.triangle_faces();
    let mut off_inlet = vec![0.0f64; zone.face_count()];
    for (t, &f) in triangles.iter().zip(&faces) {
        for p in t {
            let off = (p.truncate().length() - r3 * MM).abs();
            off_inlet[f as usize] = off_inlet[f as usize].max(off);
        }
    }
    let chords: Vec<u32> = (0..off_inlet.len() as u32)
        .filter(|i| off_inlet[*i as usize] < INLET_TOLERANCE && !dropped.contains(i))
        .collect();
    dropped.extend(chords);
    let zone_faces = zone.faces_except(&dropped)?;

    // The rest, as open surfaces: the spiral from the split outline to the throat without
    // its inlet chords, and the ring from the split outline round to the throat, open on
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
    let zone_turn = past(spiral[split][0]);
    let ring = {
        let edges = occ::wire(&[
            occ::line_edge(at(r3, zm + half), at(r2, zm + half))?,
            occ::line_edge(at(r2, zm + half), at(r2, zm - half))?,
            occ::line_edge(at(r2, zm - half), at(r3, zm - half))?,
        ])?;
        let turn = TAU - zone_turn;
        let from = if sign > 0.0 {
            split_angle
        } else {
            split_angle - turn
        };
        occ::revolve_z_by(&edges.rotated_z(from)?, turn)?
    };

    let sewn = occ::sew(&[&zone_faces, &spiral_rest, &ring], SEWING_TOLERANCE)?;
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

/// How the flow domain of a volute was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowBuild {
    /// Sewn from its surfaces ([`build_volute_sewn`]).
    Sewn,
    /// Joined by boolean operations ([`crate::build_volute`]).
    Boolean,
}

/// The flow domain of a volute sewn from its surfaces where that comes out as one
/// valid solid, else joined by boolean operations, and which of the two it is.
pub fn build_volute_flow(g: &VoluteGeometry) -> Result<(VoluteCad, FlowBuild), CadError> {
    if let Ok(cad) = build_volute_sewn(g)
        && cad.fluid.is_valid()
        && cad.fluid.solid_count() == 1
        && holds_the_design(g, &cad.fluid).is_ok()
    {
        return Ok((cad, FlowBuild::Sewn));
    }
    crate::build_volute(g).map(|cad| (cad, FlowBuild::Boolean))
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
    use tp_design::volute::{
        AreaLaw, CrossSectionInput, DischargeShape, SectionShape, StraightWall, VoluteInput,
    };

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
        let cases: [(&str, Edit); 7] = [
            ("trapezoid", |_| {}),
            ("round", |v| {
                v.cross_section = CrossSectionInput::single(SectionShape::round())
            }),
            ("rectangular", |v| {
                v.cross_section = CrossSectionInput::single(SectionShape::rectangular())
            }),
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
                // Where the kernel cannot round the edge (a rectangular or round section,
                // most discharges at this radius) the boolean build rounds it with the
                // sliver of the round and the sewn one stays sharp: they differ by the
                // tip of the wedge. A rectangular section does not come out as a
                // boolean union either.
                if shape.is_fillet() && name != "trapezoid" {
                    continue;
                }
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
                // which bends its last stretch by a tenth of a millimetre (the flat side
                // walls of a rectangular one by 0.4 mm).
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
                assert!(off < 0.5, "{case}: {off} mm");
            }
        }
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_boolean_that_drops_the_diffuser_is_refused() {
        let _slot = crate::test_slot::take();
        // The flat side walls of a rectangular section lie in the planes of the
        // diffuser's: the boolean union keeps the spiral alone, one valid solid a third
        // short. It must fail, and the flow domain come out sewn and whole.
        let mut input = VoluteInput {
            cross_section: CrossSectionInput::single(SectionShape::rectangular()),
            ..VoluteInput::default()
        };
        input.cutwater.shape = CutwaterShape::Sharp;
        input.diffuser.base_form = Param::Manual(0.5);
        let g = VoluteGeometry::build(&basis(), &input).unwrap();
        assert!(crate::build_volute(&g).is_err());
        let (cad, how) = build_volute_flow(&g).unwrap();
        assert_eq!(how, FlowBuild::Sewn);
        let expected = crate::expected_fluid_volume(&g) * MM.powi(3);
        assert!((cad.fluid.volume() / expected - 1.0).abs() < 0.02);
    }

    #[test]
    #[ignore = "slow (minutes of OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn a_rounded_diffuser_base_takes_the_fillet_at_the_tip() {
        let _slot = crate::test_slot::take();
        // With a base form factor the walls meet along a V across the width: its arms
        // take the fillet, and a radius the ring strip cannot take comes out smaller.
        for (radius, rounded) in [(Param::Auto, None), (Param::Manual(0.003), Some(1.5))] {
            let mut input = VoluteInput::default();
            input.cutwater.shape = CutwaterShape::Fillet { radius };
            input.diffuser.base_form = Param::Manual(0.5);
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let (cad, how) = build_volute_flow(&g).unwrap();
            assert_eq!(how, FlowBuild::Sewn);
            assert!(cad.fluid.is_valid() && cad.fluid.solid_count() == 1);
            match rounded {
                None => assert!(cad.warnings.is_empty(), "{:?}", cad.warnings),
                Some(r) => assert!(
                    cad.warnings
                        .iter()
                        .any(|w| w.contains(&format!("rounded with {r:.1} mm"))),
                    "{:?}",
                    cad.warnings
                ),
            }
            let mut sharp = input.clone();
            sharp.cutwater.shape = CutwaterShape::Sharp;
            let sharp = build_volute_flow(&VoluteGeometry::build(&basis(), &sharp).unwrap())
                .unwrap()
                .0;
            // Rounding the tip of the casing hands its corner to the flow.
            assert!(cad.fluid.volume() > sharp.fluid.volume());
        }
    }
}
