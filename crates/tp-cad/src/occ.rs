//! Owned OCCT shapes and checked operations, on top of the C++ bridge.
//!
//! The bridge turns every OCCT exception into an error; this layer adds input checks
//! and converts errors into [`CadError`].

use std::path::Path;

use cxx::UniquePtr;
use tp_math::DVec3;

use crate::CadError;
use crate::bridge::ffi;

/// Smallest distance between consecutive interpolation points, in mm.
const MIN_POINT_SPACING: f64 = 1e-5;

/// An owned OCCT shape, in millimetres.
pub struct Shape(UniquePtr<ffi::Shape>);

impl std::fmt::Debug for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shape")
            .field("solids", &self.solid_count())
            .field("faces", &self.face_count())
            .finish()
    }
}

fn op(
    what: &'static str,
    result: Result<UniquePtr<ffi::Shape>, cxx::Exception>,
) -> Result<Shape, CadError> {
    match result {
        Ok(shape) if !shape.is_null() => Ok(Shape(shape)),
        Ok(_) => Err(CadError::Operation(format!("{what}: empty result"))),
        Err(e) => Err(CadError::Operation(e.what().to_owned())),
    }
}

fn p3(p: DVec3) -> ffi::P3 {
    ffi::P3 {
        x: p.x,
        y: p.y,
        z: p.z,
    }
}

fn list(shapes: &[&Shape]) -> UniquePtr<ffi::ShapeList> {
    let mut list = ffi::new_shape_list();
    for s in shapes {
        ffi::push_shape(list.pin_mut(), &s.0);
    }
    list
}

impl Shape {
    /// Volume of the solids of the shape, mm³, by adaptive integration with relative
    /// tolerance 1e-3 (the achieved error is typically near 1e-5). On solids bounded by
    /// many B-spline faces this takes seconds; the plain Gauss rule is fast but can be
    /// off by percent.
    pub fn volume(&self) -> f64 {
        crate::profile::op("volume 1e-3", || ffi::volume(&self.0, 1e-3))
    }

    /// Volume with a chosen relative error (smaller is slower; 0 = plain Gauss rule).
    pub fn volume_with_tolerance(&self, eps: f64) -> f64 {
        crate::profile::op(
            if eps > 0.0 {
                "volume eps"
            } else {
                "volume gauss"
            },
            || ffi::volume(&self.0, eps),
        )
    }

    /// Volume of each solid of the shape, mm³.
    pub fn solid_volumes(&self) -> Vec<f64> {
        ffi::solid_volumes(&self.0)
    }

    pub fn solid_count(&self) -> usize {
        ffi::count_subshapes(&self.0, 0)
    }

    pub fn shell_count(&self) -> usize {
        ffi::count_subshapes(&self.0, 1)
    }

    pub fn face_count(&self) -> usize {
        ffi::count_subshapes(&self.0, 2)
    }

    /// What OCCT's checker finds wrong, one sub-shape and status code per line; empty
    /// for a sound shape.
    pub fn check_report(&self) -> String {
        ffi::check_report(&self.0)
    }

    /// Whether OCCT's checker finds the shape topologically and geometrically valid.
    pub fn is_valid(&self) -> bool {
        crate::profile::op("is_valid", || ffi::is_valid(&self.0))
    }

    /// Copy repaired by shape healing: small gaps, edges out of tolerance and wrongly
    /// oriented faces closed or fixed.
    pub fn fixed(&self) -> Result<Shape, CadError> {
        op(
            "shape fix",
            crate::profile::op("fix", || ffi::fix_shape(&self.0)),
        )
    }

    /// Copy rotated by `angle` (rad) about the z axis.
    pub fn rotated_z(&self, angle: f64) -> Result<Shape, CadError> {
        op("rotation", ffi::rotate_z(&self.0, angle))
    }

    /// `samples` points along each edge, in the order of OCCT's indexed edge map; empty
    /// for degenerate edges.
    pub fn edge_samples(&self, samples: usize) -> Vec<Vec<DVec3>> {
        let flat = crate::profile::op("edge_samples", || {
            ffi::edge_samples(&self.0, samples.max(1))
        });
        flat.chunks(samples.max(1))
            .map(|chunk| {
                if chunk.iter().any(|p| p.x.is_nan()) {
                    Vec::new()
                } else {
                    chunk.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect()
                }
            })
            .collect()
    }

    /// Triangles of the faces, facing out, no farther than `deflection` from them.
    pub fn triangles(&self, deflection: f64) -> Result<Vec<[DVec3; 3]>, CadError> {
        let flat = ffi::triangles(&self.0, deflection)
            .map_err(|e| CadError::Operation(e.what().to_owned()))?;
        Ok(flat
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t.map(|p| DVec3::new(p.x, p.y, p.z)))
            .collect())
    }

    /// The face of each triangle of the last [`Self::triangles`], as indices from 0.
    pub fn triangle_faces(&self) -> Vec<u32> {
        ffi::triangle_faces(&self.0)
    }

    /// The edges of the shells of the shape that bound only one face, `samples` points
    /// each: where the surface is open. Empty for a closed shell or solid.
    pub fn free_edges(&self, samples: usize) -> Result<Vec<Vec<DVec3>>, CadError> {
        let flat = ffi::free_edge_samples(&self.0, samples.max(1))
            .map_err(|e| CadError::Operation(e.what().to_owned()))?;
        Ok(flat
            .chunks(samples.max(1))
            .filter(|chunk| !chunk.iter().any(|p| p.x.is_nan()))
            .map(|chunk| chunk.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect())
            .collect())
    }

    /// For each face, in the order of OCCT's indexed face map, its plane as a normal and
    /// a point on it; `None` for a face that is not planar.
    pub fn face_planes(&self) -> Vec<Option<(DVec3, DVec3)>> {
        ffi::face_planes(&self.0)
            .chunks(7)
            .map(|f| {
                (f[0] > 0.5).then(|| (DVec3::new(f[1], f[2], f[3]), DVec3::new(f[4], f[5], f[6])))
            })
            .collect()
    }

    /// The faces of the shape but those at `dropped` (indices as in [`Self::face_planes`]),
    /// as a compound.
    pub fn faces_except(&self, dropped: &[u32]) -> Result<Shape, CadError> {
        op("faces", ffi::faces_except(&self.0, dropped))
    }

    /// The face at `index` (as in [`Self::face_planes`]).
    pub fn face(&self, index: u32) -> Result<Shape, CadError> {
        op("face", ffi::face_at(&self.0, index))
    }

    /// Where this edge crosses the face `face`, inside both: the point, its (u, v) on
    /// the face's surface and its parameter on the edge.
    pub fn crossings(&self, face: &Shape) -> Result<Vec<(DVec3, f64, f64, f64)>, CadError> {
        let flat = ffi::edge_face_crossings(&self.0, &face.0)
            .map_err(|e| CadError::Operation(e.what().to_owned()))?;
        Ok(flat
            .chunks(6)
            .map(|c| (DVec3::new(c[0], c[1], c[2]), c[3], c[4], c[5]))
            .collect())
    }

    /// The edges where the boundaries of this shape and `other` cross.
    pub fn section(&self, other: &Shape) -> Result<Shape, CadError> {
        op("section", ffi::section_of(&self.0, &other.0))
    }

    /// For each face (as in [`Self::face_planes`]), on which side of `solid` a point
    /// well inside it lies: 1 inside, -1 outside, 0 on its boundary (or no point
    /// found), with the point.
    pub fn face_sides(&self, solid: &Shape, tolerance: f64) -> Result<Vec<(i8, DVec3)>, CadError> {
        let flat = ffi::face_sides(&self.0, &solid.0, tolerance)
            .map_err(|e| CadError::Operation(e.what().to_owned()))?;
        Ok(flat
            .chunks(4)
            .map(|c| (c[0] as i8, DVec3::new(c[1], c[2], c[3])))
            .collect())
    }

    /// The solid the closed shells of the shape bound, facing out.
    pub fn solid(&self) -> Result<Shape, CadError> {
        op("solid", ffi::make_solid(&self.0))
    }

    /// Distance from each of `points` to the nearest face of the shape: to its surface,
    /// whether the point lies inside or outside. Slow on large shapes: tens of
    /// milliseconds a point.
    pub fn boundary_distances(&self, points: &[DVec3]) -> Result<Vec<f64>, CadError> {
        let points: Vec<ffi::P3> = points.iter().map(|p| p3(*p)).collect();
        crate::profile::op("boundary_distances", || {
            ffi::boundary_distances(&self.0, &points)
        })
        .map_err(|e| CadError::Operation(e.what().to_owned()))
    }

    /// The shape with the edges at positions `edges` of [`Self::edge_samples`] rounded
    /// with `radius`, mm.
    pub fn filleted(&self, edges: &[usize], radius: f64) -> Result<Shape, CadError> {
        let indices: Vec<u32> = edges
            .iter()
            .map(|&i| u32::try_from(i + 1).map_err(|_| CadError::Input("edge index".into())))
            .collect::<Result<_, _>>()?;
        op(
            "fillet",
            crate::profile::op("fillet", || ffi::fillet_edges(&self.0, &indices, radius)),
        )
    }

    /// Boolean difference `self − tool`.
    pub fn cut(&self, tool: &Shape) -> Result<Shape, CadError> {
        self.cut_fuzzy(tool, 0.0)
    }

    /// Boolean union.
    pub fn fuse(&self, other: &Shape) -> Result<Shape, CadError> {
        self.fuse_fuzzy(other, 0.0)
    }

    /// Boolean intersection.
    pub fn common(&self, other: &Shape) -> Result<Shape, CadError> {
        boolean(&[self], &[other], 2, 0.0)
    }

    /// Boolean difference treating gaps below `fuzzy` (model units) as contact, for
    /// faces that run nearly coincident.
    pub fn cut_fuzzy(&self, tool: &Shape, fuzzy: f64) -> Result<Shape, CadError> {
        self.cut_all_fuzzy(&[tool], fuzzy)
    }

    /// A copy with its own geometry, for an attempt that must not touch the original:
    /// a boolean may widen the tolerances of its inputs in place.
    pub fn copied(&self) -> Result<Shape, CadError> {
        op(
            "copy",
            crate::profile::op("copy", || ffi::copy_shape(&self.0)),
        )
    }

    /// `self` less all the `tools`, in one operation.
    pub fn cut_all_fuzzy(&self, tools: &[&Shape], fuzzy: f64) -> Result<Shape, CadError> {
        boolean(&[self], tools, 0, fuzzy)
    }

    /// Boolean union treating gaps below `fuzzy` (model units) as contact.
    pub fn fuse_fuzzy(&self, other: &Shape, fuzzy: f64) -> Result<Shape, CadError> {
        boolean(&[self], &[other], 1, fuzzy)
    }
}

/// `op` (0 cut, 1 fuse, 2 common) of all the `tools` with all the `arguments`.
fn boolean(
    arguments: &[&Shape],
    tools: &[&Shape],
    code: u8,
    fuzzy: f64,
) -> Result<Shape, CadError> {
    let what = ["boolean cut", "boolean fuse", "boolean common"][usize::from(code)];
    op(
        what,
        crate::profile::op(what, || {
            ffi::boolean_op(&list(arguments), &list(tools), code, fuzzy)
        }),
    )
}

/// The union of all the `pieces`, in one operation.
pub fn fuse_all_fuzzy(pieces: &[Shape], fuzzy: f64) -> Result<Shape, CadError> {
    let Some((first, rest)) = pieces.split_first() else {
        return Err(CadError::Input("nothing to fuse".into()));
    };
    if rest.is_empty() {
        return first.copied();
    }
    let rest: Vec<&Shape> = rest.iter().collect();
    boolean(&[first], &rest, 1, fuzzy)
}

/// Compound grouping several shapes without merging them.
pub fn compound(shapes: &[Shape]) -> Result<Shape, CadError> {
    let refs: Vec<&Shape> = shapes.iter().collect();
    op("compound", ffi::make_compound(&list(&refs)))
}

/// B-spline edge interpolating `points`; `periodic` closes it smoothly.
pub fn spline_edge(points: &[DVec3], periodic: bool) -> Result<Shape, CadError> {
    if points.len() < 2 {
        return Err(CadError::Input("a spline needs at least two points".into()));
    }
    let mut gaps: Vec<f64> = points.windows(2).map(|w| w[0].distance(w[1])).collect();
    if periodic {
        gaps.push(points[points.len() - 1].distance(points[0]));
    }
    if gaps.iter().any(|&d| d < MIN_POINT_SPACING) {
        return Err(CadError::Input("spline points coincide".into()));
    }
    let pts: Vec<ffi::P3> = points.iter().map(|&p| p3(p)).collect();
    op(
        "spline edge",
        crate::profile::op("spline", || ffi::spline_edge(&pts, periodic)),
    )
}

/// Straight edge from `a` to `b`.
pub fn line_edge(a: DVec3, b: DVec3) -> Result<Shape, CadError> {
    if a.distance(b) < MIN_POINT_SPACING {
        return Err(CadError::Input("line end points coincide".into()));
    }
    op("line edge", ffi::line_edge(&p3(a), &p3(b)))
}

/// Wire through `edges`, which must connect end to end.
pub fn wire(edges: &[Shape]) -> Result<Shape, CadError> {
    let refs: Vec<&Shape> = edges.iter().collect();
    op("wire", ffi::make_wire(&list(&refs)))
}

/// Planar face bounded by a closed planar `wire`.
pub fn planar_face(wire: &Shape) -> Result<Shape, CadError> {
    op("planar face", ffi::planar_face(&wire.0))
}

/// Solid swept by a planar face moved along +z by `height`.
pub fn prism_z(face: &Shape, height: f64) -> Result<Shape, CadError> {
    op("extrusion", ffi::prism_z(&face.0, height))
}

/// Cylinder of `radius` from `base` along `direction` over `height`, mm.
pub fn cylinder(
    base: DVec3,
    direction: DVec3,
    radius: f64,
    height: f64,
) -> Result<Shape, CadError> {
    if !(radius > 0.0 && height > 0.0 && direction.length() > 0.0) {
        return Err(CadError::Input(
            "a cylinder needs a positive radius and height and an axis".into(),
        ));
    }
    op(
        "cylinder",
        ffi::cylinder(&p3(base), &p3(direction.normalize()), radius, height),
    )
}

/// `shape` swept about the z axis through `angle` (rad), counter-clockwise: a wire into
/// faces, a face into a solid.
pub fn revolve_z_by(shape: &Shape, angle: f64) -> Result<Shape, CadError> {
    op("revolution", ffi::revolve_z_by(&shape.0, angle))
}

/// Solid of revolution of a planar face through 360° about the z axis.
pub fn revolve_z(face: &Shape) -> Result<Shape, CadError> {
    op(
        "revolution",
        crate::profile::op("revolution", || ffi::revolve_z(&face.0)),
    )
}

/// Closed solid lofted through closed section `wires`: B-spline side surfaces through
/// the sections, end caps on the free boundaries, sewn and checked.
pub fn loft(wires: &[Shape]) -> Result<Shape, CadError> {
    if wires.len() < 2 {
        return Err(CadError::Input("a loft needs at least two sections".into()));
    }
    let refs: Vec<&Shape> = wires.iter().collect();
    op(
        "loft",
        crate::profile::op("loft", || ffi::loft_solid(&list(&refs))),
    )
}

/// The faces lofted through section `wires` (closed or open), without end caps.
pub fn loft_shell(wires: &[Shape]) -> Result<Shape, CadError> {
    if wires.len() < 2 {
        return Err(CadError::Input("a loft needs at least two sections".into()));
    }
    let refs: Vec<&Shape> = wires.iter().collect();
    op(
        "loft shell",
        crate::profile::op("loft shell", || ffi::loft_shell(&list(&refs))),
    )
}

/// The faces of `shapes` split along the edges of `tools`.
pub fn split(shapes: &[&Shape], tools: &[&Shape]) -> Result<Shape, CadError> {
    op(
        "split",
        crate::profile::op("split", || ffi::split_by(&list(shapes), &list(tools))),
    )
}

/// The faces of `shapes` sewn where their edges lie within `tolerance` (mm).
pub fn sew(shapes: &[&Shape], tolerance: f64) -> Result<Shape, CadError> {
    op(
        "sewing",
        crate::profile::op("sew", || ffi::sew_shapes(&list(shapes), tolerance)),
    )
}

fn path_str(path: &Path) -> Result<&str, CadError> {
    path.to_str()
        .ok_or_else(|| CadError::Io(format!("{}: path is not valid UTF-8", path.display())))
}

fn io(result: Result<(), cxx::Exception>, path: &Path) -> Result<(), CadError> {
    result.map_err(|e| CadError::Io(format!("{}: {}", path.display(), e.what())))
}

/// Writes `shape` as STEP (AP214, millimetres).
pub fn write_step(shape: &Shape, path: &Path) -> Result<(), CadError> {
    io(ffi::write_step(&shape.0, path_str(path)?), path)
}

/// Writes `shape` as IGES (millimetres, B-rep mode).
pub fn write_iges(shape: &Shape, path: &Path) -> Result<(), CadError> {
    io(ffi::write_iges(&shape.0, path_str(path)?), path)
}

/// Triangulates `shape` with chordal deflection `deflection` (mm) and writes STL.
pub fn write_stl(shape: &Shape, path: &Path, deflection: f64) -> Result<(), CadError> {
    io(ffi::write_stl(&shape.0, path_str(path)?, deflection), path)
}

/// Reads the shapes of a STEP file into one shape.
pub fn read_step(path: &Path) -> Result<Shape, CadError> {
    op("STEP import", ffi::read_step(path_str(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::f64::consts::PI;

    #[test]
    fn lofted_faces_and_caps_sew_into_a_closed_solid() {
        let (bottom, top) = (circle(10.0, 0.0, 0.0), circle(10.0, 20.0, 0.0));
        let side = loft_shell(&[
            circle(10.0, 0.0, 0.0),
            circle(10.0, 10.0, 0.0),
            circle(10.0, 20.0, 0.0),
        ])
        .unwrap();
        // Open at both ends until capped.
        let open = sew(&[&side], 1e-3).unwrap();
        assert_eq!(open.free_edges(5).unwrap().len(), 2);
        let caps = [planar_face(&bottom).unwrap(), planar_face(&top).unwrap()];
        let shell = sew(&[&side, &caps[0], &caps[1]], 1e-3).unwrap();
        assert!(shell.free_edges(5).unwrap().is_empty());
        let solid = shell.solid().unwrap();
        assert!(solid.is_valid());
        assert_relative_eq!(solid.volume(), PI * 100.0 * 20.0, max_relative = 1e-3);
        // Every triangle knows its face: the side and the two caps.
        let triangles = solid.triangles(0.1).unwrap();
        let faces = solid.triangle_faces();
        assert_eq!(faces.len(), triangles.len());
        let mut distinct = faces.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), solid.face_count());
    }

    /// Rectangle (r from 10 to 20, z from 0 to 5) in the xz plane.
    fn ring_profile() -> Shape {
        let corners = [
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(20.0, 0.0, 0.0),
            DVec3::new(20.0, 0.0, 5.0),
            DVec3::new(10.0, 0.0, 5.0),
        ];
        let edges: Vec<Shape> = (0..4)
            .map(|i| line_edge(corners[i], corners[(i + 1) % 4]).unwrap())
            .collect();
        planar_face(&wire(&edges).unwrap()).unwrap()
    }

    #[test]
    fn revolved_ring_has_exact_volume() {
        let ring = revolve_z(&ring_profile()).unwrap();
        assert_eq!(ring.solid_count(), 1);
        assert!(ring.is_valid());
        assert_relative_eq!(
            ring.volume_with_tolerance(1e-9),
            PI * (400.0 - 100.0) * 5.0,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            ring.volume(),
            PI * (400.0 - 100.0) * 5.0,
            max_relative = 1e-4
        );
    }

    #[test]
    fn booleans_conserve_volume() {
        let ring = revolve_z(&ring_profile()).unwrap();
        let rotated = ring.rotated_z(0.3).unwrap();
        assert_relative_eq!(
            rotated.volume_with_tolerance(1e-9),
            ring.volume_with_tolerance(1e-9),
            max_relative = 1e-9
        );
        let both = ring.fuse(&rotated).unwrap();
        assert_relative_eq!(
            both.volume_with_tolerance(1e-9),
            ring.volume_with_tolerance(1e-9),
            max_relative = 1e-6
        );
    }

    fn circle(radius: f64, z: f64, tilt: f64) -> Shape {
        let pts: Vec<DVec3> = (0..24)
            .map(|i| {
                let a = 2.0 * PI * f64::from(i) / 24.0;
                DVec3::new(radius * a.cos(), radius * a.sin(), z + tilt * a.cos())
            })
            .collect();
        wire(&[spline_edge(&pts, true).unwrap()]).unwrap()
    }

    #[test]
    fn loft_of_two_circles_is_a_closed_cylinder() {
        let solid = loft(&[circle(10.0, 0.0, 0.0), circle(10.0, 20.0, 0.0)]).unwrap();
        assert_eq!(solid.solid_count(), 1);
        assert!(solid.is_valid());
        assert_relative_eq!(solid.volume(), PI * 100.0 * 20.0, max_relative = 1e-4);
    }

    #[test]
    fn loft_closes_non_planar_ends() {
        // Saddle-shaped end sections cannot take planar caps.
        let curved = |z: f64| {
            let pts: Vec<DVec3> = (0..24)
                .map(|i| {
                    let a = 2.0 * PI * f64::from(i) / 24.0;
                    DVec3::new(10.0 * a.cos(), 10.0 * a.sin(), z + 2.0 * (2.0 * a).cos())
                })
                .collect();
            wire(&[spline_edge(&pts, true).unwrap()]).unwrap()
        };
        let solid = loft(&[curved(0.0), curved(10.0), curved(20.0)]).unwrap();
        assert_eq!(solid.solid_count(), 1);
        assert!(solid.is_valid());
        // The saddles are symmetric, so the volume equals that of the cylinder.
        assert_relative_eq!(solid.volume(), PI * 100.0 * 20.0, max_relative = 0.01);
    }

    #[test]
    fn invalid_inputs_are_errors_not_aborts() {
        assert!(spline_edge(&[DVec3::ZERO, DVec3::ZERO], false).is_err());
        assert!(line_edge(DVec3::ONE, DVec3::ONE).is_err());
        let open = wire(&[line_edge(DVec3::ZERO, DVec3::X).unwrap()]).unwrap();
        assert!(loft(std::slice::from_ref(&open)).is_err());
        assert!(planar_face(&open).is_err());
        // Two open wires cannot be closed into a solid.
        let other = wire(&[line_edge(DVec3::Z, DVec3::new(1.0, 0.0, 1.0)).unwrap()]).unwrap();
        assert!(loft(&[open, other]).is_err());
        assert!(read_step(Path::new("/nonexistent/file.step")).is_err());
    }

    #[test]
    fn step_iges_stl_round_trip() {
        let ring = revolve_z(&ring_profile()).unwrap();
        let dir = std::env::temp_dir().join(format!("tp-cad-occ-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ring.step");
        write_step(&ring, &path).unwrap();
        let back = read_step(&path).unwrap();
        assert_relative_eq!(
            back.volume_with_tolerance(1e-9),
            ring.volume_with_tolerance(1e-9),
            max_relative = 1e-6
        );
        write_iges(&ring, &dir.join("ring.igs")).unwrap();
        write_stl(&ring, &dir.join("ring.stl"), 0.05).unwrap();
        for name in ["ring.igs", "ring.stl"] {
            assert!(std::fs::metadata(dir.join(name)).unwrap().len() > 100);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
