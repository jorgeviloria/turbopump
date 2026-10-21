// C++ side of the tp-cad bridge. Every function converts OCCT exceptions into
// std::runtime_error, which cxx turns into a Rust `Err`.
#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

#include <TopoDS_Shape.hxx>

#include "rust/cxx.h"

namespace tp_cad {

struct P3;

class Shape {
public:
  explicit Shape(TopoDS_Shape s) : shape(std::move(s)) {}
  TopoDS_Shape shape;
};

class ShapeList {
public:
  std::vector<TopoDS_Shape> items;
};

std::unique_ptr<ShapeList> new_shape_list();
void push_shape(ShapeList &list, const Shape &shape);

std::unique_ptr<Shape> spline_edge(rust::Slice<const P3> points, bool periodic);
std::unique_ptr<Shape> line_edge(const P3 &a, const P3 &b);
std::unique_ptr<Shape> make_wire(const ShapeList &edges);
std::unique_ptr<Shape> planar_face(const Shape &wire);
std::unique_ptr<Shape> revolve_z(const Shape &face);
std::unique_ptr<Shape> loft_solid(const ShapeList &wires);
std::unique_ptr<Shape> prism_z(const Shape &face, double height);
std::unique_ptr<Shape> rotate_z(const Shape &shape, double angle);
std::unique_ptr<Shape> fix_shape(const Shape &shape);
std::unique_ptr<Shape> boolean_op(const Shape &a, const Shape &b, std::uint8_t op,
                                  double fuzzy);
std::unique_ptr<Shape> make_compound(const ShapeList &shapes);

std::unique_ptr<Shape> cylinder(const P3 &base, const P3 &direction, double radius,
                                double height);
rust::Vec<P3> edge_samples(const Shape &shape, std::size_t samples);
std::unique_ptr<Shape> fillet_edges(const Shape &shape, rust::Slice<const std::uint32_t> edges,
                                    double radius);

double volume(const Shape &shape, double eps);
rust::Vec<double> solid_volumes(const Shape &shape);
std::size_t count_subshapes(const Shape &shape, std::uint8_t kind);
bool is_valid(const Shape &shape);

void write_step(const Shape &shape, rust::Str path);
void write_iges(const Shape &shape, rust::Str path);
void write_stl(const Shape &shape, rust::Str path, double deflection);
std::unique_ptr<Shape> read_step(rust::Str path);

} // namespace tp_cad
