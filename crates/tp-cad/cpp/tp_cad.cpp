// OCCT operations used by tp-cad. Every entry point runs inside `guard`, so OCCT
// exceptions (Standard_Failure, which is not a std::exception) reach Rust as errors
// instead of terminating the process.

#include "tp-cad/cpp/tp_cad.hxx"
#include "tp-cad/src/bridge.rs.h"

#include <cmath>
#include <stdexcept>
#include <string>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepOffsetAPI_MakeFilling.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRep_Tool.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRep_Builder.hxx>
#include <GProp_GProps.hxx>
#include <GeomAPI_Interpolate.hxx>
#include <IGESControl_Controller.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_Printer.hxx>
#include <IGESControl_Writer.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <ShapeAnalysis_FreeBounds.hxx>
#include <ShapeFix_Solid.hxx>
#include <Standard_Failure.hxx>
#include <StlAPI_Writer.hxx>
#include <TColgp_HArray1OfPnt.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Shell.hxx>
#include <gp.hxx>
#include <gp_Trsf.hxx>

namespace tp_cad {
namespace {

// Statically linked, OCCT's type registry (a function-local static in Standard_Type)
// can be created after global objects of the data-exchange toolkits; those then
// outlive it and crash the process at exit when they release their type handles.
// The bridge is linked before OCCT, so this initializer runs first and creates the
// registry before any other OCCT global object, which makes it the last destroyed.
const Handle(Standard_Type) registry_anchor = STANDARD_TYPE(Standard_Transient);

template <typename F> auto guard(const char *what, F &&f) -> decltype(f()) {
  try {
    return f();
  } catch (const Standard_Failure &e) {
    const char *message = e.GetMessageString();
    std::string detail = (message && *message) ? message : e.DynamicType()->Name();
    throw std::runtime_error(std::string(what) + ": " + detail);
  } catch (const std::exception &) {
    throw;
  } catch (...) {
    throw std::runtime_error(std::string(what) + ": unknown OCCT error");
  }
}

void fail(const char *what, const char *why) {
  throw std::runtime_error(std::string(what) + ": " + why);
}

std::unique_ptr<Shape> wrap(const TopoDS_Shape &shape, const char *what) {
  if (shape.IsNull()) {
    fail(what, "empty result");
  }
  return std::make_unique<Shape>(shape);
}

gp_Pnt point(const P3 &p) { return gp_Pnt(p.x, p.y, p.z); }

std::string text(rust::Str s) { return std::string(s.data(), s.size()); }

// The data-exchange writers print transfer statistics through OCCT's default
// messenger; keep only failures so the library does not write to stdout.
void quiet_messages() {
  static const bool done = [] {
    const Handle(Message_Messenger) &messenger = Message::DefaultMessenger();
    for (Message_SequenceOfPrinters::Iterator it(messenger->Printers()); it.More(); it.Next()) {
      it.Value()->SetTraceLevel(Message_Fail);
    }
    return true;
  }();
  (void)done;
}

// Volume by Gauss integration. With `eps` > 0 the integration is adaptive with that
// relative error; the plain rule (eps = 0) can lose about 0.1 % on solids bounded by
// many B-spline faces.
double signed_volume(const TopoDS_Shape &shape, double eps = 0.0) {
  GProp_GProps props;
  if (eps > 0.0) {
    BRepGProp::VolumeProperties(shape, props, eps);
  } else {
    BRepGProp::VolumeProperties(shape, props);
  }
  return props.Mass();
}

// End face of a lofted shell: planar if possible, otherwise a filling surface
// constrained by the section edges.
TopoDS_Face cap(const TopoDS_Wire &wire) {
  BRepBuilderAPI_MakeFace planar(wire, Standard_True);
  if (planar.IsDone()) {
    return planar.Face();
  }
  BRepOffsetAPI_MakeFilling filling;
  for (TopExp_Explorer e(wire, TopAbs_EDGE); e.More(); e.Next()) {
    filling.Add(TopoDS::Edge(e.Current()), GeomAbs_C0);
  }
  filling.Build();
  if (!filling.IsDone()) {
    fail("blade cap", "filling surface failed");
  }
  return TopoDS::Face(filling.Shape());
}

} // namespace

std::unique_ptr<ShapeList> new_shape_list() { return std::make_unique<ShapeList>(); }

void push_shape(ShapeList &list, const Shape &shape) { list.items.push_back(shape.shape); }

std::unique_ptr<Shape> spline_edge(rust::Slice<const P3> points, bool periodic) {
  return guard("spline edge", [&] {
    Handle(TColgp_HArray1OfPnt) array = new TColgp_HArray1OfPnt(1, static_cast<int>(points.size()));
    for (std::size_t i = 0; i < points.size(); ++i) {
      array->SetValue(static_cast<int>(i) + 1, point(points[i]));
    }
    GeomAPI_Interpolate interpolate(array, periodic ? Standard_True : Standard_False, 1e-7);
    interpolate.Perform();
    if (!interpolate.IsDone()) {
      fail("spline edge", "interpolation failed");
    }
    BRepBuilderAPI_MakeEdge edge(interpolate.Curve());
    if (!edge.IsDone()) {
      fail("spline edge", "edge construction failed");
    }
    return wrap(edge.Edge(), "spline edge");
  });
}

std::unique_ptr<Shape> line_edge(const P3 &a, const P3 &b) {
  return guard("line edge", [&] {
    BRepBuilderAPI_MakeEdge edge(point(a), point(b));
    if (!edge.IsDone()) {
      fail("line edge", "edge construction failed");
    }
    return wrap(edge.Edge(), "line edge");
  });
}

std::unique_ptr<Shape> make_wire(const ShapeList &edges) {
  return guard("wire", [&] {
    BRepBuilderAPI_MakeWire wire;
    for (const auto &e : edges.items) {
      wire.Add(TopoDS::Edge(e));
    }
    if (!wire.IsDone()) {
      fail("wire", "edges do not connect");
    }
    return wrap(wire.Wire(), "wire");
  });
}

std::unique_ptr<Shape> planar_face(const Shape &wire) {
  return guard("planar face", [&] {
    BRepBuilderAPI_MakeFace face(TopoDS::Wire(wire.shape), Standard_True);
    if (!face.IsDone()) {
      fail("planar face", "the wire is not closed and planar");
    }
    return wrap(face.Face(), "planar face");
  });
}

std::unique_ptr<Shape> revolve_z(const Shape &face) {
  return guard("revolution", [&] {
    BRepPrimAPI_MakeRevol revol(face.shape, gp::OZ(), 2.0 * M_PI);
    revol.Build();
    if (!revol.IsDone()) {
      fail("revolution", "failed");
    }
    return wrap(revol.Shape(), "revolution");
  });
}

std::unique_ptr<Shape> loft_solid(const ShapeList &wires) {
  return guard("loft", [&] {
    if (wires.items.size() < 2) {
      fail("loft", "needs at least two sections");
    }
    BRepOffsetAPI_ThruSections loft(Standard_False, Standard_False, 1e-6);
    loft.CheckCompatibility(Standard_True);
    for (const auto &w : wires.items) {
      loft.AddWire(TopoDS::Wire(w));
    }
    loft.Build();
    if (!loft.IsDone()) {
      fail("loft", "surfaces through the sections failed");
    }

    // Close the two open ends of the shell with faces on its free boundaries.
    BRepBuilderAPI_Sewing sewing(1e-3);
    sewing.Add(loft.Shape());
    ShapeAnalysis_FreeBounds bounds(loft.Shape(), 1e-6, Standard_False, Standard_False);
    int caps = 0;
    for (TopExp_Explorer w(bounds.GetClosedWires(), TopAbs_WIRE); w.More(); w.Next()) {
      sewing.Add(cap(TopoDS::Wire(w.Current())));
      ++caps;
    }
    if (caps != 2) {
      fail("loft", "the lofted shell does not have two open ends");
    }
    sewing.Perform();

    TopExp_Explorer shells(sewing.SewedShape(), TopAbs_SHELL);
    if (!shells.More()) {
      fail("loft", "sewing produced no shell");
    }
    BRepBuilderAPI_MakeSolid make_solid(TopoDS::Shell(shells.Current()));
    if (!make_solid.IsDone()) {
      fail("loft", "shell is not closed");
    }
    ShapeFix_Solid fix(make_solid.Solid());
    fix.Perform();
    TopoDS_Shape solid = fix.Solid();
    if (signed_volume(solid) < 0.0) {
      solid.Reverse();
    }
    if (!BRepCheck_Analyzer(solid).IsValid()) {
      fail("loft", "the closed blade solid is not valid");
    }
    return wrap(solid, "loft");
  });
}

std::unique_ptr<Shape> prism_z(const Shape &face, double height) {
  return guard("extrusion", [&] {
    BRepPrimAPI_MakePrism prism(face.shape, gp_Vec(0.0, 0.0, height));
    if (!prism.IsDone()) {
      fail("extrusion", "failed");
    }
    return wrap(prism.Shape(), "extrusion");
  });
}

std::unique_ptr<Shape> rotate_z(const Shape &shape, double angle) {
  return guard("rotation", [&] {
    gp_Trsf trsf;
    trsf.SetRotation(gp::OZ(), angle);
    BRepBuilderAPI_Transform transform(shape.shape, trsf, Standard_True);
    if (!transform.IsDone()) {
      fail("rotation", "failed");
    }
    return wrap(transform.Shape(), "rotation");
  });
}

// `fuzzy` is the gap in model units below which faces and edges count as touching
// (0: the exact algorithm).
std::unique_ptr<Shape> boolean_op(const Shape &a, const Shape &b, std::uint8_t op,
                                  double fuzzy) {
  return guard("boolean", [&] {
    auto run = [&](BRepAlgoAPI_BooleanOperation &algo, const char *what) {
      TopTools_ListOfShape arguments, tools;
      arguments.Append(a.shape);
      tools.Append(b.shape);
      algo.SetArguments(arguments);
      algo.SetTools(tools);
      if (fuzzy > 0.0) {
        algo.SetFuzzyValue(fuzzy);
      }
      // Oriented bounding boxes skip far pairs of faces sooner; the intersections run
      // on every core.
      algo.SetUseOBB(Standard_True);
      algo.SetRunParallel(Standard_True);
      algo.Build();
      if (!algo.IsDone() || algo.HasErrors()) {
        fail(what, "failed");
      }
      return wrap(algo.Shape(), what);
    };
    switch (op) {
    case 0: {
      BRepAlgoAPI_Cut cut;
      return run(cut, "boolean cut");
    }
    case 1: {
      BRepAlgoAPI_Fuse fuse;
      return run(fuse, "boolean fuse");
    }
    case 2: {
      BRepAlgoAPI_Common common;
      return run(common, "boolean common");
    }
    default:
      fail("boolean", "unknown operation");
    }
    return std::unique_ptr<Shape>();
  });
}

std::unique_ptr<Shape> make_compound(const ShapeList &shapes) {
  return guard("compound", [&] {
    BRep_Builder builder;
    TopoDS_Compound compound;
    builder.MakeCompound(compound);
    for (const auto &s : shapes.items) {
      builder.Add(compound, s);
    }
    return wrap(compound, "compound");
  });
}

double volume(const Shape &shape, double eps) {
  try {
    return signed_volume(shape.shape, eps);
  } catch (...) {
    return std::nan("");
  }
}

rust::Vec<double> solid_volumes(const Shape &shape) {
  rust::Vec<double> volumes;
  for (TopExp_Explorer e(shape.shape, TopAbs_SOLID); e.More(); e.Next()) {
    try {
      volumes.push_back(signed_volume(e.Current(), 1e-5));
    } catch (...) {
      volumes.push_back(std::nan(""));
    }
  }
  return volumes;
}

std::unique_ptr<Shape> cylinder_z(const P3 &base, double radius, double height) {
  return guard("cylinder", [&] {
    BRepPrimAPI_MakeCylinder cylinder(gp_Ax2(point(base), gp::DZ()), radius, height);
    return wrap(cylinder.Shape(), "cylinder");
  });
}

rust::Vec<P3> edge_samples(const Shape &shape, std::size_t samples) {
  rust::Vec<P3> out;
  TopTools_IndexedMapOfShape edges;
  TopExp::MapShapes(shape.shape, TopAbs_EDGE, edges);
  const double nan = std::nan("");
  for (int i = 1; i <= edges.Extent(); ++i) {
    const TopoDS_Edge &edge = TopoDS::Edge(edges(i));
    bool usable = !BRep_Tool::Degenerated(edge);
    std::unique_ptr<BRepAdaptor_Curve> curve;
    if (usable) {
      try {
        curve = std::make_unique<BRepAdaptor_Curve>(edge);
      } catch (...) {
        usable = false;
      }
    }
    for (std::size_t k = 0; k < samples; ++k) {
      if (!usable) {
        out.push_back(P3{nan, nan, nan});
        continue;
      }
      const double t = samples > 1 ? double(k) / double(samples - 1) : 0.5;
      const gp_Pnt p = curve->Value(curve->FirstParameter() +
                                    t * (curve->LastParameter() - curve->FirstParameter()));
      out.push_back(P3{p.X(), p.Y(), p.Z()});
    }
  }
  return out;
}

std::unique_ptr<Shape> fillet_edges(const Shape &shape, rust::Slice<const std::uint32_t> edges,
                                    double radius) {
  return guard("fillet", [&] {
    TopTools_IndexedMapOfShape map;
    TopExp::MapShapes(shape.shape, TopAbs_EDGE, map);
    BRepFilletAPI_MakeFillet fillet(shape.shape);
    for (std::uint32_t index : edges) {
      if (index < 1 || int(index) > map.Extent()) {
        fail("fillet", "edge index out of range");
      }
      fillet.Add(radius, TopoDS::Edge(map(int(index))));
    }
    fillet.Build();
    if (!fillet.IsDone()) {
      fail("fillet", "the rounded edges could not be built");
    }
    return wrap(fillet.Shape(), "fillet");
  });
}

std::size_t count_subshapes(const Shape &shape, std::uint8_t kind) {
  static const TopAbs_ShapeEnum kinds[] = {TopAbs_SOLID, TopAbs_SHELL, TopAbs_FACE, TopAbs_EDGE};
  if (kind >= 4) {
    return 0;
  }
  TopTools_IndexedMapOfShape map;
  TopExp::MapShapes(shape.shape, kinds[kind], map);
  return static_cast<std::size_t>(map.Extent());
}

bool is_valid(const Shape &shape) {
  try {
    return BRepCheck_Analyzer(shape.shape).IsValid() == Standard_True;
  } catch (...) {
    return false;
  }
}

void write_step(const Shape &shape, rust::Str path) {
  guard("STEP export", [&] {
    quiet_messages();
    STEPControl_Writer writer;
    if (writer.Transfer(shape.shape, STEPControl_AsIs) != IFSelect_RetDone) {
      fail("STEP export", "transfer failed");
    }
    if (writer.Write(text(path).c_str()) != IFSelect_RetDone) {
      fail("STEP export", "could not write the file");
    }
  });
}

void write_iges(const Shape &shape, rust::Str path) {
  guard("IGES export", [&] {
    quiet_messages();
    IGESControl_Controller::Init();
    IGESControl_Writer writer("MM", 1);
    if (!writer.AddShape(shape.shape)) {
      fail("IGES export", "transfer failed");
    }
    writer.ComputeModel();
    if (!writer.Write(text(path).c_str())) {
      fail("IGES export", "could not write the file");
    }
  });
}

void write_stl(const Shape &shape, rust::Str path, double deflection) {
  guard("STL export", [&] {
    BRepMesh_IncrementalMesh mesh(shape.shape, deflection, Standard_False, 0.5, Standard_True);
    if (!mesh.IsDone()) {
      fail("STL export", "triangulation failed");
    }
    StlAPI_Writer writer;
    if (!writer.Write(shape.shape, text(path).c_str())) {
      fail("STL export", "could not write the file");
    }
  });
}

std::unique_ptr<Shape> read_step(rust::Str path) {
  return guard("STEP import", [&] {
    quiet_messages();
    STEPControl_Reader reader;
    if (reader.ReadFile(text(path).c_str()) != IFSelect_RetDone) {
      fail("STEP import", "could not read the file");
    }
    reader.TransferRoots();
    return wrap(reader.OneShape(), "STEP import");
  });
}

} // namespace tp_cad
