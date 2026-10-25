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
        ffi::volume(&self.0, 1e-3)
    }

    /// Volume with a chosen relative error (smaller is slower; 0 = plain Gauss rule).
    pub fn volume_with_tolerance(&self, eps: f64) -> f64 {
        ffi::volume(&self.0, eps)
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

    /// Whether OCCT's checker finds the shape topologically and geometrically valid.
    pub fn is_valid(&self) -> bool {
        ffi::is_valid(&self.0)
    }

    /// Copy repaired by shape healing: small gaps, edges out of tolerance and wrongly
    /// oriented faces closed or fixed.
    pub fn fixed(&self) -> Result<Shape, CadError> {
        op("shape fix", ffi::fix_shape(&self.0))
    }

    /// Copy rotated by `angle` (rad) about the z axis.
    pub fn rotated_z(&self, angle: f64) -> Result<Shape, CadError> {
        op("rotation", ffi::rotate_z(&self.0, angle))
    }

    /// `samples` points along each edge, in the order of OCCT's indexed edge map; empty
    /// for degenerate edges.
    pub fn edge_samples(&self, samples: usize) -> Vec<Vec<DVec3>> {
        let flat = ffi::edge_samples(&self.0, samples.max(1));
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

    /// Distance from each of `points` to the nearest face of the shape: to its surface,
    /// whether the point lies inside or outside. Slow on large shapes: tens of
    /// milliseconds a point.
    pub fn boundary_distances(&self, points: &[DVec3]) -> Result<Vec<f64>, CadError> {
        let points: Vec<ffi::P3> = points.iter().map(|p| p3(*p)).collect();
        ffi::boundary_distances(&self.0, &points)
            .map_err(|e| CadError::Operation(e.what().to_owned()))
    }

    /// The shape with the edges at positions `edges` of [`Self::edge_samples`] rounded
    /// with `radius`, mm.
    pub fn filleted(&self, edges: &[usize], radius: f64) -> Result<Shape, CadError> {
        let indices: Vec<u32> = edges
            .iter()
            .map(|&i| u32::try_from(i + 1).map_err(|_| CadError::Input("edge index".into())))
            .collect::<Result<_, _>>()?;
        op("fillet", ffi::fillet_edges(&self.0, &indices, radius))
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
        op("boolean common", ffi::boolean_op(&self.0, &other.0, 2, 0.0))
    }

    /// Boolean difference treating gaps below `fuzzy` (model units) as contact, for
    /// faces that run nearly coincident.
    pub fn cut_fuzzy(&self, tool: &Shape, fuzzy: f64) -> Result<Shape, CadError> {
        op("boolean cut", ffi::boolean_op(&self.0, &tool.0, 0, fuzzy))
    }

    /// Boolean union treating gaps below `fuzzy` (model units) as contact.
    pub fn fuse_fuzzy(&self, other: &Shape, fuzzy: f64) -> Result<Shape, CadError> {
        op("boolean fuse", ffi::boolean_op(&self.0, &other.0, 1, fuzzy))
    }
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
    op("spline edge", ffi::spline_edge(&pts, periodic))
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

/// Solid of revolution of a planar face through 360° about the z axis.
pub fn revolve_z(face: &Shape) -> Result<Shape, CadError> {
    op("revolution", ffi::revolve_z(&face.0))
}

/// Closed solid lofted through closed section `wires`: B-spline side surfaces through
/// the sections, end caps on the free boundaries, sewn and checked.
pub fn loft(wires: &[Shape]) -> Result<Shape, CadError> {
    if wires.len() < 2 {
        return Err(CadError::Input("a loft needs at least two sections".into()));
    }
    let refs: Vec<&Shape> = wires.iter().collect();
    op("loft", ffi::loft_solid(&list(&refs)))
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
