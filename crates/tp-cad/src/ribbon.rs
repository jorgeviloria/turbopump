//! The zone round the cutwater of a simple cutwater, trimmed instead of joined: each
//! wall cut along the rail where the other lies the gap away, with no boolean between
//! the walls of the spiral and of the diffuser, which meet almost tangentially near the
//! inlet corners. The idea of the volute tutorial of CAESES (Friendship Systems): trim
//! each surface with a ribbon, leave a gap, and close it with a surface of its own.
//!
//! Built step by step: nothing outside the tests calls it until the flow domain is sewn
//! from it.
#![cfg_attr(not(test), allow(dead_code))]

use tp_math::DVec3;

use crate::CadError;
use crate::occ::{self, Shape};
use crate::volute::MM;

/// How far the ribbon reaches to each side of the wall, mm.
const RIBBON_REACH: f64 = 1.0;

/// How close to the ribbon every point of an edge lies for the edge to be the cut, mm.
const ON_RIBBON: f64 = 1e-3;

/// Samples taken along each edge of the cut wall.
const EDGE_SAMPLES: usize = 16;

/// A wall marked by its ribbon, mm.
pub(crate) struct WallCut {
    /// The faces of the wall with the cut in them.
    pub faces: Shape,
    /// The edge of the cut, point by point from −z to +z.
    pub edge: Vec<DVec3>,
    /// At each point of the edge, the unit direction along the wall toward the tip: the
    /// tangent edge.
    pub to_tip: Vec<DVec3>,
}

/// `wall` (mm) cut along `rail` (m), whose points `to_tip` gives the direction of the
/// wall toward the tip at: a ribbon through the rail, square to the wall in each plane,
/// imprints the edge the wall will end at.
pub(crate) fn cut_wall(
    wall: &Shape,
    rail: &[DVec3],
    to_tip: &[DVec3],
) -> Result<WallCut, CadError> {
    if rail.len() < 2 || rail.len() != to_tip.len() {
        return Err(CadError::Input(
            "a rail needs two points and a direction at each".into(),
        ));
    }
    let side = |sign: f64| -> Result<Shape, CadError> {
        let points: Vec<DVec3> = rail
            .iter()
            .zip(to_tip)
            .map(|(p, t)| *p * MM + DVec3::Z.cross(*t).normalize() * (sign * RIBBON_REACH))
            .collect();
        occ::wire(&[occ::spline_edge(&points, false)?])
    };
    let ribbon = occ::loft_shell(&[side(1.0)?, side(-1.0)?])?;
    // Its faces, not the solid: a solid split by a ribbon inside it keeps that piece of
    // ribbon as an inner face.
    let faces = occ::split(&[&wall.faces_except(&[])?], &[&ribbon])?;
    // The edges of the cut lie on the ribbon all along; joined, they run with the rail.
    let samples = faces.edge_samples(EDGE_SAMPLES);
    let mut edge: Vec<DVec3> = Vec::new();
    for points in samples.iter().filter(|e| e.len() > 1) {
        let off = ribbon.boundary_distances(points)?;
        if off.iter().all(|d| *d < ON_RIBBON) && points[0].distance(points[points.len() - 1]) > 1e-3
        {
            edge.extend(points);
        }
    }
    if edge.is_empty() {
        return Err(CadError::Operation(
            "the ribbon did not cut the wall".into(),
        ));
    }
    edge.sort_by(|a, b| a.z.total_cmp(&b.z));
    edge.dedup_by(|a, b| a.distance(*b) < 1e-6);
    // The tangent edge, between the planes of the rail as its directions turn.
    let to_tip = edge
        .iter()
        .map(|p| {
            let z = p.z / MM;
            let k = rail.partition_point(|q| q.z < z).clamp(1, rail.len() - 1);
            let (a, b) = (rail[k - 1].z, rail[k].z);
            let s = ((z - a) / (b - a)).clamp(0.0, 1.0);
            to_tip[k - 1].lerp(to_tip[k], s).normalize()
        })
        .collect();
    Ok(WallCut {
        faces,
        edge,
        to_tip,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volute::section_wire;
    use tp_design::Param;
    use tp_design::outlet::ImpellerOutlet;
    use tp_design::volute::{
        CutMeasure, CutwaterShape, DischargeShape, StraightWall, VoluteGeometry, VoluteInput,
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

    #[test]
    fn a_ribbon_marks_each_wall_along_its_rail() {
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
            for (name, wall, rail, to_tip) in [
                ("spiral", &spiral, &rails.spiral, &rails.spiral_to_tip),
                (
                    "diffuser",
                    &diffuser,
                    &rails.diffuser,
                    &rails.diffuser_to_tip,
                ),
            ] {
                let cut = super::cut_wall(wall, rail, to_tip).unwrap();
                let rail_mm: Vec<DVec3> = rail.iter().map(|p| *p * MM).collect();
                let curve = occ::spline_edge(&rail_mm, false).unwrap();
                // The edge the ribbon leaves follows the rail from end to end, upward.
                let off = curve
                    .boundary_distances(&cut.edge)
                    .unwrap()
                    .into_iter()
                    .fold(0.0, f64::max);
                assert!(
                    off < 1e-2,
                    "{shape:?} {name}: the edge {off} mm off the rail"
                );
                assert!(cut.edge.windows(2).all(|w| w[1].z > w[0].z), "{name}");
                let (first, last) = (cut.edge[0], cut.edge[cut.edge.len() - 1]);
                assert!(
                    (first.z - rail_mm[0].z).abs() < 0.5,
                    "{name} starts at {}",
                    first.z
                );
                assert!(
                    (last.z - rail_mm[20].z).abs() < 0.5,
                    "{name} ends at {}",
                    last.z
                );
                // Its tangent edge runs along the wall, toward the tip.
                assert_eq!(cut.to_tip.len(), cut.edge.len());
                let ahead: Vec<DVec3> = cut
                    .edge
                    .iter()
                    .zip(&cut.to_tip)
                    .map(|(p, t)| *p + *t * 0.5)
                    .collect();
                let along = wall
                    .boundary_distances(&ahead)
                    .unwrap()
                    .into_iter()
                    .fold(0.0, f64::max);
                assert!(
                    along < 1e-2,
                    "{shape:?} {name}: the tangent leaves the wall by {along} mm"
                );
                // The wall itself is still there, marked with the new edge: within the inlet
                // width alone the ribbon does not run from border to border of a face, so
                // it does not part it yet.
                assert!(
                    cut.faces.edge_samples(2).len() > wall.edge_samples(2).len(),
                    "{name}"
                );
            }
        }
    }

    /// How the walls of the spiral and of the diffuser meet round the cutwater, in and
    /// out of the inlet width, over the designs of the trimmed-zone matrix: where they
    /// cross and at what angle. Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "probe"]
    fn how_the_walls_meet_round_the_cutwater() {
        use tp_design::volute::{AreaLaw, CrossSectionInput, SectionShape};
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
            ("330 wrap", |v| {
                v.spiral.wrap = Param::Manual(330f64.to_radians())
            }),
            ("stepanoff arcs", |v| {
                v.spiral.law = AreaLaw::Stepanoff;
                v.spiral.arcs.enabled = true;
            }),
        ];
        let normal_at = |triangles: &[[DVec3; 3]], p: DVec3| -> DVec3 {
            let t = triangles
                .iter()
                .min_by(|a, b| {
                    let c = |t: &[DVec3; 3]| ((t[0] + t[1] + t[2]) / 3.0).distance(p);
                    c(a).total_cmp(&c(b))
                })
                .unwrap();
            (t[1] - t[0]).cross(t[2] - t[0]).normalize()
        };
        let stats = |v: &mut Vec<f64>| -> String {
            if v.is_empty() {
                return "-".into();
            }
            v.sort_by(f64::total_cmp);
            format!("{:.1}/{:.1}/{:.1}", v[0], v[v.len() / 2], v[v.len() - 1])
        };
        for (name, edit) in cases {
            for base in [0.0, 0.5, 1.0] {
                let mut input = VoluteInput::default();
                edit(&mut input);
                input.diffuser.base_form = Param::Manual(base);
                input.cutwater.shape = CutwaterShape::Simple {
                    cut: Param::Manual(0.003),
                    handle: Param::Auto,
                    measure: CutMeasure::Across,
                };
                let g = match VoluteGeometry::build(&basis(), &input) {
                    Ok(g) => g,
                    Err(e) => {
                        println!("{name} base {base}: design {e}");
                        continue;
                    }
                };
                let (half, zm) = (g.design.inlet_width / 2.0 * MM, g.basis.midplane * MM);
                let rz = g.inlet_ring().1 * MM;
                let Some(tip) = g.tongue_cut(g.basis.midplane).and_then(|c| c.tip) else {
                    println!("{name} base {base}: no tip");
                    continue;
                };
                let tip_angle = tip.y.atan2(tip.x);
                let (spiral, diffuser) = walls(&g).unwrap();
                let crossing = spiral.section(&diffuser).unwrap();
                let turn = |p: DVec3| {
                    (p.y.atan2(p.x) - tip_angle + std::f64::consts::PI)
                        .rem_euclid(std::f64::consts::TAU)
                        - std::f64::consts::PI
                };
                let points: Vec<DVec3> = crossing
                    .edge_samples(40)
                    .into_iter()
                    .flatten()
                    .filter(|p| turn(*p).abs() < 0.4)
                    .collect();
                let (ts, td) = (
                    spiral.triangles(0.05).unwrap(),
                    diffuser.triangles(0.05).unwrap(),
                );
                let near = |tris: &[[DVec3; 3]]| -> Vec<[DVec3; 3]> {
                    tris.iter()
                        .filter(|t| turn(t[0]).abs() < 0.5)
                        .copied()
                        .collect()
                };
                let (ts, td) = (near(&ts), near(&td));
                let (mut band, mut side) = (Vec::new(), Vec::new());
                let (mut side_r, mut side_turn, mut side_z) = (Vec::new(), Vec::new(), Vec::new());
                for p in &points {
                    let a = normal_at(&ts, *p)
                        .dot(normal_at(&td, *p))
                        .abs()
                        .min(1.0)
                        .acos()
                        .to_degrees();
                    if (p.z - zm).abs() <= half {
                        band.push(a);
                    } else {
                        side.push(a);
                        side_r.push(p.truncate().length() - rz);
                        side_turn.push(turn(*p).to_degrees());
                        side_z.push((p.z - zm).abs() - half);
                    }
                }
                println!(
                    "{name} base {base}: band {} pts angle {} deg | side {} pts angle {} deg, r-r'z {} mm, turn {} deg, beyond band {} mm",
                    band.len(),
                    stats(&mut band),
                    side.len(),
                    stats(&mut side),
                    stats(&mut side_r),
                    stats(&mut side_turn),
                    stats(&mut side_z)
                );
            }
        }
    }
}
