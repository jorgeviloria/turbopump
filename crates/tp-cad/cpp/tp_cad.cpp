// OCCT operations used by tp-cad. Every entry point runs inside `guard`, so OCCT
// exceptions reach Rust as errors that name the operation, instead of terminating the
// process. OCCT is built to turn signals into exceptions; `guard` does the same here,
// so a crash inside the kernel (a fillet that reads a null pointer) fails that one
// operation instead of the process.
#define OCC_CONVERT_SIGNALS

#include "tp-cad/cpp/tp_cad.hxx"
#include "tp-cad/src/bridge.rs.h"

#include <algorithm>
#include <mutex>
#include <cmath>
#include <stdexcept>
#include <string>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Section.hxx>
#include <BRepAlgoAPI_Splitter.hxx>
#include <BRepClass3d_SolidClassifier.hxx>
#include <BRepTools.hxx>
#include <BRepClass_FaceClassifier.hxx>
#include <GeomAPI_IntCS.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <gp_Pln.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_NurbsConvert.hxx>
#include <Geom_SurfaceOfRevolution.hxx>
#include <Geom_TrimmedCurve.hxx>
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
#include <NCollection_HArray1.hxx>
#include <NCollection_IndexedMap.hxx>
#include <NCollection_List.hxx>
#include <NCollection_Sequence.hxx>
#include <OSD.hxx>
#include <Standard_ErrorHandler.hxx>
#include <OSD_Parallel.hxx>
#include <OSD_ThreadPool.hxx>
#include <Poly_Triangulation.hxx>
#include <TopLoc_Location.hxx>
#include <IGESControl_Controller.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_Printer.hxx>
#include <IGESControl_Writer.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <ShapeAnalysis_FreeBounds.hxx>
#include <ShapeFix_Shape.hxx>
#include <ShapeFix_Solid.hxx>
#include <Standard_Failure.hxx>
#include <StlAPI_Writer.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Shell.hxx>
#include <gp.hxx>
#include <gp_Trsf.hxx>

namespace tp_cad {
namespace {

constexpr int MAX_OCCT_THREADS = 4;

// Statically linked, OCCT's type registry (a function-local static in Standard_Type)
// can be created after global objects of the data-exchange toolkits; those then
// outlive it and crash the process at exit when they release their type handles.
// The bridge is linked before OCCT, so this initializer runs first and creates the
// registry before any other OCCT global object, which makes it the last destroyed.
const occ::handle<Standard_Type> registry_anchor = STANDARD_TYPE(Standard_Transient);

// Every parallel OCCT algorithm (booleans, meshing) draws from the default thread pool,
// which is sized on first use. Cap it so a boolean leaves the machine responsive: with
// all cores per boolean, a few CAD builds at once froze a 10-core laptop.
const int occt_threads = [] {
  const int threads = std::min(MAX_OCCT_THREADS, OSD_Parallel::NbLogicalProcessors());
  OSD_ThreadPool::DefaultPool(threads);
  return threads;
}();

template <typename F> auto guard(const char *what, F &&f) -> decltype(f()) {
  static std::once_flag signals;
  std::call_once(signals, [] { OSD::SetSignal(OSD_SignalMode_Set, false); });
  try {
    OCC_CATCH_SIGNALS
    return f();
  } catch (const Standard_Failure &e) {
    const char *message = e.what();
    std::string detail = (message && *message) ? message : e.ExceptionType();
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
    const occ::handle<Message_Messenger> &messenger = Message::DefaultMessenger();
    for (NCollection_Sequence<occ::handle<Message_Printer>>::Iterator it(messenger->Printers());
         it.More(); it.Next()) {
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
  BRepBuilderAPI_MakeFace planar(wire, true);
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
    occ::handle<NCollection_HArray1<gp_Pnt>> array =
        new NCollection_HArray1<gp_Pnt>(1, static_cast<int>(points.size()));
    for (std::size_t i = 0; i < points.size(); ++i) {
      array->SetValue(static_cast<int>(i) + 1, point(points[i]));
    }
    GeomAPI_Interpolate interpolate(array, periodic, 1e-7);
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
    BRepBuilderAPI_MakeFace face(TopoDS::Wire(wire.shape), true);
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
    BRepOffsetAPI_ThruSections loft(false, false, 1e-6);
    loft.CheckCompatibility(true);
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
    ShapeAnalysis_FreeBounds bounds(loft.Shape(), 1e-6, false, false);
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

// The faces lofted through closed or open section `wires`, without end caps: one face
// per edge of the sections.
std::unique_ptr<Shape> loft_shell(const ShapeList &wires) {
  return guard("loft shell", [&] {
    if (wires.items.size() < 2) {
      fail("loft shell", "needs at least two sections");
    }
    BRepOffsetAPI_ThruSections loft(false, false, 1e-6);
    loft.CheckCompatibility(true);
    for (const auto &w : wires.items) {
      loft.AddWire(TopoDS::Wire(w));
    }
    loft.Build();
    if (!loft.IsDone()) {
      fail("loft shell", "surfaces through the sections failed");
    }
    return wrap(loft.Shape(), "loft shell");
  });
}

// The faces of all `shapes` sewn where their edges lie within `tolerance` of each other.
std::unique_ptr<Shape> sew_shapes(const ShapeList &shapes, double tolerance) {
  return guard("sewing", [&] {
    BRepBuilderAPI_Sewing sewing(tolerance);
    for (const auto &s : shapes.items) {
      sewing.Add(s);
    }
    sewing.Perform();
    return wrap(sewing.SewedShape(), "sewing");
  });
}

// The solid bounded by the closed shells of `shape`, its faces facing out.
std::unique_ptr<Shape> make_solid(const Shape &shape) {
  return guard("solid", [&] {
    BRepBuilderAPI_MakeSolid make;
    int shells = 0;
    for (TopExp_Explorer e(shape.shape, TopAbs_SHELL); e.More(); e.Next()) {
      make.Add(TopoDS::Shell(e.Current()));
      ++shells;
    }
    if (shells == 0 || !make.IsDone()) {
      fail("solid", "no shell to close");
    }
    ShapeFix_Solid fix(make.Solid());
    fix.Perform();
    TopoDS_Shape solid = fix.Solid();
    if (signed_volume(solid) < 0.0) {
      solid.Reverse();
    }
    return wrap(solid, "solid");
  });
}

// `samples` points along each edge of the shells of `shape` that bounds only one face:
// where its surface is open.
rust::Vec<P3> free_edge_samples(const Shape &shape, std::size_t samples) {
  return guard("free edges", [&] {
    ShapeAnalysis_FreeBounds bounds(shape.shape, false, false);
    TopoDS_Compound all;
    BRep_Builder builder;
    builder.MakeCompound(all);
    for (const TopoDS_Compound &wires : {bounds.GetClosedWires(), bounds.GetOpenWires()}) {
      if (!wires.IsNull()) {
        builder.Add(all, wires);
      }
    }
    return edge_samples(Shape{all}, samples);
  });
}

// For each triangle of the last triangulation of `shape`, in the order of `triangles`,
// the index of its face in OCCT's indexed face map, from 0.
rust::Vec<std::uint32_t> triangle_faces(const Shape &shape) {
  NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> faces;
  TopExp::MapShapes(shape.shape, TopAbs_FACE, faces);
  rust::Vec<std::uint32_t> out;
  for (TopExp_Explorer f(shape.shape, TopAbs_FACE); f.More(); f.Next()) {
    const TopoDS_Face &face = TopoDS::Face(f.Current());
    TopLoc_Location location;
    const occ::handle<Poly_Triangulation> facets = BRep_Tool::Triangulation(face, location);
    if (facets.IsNull()) {
      continue;
    }
    const auto index = static_cast<std::uint32_t>(faces.FindIndex(face) - 1);
    for (int i = 1; i <= facets->NbTriangles(); ++i) {
      out.push_back(index);
    }
  }
  return out;
}

// `shape` swept about the z axis through `angle` (rad): a wire into faces, a face into a
// solid.
std::unique_ptr<Shape> revolve_z_by(const Shape &shape, double angle) {
  return guard("revolution", [&] {
    BRepPrimAPI_MakeRevol revol(shape.shape, gp::OZ(), angle);
    revol.Build();
    if (!revol.IsDone()) {
      fail("revolution", "failed");
    }
    return wrap(revol.Shape(), "revolution");
  });
}

// The surfaces swept by the edges of `shape` turning a full turn about the z axis, as
// B-spline faces bounded by their own parameters: an edge across the axis sweeps an
// annulus that, kept a plane, only its trimming circles bound, and importers that
// draw surfaces untrimmed show it as an endless sheet.
std::unique_ptr<Shape> revolved_surfaces_z(const Shape &shape) {
  return guard("revolution", [&] {
    BRep_Builder builder;
    TopoDS_Compound faces;
    builder.MakeCompound(faces);
    for (TopExp_Explorer e(shape.shape, TopAbs_EDGE); e.More(); e.Next()) {
      double first = 0.0;
      double last = 0.0;
      Handle(Geom_Curve) curve = BRep_Tool::Curve(TopoDS::Edge(e.Current()), first, last);
      if (curve.IsNull()) {
        continue;
      }
      Handle(Geom_SurfaceOfRevolution) surface = new Geom_SurfaceOfRevolution(
          new Geom_TrimmedCurve(curve, first, last), gp::OZ());
      BRepBuilderAPI_MakeFace face(surface, 0.0, 2.0 * M_PI, first, last, 1e-7);
      if (!face.IsDone()) {
        fail("revolution", "a face of revolution failed");
      }
      BRepBuilderAPI_NurbsConvert nurbs(face.Face(), true);
      builder.Add(faces, nurbs.Shape());
    }
    return wrap(faces, "revolution");
  });
}

// For each face of `shape`, in OCCT's indexed face map: 1 if planar, else 0, then the
// plane's normal and a point on it (zeros for others): seven numbers a face.
rust::Vec<double> face_planes(const Shape &shape) {
  NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> faces;
  TopExp::MapShapes(shape.shape, TopAbs_FACE, faces);
  rust::Vec<double> out;
  for (int i = 1; i <= faces.Extent(); ++i) {
    BRepAdaptor_Surface surface(TopoDS::Face(faces(i)));
    if (surface.GetType() == GeomAbs_Plane) {
      const gp_Pln plane = surface.Plane();
      const gp_Dir n = plane.Axis().Direction();
      const gp_Pnt p = plane.Location();
      for (double v : {1.0, n.X(), n.Y(), n.Z(), p.X(), p.Y(), p.Z()}) {
        out.push_back(v);
      }
    } else {
      for (int k = 0; k < 7; ++k) {
        out.push_back(0.0);
      }
    }
  }
  return out;
}

// The faces of `shape` whose indices (from 0, in OCCT's indexed face map) are not in
// `dropped`, as a compound.
std::unique_ptr<Shape> faces_except(const Shape &shape, rust::Slice<const std::uint32_t> dropped) {
  return guard("faces", [&] {
    NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> faces;
    TopExp::MapShapes(shape.shape, TopAbs_FACE, faces);
    TopoDS_Compound kept;
    BRep_Builder builder;
    builder.MakeCompound(kept);
    for (int i = 1; i <= faces.Extent(); ++i) {
      bool drop = false;
      for (std::uint32_t d : dropped) {
        drop = drop || int(d) + 1 == i;
      }
      if (!drop) {
        builder.Add(kept, faces(i));
      }
    }
    return wrap(kept, "faces");
  });
}

// The face of `shape` at `index` (from 0, in OCCT's indexed face map).
std::unique_ptr<Shape> face_at(const Shape &shape, std::uint32_t index) {
  return guard("face", [&] {
    NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> faces;
    TopExp::MapShapes(shape.shape, TopAbs_FACE, faces);
    if (int(index) + 1 > faces.Extent()) {
      fail("face", "index out of range");
    }
    return wrap(faces(int(index) + 1), "face");
  });
}

// Where the edge `edge` crosses the face `face`, inside both: six numbers a crossing, the
// point, its (u, v) on the surface of the face and its parameter on the edge.
rust::Vec<double> edge_face_crossings(const Shape &edge, const Shape &face) {
  return guard("crossings", [&] {
    double first = 0.0, last = 0.0;
    const TopoDS_Edge &e = TopoDS::Edge(edge.shape);
    const occ::handle<Geom_Curve> curve = BRep_Tool::Curve(e, first, last);
    rust::Vec<double> out;
    if (curve.IsNull()) {
      return out;
    }
    const TopoDS_Face &f = TopoDS::Face(face.shape);
    const occ::handle<Geom_Surface> surface = BRep_Tool::Surface(f);
    GeomAPI_IntCS crossing(new Geom_TrimmedCurve(curve, first, last), surface);
    if (!crossing.IsDone()) {
      return out;
    }
    for (int i = 1; i <= crossing.NbPoints(); ++i) {
      double u = 0.0, v = 0.0, w = 0.0;
      crossing.Parameters(i, u, v, w);
      BRepClass_FaceClassifier inside(f, gp_Pnt2d(u, v), 1e-7);
      if (inside.State() == TopAbs_OUT) {
        continue;
      }
      const gp_Pnt p = crossing.Point(i);
      for (double x : {p.X(), p.Y(), p.Z(), u, v, w}) {
        out.push_back(x);
      }
    }
    return out;
  });
}

// The edges where the boundaries of `a` and `b` cross, as a compound.
std::unique_ptr<Shape> section_of(const Shape &a, const Shape &b) {
  return guard("section", [&] {
    BRepAlgoAPI_Section section(a.shape, b.shape, false);
    section.ComputePCurveOn1(true);
    section.Approximation(true);
    section.Build();
    if (!section.IsDone()) {
      fail("section", "failed");
    }
    return wrap(section.Shape(), "section");
  });
}

// The faces of all `shapes` split along the edges of `tools`, as one shape.
std::unique_ptr<Shape> split_by(const ShapeList &shapes, const ShapeList &tools) {
  return guard("split", [&] {
    BRepAlgoAPI_Splitter splitter;
    NCollection_List<TopoDS_Shape> args, tls;
    for (const auto &s : shapes.items) {
      args.Append(s);
    }
    for (const auto &t : tools.items) {
      tls.Append(t);
    }
    splitter.SetArguments(args);
    splitter.SetTools(tls);
    splitter.SetRunParallel(true);
    splitter.Build();
    if (!splitter.IsDone() || splitter.HasErrors()) {
      fail("split", "failed");
    }
    return wrap(splitter.Shape(), "split");
  });
}

// For each face of `faces` (OCCT's indexed face map), where a point well inside it lies
// with respect to the solid `solid`: 1 inside, -1 outside, 0 on its boundary or no point
// found; then the point.
rust::Vec<double> face_sides(const Shape &faces, const Shape &solid, double tolerance) {
  return guard("classify", [&] {
    NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> map;
    TopExp::MapShapes(faces.shape, TopAbs_FACE, map);
    BRepClass3d_SolidClassifier classifier(solid.shape);
    rust::Vec<double> out;
    for (int i = 1; i <= map.Extent(); ++i) {
      const TopoDS_Face &face = TopoDS::Face(map(i));
      double u0, u1, v0, v1;
      BRepTools::UVBounds(face, u0, u1, v0, v1);
      const occ::handle<Geom_Surface> surface = BRep_Tool::Surface(face);
      // The sample farthest inside the face's own boundary, on a grid.
      double side = 0.0;
      gp_Pnt at(0, 0, 0);
      bool found = false;
      const int n = 9;
      for (int a = 1; a < n && !found; ++a) {
        for (int b = 1; b < n && !found; ++b) {
          const double u = u0 + (u1 - u0) * ((a + 0.5 * (b % 2)) / n);
          const double v = v0 + (v1 - v0) * (double(b) / n);
          BRepClass_FaceClassifier inside(face, gp_Pnt2d(u, v), 1e-7);
          if (inside.State() != TopAbs_IN) {
            continue;
          }
          at = surface->Value(u, v);
          classifier.Perform(at, tolerance);
          const TopAbs_State state = classifier.State();
          side = state == TopAbs_IN ? 1.0 : state == TopAbs_OUT ? -1.0 : 0.0;
          found = state != TopAbs_ON;
        }
      }
      for (double x : {side, at.X(), at.Y(), at.Z()}) {
        out.push_back(x);
      }
    }
    return out;
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
    BRepBuilderAPI_Transform transform(shape.shape, trsf, true);
    if (!transform.IsDone()) {
      fail("rotation", "failed");
    }
    return wrap(transform.Shape(), "rotation");
  });
}

// The shape repaired by OCCT's shape healing: small gaps, edges out of tolerance and
// wrongly oriented faces that a boolean operation may leave behind.
std::unique_ptr<Shape> fix_shape(const Shape &shape) {
  return guard("shape fix", [&] {
    ShapeFix_Shape fix(shape.shape);
    fix.Perform();
    return wrap(fix.Shape(), "shape fix");
  });
}

// `fuzzy` is the gap in model units below which faces and edges count as touching
// (0: the exact algorithm).
// All the tools act on all the arguments in one operation, which intersects every
// pair once instead of building each intermediate result.
std::unique_ptr<Shape> boolean_op(const ShapeList &arguments, const ShapeList &tools,
                                  std::uint8_t op, double fuzzy) {
  return guard("boolean", [&] {
    auto run = [&](BRepAlgoAPI_BooleanOperation &algo, const char *what) {
      NCollection_List<TopoDS_Shape> args, tls;
      for (const auto &s : arguments.items) {
        args.Append(s);
      }
      for (const auto &s : tools.items) {
        tls.Append(s);
      }
      algo.SetArguments(args);
      algo.SetTools(tls);
      if (fuzzy > 0.0) {
        algo.SetFuzzyValue(fuzzy);
      }
      // Not in the non-destructive mode: it made fusions come out empty that work
      // without it (the ring and channel of a compensated spiral).
      // Oriented bounding boxes skip far pairs of faces sooner; the intersections run
      // in parallel.
      algo.SetUseOBB(true);
      algo.SetRunParallel(true);
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

// A copy with its own geometry: a boolean may widen the tolerances of its inputs in
// place, which must not carry over from one attempt to the next.
std::unique_ptr<Shape> copy_shape(const Shape &shape) {
  return guard("copy", [&] {
    BRepBuilderAPI_Copy copy(shape.shape, true);
    return wrap(copy.Shape(), "copy");
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

std::unique_ptr<Shape> cylinder(const P3 &base, const P3 &direction, double radius,
                                double height) {
  return guard("cylinder", [&] {
    const gp_Dir axis(direction.x, direction.y, direction.z);
    BRepPrimAPI_MakeCylinder cylinder(gp_Ax2(point(base), axis), radius, height);
    return wrap(cylinder.Shape(), "cylinder");
  });
}

rust::Vec<P3> edge_samples(const Shape &shape, std::size_t samples) {
  rust::Vec<P3> out;
  NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> edges;
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

rust::Vec<P3> triangles(const Shape &shape, double deflection) {
  return guard("triangulation", [&] {
    BRepMesh_IncrementalMesh mesh(shape.shape, deflection, false, 0.2, true);
    if (!mesh.IsDone()) {
      fail("triangulation", "meshing failed");
    }
    rust::Vec<P3> out;
    for (TopExp_Explorer f(shape.shape, TopAbs_FACE); f.More(); f.Next()) {
      const TopoDS_Face &face = TopoDS::Face(f.Current());
      TopLoc_Location location;
      const occ::handle<Poly_Triangulation> facets = BRep_Tool::Triangulation(face, location);
      if (facets.IsNull()) {
        continue;
      }
      const gp_Trsf place = location.Transformation();
      // A reversed face runs its triangles the other way, so that they face out.
      const bool reversed = face.Orientation() == TopAbs_REVERSED;
      for (int i = 1; i <= facets->NbTriangles(); ++i) {
        int a, b, c;
        facets->Triangle(i).Get(a, b, c);
        if (reversed) {
          std::swap(b, c);
        }
        for (int k : {a, b, c}) {
          const gp_Pnt p = facets->Node(k).Transformed(place);
          out.push_back(P3{p.X(), p.Y(), p.Z()});
        }
      }
    }
    return out;
  });
}

rust::Vec<double> boundary_distances(const Shape &shape, rust::Slice<const P3> points) {
  return guard("distance", [&] {
    // To the faces of the shape: measured to a solid, a point inside it is at zero.
    TopoDS_Compound faces;
    BRep_Builder builder;
    builder.MakeCompound(faces);
    for (TopExp_Explorer f(shape.shape, TopAbs_FACE); f.More(); f.Next()) {
      builder.Add(faces, f.Current());
    }
    rust::Vec<double> out;
    for (const P3 &p : points) {
      const TopoDS_Vertex vertex = BRepBuilderAPI_MakeVertex(gp_Pnt(p.x, p.y, p.z)).Vertex();
      BRepExtrema_DistShapeShape distance(faces, vertex);
      out.push_back(distance.IsDone() ? distance.Value() : std::nan(""));
    }
    return out;
  });
}

std::unique_ptr<Shape> fillet_edges(const Shape &shape, rust::Slice<const std::uint32_t> edges,
                                    double radius) {
  return guard("fillet", [&] {
    NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> map;
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
  NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> map;
  TopExp::MapShapes(shape.shape, kinds[kind], map);
  return static_cast<std::size_t>(map.Extent());
}

bool is_valid(const Shape &shape) {
  try {
    return BRepCheck_Analyzer(shape.shape).IsValid();
  } catch (...) {
    return false;
  }
}

rust::String check_report(const Shape &shape) {
  std::string out;
  try {
    BRepCheck_Analyzer analyzer(shape.shape);
    const TopAbs_ShapeEnum kinds[] = {TopAbs_SOLID, TopAbs_SHELL, TopAbs_FACE, TopAbs_WIRE,
                                      TopAbs_EDGE, TopAbs_VERTEX};
    const char *names[] = {"solid", "shell", "face", "wire", "edge", "vertex"};
    for (int k = 0; k < 6; ++k) {
      NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> map;
      TopExp::MapShapes(shape.shape, kinds[k], map);
      for (int i = 1; i <= map.Extent(); ++i) {
        const auto &result = analyzer.Result(map(i));
        if (result.IsNull()) continue;
        for (const auto &st : result->Status()) {
          if (st != BRepCheck_NoError) {
            out += std::string(names[k]) + " " + std::to_string(i - 1) + ": " +
                   std::to_string(static_cast<int>(st)) + "\n";
          }
        }
        if (kinds[k] == TopAbs_EDGE || kinds[k] == TopAbs_WIRE) {
          NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher> faces;
          TopExp::MapShapes(shape.shape, TopAbs_FACE, faces);
          for (int f = 1; f <= faces.Extent(); ++f) {
            try {
              for (const auto &st : result->StatusOnShape(faces(f))) {
                if (st != BRepCheck_NoError) {
                  out += std::string(names[k]) + " " + std::to_string(i - 1) + " on face " +
                         std::to_string(f - 1) + ": " + std::to_string(static_cast<int>(st)) + "\n";
                }
              }
            } catch (...) {
            }
          }
        }
      }
    }
  } catch (...) {
    out += "check threw\n";
  }
  return rust::String(out);
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
    // Solids as B-rep solids (entity 186); a shape without solids, the open surfaces
    // and curves of a volute, as trimmed surfaces (144) and curves, which every
    // importer reads: open shells in B-rep mode come out as shells no solid holds,
    // and importers that look for solids or trimmed surfaces find nothing.
    TopExp_Explorer solids(shape.shape, TopAbs_SOLID);
    IGESControl_Writer writer("MM", solids.More() ? 1 : 0);
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
    BRepMesh_IncrementalMesh mesh(shape.shape, deflection, false, 0.5, true);
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
