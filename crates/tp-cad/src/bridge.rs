//! cxx bridge to the C++ side in `cpp/tp_cad.cpp`.

// FFI with C++ needs `unsafe extern "C++"`. This and the CoolProp loader are the only
// modules of the workspace allowed to use `unsafe`. The C++ side converts every OCCT
// exception into an error, and functions that can fail return `Result`.
#![allow(unsafe_code)]

#[cxx::bridge(namespace = "tp_cad")]
pub(crate) mod ffi {
    /// A 3D point in model units (mm).
    #[derive(Debug, Clone, Copy)]
    struct P3 {
        x: f64,
        y: f64,
        z: f64,
    }

    unsafe extern "C++" {
        include!("tp-cad/cpp/tp_cad.hxx");

        type Shape;
        type ShapeList;

        fn new_shape_list() -> UniquePtr<ShapeList>;
        fn push_shape(list: Pin<&mut ShapeList>, shape: &Shape);

        fn spline_edge(points: &[P3], periodic: bool) -> Result<UniquePtr<Shape>>;
        fn line_edge(a: &P3, b: &P3) -> Result<UniquePtr<Shape>>;
        fn make_wire(edges: &ShapeList) -> Result<UniquePtr<Shape>>;
        fn planar_face(wire: &Shape) -> Result<UniquePtr<Shape>>;
        fn revolve_z(face: &Shape) -> Result<UniquePtr<Shape>>;
        fn loft_solid(wires: &ShapeList) -> Result<UniquePtr<Shape>>;
        /// Extrusion of a planar face along +z by `height`.
        fn prism_z(face: &Shape, height: f64) -> Result<UniquePtr<Shape>>;
        fn rotate_z(shape: &Shape, angle: f64) -> Result<UniquePtr<Shape>>;
        fn fix_shape(shape: &Shape) -> Result<UniquePtr<Shape>>;
        /// `op`: 0 cut, 1 fuse, 2 common, of all the `tools` with all the `arguments`.
        fn boolean_op(
            arguments: &ShapeList,
            tools: &ShapeList,
            op: u8,
            fuzzy: f64,
        ) -> Result<UniquePtr<Shape>>;
        /// A copy of the shape with its own geometry.
        fn copy_shape(shape: &Shape) -> Result<UniquePtr<Shape>>;
        fn make_compound(shapes: &ShapeList) -> Result<UniquePtr<Shape>>;
        /// Cylinder from `base` along `direction`.
        fn cylinder(
            base: &P3,
            direction: &P3,
            radius: f64,
            height: f64,
        ) -> Result<UniquePtr<Shape>>;
        /// `samples` points along each edge, in the order of OCCT's indexed edge map
        /// (indices from 1); NaN for degenerate edges.
        fn edge_samples(shape: &Shape, samples: usize) -> Vec<P3>;
        /// Triangles of the faces, three points each, facing out, within `deflection`.
        fn triangles(shape: &Shape, deflection: f64) -> Result<Vec<P3>>;
        /// Distance from each point to the nearest face; NaN where it cannot be found.
        fn boundary_distances(shape: &Shape, points: &[P3]) -> Result<Vec<f64>>;
        /// Rounds the edges of the indexed edge map with `radius`.
        fn fillet_edges(shape: &Shape, edges: &[u32], radius: f64) -> Result<UniquePtr<Shape>>;

        /// Volume; `eps` > 0 integrates adaptively to that relative error.
        fn volume(shape: &Shape, eps: f64) -> f64;
        fn solid_volumes(shape: &Shape) -> Vec<f64>;
        /// `kind`: 0 solid, 1 shell, 2 face, 3 edge.
        fn count_subshapes(shape: &Shape, kind: u8) -> usize;
        fn is_valid(shape: &Shape) -> bool;

        fn write_step(shape: &Shape, path: &str) -> Result<()>;
        fn write_iges(shape: &Shape, path: &str) -> Result<()>;
        fn write_stl(shape: &Shape, path: &str, deflection: f64) -> Result<()>;
        fn read_step(path: &str) -> Result<UniquePtr<Shape>>;
    }
}
