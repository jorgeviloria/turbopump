//! Named boundaries of the flow domains: the faces of a solid grouped by what they are
//! (inflow, outflow, walls, cutwater…), written one file each as the CFD tools take them.
//!
//! The faces of a domain come out of Open CASCADE with no names, and their indices
//! change with the design, so they are named by what they are: where they lie, and how
//! they face, measured on their triangles.

use std::path::{Path, PathBuf};

use tp_design::impeller::ImpellerGeometry;
use tp_design::volute::VoluteGeometry;
use tp_math::DVec3;

use crate::CadError;
use crate::occ::{self, Shape};
use crate::volute::MM;

/// Deflection of the triangles the faces are measured on, mm.
const MEASURE_DEFLECTION: f64 = 0.5;

/// What the triangles say about one face, in mm.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FaceInfo {
    /// In the order of OCCT's indexed face map.
    pub index: u32,
    pub area: f64,
    /// Centre of its area.
    pub centre: DVec3,
    /// Smallest and largest distance of its points from the pump axis (z).
    pub radius: (f64, f64),
    /// Lowest and highest z of its points.
    pub z: (f64, f64),
    /// Mean of |n·z| over its area: 1 for a face square to the axis, 0 for one along it.
    pub axial: f64,
    /// The mean normal, out of the solid.
    pub normal: DVec3,
    pub planar: bool,
    /// Points spread over the face: the centres of some of its triangles.
    pub samples: Vec<DVec3>,
}

/// How many points of a face are kept to measure its distance to something.
const SAMPLES: usize = 24;

/// Every face of `shape`, measured.
pub(crate) fn face_infos(shape: &Shape) -> Result<Vec<FaceInfo>, CadError> {
    let triangles = shape.triangles(MEASURE_DEFLECTION)?;
    let owners = shape.triangle_faces();
    let planes = shape.face_planes();
    let mut infos: Vec<Option<FaceInfo>> = vec![None; planes.len()];
    let mut sums: Vec<(f64, DVec3, f64, DVec3)> =
        vec![(0.0, DVec3::ZERO, 0.0, DVec3::ZERO); planes.len()];
    for (t, &face) in triangles.iter().zip(&owners) {
        let k = face as usize;
        let Some(sum) = sums.get_mut(k) else { continue };
        let cross = (t[1] - t[0]).cross(t[2] - t[0]);
        let area = 0.5 * cross.length();
        if area <= 0.0 {
            continue;
        }
        let normal = cross.normalize();
        let centre = (t[0] + t[1] + t[2]) / 3.0;
        sum.0 += area;
        sum.1 += centre * area;
        sum.2 += normal.z.abs() * area;
        sum.3 += normal * area;
        let info = infos[k].get_or_insert(FaceInfo {
            index: face,
            area: 0.0,
            centre: DVec3::ZERO,
            radius: (f64::INFINITY, f64::NEG_INFINITY),
            z: (f64::INFINITY, f64::NEG_INFINITY),
            axial: 0.0,
            normal: DVec3::ZERO,
            planar: planes[k].is_some(),
            samples: Vec::new(),
        });
        info.samples.push(centre);
        for p in t {
            let r = p.truncate().length();
            info.radius = (info.radius.0.min(r), info.radius.1.max(r));
            info.z = (info.z.0.min(p.z), info.z.1.max(p.z));
        }
    }
    Ok(infos
        .into_iter()
        .zip(sums)
        .filter_map(|(info, (area, centre, axial, normal))| {
            let mut info = info?;
            info.area = area;
            info.centre = centre / area;
            info.axial = axial / area;
            info.normal = normal.normalize_or_zero();
            let stride = (info.samples.len() / SAMPLES).max(1);
            info.samples = info.samples.iter().copied().step_by(stride).collect();
            Some(info)
        })
        .collect())
}

/// A named boundary of a domain: the faces of the solid, by index, that make it.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub name: String,
    /// In the order of OCCT's indexed face map.
    pub faces: Vec<u32>,
}

/// The patches of `names` with the faces assigned to each, empty ones left out.
fn collect(names: &[&str], assigned: Vec<(u32, usize)>) -> Vec<Patch> {
    names
        .iter()
        .enumerate()
        .map(|(k, name)| Patch {
            name: (*name).to_owned(),
            faces: assigned
                .iter()
                .filter(|(_, patch)| *patch == k)
                .map(|(face, _)| *face)
                .collect(),
        })
        .filter(|p| !p.faces.is_empty())
        .collect()
}

/// Distance from `p` to the segment `a`–`b`.
fn distance_to_segment(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-30)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Whether most of the samples of `face` lie within `reach` mm of the polylines `lines`;
/// seen from the pump axis (z left out) when `flat`, for what runs straight along it.
fn lies_on(face: &FaceInfo, lines: &[Vec<DVec3>], reach: f64, flat: bool) -> bool {
    if lines.is_empty() || face.samples.is_empty() {
        return false;
    }
    let seen = |p: DVec3| if flat { DVec3::new(p.x, p.y, 0.0) } else { p };
    let near = face
        .samples
        .iter()
        .filter(|&&p| {
            lines.iter().any(|line| {
                line.windows(2)
                    .any(|w| distance_to_segment(seen(p), seen(w[0]), seen(w[1])) < reach)
            })
        })
        .count();
    near as f64 >= 0.8 * face.samples.len() as f64
}

/// The boundaries of the flow domain `fluid` of the volute `g`: `Inflow` (the cylinder
/// at the impeller outlet), `Outflow` (the end of the diffuser), `Spiral` (its walls and
/// those of the ring before it), `Diffuser`, `CutWater` (the round or curve that closes
/// the tip, when there is one) and `Splitter` (the rib of a double volute).
pub fn volute_patches(g: &VoluteGeometry, fluid: &Shape) -> Result<Vec<Patch>, CadError> {
    const NAMES: [&str; 6] = [
        "Inflow", "Outflow", "Spiral", "Diffuser", "CutWater", "Splitter",
    ];
    const INFLOW: usize = 0;
    const OUTFLOW: usize = 1;
    const SPIRAL: usize = 2;
    const DIFFUSER: usize = 3;
    const CUTWATER: usize = 4;
    const SPLITTER: usize = 5;
    let infos = face_infos(fluid)?;
    let sign = g.basis.rotation_sign;
    // The widest the spiral gets: beyond it only the diffuser reaches.
    let spiral_reach = [
        &g.sections,
        &g.inner_sections,
        &g.outer_channel,
        &g.envelope,
        &g.merged,
    ]
    .into_iter()
    .flatten()
    .flat_map(|s| s.points(sign))
    .map(|p| p.truncate().length() * MM)
    .fold(0.0, f64::max);
    let lowest = infos
        .iter()
        .map(|f| f.radius.0)
        .fold(f64::INFINITY, f64::min);
    let outlet = g.nozzle.last();
    let mm = |line: &[DVec3]| line.iter().map(|p| *p * MM).collect::<Vec<_>>();
    let rib: Vec<Vec<DVec3>> = g
        .rib
        .iter()
        .flat_map(|r| &r.planes)
        .flat_map(|p| [&p.inner_face, &p.outer_face, &p.nose, &p.tail])
        .map(|line| mm(line))
        .collect();
    let nose: Vec<Vec<DVec3>> = g.cutwater_slices.iter().map(|s| mm(s.nose_arc())).collect();
    let assigned = infos
        .iter()
        .map(|f| {
            let patch = if f.radius.1 <= lowest + 0.01 && f.axial < 0.05 {
                INFLOW
            } else if f.planar
                && outlet.is_some_and(|n| {
                    f.normal.dot(n.tangent).abs() > 0.99 && f.centre.distance(n.center * MM) < 2.0
                })
            {
                OUTFLOW
            } else if lies_on(f, &rib, 2.0, true) {
                SPLITTER
            } else if lies_on(f, &nose, 2.0, false) {
                CUTWATER
            } else if f.radius.1 > spiral_reach + 1.0 {
                DIFFUSER
            } else {
                SPIRAL
            };
            (f.index, patch)
        })
        .collect();
    Ok(collect(&NAMES, assigned))
}

/// The boundaries of the flow domain `fluid` of an impeller: `Inflow` (the annulus at
/// the eye), `Outflow` (the cylinder at the outlet), `Hub`, `Shroud` and `Blade` (every
/// face of the blades).
pub fn impeller_patches(g: &ImpellerGeometry, fluid: &Shape) -> Result<Vec<Patch>, CadError> {
    const NAMES: [&str; 5] = ["Inflow", "Outflow", "Hub", "Shroud", "Blade"];
    const INFLOW: usize = 0;
    const OUTFLOW: usize = 1;
    const HUB: usize = 2;
    const SHROUD: usize = 3;
    const BLADE: usize = 4;
    let infos = face_infos(fluid)?;
    let meridian = |curve: &tp_design::impeller::wall::Wall| -> Vec<DVec3> {
        curve
            .sample(400)
            .iter()
            .map(|p| DVec3::new(p.x * MM, p.y * MM, 0.0))
            .collect()
    };
    let (hub, shroud) = (meridian(&g.contour.hub), meridian(&g.contour.shroud));
    let outlet = infos.iter().map(|f| f.radius.1).fold(0.0, f64::max);
    let lowest = infos.iter().map(|f| f.z.0).fold(f64::INFINITY, f64::min);
    // How far, on average, the points of a face are from a curve of the meridian.
    let apart = |f: &FaceInfo, curve: &[DVec3]| -> f64 {
        let total: f64 = f
            .samples
            .iter()
            .map(|p| {
                let p = DVec3::new(p.z, p.truncate().length(), 0.0);
                curve
                    .windows(2)
                    .map(|w| distance_to_segment(p, w[0], w[1]))
                    .fold(f64::INFINITY, f64::min)
            })
            .sum();
        total / f.samples.len().max(1) as f64
    };
    let assigned = infos
        .iter()
        .map(|f| {
            let patch = if f.planar && f.axial > 0.99 && f.z.1 <= lowest + 0.01 {
                INFLOW
            } else if f.radius.0 >= outlet - 0.1 && f.axial < 0.05 {
                OUTFLOW
            } else {
                let (to_hub, to_shroud) = (apart(f, &hub), apart(f, &shroud));
                if to_hub.min(to_shroud) > 0.5 {
                    BLADE
                } else if to_hub < to_shroud {
                    HUB
                } else {
                    SHROUD
                }
            };
            (f.index, patch)
        })
        .collect();
    Ok(collect(&NAMES, assigned))
}

/// Writes every patch of `shape` to its own file `dir/{stem}-{name}.{format}` (`step`,
/// `igs` or `stl`; STL with triangles `deflection` mm from the faces), and gives the
/// files.
pub fn write_patches(
    shape: &Shape,
    patches: &[Patch],
    dir: &Path,
    stem: &str,
    format: &str,
    deflection: f64,
) -> Result<Vec<PathBuf>, CadError> {
    std::fs::create_dir_all(dir).map_err(|e| CadError::Io(format!("{}: {e}", dir.display())))?;
    patches
        .iter()
        .map(|patch| {
            let faces = patch
                .faces
                .iter()
                .map(|&k| shape.face(k))
                .collect::<Result<Vec<_>, _>>()?;
            let pieces = occ::compound(&faces)?;
            let path = dir.join(format!("{stem}-{}.{format}", patch.name));
            match format {
                "step" => occ::write_step(&pieces, &path)?,
                "igs" => occ::write_iges(&pieces, &path)?,
                _ => occ::write_stl(&pieces, &path, deflection)?,
            }
            Ok(path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_design::outlet::ImpellerOutlet;

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

    /// Every face of `shape` in exactly one patch, and the areas adding up.
    fn assert_covers(shape: &Shape, patches: &[Patch], case: &str) -> Vec<FaceInfo> {
        let infos = face_infos(shape).unwrap();
        let mut seen: Vec<u32> = patches.iter().flat_map(|p| p.faces.clone()).collect();
        seen.sort_unstable();
        let mut all: Vec<u32> = infos.iter().map(|f| f.index).collect();
        all.sort_unstable();
        assert_eq!(seen, all, "{case}: every face in one patch");
        infos
    }

    fn area_of(infos: &[FaceInfo], patch: &Patch) -> f64 {
        patch
            .faces
            .iter()
            .filter_map(|k| infos.iter().find(|f| f.index == *k))
            .map(|f| f.area)
            .sum()
    }

    #[test]
    #[ignore = "slow (OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_faces_of_a_volute_are_named_by_what_they_are() {
        use tp_design::volute::{CutwaterShape, VoluteGeometry, VoluteInput, VoluteKind};
        let _slot = crate::test_slot::take();
        let rounded = CutwaterShape::Fillet {
            radius: tp_design::Param::Auto,
        };
        for (case, kind, shape, extra) in [
            (
                "single sharp",
                VoluteKind::Single,
                CutwaterShape::Sharp,
                None,
            ),
            (
                "single rounded",
                VoluteKind::Single,
                rounded,
                Some("CutWater"),
            ),
            (
                "double sharp",
                VoluteKind::Double,
                CutwaterShape::Sharp,
                Some("Splitter"),
            ),
        ] {
            let mut input = VoluteInput::default();
            input.setup.kind = kind;
            input.cutwater.shape = shape;
            let g = VoluteGeometry::build(&basis(), &input).unwrap();
            let cad = crate::build_volute(&g).unwrap();
            let patches = volute_patches(&g, &cad.fluid).unwrap();
            let infos = assert_covers(&cad.fluid, &patches, case);
            let names: Vec<&str> = patches.iter().map(|p| p.name.as_str()).collect();
            let mut expected = vec!["Inflow", "Outflow", "Spiral", "Diffuser"];
            expected.extend(extra);
            let mut sorted = names.clone();
            sorted.sort_unstable();
            expected.sort_unstable();
            assert_eq!(sorted, expected, "{case}: {names:?}");
            let patch = |name: &str| patches.iter().find(|p| p.name == name).unwrap();
            // The inflow is the cylinder at the impeller outlet, one face all round; the
            // outflow the end of the diffuser, as large as its last section.
            let inflow = patch("Inflow");
            assert_eq!(inflow.faces.len(), 1, "{case}");
            let face = infos.iter().find(|f| f.index == inflow.faces[0]).unwrap();
            let (radius, height) = (face.radius.1, face.z.1 - face.z.0);
            assert!((face.radius.1 - face.radius.0).abs() < 0.01, "{case}");
            let lateral = std::f64::consts::TAU * radius * height;
            assert!(
                (face.area / lateral - 1.0).abs() < 0.01,
                "{case}: {} vs {lateral}",
                face.area
            );
            let last = g.nozzle.last().unwrap();
            let outflow = area_of(&infos, patch("Outflow"));
            assert!(
                (outflow / (last.area * 1e6) - 1.0).abs() < 0.05,
                "{case}: {outflow} vs {}",
                last.area * 1e6
            );
            // The walls are most of it.
            let walls = area_of(&infos, patch("Spiral")) + area_of(&infos, patch("Diffuser"));
            let total: f64 = infos.iter().map(|f| f.area).sum();
            assert!(walls > 0.7 * total, "{case}");
            if let Some(name) = extra {
                assert!(area_of(&infos, patch(name)) > 0.0, "{case}");
            }
        }
    }

    #[test]
    #[ignore = "slow (OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn the_faces_of_an_impeller_are_named_by_what_they_are() {
        use crate::impeller::tests_support::reference;
        use crate::{CadOptions, build_impeller};
        let _slot = crate::test_slot::take();
        let g = reference();
        let cad = build_impeller(
            &g,
            &CadOptions {
                fillet_radius: Some(0.0),
                ..CadOptions::default()
            },
        )
        .unwrap();
        let patches = impeller_patches(&g, &cad.fluid).unwrap();
        let infos = assert_covers(&cad.fluid, &patches, "impeller");
        let patch = |name: &str| patches.iter().find(|p| p.name == name).unwrap();
        let blades = g.blade.count as usize;
        // One piece of the outlet cylinder between each two blades, one face a blade.
        assert_eq!(patch("Outflow").faces.len(), blades);
        assert_eq!(patch("Blade").faces.len(), blades);
        assert_eq!(patch("Inflow").faces.len(), 1);
        // The hub and the shroud are the two big walls; the outlet cylinder at the
        // outlet radius.
        assert!(area_of(&infos, patch("Hub")) > area_of(&infos, patch("Blade")) * 0.4);
        assert!(area_of(&infos, patch("Shroud")) > area_of(&infos, patch("Outflow")));
        let outlet = g.contour.hub.sample(2).last().unwrap().y * 1e3;
        for k in &patch("Outflow").faces {
            let face = infos.iter().find(|f| f.index == *k).unwrap();
            assert!(
                (face.radius.0 - outlet).abs() < 0.5,
                "{} vs {outlet}",
                face.radius.0
            );
        }
    }

    #[test]
    #[ignore = "slow (OCCT booleans); CI runs it with `cargo test -p tp-cad --lib -- --ignored`"]
    fn every_patch_is_written_to_a_file_of_its_own() {
        use tp_design::volute::{VoluteGeometry, VoluteInput};
        let _slot = crate::test_slot::take();
        let g = VoluteGeometry::build(&basis(), &VoluteInput::default()).unwrap();
        let cad = crate::build_volute(&g).unwrap();
        let patches = volute_patches(&g, &cad.fluid).unwrap();
        let dir = std::env::temp_dir().join(format!("tp-patches-{}", std::process::id()));
        for format in ["stl", "step"] {
            let files = write_patches(&cad.fluid, &patches, &dir, "volute", format, 0.5).unwrap();
            assert_eq!(files.len(), patches.len());
            for (file, patch) in files.iter().zip(&patches) {
                let name = file.file_name().unwrap().to_string_lossy().into_owned();
                assert_eq!(name, format!("volute-{}.{format}", patch.name));
                assert!(std::fs::metadata(file).unwrap().len() > 200, "{name}");
            }
        }
        // A STEP file reads back as the faces of its patch.
        let back = crate::read_step(&dir.join("volute-Outflow.step")).unwrap();
        assert_eq!(
            back.face_count(),
            patches
                .iter()
                .find(|p| p.name == "Outflow")
                .unwrap()
                .faces
                .len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
