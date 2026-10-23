//! Meridional view: hub and shroud contours, leading and trailing edges, stream lines,
//! and draggable handles that edit the meridional-contour inputs. On request, the
//! two-dimensional potential flow: its stream lines and the meridional velocity along
//! hub and shroud.

use std::f64::consts::PI;

use eframe::egui::containers::menu::{MenuButton, MenuConfig};
use eframe::egui::{self, Color32, CursorIcon, DragValue, PopupCloseBehavior, RichText, Sense, Ui};
use egui_plot::{
    Corner, Legend, Line, LineStyle, MarkerShape, Plot, PlotBounds, PlotPoint, PlotPoints,
    PlotTransform, Points, Text,
};
use tp_design::impeller::meridian::{
    AxialReference, DesignMode, EdgeExtent, InletTilt, LeadingEdgeShape, MeridionalContour,
    MeridionalInput, chord_coordinates,
};
use tp_design::impeller::potential::{self, PotentialFlow, PotentialMesh};
use tp_design::impeller::wall::{self, Segment, SegmentInput, SegmentKind, Wall, WallPoint};
use tp_design::impeller::{ImpellerBasis, ImpellerGeometry, ImpellerGeometryInput};
use tp_design::pump::CentrifugalPumpInput;
use tp_design::{MainDimensionsInput, Param};
use tp_math::DVec2;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::{Palette, unavailable};
use crate::app::EditCtx;
use crate::i18n::{tf, tr};

/// A wall of the meridional section: hub and shroud of the flow path, and the wires
/// that close the hub disk and the shroud cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Hub,
    Shroud,
    HubMaterial,
    ShroudMaterial,
    /// The casing walls of the side chambers.
    FrontCasing,
    RearCasing,
    /// The center line the walls follow in its design mode.
    Center,
}

impl Side {
    /// The walls of the flow path.
    pub(crate) const WALLS: [Side; 2] = [Side::Hub, Side::Shroud];
    /// The wires of the material.
    pub(crate) const MATERIALS: [Side; 2] = [Side::HubMaterial, Side::ShroudMaterial];
    /// The casing walls of the side chambers.
    pub(crate) const CASINGS: [Side; 2] = [Side::FrontCasing, Side::RearCasing];
    /// The center line alone.
    pub(crate) const CENTER: [Side; 1] = [Side::Center];

    pub(crate) fn wall(self, contour: &MeridionalContour) -> &Wall {
        match self {
            Side::Hub => &contour.hub,
            Side::Shroud => &contour.shroud,
            Side::HubMaterial => &contour.hub_material,
            Side::ShroudMaterial => &contour.shroud_material,
            Side::FrontCasing => &contour.chambers.front_casing,
            Side::RearCasing => &contour.chambers.rear_casing,
            Side::Center => &contour.center_line,
        }
    }

    fn segments(self, input: &mut MeridionalInput) -> &mut Option<Vec<SegmentInput>> {
        match self {
            Side::Hub => &mut input.hub_segments,
            Side::Shroud => &mut input.shroud_segments,
            Side::HubMaterial => &mut input.hub_material,
            Side::ShroudMaterial => &mut input.shroud_material,
            Side::FrontCasing => &mut input.chambers.front,
            Side::RearCasing => &mut input.chambers.rear,
            Side::Center => &mut input.center_line,
        }
    }

    fn drawn(self, input: &MeridionalInput) -> bool {
        match self {
            Side::Hub => input.hub_segments.is_some(),
            Side::Shroud => input.shroud_segments.is_some(),
            Side::HubMaterial => input.hub_material.is_some(),
            Side::ShroudMaterial => input.shroud_material.is_some(),
            Side::FrontCasing => input.chambers.front.is_some(),
            Side::RearCasing => input.chambers.rear.is_some(),
            Side::Center => input.center_line.is_some(),
        }
    }

    /// Whether it is a wire round the flow path, material or casing: its points show
    /// automatic or drawn, and it goes back to automatic as a whole.
    pub(crate) fn is_wire(self) -> bool {
        !matches!(self, Side::Hub | Side::Shroud)
    }

    /// The color it is drawn in.
    fn color(self, palette: &Palette) -> Color32 {
        match self {
            Side::Hub | Side::HubMaterial => palette.hub,
            Side::Shroud | Side::ShroudMaterial => palette.shroud,
            Side::FrontCasing | Side::RearCasing => palette.volute,
            Side::Center => palette.stator,
        }
    }

    /// Its name, as a menu heading.
    fn title(self) -> &'static str {
        tr(match self {
            Side::Hub => "Hub",
            Side::Shroud => "Shroud",
            Side::HubMaterial => "Hub disk",
            Side::ShroudMaterial => "Shroud cover",
            Side::FrontCasing => "Front casing wall",
            Side::RearCasing => "Rear casing wall",
            Side::Center => "Center line",
        })
    }
}

/// A point of the meridional contour the user can drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    /// Tangent lengths of an automatic wall: they slide along their tangent line.
    HubInlet,
    HubOutlet,
    ShroudInlet,
    ShroudOutlet,
    /// A control point or joint of a wall drawn segment by segment.
    Point(Side, WallPoint),
    LeadingEdgeHub,
    LeadingEdgeShroud,
    /// Inner control point of a Bézier leading edge.
    LeadingEdgeControl(usize),
    /// The axial length: at the hub end of the outlet, or at the hub start of the inlet
    /// when the outlet is the axial reference.
    AxialLength,
    /// The reference point of the axial position: it moves the whole meridian.
    AxialReference,
    /// The shroud point of the inlet line: it moves axially and leans the line.
    InletShroud,
    /// Middle of the trailing edge: it moves radially and sets d₂.
    TrailingEdge,
    /// Centre of the rounded corner of arc segment `.1`: it moves along the bisector
    /// and sets the radius.
    ArcCenter(Side, usize),
}

impl Handle {
    fn label(self) -> &'static str {
        tr(match self {
            Handle::HubInlet => "hub inlet tangent",
            Handle::HubOutlet => "hub outlet tangent",
            Handle::ShroudInlet => "shroud inlet tangent",
            Handle::ShroudOutlet => "shroud outlet tangent",
            Handle::Point(Side::Hub, WallPoint::Joint { .. }) => "hub joint",
            Handle::Point(Side::Hub, WallPoint::Control { .. }) => "hub control point",
            Handle::Point(Side::Shroud, WallPoint::Joint { .. }) => "shroud joint",
            Handle::Point(Side::Shroud, WallPoint::Control { .. }) => "shroud control point",
            Handle::LeadingEdgeHub => "leading edge on the hub",
            Handle::LeadingEdgeShroud => "leading edge on the shroud",
            Handle::LeadingEdgeControl(_) => "leading edge control point",
            Handle::AxialLength => "axial length",
            Handle::TrailingEdge => "trailing edge",
            Handle::AxialReference => "axial reference point",
            Handle::InletShroud => "shroud point of the inlet",
            Handle::ArcCenter(Side::Hub, _) => "hub arc centre",
            Handle::ArcCenter(Side::Shroud, _) => "shroud arc centre",
            Handle::Point(Side::HubMaterial, WallPoint::Joint { .. }) => "hub disk corner",
            Handle::Point(Side::HubMaterial, WallPoint::Control { .. }) => "hub disk control point",
            Handle::Point(Side::ShroudMaterial, WallPoint::Joint { .. }) => "shroud cover corner",
            Handle::Point(Side::ShroudMaterial, WallPoint::Control { .. }) => {
                "shroud cover control point"
            }
            Handle::ArcCenter(Side::HubMaterial, _) => "hub disk arc centre",
            Handle::ArcCenter(Side::ShroudMaterial, _) => "shroud cover arc centre",
            Handle::Point(Side::FrontCasing, WallPoint::Joint { .. }) => "front casing corner",
            Handle::Point(Side::FrontCasing, WallPoint::Control { .. }) => {
                "front casing control point"
            }
            Handle::Point(Side::RearCasing, WallPoint::Joint { .. }) => "rear casing corner",
            Handle::Point(Side::RearCasing, WallPoint::Control { .. }) => {
                "rear casing control point"
            }
            Handle::ArcCenter(Side::FrontCasing, _) => "front casing arc centre",
            Handle::ArcCenter(Side::RearCasing, _) => "rear casing arc centre",
            Handle::Point(Side::Center, WallPoint::Joint { .. }) => "center line corner",
            Handle::Point(Side::Center, WallPoint::Control { .. }) => "center line control point",
            Handle::ArcCenter(Side::Center, _) => "center line arc centre",
        })
    }

    /// The wall point this handle stands for, if it is one.
    fn wall_point(self) -> Option<(Side, WallPoint)> {
        let control = |index| WallPoint::Control { segment: 0, index };
        match self {
            Handle::HubInlet => Some((Side::Hub, control(0))),
            Handle::HubOutlet => Some((Side::Hub, control(1))),
            Handle::ShroudInlet => Some((Side::Shroud, control(0))),
            Handle::ShroudOutlet => Some((Side::Shroud, control(1))),
            Handle::Point(side, at) => Some((side, at)),
            _ => None,
        }
    }
}

/// Every handle of the contour: the tangent handles of automatic walls, the points of
/// drawn ones, the leading edge and the axial length.
pub(crate) fn handles(contour: &MeridionalContour, input: &MeridionalInput) -> Vec<Handle> {
    let mut handles = Vec::new();
    // Walls that follow the center line have no handles of their own: it has them.
    let walls: &[(Side, [Handle; 2])] = if input.design_mode == DesignMode::CenterLine {
        handles.extend(wire_handles(contour, &Side::CENTER));
        &[]
    } else {
        &[
            (Side::Hub, [Handle::HubInlet, Handle::HubOutlet]),
            (Side::Shroud, [Handle::ShroudInlet, Handle::ShroudOutlet]),
        ]
    };
    for &(side, automatic) in walls {
        if side.drawn(input) {
            let wall = side.wall(contour);
            handles.extend(
                wall.points()
                    .into_iter()
                    .map(|(at, _)| Handle::Point(side, at)),
            );
            handles.extend(
                wall.segments()
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.fillet().is_some())
                    .map(|(i, _)| Handle::ArcCenter(side, i)),
            );
        } else {
            handles.extend(automatic);
        }
    }
    handles.extend([Handle::LeadingEdgeHub, Handle::LeadingEdgeShroud]);
    if matches!(contour.leading_edge_shape, LeadingEdgeShape::Bezier { .. }) {
        handles.extend([Handle::LeadingEdgeControl(0), Handle::LeadingEdgeControl(1)]);
    }
    handles.extend([
        Handle::AxialLength,
        Handle::AxialReference,
        Handle::InletShroud,
        Handle::TrailingEdge,
    ]);
    handles
}

/// The handles of the wires `sides` round the flow path: every point, drawn or
/// automatic (dragging one turns its wire into drawn segments), and the centres of
/// rounded corners.
pub(crate) fn wire_handles(contour: &MeridionalContour, sides: &[Side]) -> Vec<Handle> {
    let mut handles = Vec::new();
    for &side in sides {
        let wall = side.wall(contour);
        handles.extend(
            wall.points()
                .into_iter()
                .map(|(at, _)| Handle::Point(side, at)),
        );
        handles.extend(
            wall.segments()
                .iter()
                .enumerate()
                .filter(|(_, s)| s.fillet().is_some())
                .map(|(i, _)| Handle::ArcCenter(side, i)),
        );
    }
    handles
}

/// Where a handle sits, (z, r) in m.
pub(crate) fn handle_position(handle: Handle, contour: &MeridionalContour) -> DVec2 {
    match handle {
        Handle::LeadingEdgeHub => contour.hub_at(contour.leading_edge.0),
        Handle::LeadingEdgeShroud => contour.shroud_at(contour.leading_edge.1),
        Handle::AxialLength => match contour.axial_reference {
            AxialReference::Inlet => contour.hub.end(),
            AxialReference::Outlet => contour.hub.start(),
        },
        Handle::AxialReference => contour.axial_reference_point(),
        Handle::InletShroud => contour.shroud.start(),
        Handle::TrailingEdge => contour.hub.end().lerp(contour.shroud.end(), 0.5),
        Handle::ArcCenter(side, segment) => {
            let wall = side.wall(contour);
            wall.segments()
                .get(segment)
                .and_then(|s| s.fillet())
                .map_or_else(|| wall.end(), |f| f.center)
        }
        Handle::LeadingEdgeControl(i) => contour
            .leading_edge_curve()
            .and_then(|c| c.control_points().get(i + 1).copied())
            .unwrap_or_else(|| contour.hub_at(contour.leading_edge.0)),
        _ => {
            let (side, at) = handle.wall_point().expect("other handles are wall points");
            side.wall(contour)
                .point(at)
                .unwrap_or_else(|| side.wall(contour).end())
        }
    }
}

/// Moves inner control point `index` of a Bézier leading edge to `to`.
pub(crate) fn place_edge_control(
    index: usize,
    to: DVec2,
    contour: &MeridionalContour,
    input: &mut MeridionalInput,
) {
    if let LeadingEdgeShape::Bezier { mut controls } = contour.leading_edge_shape {
        let hub = contour.hub_at(contour.leading_edge.0);
        let shroud = contour.shroud_at(contour.leading_edge.1);
        controls[index] = chord_coordinates(hub, shroud, to);
        input.leading_edge_shape = LeadingEdgeShape::Bezier { controls };
    }
}

/// Moves a wall point to `to` exactly, turning an automatic wall into a drawn one.
pub(crate) fn place(
    side: Side,
    at: WallPoint,
    to: DVec2,
    contour: &MeridionalContour,
    input: &mut MeridionalInput,
) {
    *side.segments(input) = Some(wall::move_point(side.wall(contour), at, to));
}

/// Fraction of the way from `origin` to `corner` of the projection of `pointer`.
fn tangent_fraction(origin: DVec2, corner: DVec2, pointer: DVec2) -> f64 {
    let axis = corner - origin;
    ((pointer - origin).dot(axis) / axis.length_squared()).clamp(0.05, 0.95)
}

/// Arc-length fraction of the contour point nearest to `pointer`.
fn nearest_fraction(at: impl Fn(f64) -> DVec2, pointer: DVec2) -> f64 {
    const SAMPLES: usize = 400;
    (0..=SAMPLES)
        .map(|i| i as f64 / SAMPLES as f64)
        .min_by(|&a, &b| {
            at(a)
                .distance_squared(pointer)
                .total_cmp(&at(b).distance_squared(pointer))
        })
        .unwrap_or(0.0)
        .min(0.9)
}

/// Moves `handle` to `pointer` ((z, r) in m) by changing the matching input.
pub(crate) fn apply_drag(
    handle: Handle,
    pointer: DVec2,
    contour: &MeridionalContour,
    basis: &ImpellerBasis,
    input: &mut MeridionalInput,
) {
    match handle {
        Handle::Point(side, at) => return place(side, at, pointer, contour, input),
        Handle::ArcCenter(side, segment) => {
            let wall = side.wall(contour);
            if let Some(radius) = wall::radius_for_center(wall, segment, pointer) {
                *side.segments(input) = Some(wall::set_radius(wall, segment, radius));
            }
            return;
        }
        Handle::LeadingEdgeControl(i) => return place_edge_control(i, pointer, contour, input),
        _ => {}
    }
    let hub = contour.hub.segments()[0].control_points();
    let shroud = contour.shroud.segments()[0].control_points();
    // The tangents meet at the corner of the axial inlet and the outlet direction
    // (radial, or inclined at ε_s for a semi-axial impeller).
    let corner = |p: &[DVec2]| {
        let (a, b) = (p[1] - p[0], p[2] - p[3]);
        let det = a.perp_dot(b);
        if det.abs() < 1e-15 {
            DVec2::new(p[3].x, p[0].y)
        } else {
            p[0] + a * ((p[3] - p[0]).perp_dot(b) / det)
        }
    };
    let (hub_corner, shroud_corner) = (corner(hub), corner(shroud));
    match handle {
        Handle::HubInlet => {
            input.hub_inlet_tangent = Param::Manual(tangent_fraction(hub[0], hub_corner, pointer));
        }
        Handle::HubOutlet => {
            input.hub_outlet_tangent = Param::Manual(tangent_fraction(hub[3], hub_corner, pointer));
        }
        Handle::ShroudInlet => {
            input.shroud_inlet_tangent =
                Param::Manual(tangent_fraction(shroud[0], shroud_corner, pointer));
        }
        Handle::ShroudOutlet => {
            input.shroud_outlet_tangent =
                Param::Manual(tangent_fraction(shroud[3], shroud_corner, pointer));
        }
        Handle::LeadingEdgeHub => {
            let hub = nearest_fraction(|t| contour.hub_at(t), pointer);
            let (p, shroud) = (
                contour.hub_at(hub),
                contour.shroud_at(contour.leading_edge.1),
            );
            // A hub point that follows the shroud point takes a new offset instead.
            match input.leading_edge_extent {
                EdgeExtent::Independent => input.leading_edge_hub = Param::Manual(hub),
                EdgeExtent::Axial(_) => {
                    input.leading_edge_extent = EdgeExtent::Axial(p.x - shroud.x)
                }
                EdgeExtent::Radial(_) => {
                    input.leading_edge_extent = EdgeExtent::Radial(p.y - shroud.y);
                }
            }
        }
        Handle::LeadingEdgeShroud => {
            input.leading_edge_shroud =
                Param::Manual(nearest_fraction(|t| contour.shroud_at(t), pointer));
        }
        Handle::AxialLength => {
            // From the fixed end: the inlet, or the outlet when it is the reference.
            let length = match contour.axial_reference {
                AxialReference::Inlet => pointer.x - contour.hub.start().x,
                AxialReference::Outlet => contour.hub.end().x - pointer.x,
            };
            let min = basis.outlet_width * 1.1;
            let max = basis.outlet_diameter * 2.0;
            input.axial_length = Param::Manual(length.clamp(min, max));
        }
        Handle::AxialReference => {
            // The whole meridian, drawn points included, moves with it.
            let delta = pointer.x - contour.axial_reference_point().x;
            input.axial_position += delta;
            input.shift(delta);
        }
        Handle::InletShroud => {
            let (hub, shroud) = (contour.hub.start(), contour.shroud.start());
            // The inlet reference keeps the middle of the line in place.
            let tilt = match contour.axial_reference {
                AxialReference::Inlet => 2.0 * (pointer.x - hub.lerp(shroud, 0.5).x),
                AxialReference::Outlet => pointer.x - hub.x,
            };
            let height = (shroud.y - hub.y).max(1e-9);
            input.inlet_tilt = match input.inlet_tilt {
                InletTilt::Angle(_) => InletTilt::Angle((tilt / height).atan()),
                _ => InletTilt::Axial(tilt),
            };
        }
        // The outlet belongs to the main dimensions: see `drag_change`.
        Handle::Point(..)
        | Handle::LeadingEdgeControl(_)
        | Handle::TrailingEdge
        | Handle::ArcCenter(..) => {}
    }
}

/// An edit of the diagram: the geometry inputs, or the main dimensions for the
/// diameters and the outlet. One is made per edit and applied at once, so the size of
/// its variants does not matter.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Change {
    Geometry(ImpellerGeometryInput),
    Main(CentrifugalPumpInput),
    /// Both at once, as the hub end of a semi-axial outlet: L and d₂i.
    Both(Box<(ImpellerGeometryInput, CentrifugalPumpInput)>),
}

impl Change {
    /// Applies the change as part of the current gesture.
    pub(crate) fn apply(self, cx: &mut EditCtx<'_>, id: ComponentId, label: &str) {
        match self {
            Change::Geometry(input) => cx.edit(label, |p| p.set_impeller_geometry(id, input)),
            Change::Main(input) => cx.edit(label, |p| {
                p.set_main_dimensions(id, MainDimensionsInput::CentrifugalPump(input))
            }),
            Change::Both(both) => cx.edit(label, |p| {
                let (geometry, main) = *both;
                p.set_main_dimensions(id, MainDimensionsInput::CentrifugalPump(main))?;
                p.set_impeller_geometry(id, geometry)
            }),
        };
    }
}

/// The inputs the diagram edits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub geometry: &'a ImpellerGeometryInput,
    /// None when the impeller has no pump main dimensions.
    pub main: Option<&'a CentrifugalPumpInput>,
    /// Whether the outlet is semi-axial: its hub end has a diameter d₂i of its own.
    pub semi_axial: bool,
}

impl Inputs<'_> {
    /// The geometry inputs changed by `edit`, if that changes them.
    fn geometry(&self, edit: impl FnOnce(&mut ImpellerGeometryInput)) -> Option<Change> {
        let mut input = self.geometry.clone();
        edit(&mut input);
        (input != *self.geometry).then_some(Change::Geometry(input))
    }

    /// Both inputs changed by their edits, if either changes.
    fn both(
        &self,
        geometry: impl FnOnce(&mut ImpellerGeometryInput),
        main: impl FnOnce(&mut CentrifugalPumpInput),
    ) -> Option<Change> {
        let current = self.main?;
        let (mut g, mut m) = (self.geometry.clone(), current.clone());
        geometry(&mut g);
        main(&mut m);
        (g != *self.geometry || m != *current).then(|| Change::Both(Box::new((g, m))))
    }

    /// The main dimensions changed by `edit`, if that changes them.
    fn main(&self, edit: impl FnOnce(&mut CentrifugalPumpInput)) -> Option<Change> {
        let current = self.main?;
        let mut input = current.clone();
        edit(&mut input);
        (input != *current).then_some(Change::Main(input))
    }
}

/// The change that moves `handle` to `pointer` ((z, r) in m), if any.
pub(crate) fn drag_change(
    handle: Handle,
    pointer: DVec2,
    g: &ImpellerGeometry,
    inputs: Inputs<'_>,
) -> Option<Change> {
    if handle == Handle::TrailingEdge {
        // The shroud end follows the pointer radially: d₂ is its diameter.
        let middle = handle_position(handle, &g.contour);
        let radius = pointer.y + g.contour.shroud.end().y - middle.y;
        let b = &g.basis;
        let d2 = (2.0 * radius).clamp(b.inlet_diameter * 1.05, b.inlet_diameter * 8.0);
        return inputs.main(|m| m.outlet_diameter = Param::Manual(d2));
    }
    if handle == Handle::AxialLength && inputs.semi_axial {
        // The hub end of a semi-axial outlet: L along the axis, d₂i across it.
        let b = &g.basis;
        let d2i = (2.0 * pointer.y).clamp(b.hub_diameter * 1.05, b.outlet_diameter);
        return inputs.both(
            |input| apply_drag(handle, pointer, &g.contour, b, &mut input.meridional),
            |m| m.outlet_hub_diameter = Param::Manual(d2i),
        );
    }
    inputs
        .geometry(|input| apply_drag(handle, pointer, &g.contour, &g.basis, &mut input.meridional))
}

/// Whether the input behind `handle` is set by hand. The points of drawn walls and of
/// a Bézier leading edge always are.
pub(crate) fn is_manual(handle: Handle, inputs: Inputs<'_>) -> bool {
    let m = &inputs.geometry.meridional;
    match handle {
        Handle::HubInlet => !m.hub_inlet_tangent.is_auto(),
        Handle::HubOutlet => !m.hub_outlet_tangent.is_auto(),
        Handle::ShroudInlet => !m.shroud_inlet_tangent.is_auto(),
        Handle::ShroudOutlet => !m.shroud_outlet_tangent.is_auto(),
        Handle::LeadingEdgeHub => {
            !m.leading_edge_hub.is_auto() || m.leading_edge_extent != EdgeExtent::Independent
        }
        Handle::LeadingEdgeShroud => !m.leading_edge_shroud.is_auto(),
        Handle::AxialLength => {
            !m.axial_length.is_auto()
                || inputs.semi_axial
                    && inputs
                        .main
                        .is_some_and(|d| !d.outlet_hub_diameter.is_auto())
        }
        Handle::TrailingEdge => inputs.main.is_some_and(|d| !d.outlet_diameter.is_auto()),
        Handle::AxialReference => m.axial_position.abs() > 1e-12,
        Handle::InletShroud => m.inlet_tilt != InletTilt::Normal,
        // A point of a wire round the flow path may still be automatic.
        Handle::Point(side, _) | Handle::ArcCenter(side, _) => !side.is_wire() || side.drawn(m),
        Handle::LeadingEdgeControl(_) => true,
    }
}

/// The change that returns the input behind `handle` to automatic, when it is manual
/// and has an automatic value.
pub(crate) fn reset_change(handle: Handle, inputs: Inputs<'_>) -> Option<Change> {
    if handle == Handle::TrailingEdge {
        return inputs.main(|d| d.outlet_diameter = Param::Auto);
    }
    // A wire round the flow path goes back to automatic as a whole.
    if let Handle::Point(side, _) | Handle::ArcCenter(side, _) = handle
        && side.is_wire()
    {
        return inputs.geometry(|input| *side.segments(&mut input.meridional) = None);
    }
    if handle == Handle::AxialReference {
        return inputs.geometry(|input| {
            let m = &mut input.meridional;
            m.shift(-m.axial_position);
            m.axial_position = 0.0;
        });
    }
    if handle == Handle::InletShroud {
        return inputs.geometry(|input| input.meridional.inlet_tilt = InletTilt::Normal);
    }
    if handle == Handle::AxialLength && inputs.semi_axial {
        return inputs.both(
            |input| input.meridional.axial_length = Param::Auto,
            |d| d.outlet_hub_diameter = Param::Auto,
        );
    }
    inputs.geometry(|input| {
        let m = &mut input.meridional;
        if handle == Handle::LeadingEdgeHub {
            m.leading_edge_extent = EdgeExtent::Independent;
        }
        let param = match handle {
            Handle::HubInlet => &mut m.hub_inlet_tangent,
            Handle::HubOutlet => &mut m.hub_outlet_tangent,
            Handle::ShroudInlet => &mut m.shroud_inlet_tangent,
            Handle::ShroudOutlet => &mut m.shroud_outlet_tangent,
            Handle::LeadingEdgeHub => &mut m.leading_edge_hub,
            Handle::LeadingEdgeShroud => &mut m.leading_edge_shroud,
            Handle::AxialLength => &mut m.axial_length,
            Handle::Point(..)
            | Handle::LeadingEdgeControl(_)
            | Handle::TrailingEdge
            | Handle::AxialReference
            | Handle::InletShroud
            | Handle::ArcCenter(..) => return,
        };
        *param = Param::Auto;
    })
}

/// What the meridional diagram draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Display {
    pub stream_lines: bool,
    pub quasi_orthogonals: bool,
    pub potential: bool,
    /// What the potential flow shows: stream lines, iso-lines and a color map of c_m,
    /// velocity vectors; and whether it takes the blockage of the blades into account.
    pub potential_view: PotentialView,
    /// The main dimensions d_n, d₁, d₂, b₂ and L. The point under the pointer is
    /// dimensioned either way.
    pub dimensions: bool,
    /// The point of largest curvature of hub and shroud.
    pub curvature: bool,
    /// Circles as wide as the channel normal to the flow.
    pub area_circles: bool,
    /// The diffuser or volute that follows.
    pub adjacent: bool,
    pub legend: bool,
    /// Cross-section area and wall curvature along the channel, under the diagram.
    pub charts: bool,
    /// Grid that dragged points snap to, m; 0 leaves them free.
    pub snap: f64,
    /// A dragged point takes the z or r of another point near it.
    pub snap_points: bool,
    /// A dragged point lands on a line through two other points near it.
    pub snap_lines: bool,
    /// The material wires are drawn, faintly unless they are being edited.
    pub material_lines: bool,
    /// What the handles edit.
    pub page: Page,
}

/// What the potential flow shows over the diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PotentialView {
    pub stream_lines: bool,
    pub iso_lines: bool,
    pub map: bool,
    pub vectors: bool,
    /// Solve with the blockage of the blade thickness.
    pub blades: bool,
}

impl PotentialView {
    const DEFAULT: PotentialView = PotentialView {
        stream_lines: true,
        iso_lines: false,
        map: false,
        vectors: false,
        blades: false,
    };
}

/// What the handles of the diagram edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    /// Hub, shroud and blade edges.
    FlowPath,
    /// The closed material of hub disk and shroud cover.
    Materials,
    /// The casing walls of the side chambers.
    Chambers,
}

impl Display {
    /// The walls the handles and the menus edit; on the flow path, the center line
    /// instead of hub and shroud when they follow it (`center`).
    pub(crate) fn edited(&self, center: bool) -> &'static [Side] {
        match self.page {
            Page::FlowPath if center => &Side::CENTER,
            Page::FlowPath => &Side::WALLS,
            Page::Materials => &Side::MATERIALS,
            Page::Chambers => &Side::CASINGS,
        }
    }
}

impl Display {
    const VIEW: Display = Display {
        stream_lines: true,
        quasi_orthogonals: false,
        potential: false,
        potential_view: PotentialView::DEFAULT,
        dimensions: false,
        curvature: false,
        area_circles: false,
        adjacent: true,
        legend: true,
        charts: false,
        snap: 0.0,
        snap_points: false,
        snap_lines: false,
        material_lines: false,
        page: Page::FlowPath,
    };
    pub(crate) const EDITOR: Display = Display {
        stream_lines: false,
        quasi_orthogonals: false,
        potential: false,
        potential_view: PotentialView::DEFAULT,
        dimensions: true,
        curvature: false,
        area_circles: false,
        adjacent: false,
        legend: false,
        charts: false,
        snap: 0.0,
        snap_points: true,
        snap_lines: false,
        material_lines: true,
        page: Page::FlowPath,
    };
}

/// How the diagram should frame the drawing on the next frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    /// Hub, shroud and their dimensions.
    Meridian,
    /// Everything drawn, adjacent components included.
    All,
}

/// A dimension of the diagram.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Dimension {
    AxialLength,
    OutletWidth,
    /// The diameters of the main dimensions: d_n and d₁ at the inlet, d₂ at the outlet.
    HubDiameter,
    InletDiameter,
    OutletDiameter,
    /// Radius of the rounded corner of an arc segment of a wall.
    ArcRadius(Side, usize),
    /// Axial position of a handle, from the inlet plane.
    Z(Handle),
    /// Radius of a handle, from the axis.
    R(Handle),
}

impl Dimension {
    fn label(self) -> String {
        match self {
            Dimension::AxialLength => tr("Axial length L").to_owned(),
            Dimension::OutletWidth => tr("Outlet width b₂").to_owned(),
            Dimension::HubDiameter => tr("Hub diameter d_n").to_owned(),
            Dimension::InletDiameter => tr("Eye diameter d₁").to_owned(),
            Dimension::OutletDiameter => tr("Outlet diameter d₂").to_owned(),
            Dimension::ArcRadius(..) => tr("Corner radius R").to_owned(),
            Dimension::Z(h) => tf("z of the {}", &[&h.label()]),
            Dimension::R(h) => tf("r of the {}", &[&h.label()]),
        }
    }

    /// The symbol written before the value, for the main dimensions.
    fn symbol(self) -> Option<&'static str> {
        match self {
            Dimension::AxialLength => Some("L"),
            Dimension::OutletWidth => Some("b₂"),
            Dimension::HubDiameter => Some("d_n"),
            Dimension::InletDiameter => Some("d₁"),
            Dimension::OutletDiameter => Some("d₂"),
            Dimension::ArcRadius(..) => Some("R"),
            Dimension::Z(_) | Dimension::R(_) => None,
        }
    }

    /// Whether it is one of the main dimensions of the impeller, set in their step.
    fn is_main(self) -> bool {
        matches!(
            self,
            Dimension::OutletWidth
                | Dimension::HubDiameter
                | Dimension::InletDiameter
                | Dimension::OutletDiameter
        )
    }

    /// Whether the value can be typed: the main dimensions and the points that move
    /// freely.
    fn editable(self) -> bool {
        match self {
            Dimension::Z(h) | Dimension::R(h) => {
                h.wall_point().is_some() || matches!(h, Handle::LeadingEdgeControl(_))
            }
            _ => true,
        }
    }

    /// The value as drawn: the symbol of a main dimension, then the length.
    pub(crate) fn text(self, value: f64, units: &DisplayUnits) -> String {
        let length = units.show(QuantityKind::Length, value);
        match self.symbol() {
            Some(symbol) => format!("{symbol} {length}"),
            None => length,
        }
    }
}

/// A dimension as drawn: the dimension line, its extension lines and its value (m).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DimensionLine {
    pub dimension: Dimension,
    pub from: DVec2,
    pub to: DVec2,
    pub extensions: Vec<[DVec2; 2]>,
    pub value: f64,
}

/// The dimensions to draw: with `permanent`, the main dimensions L, b₂, d_n, d₁ and d₂;
/// and the axial position and radius of the `hovered` point. Axial dimensions stack
/// under the axis, d_n and d₁ left of the inlet, d₂ and the radii right of the
/// impeller (our own layout).
pub(crate) fn dimension_lines(
    contour: &MeridionalContour,
    basis: &ImpellerBasis,
    permanent: bool,
    hovered: Option<Handle>,
) -> Vec<DimensionLine> {
    let gap = 0.07 * basis.outlet_diameter / 2.0;
    let (hub, shroud) = (&contour.hub, &contour.shroud);
    let z_max = hub
        .sample(120)
        .iter()
        .chain(&shroud.sample(120))
        .map(|p| p.x)
        .fold(contour.hub.end().x, f64::max);
    let z_min = hub.start().x.min(shroud.start().x);
    // Axial dimensions start at the hub point of the inlet.
    let z0 = hub.start().x;
    let mut lines = Vec::new();
    let mut rows = 0;
    let mut columns = 0;
    let mut axial = |lines: &mut Vec<DimensionLine>, dimension, p: DVec2| {
        rows += 1;
        let y = -gap * rows as f64;
        lines.push(DimensionLine {
            dimension,
            from: DVec2::new(z0, y),
            to: DVec2::new(p.x, y),
            extensions: vec![
                [DVec2::new(z0, 0.0), DVec2::new(z0, y)],
                [p, DVec2::new(p.x, y)],
            ],
            value: p.x - z0,
        });
    };
    // From the axis up to `p`, at axial position `x`.
    let radius = |dimension, p: DVec2, x: f64, value: f64| DimensionLine {
        dimension,
        from: DVec2::new(x, 0.0),
        to: DVec2::new(x, p.y),
        extensions: vec![[p, DVec2::new(x, p.y)]],
        value,
    };
    let mut right = |lines: &mut Vec<DimensionLine>, dimension, p: DVec2, value: f64| {
        columns += 1;
        lines.push(radius(dimension, p, z_max + gap * columns as f64, value));
    };
    if permanent {
        axial(&mut lines, Dimension::AxialLength, hub.end());
        // b₂ normal to the outlet direction ε_s: on a semi-axial impeller the two ends
        // are offset along that direction too, so their distance is not b₂.
        let eps = basis.outlet_inclination;
        let along = DVec2::new(eps.sin(), eps.cos());
        let across = DVec2::new(eps.cos(), -eps.sin());
        let a = shroud.end() + along * gap;
        let b = a + across * basis.outlet_width;
        lines.push(DimensionLine {
            dimension: Dimension::OutletWidth,
            from: a,
            to: b,
            extensions: vec![[shroud.end(), a], [hub.end(), b]],
            value: basis.outlet_width,
        });
        right(
            &mut lines,
            Dimension::OutletDiameter,
            shroud.end(),
            basis.outlet_diameter,
        );
        // The smaller inlet diameter nearer the inlet; no d_n without a hub.
        let inlet = [
            (Dimension::HubDiameter, hub.start(), basis.hub_diameter),
            (
                Dimension::InletDiameter,
                shroud.start(),
                basis.inlet_diameter,
            ),
        ];
        let mut column = 0;
        for (dimension, p, value) in inlet {
            if p.y > 1e-9 {
                column += 1;
                lines.push(radius(dimension, p, z_min - gap * column as f64, value));
            }
        }
    }
    if let Some(Handle::ArcCenter(side, segment)) = hovered
        && let Some(f) = side
            .wall(contour)
            .segments()
            .get(segment)
            .and_then(|s| s.fillet())
    {
        // From the centre to the middle of the arc.
        let middle = f.tangent_points[0].lerp(f.tangent_points[1], 0.5);
        let to = f.center + (middle - f.center).normalize_or_zero() * f.radius;
        lines.push(DimensionLine {
            dimension: Dimension::ArcRadius(side, segment),
            from: f.center,
            to,
            extensions: Vec::new(),
            value: f.radius,
        });
    } else if let Some(h) = hovered
        && !matches!(
            h,
            Handle::AxialLength | Handle::TrailingEdge | Handle::AxialReference
        )
    {
        let p = handle_position(h, contour);
        axial(&mut lines, Dimension::Z(h), p);
        right(&mut lines, Dimension::R(h), p, p.y);
    }
    lines
}

/// The change that sets dimension `dimension` to `value` (m), if it changes anything.
fn set_dimension(
    dimension: Dimension,
    value: f64,
    g: &ImpellerGeometry,
    inputs: Inputs<'_>,
) -> Option<Change> {
    let contour = &g.contour;
    let length = value.max(1e-4);
    match dimension {
        Dimension::AxialLength => {
            return inputs.geometry(|input| {
                input.meridional.axial_length =
                    Param::Manual(value.max(g.basis.outlet_width * 1.01));
            });
        }
        Dimension::OutletWidth => {
            return inputs.main(|m| m.outlet_width = Param::Manual(length));
        }
        Dimension::HubDiameter => {
            return inputs.main(|m| m.hub_diameter = Param::Manual(value.max(0.0)));
        }
        Dimension::InletDiameter => {
            return inputs.main(|m| m.inlet_diameter = Param::Manual(length));
        }
        Dimension::OutletDiameter => {
            return inputs.main(|m| m.outlet_diameter = Param::Manual(length));
        }
        Dimension::ArcRadius(side, segment) => {
            return inputs.geometry(|input| {
                let wall = side.wall(contour);
                *side.segments(&mut input.meridional) =
                    Some(wall::set_radius(wall, segment, value.max(0.0)));
            });
        }
        Dimension::Z(_) | Dimension::R(_) => {}
    }
    let moved = |h: Handle, z: Option<f64>, r: Option<f64>| {
        let p = handle_position(h, contour);
        DVec2::new(z.unwrap_or(p.x), r.unwrap_or(p.y))
    };
    let (handle, to) = match dimension {
        Dimension::Z(h) => (h, moved(h, Some(contour.hub.start().x + value), None)),
        Dimension::R(h) => (h, moved(h, None, Some(value.max(0.0)))),
        _ => return None,
    };
    inputs.geometry(|input| {
        let m = &mut input.meridional;
        match handle {
            Handle::LeadingEdgeControl(i) => place_edge_control(i, to, contour, m),
            h => {
                if let Some((side, at)) = h.wall_point() {
                    place(side, at, to, contour, m);
                }
            }
        }
    })
}

/// The distance being measured: a line between the two points and its length with
/// the axial and radial parts.
fn paint_measure(
    ui: &Ui,
    transform: &PlotTransform,
    a: DVec2,
    b: DVec2,
    scale: f64,
    units: &DisplayUnits,
) {
    let painter = ui.painter_at(*transform.frame());
    let color = Palette::of(ui).handle_active;
    let screen =
        |p: DVec2| transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
    let (sa, sb) = (screen(a), screen(b));
    painter.line_segment([sa, sb], egui::Stroke::new(1.5, color));
    for p in [sa, sb] {
        painter.circle_stroke(p, 3.0, egui::Stroke::new(1.5, color));
    }
    let length = |v: f64| units.show(QuantityKind::Length, v);
    let text = format!(
        "{}\nΔz {}   Δr {}",
        length(a.distance(b)),
        length(b.x - a.x),
        length(b.y - a.y)
    );
    let galley = painter.layout(text, egui::FontId::proportional(12.0), color, f32::INFINITY);
    let rect = egui::Rect::from_min_size(sb + egui::vec2(10.0, 6.0), galley.size()).expand(3.0);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    painter.galley(rect.min + egui::vec2(3.0, 3.0), galley, color);
}

/// Whether the value of a dimension line from `a` to `b` (screen) is turned to read
/// along it, as on a drawing: on the lines that are nearly vertical.
fn reads_upwards(a: egui::Pos2, b: egui::Pos2) -> bool {
    (b - a).normalized().x.abs() < 0.3
}

/// Where the value of a dimension line from `a` to `b` (screen) goes: in the middle,
/// or past the end when it does not fit between the arrows.
fn value_rect(a: egui::Pos2, b: egui::Pos2, text: egui::Vec2) -> egui::Rect {
    let size = if reads_upwards(a, b) {
        egui::vec2(text.y + 2.0, text.x + 6.0)
    } else {
        text + egui::vec2(6.0, 2.0)
    };
    let along = (b - a).normalized();
    // How far the value reaches along the line.
    let extent = size.x * along.x.abs() + size.y * along.y.abs();
    let center = if extent + 16.0 < (b - a).length() {
        a + (b - a) * 0.5
    } else {
        b + along * (extent / 2.0 + 4.0)
    };
    egui::Rect::from_center_size(center, size)
}

/// Dimensions over the plot: extension lines, the dimension line with an arrow at each
/// end and the value on a background, green when it can be typed.
fn paint_dimensions(
    ui: &Ui,
    transform: &PlotTransform,
    dimensions: &[DimensionLine],
    texts: &[String],
    hovered: Option<Dimension>,
    scale: f64,
) {
    let painter = ui.painter_at(*transform.frame());
    let palette = Palette::of(ui);
    let visuals = ui.visuals();
    let screen =
        |p: DVec2| transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
    let thin = egui::Stroke::new(0.6, palette.guide);
    let solid = egui::Stroke::new(1.0, visuals.text_color());
    for (d, text) in dimensions.iter().zip(texts) {
        for [a, b] in &d.extensions {
            painter.line_segment([screen(*a), screen(*b)], thin);
        }
        let (a, b) = (screen(d.from), screen(d.to));
        painter.line_segment([a, b], solid);
        let along = (b - a).normalized();
        let across = along.rot90();
        for (tip, back) in [(a, along), (b, -along)] {
            let base = tip + back * 7.0;
            painter.add(egui::Shape::convex_polygon(
                vec![tip, base + across * 2.5, base - across * 2.5],
                visuals.text_color(),
                egui::Stroke::NONE,
            ));
        }
        let color = if hovered == Some(d.dimension) {
            palette.handle_active
        } else if d.dimension.editable() {
            palette.handle
        } else {
            visuals.text_color()
        };
        let galley = painter.layout_no_wrap(text.clone(), egui::FontId::proportional(12.0), color);
        let rect = value_rect(a, b, galley.size());
        painter.rect_filled(rect, 2.0, visuals.extreme_bg_color);
        if hovered == Some(d.dimension) {
            painter.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, color),
                egui::StrokeKind::Inside,
            );
        }
        let half = galley.size() / 2.0;
        if reads_upwards(a, b) {
            // Turned a quarter anticlockwise about its top-left corner.
            let corner = rect.center() + egui::vec2(-half.y, half.x);
            painter.add(
                egui::epaint::TextShape::new(corner, galley, color)
                    .with_angle(-std::f32::consts::FRAC_PI_2),
            );
        } else {
            painter.galley(rect.center() - half, galley, color);
        }
    }
}

/// Screen distance within which a dragged point snaps, px.
const SNAP_REACH: f32 = 6.0;

/// `pointer` ((z, r) in m) snapped to the points `others`: their z or r when the
/// pointer passes within reach on screen, else the nearest line through two of them.
/// Returns the point and the guide lines to show.
fn snap_to(
    pointer: DVec2,
    others: &[DVec2],
    display: Display,
    transform: &PlotTransform,
    scale: f64,
) -> (DVec2, Vec<[DVec2; 2]>) {
    let screen =
        |p: DVec2| transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
    let at = screen(pointer);
    let mut snapped = pointer;
    let mut guides = Vec::new();
    if display.snap_points {
        let nearest = |axis: fn(egui::Pos2) -> f32| {
            others
                .iter()
                .map(|&o| (o, (axis(screen(o)) - axis(at)).abs()))
                .filter(|&(_, d)| d < SNAP_REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(o, _)| o)
        };
        let column = nearest(|p| p.x);
        let row = nearest(|p| p.y);
        if let Some(o) = column {
            snapped.x = o.x;
        }
        if let Some(o) = row {
            snapped.y = o.y;
        }
        guides.extend(column.map(|o| [o, snapped]));
        guides.extend(row.map(|o| [o, snapped]));
    }
    if display.snap_lines && guides.is_empty() {
        let mut best: Option<(f32, DVec2, DVec2)> = None;
        for (i, &a) in others.iter().enumerate() {
            for &b in &others[i + 1..] {
                let (sa, sb) = (screen(a), screen(b));
                let ab = sb - sa;
                if ab.length() < 2.0 * SNAP_REACH {
                    continue;
                }
                let distance = (at - sa).dot(ab.rot90().normalized()).abs();
                if distance < SNAP_REACH && best.is_none_or(|(d, ..)| distance < d) {
                    best = Some((distance, a, b));
                }
            }
        }
        if let Some((_, a, b)) = best {
            let ab = b - a;
            snapped = a + ab * ((pointer - a).dot(ab) / ab.length_squared());
            // The guide runs from the farther point through the snapped one.
            let from = if a.distance(snapped) > b.distance(snapped) {
                a
            } else {
                b
            };
            guides.push([from, snapped]);
        }
    }
    (snapped, guides)
}

/// The guide lines of the snap acting on the dragged point, dotted.
fn paint_guides(ui: &Ui, transform: &PlotTransform, guides: &[[DVec2; 2]], scale: f64) {
    let painter = ui.painter_at(*transform.frame());
    let screen =
        |p: DVec2| transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
    let stroke = egui::Stroke::new(1.0, ui.visuals().error_fg_color);
    for [a, b] in guides {
        painter.extend(egui::Shape::dotted_line(
            &[screen(*a), screen(*b)],
            stroke.color,
            4.0,
            1.0,
        ));
    }
}

/// A color for `t` ∈ [0, 1], from slow (deep blue) to fast (warm red) through teal,
/// green and yellow (our ramp).
fn ramp(t: f32) -> Color32 {
    const STOPS: [(u8, u8, u8); 5] = [
        (40, 60, 150),
        (30, 150, 170),
        (90, 180, 90),
        (230, 200, 60),
        (210, 70, 50),
    ];
    let t = t.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let i = (t.floor() as usize).min(STOPS.len() - 2);
    let f = t - i as f32;
    let (a, b) = (STOPS[i], STOPS[i + 1]);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * f).round() as u8;
    Color32::from_rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// The meridional velocity of the potential flow over the plot: a color map, iso-lines
/// and vectors as `view` asks, with a color bar of the range in the top-left corner.
fn paint_potential(
    ui: &Ui,
    transform: &PlotTransform,
    flow: &PotentialFlow,
    view: PotentialView,
    scale: f64,
    units: &DisplayUnits,
) {
    if !(view.map || view.iso_lines || view.vectors) {
        return;
    }
    let painter = ui.painter_at(*transform.frame());
    let screen =
        |p: DVec2| transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
    let (ns, nj) = (flow.stations, flow.across);
    let index = |i: usize, j: usize| i * nj + j;
    // The range of the channel proper, without the extensions.
    let (first, last) = flow.channel;
    let range = (first..=last).flat_map(|i| (0..nj).map(move |j| flow.cm[i * nj + j]));
    let (lo, hi) = range.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
        (lo.min(c), hi.max(c))
    });
    let span = (hi - lo).max(1e-9);
    let level = |c: f64| ((c - lo) / span) as f32;
    if view.map {
        let mut mesh = egui::epaint::Mesh::default();
        for k in 0..ns * nj {
            let color = ramp(level(flow.cm[k])).gamma_multiply(0.75);
            mesh.colored_vertex(screen(flow.nodes[k]), color);
        }
        for i in 0..ns - 1 {
            for j in 0..nj - 1 {
                let (a, b, c, d) = (
                    index(i, j),
                    index(i + 1, j),
                    index(i + 1, j + 1),
                    index(i, j + 1),
                );
                mesh.add_triangle(a as u32, b as u32, c as u32);
                mesh.add_triangle(a as u32, c as u32, d as u32);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    if view.iso_lines {
        // Marching squares on each cell, at ten levels across the range.
        let stroke = egui::Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.7));
        for n in 1..10 {
            let value = lo + span * f64::from(n) / 10.0;
            for i in 0..ns - 1 {
                for j in 0..nj - 1 {
                    let corners = [
                        index(i, j),
                        index(i + 1, j),
                        index(i + 1, j + 1),
                        index(i, j + 1),
                    ];
                    let mut crossings = Vec::with_capacity(4);
                    for e in 0..4 {
                        let (a, b) = (corners[e], corners[(e + 1) % 4]);
                        let (ca, cb) = (flow.cm[a], flow.cm[b]);
                        if (ca - value) * (cb - value) < 0.0 {
                            let t = (value - ca) / (cb - ca);
                            crossings.push(flow.nodes[a].lerp(flow.nodes[b], t));
                        }
                    }
                    for pair in crossings.as_chunks::<2>().0 {
                        painter.line_segment([screen(pair[0]), screen(pair[1])], stroke);
                    }
                }
            }
        }
    }
    if view.vectors {
        // Every sixth node along and every fourth across; the fastest 1.5 cells long.
        let reach = flow
            .nodes
            .windows(2)
            .map(|w| screen(w[0]).distance(screen(w[1])))
            .fold(0.0_f32, f32::max)
            .min(30.0);
        for i in (0..ns).step_by(6) {
            for j in (1..nj - 1).step_by(4) {
                let k = index(i, j);
                let v = flow.velocity[k];
                let from = screen(flow.nodes[k]);
                let tip = screen(flow.nodes[k] + v.normalize_or_zero() * 1e-3);
                let along = (tip - from).normalized();
                let length = reach * 1.5 * (v.length() / hi.max(1e-9)) as f32;
                let to = from + along * length;
                // Over the color map the arrows take the text color to stand out.
                let color = if view.map {
                    ui.visuals().text_color()
                } else {
                    ramp(level(v.length()))
                };
                let stroke = egui::Stroke::new(1.4, color);
                painter.line_segment([from, to], stroke);
                let back = to - along * 5.0;
                let side = along.rot90() * 2.5;
                painter.add(egui::Shape::convex_polygon(
                    vec![to, back + side, back - side],
                    color,
                    egui::Stroke::NONE,
                ));
            }
        }
    }
    // The color bar: slow at the bottom, fast at the top.
    if view.map || view.vectors {
        let frame = transform.frame();
        let bar =
            egui::Rect::from_min_size(frame.min + egui::vec2(12.0, 12.0), egui::vec2(10.0, 90.0));
        let mut mesh = egui::epaint::Mesh::default();
        let steps = 12;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let y = bar.bottom() - t * bar.height();
            mesh.colored_vertex(egui::pos2(bar.left(), y), ramp(t));
            mesh.colored_vertex(egui::pos2(bar.right(), y), ramp(t));
            if s > 0 {
                let b = (2 * s) as u32;
                mesh.add_triangle(b - 2, b - 1, b);
                mesh.add_triangle(b - 1, b + 1, b);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        let text = ui.visuals().text_color();
        let font = egui::FontId::proportional(11.0);
        let show = |c: f64| units.show(QuantityKind::Velocity, c);
        painter.text(
            bar.right_top() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_TOP,
            show(hi),
            font.clone(),
            text,
        );
        painter.text(
            bar.right_bottom() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_BOTTOM,
            show(lo),
            font,
            text,
        );
    }
}

/// Where walls cross, ringed in the error color.
fn paint_crossings(
    ui: &Ui,
    transform: &PlotTransform,
    crossings: &[tp_design::impeller::crossings::Crossing],
    scale: f64,
) {
    let painter = ui.painter_at(*transform.frame());
    let stroke = egui::Stroke::new(2.0, ui.visuals().error_fg_color);
    for c in crossings {
        let at = transform.position_from_point(&PlotPoint::new(c.at.x * scale, c.at.y * scale));
        painter.circle_stroke(at, 7.0, stroke);
    }
}

/// The pointer position in the bottom-right corner of the diagram.
fn paint_pointer(ui: &Ui, transform: &PlotTransform, p: DVec2, units: &DisplayUnits) {
    let frame = *transform.frame();
    let painter = ui.painter_at(frame);
    let visuals = ui.visuals();
    let text = format!(
        "z {}   r {}",
        units.show(QuantityKind::Length, p.x),
        units.show(QuantityKind::Length, p.y)
    );
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::monospace(11.0),
        visuals.weak_text_color(),
    );
    let rect = egui::Rect::from_min_size(
        frame.right_bottom() - galley.size() - egui::vec2(10.0, 8.0),
        galley.size(),
    );
    painter.rect_filled(rect.expand(3.0), 2.0, visuals.extreme_bg_color);
    painter.galley(rect.min, galley, visuals.weak_text_color());
}

/// The color of what `handle` shapes.
fn handle_color(handle: Handle, palette: &Palette) -> Color32 {
    match handle {
        Handle::HubInlet | Handle::HubOutlet | Handle::AxialLength => palette.hub,
        Handle::Point(side, _) | Handle::ArcCenter(side, _) => side.color(palette),
        Handle::ShroudInlet | Handle::ShroudOutlet => palette.shroud,
        Handle::LeadingEdgeHub | Handle::LeadingEdgeShroud | Handle::LeadingEdgeControl(_) => {
            palette.leading_edge
        }
        Handle::TrailingEdge => palette.trailing_edge,
        Handle::AxialReference => palette.stator,
        Handle::InletShroud => palette.shroud,
    }
}

/// The handles over the plot, in the color of what they shape: hollow while their
/// input is automatic, filled once it is set by hand, squares at the joints of drawn
/// walls.
fn paint_handles(
    ui: &Ui,
    transform: &PlotTransform,
    handles: &[(Handle, DVec2)],
    active: Option<Handle>,
    inputs: Inputs<'_>,
    scale: f64,
) {
    let painter = ui.painter_at(*transform.frame());
    let palette = Palette::of(ui);
    let background = ui.visuals().extreme_bg_color;
    for &(handle, p) in handles {
        let at = transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
        let current = active == Some(handle);
        let color = if current {
            palette.handle_active
        } else {
            handle_color(handle, &palette)
        };
        let radius = if current { 6.0 } else { 4.5 };
        let fill = if is_manual(handle, inputs) {
            color
        } else {
            background
        };
        let stroke = egui::Stroke::new(1.8, color);
        if matches!(handle, Handle::ArcCenter(..)) {
            painter.circle_stroke(at, radius - 1.0, stroke);
            for d in [egui::vec2(radius, 0.0), egui::vec2(0.0, radius)] {
                painter.line_segment([at - d, at + d], stroke);
            }
        } else if matches!(handle, Handle::Point(_, WallPoint::Joint { .. })) {
            let square = egui::Rect::from_center_size(at, egui::Vec2::splat(2.0 * radius));
            painter.rect(square, 0.0, fill, stroke, egui::StrokeKind::Middle);
        } else {
            painter.circle(at, radius, fill, stroke);
        }
    }
}

/// A curve drawn for comparison.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reference {
    pub name: String,
    /// (z, r) in m.
    pub points: Vec<DVec2>,
}

/// `p` on the grid of step `step` (m); unchanged when the step is 0.
fn snap(p: DVec2, step: f64) -> DVec2 {
    if step > 0.0 {
        (p / step).round() * step
    } else {
        p
    }
}

/// The points of a wall as text, one "z<TAB>r" line each in display units, to paste
/// into a spreadsheet or another program.
pub(crate) fn points_text(wall: &Wall, units: &DisplayUnits) -> String {
    let scale = units.to_display(QuantityKind::Length, 1.0);
    let unit = units.get(QuantityKind::Length).symbol;
    let mut text = format!("# z [{unit}]\tr [{unit}]\n");
    for p in wall.sample(100) {
        text.push_str(&format!("{:.6}\t{:.6}\n", p.x * scale, p.y * scale));
    }
    text
}

/// Points typed or pasted as text: one "z r" pair a line in display units, separated
/// by spaces, tabs or semicolons, or by a comma when the numbers use a decimal point.
/// A decimal comma works with the other separators. Lines starting with # are notes.
pub(crate) fn parse_points(text: &str, units: &DisplayUnits) -> Vec<DVec2> {
    let scale = units.to_display(QuantityKind::Length, 1.0);
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let mut fields: Vec<&str> = line
                .split(|c: char| c.is_whitespace() || c == ';')
                .filter(|f| !f.is_empty())
                .collect();
            if fields.len() == 1 {
                fields = line.split(',').collect();
            }
            let [z, r] = fields[..] else {
                return None;
            };
            let number = |f: &str| f.trim().replace(',', ".").parse::<f64>().ok();
            Some(DVec2::new(number(z)? / scale, number(r)? / scale))
        })
        .collect()
}

/// Points of a circle.
fn circle(center: DVec2, radius: f64) -> Vec<DVec2> {
    (0..=48)
        .map(|i| {
            let a = i as f64 / 48.0 * std::f64::consts::TAU;
            center + DVec2::new(a.cos(), a.sin()) * radius
        })
        .collect()
}

/// What a potential flow was solved for: the geometry revision, the impeller and
/// whether with blade blockage.
type PotentialKey = (u64, ComponentId, bool);

/// State of the meridional view and its editor between frames.
#[derive(Debug)]
pub(crate) struct MeridionalView {
    dragging: Option<Handle>,
    /// What the context menu was opened on.
    menu: Option<MenuTarget>,
    /// Plot transform of the last frame, used to find the handle under the pointer
    /// before the plot is drawn.
    transform: Option<PlotTransform>,
    pub(crate) view: Display,
    pub(crate) editor: Display,
    /// Framing asked for the next frame.
    pub(crate) fit: Option<Fit>,
    /// The dimension being typed.
    dimension_edit: Option<DimensionEdit>,
    /// Pointer position in the diagram, (z, r) in m.
    pub(crate) pointer: Option<DVec2>,
    /// Whether the editor has framed the meridian since it was last opened.
    pub(crate) framed: bool,
    /// The distance being measured with Alt and a drag, from and to (z, r) in m.
    measure: Option<(DVec2, DVec2)>,
    /// Guide lines of the snap acting on the dragged point, (z, r) in m.
    guides: Vec<[DVec2; 2]>,
    /// Curves drawn for comparison: pasted points or copies of a wall.
    pub(crate) references: Vec<Reference>,
    /// Points typed or pasted for a new reference curve.
    pub(crate) reference_text: String,
    /// Potential flow of the last geometry it was solved for.
    potential: Option<(PotentialKey, Result<PotentialFlow, String>)>,
}

impl Default for MeridionalView {
    fn default() -> Self {
        Self {
            dragging: None,
            menu: None,
            transform: None,
            view: Display::VIEW,
            editor: Display::EDITOR,
            fit: None,
            dimension_edit: None,
            pointer: None,
            framed: false,
            measure: None,
            guides: Vec::new(),
            references: Vec::new(),
            reference_text: String::new(),
            potential: None,
        }
    }
}

fn line(points: impl IntoIterator<Item = DVec2>, scale: f64) -> PlotPoints<'static> {
    points
        .into_iter()
        .map(|p| [p.x * scale, p.y * scale])
        .collect()
}

/// What the diagram needs besides the view state.
pub(crate) struct Scene<'a> {
    pub id: ComponentId,
    pub g: &'a ImpellerGeometry,
    pub current: &'a ImpellerGeometryInput,
    pub stator: Option<&'a tp_design::diffuser::DiffuserDesign>,
    pub volute: Option<&'a tp_design::volute::VoluteGeometry>,
    pub potential: Option<&'a Result<PotentialFlow, String>>,
    pub display: Display,
    pub size: egui::Vec2,
}

impl MeridionalView {
    /// Plot transform of the last frame.
    #[cfg(test)]
    pub(crate) fn transform(&self) -> Option<&PlotTransform> {
        self.transform.as_ref()
    }

    /// The distance being measured.
    #[cfg(test)]
    pub(crate) fn measured(&self) -> Option<(DVec2, DVec2)> {
        self.measure
    }

    /// The potential flow of `g`, solved again only when the geometry changes.
    pub(crate) fn potential(
        &mut self,
        revision: u64,
        id: ComponentId,
        g: &ImpellerGeometry,
    ) -> Option<&Result<PotentialFlow, String>> {
        let blades = self.view.potential_view.blades;
        let key = (revision, id, blades);
        if self.potential.as_ref().is_none_or(|(k, _)| *k != key) {
            let mesh = PotentialMesh::default();
            let solution = if blades {
                potential::solve_with_blades(&g.contour, g.basis.flow, mesh, &g.blade)
            } else {
                potential::solve(&g.contour, g.basis.flow, mesh)
            }
            .map_err(|e| e.to_string());
            self.potential = Some((key, solution));
        }
        self.potential.as_ref().map(|(_, s)| s)
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        id: ComponentId,
        stator: Option<ComponentId>,
        volute: Option<ComponentId>,
    ) {
        let derived = cx.derived;
        let stator = stator.and_then(|s| derived.diffuser(s));
        let volute = volute.and_then(|v| derived.volute(v));
        let Some(results) = derived.impeller(id) else {
            return;
        };
        let g = match &results.geometry {
            Ok(g) => g,
            Err(e) => {
                unavailable(ui, e);
                return;
            }
        };
        let units = cx.units();
        // Back from the editor, whose framing includes its dimensions.
        if self.framed {
            self.fit = Some(Fit::All);
        }
        self.framed = false;

        ui.horizontal_wrapped(|ui| {
            let d = &mut self.view;
            ui.checkbox(&mut d.stream_lines, tr("stream lines"));
            ui.checkbox(&mut d.quasi_orthogonals, tr("quasi-orthogonals"));
            ui.checkbox(&mut d.dimensions, tr("dimensions"));
            ui.checkbox(&mut d.potential, tr("potential flow"))
                .on_hover_text(tr(
                    "Two-dimensional potential flow between hub and shroud: its stream \
                     lines, and the meridional velocity along the walls against the \
                     quasi-one-dimensional flow of the design",
                ));
            if d.potential {
                let v = &mut d.potential_view;
                MenuButton::new(format!("{} {}", crate::theme::icon::EYE, tr("Potential")))
                    .config(
                        MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside),
                    )
                    .ui(ui, |ui| {
                        ui.checkbox(&mut v.stream_lines, tr("stream lines"));
                        ui.checkbox(&mut v.iso_lines, tr("c_m iso-lines"));
                        ui.checkbox(&mut v.map, tr("c_m color map"));
                        ui.checkbox(&mut v.vectors, tr("velocity vectors"));
                        ui.separator();
                        ui.checkbox(&mut v.blades, tr("blade blockage"))
                            .on_hover_text(tr(
                                "Solve with the thickness of the blades: among them only the \
                                 open part of the circumference carries the flow (Gülich eqs. \
                                 3.1.7 and 3.2.3)",
                            ));
                    });
            }
            ui.weak("(?)").on_hover_text(tr(
                "Drag the round handles to shape the contour; right-click a point or a \
                 wall to type exact values, split a wall into segments or join them. Drag \
                 the background to pan, scroll to zoom, double-click to fit.",
            ));
        });

        let current = cx
            .doc
            .project()
            .impeller_geometry_input(id)
            .unwrap_or_default();
        let display = self.view;
        let flow = display
            .potential
            .then(|| self.potential(derived.revision(), id, g).cloned())
            .flatten();
        let velocity_height = if flow.is_some() {
            (ui.available_height() * 0.3).max(120.0)
        } else {
            0.0
        };
        let size = egui::vec2(
            ui.available_width(),
            (ui.available_height() - velocity_height).max(150.0),
        );
        self.diagram(
            ui,
            cx,
            &Scene {
                id,
                g,
                current: &current,
                stator,
                volute,
                potential: flow.as_ref(),
                display,
                size,
            },
        );
        match &flow {
            Some(Ok(flow)) => wall_velocities(ui, flow, g, id, velocity_height, &units),
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("Potential flow: {e}"));
            }
            None => {}
        }
    }

    /// The meridional diagram with its handles, menus, dimensions and drags.
    pub(crate) fn diagram(&mut self, ui: &mut Ui, cx: &mut EditCtx<'_>, scene: &Scene<'_>) {
        let Scene {
            id,
            g,
            current,
            stator,
            volute,
            potential,
            display,
            size,
        } = *scene;
        let units = cx.units();
        let unit = units.get(QuantityKind::Length).symbol;
        let scale = units.to_display(QuantityKind::Length, 1.0);
        let palette = Palette::of(ui);
        let main = cx.doc.project().pump_impeller_input(id).ok();
        let inputs = Inputs {
            geometry: current,
            main: main.as_ref(),
            semi_axial: g.basis.outlet_inclination > 0.0,
        };

        let center_mode = current.meridional.design_mode == DesignMode::CenterLine;
        let editable = match display.page {
            Page::FlowPath => handles(&g.contour, &current.meridional),
            _ => wire_handles(&g.contour, display.edited(false)),
        };
        let handles: Vec<(Handle, DVec2)> = editable
            .into_iter()
            .map(|h| (h, handle_position(h, &g.contour)))
            .collect();
        let pointer_pos = ui.ctx().pointer_hover_pos();
        let hovered = match (pointer_pos, &self.transform) {
            (Some(pos), Some(t)) if t.frame().contains(pos) => handles
                .iter()
                .map(|&(h, p)| {
                    let screen = t.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
                    (h, screen.distance(pos))
                })
                .filter(|&(_, d)| d < 10.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| h),
            _ => None,
        };
        let active = self.dragging.or(hovered);
        // Keep the view still while dragging, or the plot would refit under the pointer.
        let frozen = self
            .dragging
            .and(self.transform.as_ref())
            .map(|t| *t.bounds());
        // The main dimensions belong to the flow path; the material shows only the
        // point under the pointer.
        let permanent = display.dimensions && display.page == Page::FlowPath;
        let dimensions = dimension_lines(&g.contour, &g.basis, permanent, active);
        // Dimension values as drawn, to find the one under the pointer.
        let font = egui::FontId::proportional(12.0);
        let texts: Vec<String> = dimensions
            .iter()
            .map(|d| d.dimension.text(d.value, &units))
            .collect();
        let label_rect = |t: &PlotTransform, d: &DimensionLine, text: &str| {
            let screen =
                |p: DVec2| t.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
            let galley = ui
                .painter()
                .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE);
            value_rect(screen(d.from), screen(d.to), galley.size())
        };
        let hovered_dimension = match (pointer_pos, &self.transform, hovered) {
            (Some(pos), Some(t), None) => dimensions
                .iter()
                .zip(&texts)
                .find(|(d, text)| d.dimension.editable() && label_rect(t, d, text).contains(pos))
                .map(|(d, _)| d.dimension),
            _ => None,
        };
        let fit = self.fit.take();
        // Extent of the contour with its permanent dimensions, and room for their
        // values. The dimensions are painted over the plot so they never move its bounds.
        let extent = |points: &[DVec2]| {
            let (mut lo, mut hi) = (points[0], points[0]);
            for p in points {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            let margin = (hi - lo) * 0.12;
            ((lo - margin) * scale, (hi + margin) * scale)
        };
        let mut outline: Vec<DVec2> = g.contour.hub.sample(120);
        outline.extend(g.contour.shroud.sample(120));
        if display.material_lines || display.page != Page::FlowPath {
            for side in Side::MATERIALS {
                outline.extend(side.wall(&g.contour).sample(120));
            }
        }
        if display.page == Page::Chambers {
            for side in Side::CASINGS {
                outline.extend(side.wall(&g.contour).sample(120));
            }
        }
        outline.extend(
            dimension_lines(&g.contour, &g.basis, display.dimensions, None)
                .iter()
                .flat_map(|d| [d.from, d.to]),
        );
        let meridian_bounds = extent(&outline);

        let sections = &g.blade.sections;
        // Alt and a drag measure a distance instead of panning.
        let measuring = ui.input(|i| i.modifiers.alt);
        let mut plot = Plot::new(("meridional", id.0))
            .width(size.x)
            .height(size.y)
            .data_aspect(1.0)
            .allow_drag(active.is_none() && !measuring)
            // A double click on a handle sets it back to automatic, elsewhere it fits.
            .allow_double_click_reset(hovered.is_none())
            .sense(Sense::click_and_drag())
            .x_axis_label(format!("z [{unit}]"))
            .y_axis_label(format!("r [{unit}]"));
        if display.legend {
            plot = plot.legend(Legend::default().position(Corner::RightBottom));
        }
        let references = self.references.clone();
        let response = plot.show(ui, |plot| {
            for reference in &references {
                plot.line(
                    Line::new(
                        reference.name.clone(),
                        line(reference.points.iter().copied(), scale),
                    )
                    .color(palette.guide)
                    .style(LineStyle::dashed_loose())
                    .width(1.5),
                );
            }
            if let Some(bounds) = frozen {
                plot.set_plot_bounds(bounds);
            }
            match fit {
                Some(Fit::Meridian) => {
                    let (lo, hi) = meridian_bounds;
                    plot.set_plot_bounds(PlotBounds::from_min_max([lo.x, lo.y], [hi.x, hi.y]));
                }
                Some(Fit::All) => plot.set_auto_bounds(true),
                None => {}
            }
            if display.quasi_orthogonals {
                for qo in g.channel.quasi_orthogonals.iter().step_by(4) {
                    plot.line(
                        Line::new("quasi-orthogonals", line([qo.hub, qo.shroud], scale))
                            .color(palette.guide)
                            .width(0.5),
                    );
                }
            }
            if display.area_circles {
                // The width normal to the flow is A/(π(r_h + r_s)) on each line.
                for qo in g.channel.quasi_orthogonals.iter().step_by(4) {
                    let width = qo.area / (PI * (qo.hub.y + qo.shroud.y));
                    let center = qo.hub.lerp(qo.shroud, 0.5);
                    plot.line(
                        Line::new("area circles", line(circle(center, width / 2.0), scale))
                            .color(palette.stator)
                            .width(0.8),
                    );
                }
            }
            if let Some(Ok(flow)) = potential
                && display.potential_view.stream_lines
            {
                for fraction in [0.25, 0.5, 0.75] {
                    let stream = flow.streamline(fraction);
                    plot.line(
                        Line::new("potential flow", line(stream.points, scale))
                            .color(palette.stator)
                            .width(1.2)
                            .style(LineStyle::dashed_loose()),
                    );
                }
            }
            if display.stream_lines {
                let n = g.channel.spans.len();
                for span in g.channel.spans.iter().skip(1).take(n.saturating_sub(2)) {
                    plot.line(
                        Line::new("stream lines", line(span.points.iter().copied(), scale))
                            .color(palette.guide)
                            .width(0.8),
                    );
                }
            }
            // The wires round the flow path: strong while edited, faint otherwise; the
            // chambers when they are designed or edited.
            let chambers = current.meridional.chambers.enabled || display.page == Page::Chambers;
            let wires = Side::MATERIALS
                .into_iter()
                .filter(|_| display.material_lines || display.page != Page::FlowPath)
                .chain(Side::CASINGS.into_iter().filter(|_| chambers));
            for side in wires {
                let (color, width) = if display.edited(center_mode).contains(&side) {
                    (side.color(&palette), 1.8)
                } else {
                    (side.color(&palette).gamma_multiply(0.45), 1.0)
                };
                plot.line(
                    Line::new("material", line(side.wall(&g.contour).sample(240), scale))
                        .color(color)
                        .width(width),
                );
            }
            if chambers {
                // The openings of the chambers: to the volute at r₂, to the suction
                // at the seal, and the shaft seal behind.
                let c = &g.contour.chambers;
                for outline in [&c.front, &c.rear] {
                    let closing = [outline[outline.len() - 1], outline[0]];
                    plot.line(
                        Line::new("", line(closing, scale))
                            .color(palette.volute.gamma_multiply(0.45))
                            .style(LineStyle::dashed_loose())
                            .width(1.0),
                    );
                }
            }
            if center_mode {
                plot.line(
                    Line::new(
                        "center line",
                        line(g.contour.center_line.sample(240), scale),
                    )
                    .color(palette.stator)
                    .style(LineStyle::dashed_loose())
                    .width(1.5),
                );
            }
            // The inlet line, normal to the axis or leaning.
            plot.line(
                Line::new(
                    "",
                    line([g.contour.hub.start(), g.contour.shroud.start()], scale),
                )
                .color(palette.guide)
                .width(1.0),
            );
            plot.line(
                Line::new("hub", line(g.contour.hub.sample(240), scale))
                    .color(palette.hub)
                    .width(2.0),
            );
            plot.line(
                Line::new("shroud", line(g.contour.shroud.sample(240), scale))
                    .color(palette.shroud)
                    .width(2.0),
            );
            plot.line(
                Line::new(
                    "leading edge",
                    line(sections.iter().map(|s| s.meridional[0]), scale),
                )
                .color(palette.leading_edge)
                .width(2.0),
            );
            plot.line(
                Line::new(
                    "trailing edge",
                    line(
                        sections.iter().filter_map(|s| s.meridional.last().copied()),
                        scale,
                    ),
                )
                .color(palette.trailing_edge)
                .width(2.0),
            );
            // Control polygons in the color of their curve: Bézier and B-spline
            // polygons, the corner lines of an arc and the radii to its centre.
            let mut polygons: Vec<(Vec<DVec2>, Color32)> = Vec::new();
            for &side in display.edited(center_mode) {
                let color = side.color(&palette);
                for segment in side.wall(&g.contour).segments() {
                    if segment.kind() != SegmentKind::Line {
                        polygons.push((segment.control_points().to_vec(), color));
                    }
                    if let Some(f) = segment.fillet() {
                        for t in f.tangent_points {
                            polygons.push((vec![f.center, t], color));
                        }
                    }
                }
            }
            if let Some(curve) = g.contour.leading_edge_curve().filter(|c| c.degree() > 1)
                && display.page == Page::FlowPath
            {
                polygons.push((curve.control_points().to_vec(), palette.leading_edge));
            }
            for (points, color) in polygons {
                plot.line(
                    Line::new("", line(points, scale))
                        .color(color.gamma_multiply(0.7))
                        .style(LineStyle::dashed_dense())
                        .width(1.0),
                );
            }
            if display.curvature {
                for (wall, color) in [
                    (&g.contour.hub, palette.hub),
                    (&g.contour.shroud, palette.shroud),
                ] {
                    let profile = wall.curvature(60);
                    let Some(&(_, kappa, p)) = profile.iter().max_by(|a, b| a.1.total_cmp(&b.1))
                    else {
                        continue;
                    };
                    if kappa <= 0.0 {
                        continue;
                    }
                    plot.points(
                        Points::new("curvature maximum", vec![[p.x * scale, p.y * scale]])
                            .shape(MarkerShape::Up)
                            .radius(6.0)
                            .color(color)
                            .filled(false),
                    );
                    plot.text(
                        Text::new(
                            "",
                            PlotPoint::new(p.x * scale, p.y * scale),
                            RichText::new(format!(
                                "  R {}",
                                units.show(QuantityKind::Length, 1.0 / kappa)
                            ))
                            .size(11.0)
                            .color(color),
                        )
                        .anchor(egui::Align2::LEFT_TOP),
                    );
                }
            }
            if display.adjacent
                && let Some(d) = stator
            {
                // Side walls from the impeller outlet to d₄, and the vane edges.
                let (r2, r3, r4) = (
                    d.impeller_diameter / 2.0,
                    d.inlet_diameter / 2.0,
                    d.outlet_diameter / 2.0,
                );
                for side in [-1.0, 1.0] {
                    let wall: Vec<DVec2> = [r2, r3, r4]
                        .iter()
                        .map(|&r| DVec2::new(d.midplane + side * d.half_width(r), r))
                        .collect();
                    plot.line(
                        Line::new("diffuser", line(wall, scale))
                            .color(palette.stator)
                            .width(1.5),
                    );
                }
                let edges: &[f64] = if d.vanes.is_some() { &[r3, r4] } else { &[r4] };
                for &r in edges {
                    let h = d.half_width(r);
                    plot.line(
                        Line::new(
                            "diffuser",
                            line(
                                [DVec2::new(d.midplane - h, r), DVec2::new(d.midplane + h, r)],
                                scale,
                            ),
                        )
                        .color(palette.stator)
                        .width(1.0),
                    );
                }
            }
            if display.adjacent
                && let Some(v) = volute
            {
                // Inlet ring walls and a few cross sections, folded into one plane.
                let (inner, outer) = v.inlet_ring();
                let zm = v.basis.midplane;
                let half = v.design.inlet_width / 2.0;
                for z in [zm - half, zm + half] {
                    plot.line(
                        Line::new(
                            "volute",
                            line([DVec2::new(z, inner), DVec2::new(z, outer)], scale),
                        )
                        .color(palette.volute)
                        .width(1.5),
                    );
                }
                // The bend and neck of an internal volute.
                if let Some(bend) = &v.bend {
                    let rz = v.design.stagnation_radius;
                    for wall in [&bend.outer, &bend.inner] {
                        let points = wall
                            .iter()
                            .flatten()
                            .map(|p| DVec2::new(zm + p.x, rz + p.y));
                        plot.line(
                            Line::new("volute", line(points, scale))
                                .color(palette.volute)
                                .width(1.5),
                        );
                    }
                }
                let n = v.sections.len();
                for index in [n / 4, n / 2, 3 * n / 4, n - 1] {
                    let s = &v.sections[index];
                    let mut points: Vec<DVec2> =
                        s.outline.iter().map(|p| DVec2::new(p.y, p.x)).collect();
                    points.push(points[0]);
                    plot.line(
                        Line::new("volute", line(points, scale))
                            .color(palette.volute)
                            .width(if index == n - 1 { 2.0 } else { 1.0 }),
                    );
                }
            }
            plot.pointer_coordinate()
        });

        super::settle(ui, ("meridional", id.0), &response.transform);
        paint_dimensions(
            ui,
            &response.transform,
            &dimensions,
            &texts,
            hovered_dimension,
            scale,
        );
        if let Some(Ok(flow)) = potential {
            paint_potential(
                ui,
                &response.transform,
                flow,
                display.potential_view,
                scale,
                &units,
            );
        }
        paint_handles(ui, &response.transform, &handles, active, inputs, scale);
        paint_crossings(ui, &response.transform, &g.contour.crossings, scale);
        self.pointer = response.inner.map(|p| DVec2::new(p.x / scale, p.y / scale));
        if let Some(p) = self.pointer {
            paint_pointer(ui, &response.transform, p, &units);
        }
        self.transform = Some(response.transform);
        let plot_response = &response.response;
        if measuring && hovered.is_none() && plot_response.drag_started() {
            self.measure = self.pointer.map(|p| (p, p));
        } else if let (Some((_, end)), Some(p)) = (&mut self.measure, self.pointer)
            && measuring
            && plot_response.dragged()
        {
            *end = p;
        } else if !measuring && ui.input(|i| i.pointer.primary_pressed()) {
            self.measure = None;
        }
        if let Some((a, b)) = self.measure {
            paint_measure(ui, &response.transform, a, b, scale, &units);
        }
        if plot_response.drag_started() && hovered.is_some() {
            self.dragging = hovered;
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.dragging = None;
            self.guides.clear();
        }
        paint_guides(ui, &response.transform, &self.guides, scale);
        if let Some(handle) = active {
            ui.ctx().set_cursor_icon(if self.dragging.is_some() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
            let state = if reset_change(handle, inputs).is_some() {
                tr("set by hand; double-click to make it automatic")
            } else if is_manual(handle, inputs) {
                ""
            } else {
                tr("automatic; drag it to set it by hand")
            };
            let text = if state.is_empty() {
                handle.label().to_owned()
            } else {
                format!("{}\n{state}", handle.label())
            };
            plot_response.clone().on_hover_text(text);
            if plot_response.double_clicked()
                && let Some(change) = reset_change(handle, inputs)
            {
                change.apply(cx, id, "reset to automatic");
            }
        } else if let Some(dimension) = hovered_dimension {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            plot_response
                .clone()
                .on_hover_text(tf("{}: click to type it", &[&dimension.label()]));
            if plot_response.clicked()
                && let Some(pos) = pointer_pos
            {
                self.dimension_edit = Some(DimensionEdit {
                    dimension,
                    at: pos,
                    text: None,
                    invalid: false,
                    select: true,
                });
            }
        }
        if plot_response.secondary_clicked() {
            let edge: Vec<DVec2> = sections.iter().map(|s| s.meridional[0]).collect();
            let on_edge = ui
                .ctx()
                .pointer_latest_pos()
                .is_some_and(|at| near_line(&edge, at, &response.transform, scale));
            self.menu = Some(match hovered {
                Some(handle) => MenuTarget::Handle(handle),
                None if on_edge => MenuTarget::LeadingEdge,
                None => response
                    .inner
                    .and_then(|pointer| {
                        let pointer = DVec2::new(pointer.x / scale, pointer.y / scale);
                        let sides = display.edited(center_mode);
                        wall_under(&g.contour, sides, pointer, &response.transform, scale)
                    })
                    .map_or(MenuTarget::Empty, |(side, segment, t)| {
                        MenuTarget::Segment(side, segment, t)
                    }),
            });
        }
        if let Some(target) = self.menu {
            plot_response.context_menu(|ui| {
                if let Some(change) =
                    context_menu(ui, target, g, inputs, &units, &mut self.references)
                {
                    change.apply(cx, id, target.edit_label());
                }
            });
        }
        if let (Some(handle), Some(pointer)) = (self.dragging, response.inner)
            && plot_response.dragged()
        {
            let pointer = snap(
                DVec2::new(pointer.x / scale, pointer.y / scale),
                display.snap,
            );
            // Command (Ctrl) held drags freely.
            let free = ui.input(|i| i.modifiers.command);
            let others: Vec<DVec2> = handles
                .iter()
                .filter(|(h, _)| *h != handle)
                .map(|&(_, p)| p)
                .chain([
                    g.contour.hub.start(),
                    g.contour.hub.end(),
                    g.contour.shroud.start(),
                    g.contour.shroud.end(),
                ])
                .collect();
            let (pointer, guides) = if free {
                (pointer, Vec::new())
            } else {
                snap_to(pointer, &others, display, &response.transform, scale)
            };
            self.guides = guides;
            if let Some(change) = drag_change(handle, pointer, g, inputs) {
                change.apply(cx, id, &tf("drag {}", &[&handle.label()]));
                // Show the new geometry even if the pointer stops here.
                ui.ctx().request_repaint();
            }
        }
        self.dimension_box(ui, cx, id, g, inputs, &dimensions);
    }

    /// The box in which a clicked dimension is typed. It opens with the value selected,
    /// so typing replaces it; Enter or OK applies it, Escape or a click elsewhere closes
    /// the box without applying.
    fn dimension_box(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        id: ComponentId,
        g: &ImpellerGeometry,
        inputs: Inputs<'_>,
        dimensions: &[DimensionLine],
    ) {
        let Some(mut edit) = self.dimension_edit.take() else {
            return;
        };
        // The point it measured may be gone.
        let Some(line) = dimensions.iter().find(|d| d.dimension == edit.dimension) else {
            return;
        };
        let units = cx.units();
        let scale = units.to_display(QuantityKind::Length, 1.0);
        let unit = units.get(QuantityKind::Length).symbol;
        let opening = edit.text.is_none();
        let select = std::mem::take(&mut edit.select);
        let text = edit
            .text
            .get_or_insert_with(|| format_value(line.value * scale));
        let field = egui::Id::new(("dimension_edit_field", id.0));
        let (mut apply, mut cancel) = (false, false);
        let area = egui::Area::new(egui::Id::new(("dimension_edit", id.0)))
            .order(egui::Order::Foreground)
            .fixed_pos(edit.at + egui::vec2(8.0, 8.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new(edit.dimension.label()).strong());
                    if converts_wall(edit.dimension, inputs.geometry) {
                        ui.weak(tr("Typing a value turns this wall into drawn segments."));
                    }
                    if edit.dimension.is_main() {
                        ui.weak(tr("A main dimension: typing a value sets it by hand."));
                    }
                    ui.horizontal(|ui| {
                        let mut box_ = egui::TextEdit::singleline(text)
                            .id(field)
                            .desired_width(80.0);
                        if edit.invalid {
                            box_ = box_.text_color(ui.visuals().error_fg_color);
                        }
                        let response = ui.add(box_);
                        ui.label(unit);
                        if select {
                            response.request_focus();
                            // Select the whole value so the first key replaces it.
                            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field) {
                                let all = egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(text.chars().count()),
                                );
                                state.cursor.set_char_range(Some(all));
                                state.store(ui.ctx(), field);
                            }
                        }
                        apply |=
                            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        apply |= ui.button(tr("OK")).clicked();
                    });
                });
            });
        // Consumed, so Escape does not also fold the properties panel.
        cancel |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        cancel |= !opening
            && ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !area.response.rect.contains(p))
            });
        if apply {
            match parse_value(text) {
                Some(value) => {
                    if let Some(change) = set_dimension(edit.dimension, value / scale, g, inputs) {
                        change.apply(cx, id, "edit dimension");
                    }
                    return;
                }
                None => {
                    // Back into the field, selected, to correct it.
                    edit.invalid = true;
                    edit.select = true;
                }
            }
        }
        if !cancel {
            self.dimension_edit = Some(edit);
        }
    }
}

/// A dimension being typed.
#[derive(Debug, Clone)]
struct DimensionEdit {
    dimension: Dimension,
    /// Where the box opens.
    at: egui::Pos2,
    /// The text typed so far; none until the box is first shown.
    text: Option<String>,
    /// Whether the last text applied was not a number.
    invalid: bool,
    /// Whether the field takes the focus with its text selected on the next frame.
    select: bool,
}

/// A value for the dimension box: up to four decimals, without trailing zeros.
fn format_value(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A typed number; a decimal comma is read as a point.
fn parse_value(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// Whether typing `dimension` turns an automatic wall into drawn segments.
fn converts_wall(dimension: Dimension, current: &ImpellerGeometryInput) -> bool {
    match dimension {
        Dimension::Z(h) | Dimension::R(h) => {
            matches!(
                h,
                Handle::HubInlet | Handle::HubOutlet | Handle::ShroudInlet | Handle::ShroudOutlet
            ) && h
                .wall_point()
                .is_some_and(|(side, _)| !side.drawn(&current.meridional))
        }
        _ => false,
    }
}

/// What a right click landed on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuTarget {
    Handle(Handle),
    LeadingEdge,
    /// Segment `.1` of a wall, at curve parameter `.2`.
    Segment(Side, usize, f64),
    Empty,
}

impl MenuTarget {
    fn edit_label(self) -> &'static str {
        match self {
            MenuTarget::Handle(_) => "edit point",
            MenuTarget::LeadingEdge => "edit leading edge",
            MenuTarget::Segment(..) | MenuTarget::Empty => "edit wall segments",
        }
    }
}

/// The wall segment within reach of `pointer` on screen, with its curve parameter.
fn wall_under(
    contour: &MeridionalContour,
    sides: &[Side],
    pointer: DVec2,
    transform: &PlotTransform,
    scale: f64,
) -> Option<(Side, usize, f64)> {
    let screen =
        |p: DVec2| transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale));
    let at = screen(pointer);
    sides
        .iter()
        .map(|&side| {
            let wall = side.wall(contour);
            let (segment, t) = wall.nearest(pointer);
            let d = screen(wall.segments()[segment].eval(t)).distance(at);
            (side, segment, t, d)
        })
        .filter(|&(.., d)| d < 8.0)
        .min_by(|a, b| a.3.total_cmp(&b.3))
        .map(|(side, segment, t, _)| (side, segment, t))
}

/// Whether `at` (screen) is within reach of the polyline `points` ((z, r) in m).
fn near_line(points: &[DVec2], at: egui::Pos2, transform: &PlotTransform, scale: f64) -> bool {
    let screen: Vec<egui::Pos2> = points
        .iter()
        .map(|p| transform.position_from_point(&PlotPoint::new(p.x * scale, p.y * scale)))
        .collect();
    screen.windows(2).any(|s| {
        let (a, b) = (s[0], s[1]);
        let ab = b - a;
        let t = ((at - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
        (a + ab * t).distance(at) < 8.0
    })
}

/// A Bézier leading edge through the current one at its thirds, so the edge keeps its
/// place when it turns into a curve.
pub(crate) fn bezier_along_edge(g: &ImpellerGeometry) -> LeadingEdgeShape {
    let contour = &g.contour;
    let edge: Vec<DVec2> = g.blade.sections.iter().map(|s| s.meridional[0]).collect();
    match tp_math::Polyline::new(edge) {
        Ok(line) => LeadingEdgeShape::bezier_through(
            contour.hub_at(contour.leading_edge.0),
            line.point_at_fraction(1.0 / 3.0),
            line.point_at_fraction(2.0 / 3.0),
            contour.shroud_at(contour.leading_edge.1),
        ),
        Err(_) => LeadingEdgeShape::Line,
    }
}

/// Where the meridian sits along the axis: the reference point and its position.
/// Choosing the other reference keeps the meridian in place; a new position moves it
/// whole, drawn points included.
pub(crate) fn axial_placement(
    ui: &mut Ui,
    contour: &MeridionalContour,
    m: &mut MeridionalInput,
    units: &DisplayUnits,
) {
    ui.horizontal(|ui| {
        ui.label(tr("reference"));
        for (reference, name) in [
            (AxialReference::Inlet, tr("middle of the inlet")),
            (AxialReference::Outlet, tr("middle of the outlet")),
        ] {
            if ui.radio(m.axial_reference == reference, name).clicked()
                && m.axial_reference != reference
            {
                let point = match reference {
                    AxialReference::Inlet => contour.hub.start().lerp(contour.shroud.start(), 0.5),
                    AxialReference::Outlet => contour.hub.end().lerp(contour.shroud.end(), 0.5),
                };
                m.axial_reference = reference;
                m.axial_position = point.x;
            }
        }
    });
    let mut position = m.axial_position;
    if length_field(ui, tr("axial position z"), &mut position, units) {
        m.shift(position - m.axial_position);
        m.axial_position = position;
    }
}

/// How the inlet line leans: normal to the axis, by the axial distance of its shroud
/// point, or by an angle. Changing the way keeps the line as it is.
pub(crate) fn inlet_tilt(
    ui: &mut Ui,
    contour: &MeridionalContour,
    m: &mut MeridionalInput,
    units: &DisplayUnits,
) {
    let height = (contour.shroud.start().y - contour.hub.start().y).max(1e-9);
    let offset = m.inlet_tilt.offset(height);
    ui.horizontal(|ui| {
        for (tilt, name) in [
            (InletTilt::Normal, tr("normal to the axis")),
            (InletTilt::Axial(offset), tr("axial distance")),
            (InletTilt::Angle((offset / height).atan()), tr("angle")),
        ] {
            let same = std::mem::discriminant(&tilt) == std::mem::discriminant(&m.inlet_tilt);
            if ui.radio(same, name).clicked() && !same {
                m.inlet_tilt = tilt;
            }
        }
    });
    match &mut m.inlet_tilt {
        InletTilt::Normal => {}
        InletTilt::Axial(dz) => {
            length_field(ui, "Δz", dz, units);
        }
        InletTilt::Angle(angle) => {
            let mut degrees = angle.to_degrees();
            ui.horizontal(|ui| {
                ui.label(tr("from the normal to the axis"));
                if ui
                    .add(
                        DragValue::new(&mut degrees)
                            .speed(0.2)
                            .range(-60.0..=60.0)
                            .max_decimals(2)
                            .suffix("°"),
                    )
                    .changed()
                {
                    *angle = degrees.to_radians();
                }
            });
        }
    }
}

/// The shape choice of the leading edge.
/// The names of the ways the hub point of the leading edge can follow its shroud point.
pub(crate) fn extent_name(extent: EdgeExtent) -> &'static str {
    tr(match extent {
        EdgeExtent::Independent => "independent",
        EdgeExtent::Axial(_) => "same axial distance",
        EdgeExtent::Radial(_) => "same radial distance",
    })
}

/// `extent` of kind `kind` with the offset of the edge as it is now in `g`.
pub(crate) fn extent_like(kind: EdgeExtent, g: &ImpellerGeometry) -> EdgeExtent {
    let c = &g.contour;
    let (hub, shroud) = (c.hub_at(c.leading_edge.0), c.shroud_at(c.leading_edge.1));
    match kind {
        EdgeExtent::Independent => EdgeExtent::Independent,
        EdgeExtent::Axial(_) => EdgeExtent::Axial(hub.x - shroud.x),
        EdgeExtent::Radial(_) => EdgeExtent::Radial(hub.y - shroud.y),
    }
}

/// How the hub point of the leading edge follows its shroud point, with the offset.
pub(crate) fn leading_edge_extent(
    ui: &mut Ui,
    g: &ImpellerGeometry,
    m: &mut MeridionalInput,
    units: &DisplayUnits,
) {
    ui.label(tr("Hub point")).on_hover_text(tr(
        "Independent of the shroud point, or following it at a constant axial or \
             radial distance: 0 puts the edge normal to the axis or parallel to it",
    ));
    let current = m.leading_edge_extent;
    for kind in [
        EdgeExtent::Independent,
        EdgeExtent::Axial(0.0),
        EdgeExtent::Radial(0.0),
    ] {
        let same = std::mem::discriminant(&kind) == std::mem::discriminant(&current);
        if ui.radio(same, extent_name(kind)).clicked() && !same {
            m.leading_edge_extent = extent_like(kind, g);
        }
    }
    match &mut m.leading_edge_extent {
        EdgeExtent::Independent => {}
        EdgeExtent::Axial(offset) => {
            length_field(ui, "Δz", offset, units);
        }
        EdgeExtent::Radial(offset) => {
            length_field(ui, "Δr", offset, units);
        }
    }
}

pub(crate) fn leading_edge_shape(ui: &mut Ui, g: &ImpellerGeometry, m: &mut MeridionalInput) {
    let contour = &g.contour;
    let current = contour.leading_edge_shape;
    let mut kind = std::mem::discriminant(&current);
    let options = [
        (LeadingEdgeShape::Channel, tr("Along the channel")),
        (LeadingEdgeShape::Line, tr("Straight line")),
        (
            LeadingEdgeShape::Bezier {
                controls: [DVec2::ZERO; 2],
            },
            tr("Bézier"),
        ),
    ];
    for (shape, label) in options {
        let d = std::mem::discriminant(&shape);
        if ui.radio(kind == d, label).clicked() && kind != d {
            kind = d;
            m.leading_edge_shape = match shape {
                LeadingEdgeShape::Bezier { .. } => bezier_along_edge(g),
                other => other,
            };
        }
    }
}

/// A length typed in display units; true when it changed.
fn length_field(ui: &mut Ui, label: &str, value: &mut f64, units: &DisplayUnits) -> bool {
    let unit = units.get(QuantityKind::Length);
    let scale = units.to_display(QuantityKind::Length, 1.0);
    let mut shown = *value * scale;
    let changed = ui
        .horizontal(|ui| {
            ui.label(label);
            ui.add(
                DragValue::new(&mut shown)
                    .speed(0.1)
                    .max_decimals(4)
                    .suffix(format!(" {}", unit.symbol)),
            )
            .changed()
        })
        .inner;
    if changed {
        *value = shown / scale;
    }
    changed
}

/// Whether an inner point of `segment` can go: a Bézier keeps one, a B-spline as many
/// as its degree needs, an arc its corner.
fn removable(segment: &Segment) -> bool {
    let points = segment.control_points().len();
    match segment.kind() {
        SegmentKind::Bezier => points > 3,
        SegmentKind::BSpline => points > segment.degree() + 1,
        SegmentKind::Line | SegmentKind::Arc => false,
    }
}

/// A direction fixed or free: a check box and its angle in degrees from the z axis,
/// starting at `current` when it is first fixed. The new value when it changed.
fn angle_field(ui: &mut Ui, label: &str, value: Option<f64>, current: f64) -> Option<Option<f64>> {
    ui.horizontal(|ui| {
        let mut fixed = value.is_some();
        let mut degrees = value.unwrap_or(current).to_degrees();
        let toggled = ui.checkbox(&mut fixed, label).changed();
        let typed = ui
            .add_enabled(
                fixed,
                DragValue::new(&mut degrees)
                    .speed(0.2)
                    .range(-180.0..=180.0)
                    .max_decimals(2)
                    .suffix("°"),
            )
            .changed();
        (toggled || typed).then(|| fixed.then(|| degrees.to_radians()))
    })
    .inner
}

/// The shape of wall segment `segment`: its type and what that type offers. The new
/// segments when something changed.
fn segment_menu(
    ui: &mut Ui,
    wall: &Wall,
    segment: usize,
    t: f64,
    units: &DisplayUnits,
) -> Option<Vec<SegmentInput>> {
    let curve = wall.segments().get(segment)?;
    let mut changed = None;
    let mut kind = curve.kind();
    ui.horizontal_wrapped(|ui| {
        for (option, name) in [
            (SegmentKind::Bezier, tr("Bézier")),
            (SegmentKind::Line, tr("Straight line")),
            (SegmentKind::Arc, tr("Lines and arc")),
            (SegmentKind::BSpline, tr("B-spline")),
        ] {
            ui.radio_value(&mut kind, option, name);
        }
    });
    if kind != curve.kind() {
        return Some(wall::set_kind(wall, segment, kind));
    }
    match curve.kind() {
        SegmentKind::Arc => {
            let mut radius = curve.fillet().map_or(0.0, |f| f.radius);
            if length_field(ui, tr("corner radius"), &mut radius, units) {
                changed = Some(wall::set_radius(wall, segment, radius));
            }
        }
        SegmentKind::BSpline => {
            ui.horizontal(|ui| {
                ui.label(tr("degree"));
                let mut degree = curve.degree();
                for d in wall::SPLINE_DEGREES {
                    ui.radio_value(&mut degree, d, d.to_string());
                }
                if degree != curve.degree() {
                    changed = Some(wall::set_degree(wall, segment, degree));
                }
            });
            if ui
                .button(tr("Insert knot here"))
                .on_hover_text(tr("One more control point here; the curve keeps its shape"))
                .clicked()
            {
                changed = Some(wall::insert_knot(wall, segment, t));
                ui.close();
            }
        }
        SegmentKind::Bezier => {
            let mut coupled = curve.coupled();
            if ui
                .checkbox(&mut coupled, tr("Coupled control points"))
                .on_hover_text(tr(
                    "The second and the second-last points stay on their lines to the \
                     middle points, so the curvature is zero at both ends; the curve \
                     becomes of degree 4",
                ))
                .changed()
            {
                changed = Some(wall::set_coupled(wall, segment, coupled));
            }
            let (start, end) = curve.angles();
            let heading = |t: f64| curve.direction(t).to_angle();
            if let Some(start) = angle_field(ui, tr("start angle"), start, heading(0.0)) {
                changed = Some(wall::set_angles(wall, segment, start, end));
            }
            if let Some(end) = angle_field(ui, tr("end angle"), end, heading(1.0)) {
                changed = Some(wall::set_angles(wall, segment, start, end));
            }
        }
        SegmentKind::Line => {}
    }
    if curve.kind() != SegmentKind::Arc
        && ui
            .button(tr("Add control point"))
            .on_hover_text(tr("One more control point; the curve keeps its shape"))
            .clicked()
    {
        changed = Some(wall::add_control(wall, segment));
        ui.close();
    }
    changed
}

/// Moving segment `segment` of `side` away from the channel by a distance typed here,
/// kept between menus. The new segments when it was applied.
fn offset_menu(
    ui: &mut Ui,
    contour: &MeridionalContour,
    side: Side,
    segment: usize,
    units: &DisplayUnits,
) -> Option<Vec<SegmentInput>> {
    let key = egui::Id::new("segment_offset");
    let mut distance = ui
        .ctx()
        .data(|d| d.get_temp::<f64>(key))
        .unwrap_or(contour.material.wall_thickness);
    let (moved, apply) = ui
        .horizontal(|ui| {
            let moved = length_field(ui, tr("offset"), &mut distance, units);
            let apply = ui
                .button(tr("Apply"))
                .on_hover_text(tr(
                    "Moves the segment this far away from the channel (negative: towards \
                     it); its joints move along, the ends of the wall stay",
                ))
                .clicked();
            (moved, apply)
        })
        .inner;
    if moved {
        ui.ctx().data_mut(|d| d.insert_temp(key, distance));
    }
    if !apply {
        return None;
    }
    ui.close();
    // A point inside the channel: the middle between hub and shroud.
    let inside = contour.hub_at(0.5).lerp(contour.shroud_at(0.5), 0.5);
    Some(wall::offset_segment(
        side.wall(contour),
        segment,
        distance,
        inside,
    ))
}

/// The context menu of the meridional view, with a way back to automatic for the
/// handles set by hand; the change when it changed something.
fn context_menu(
    ui: &mut Ui,
    target: MenuTarget,
    g: &ImpellerGeometry,
    inputs: Inputs<'_>,
    units: &DisplayUnits,
    references: &mut Vec<Reference>,
) -> Option<Change> {
    let mut change = edit_menu(ui, target, g, inputs, units, references);
    if let MenuTarget::Handle(handle) = target
        && let Some(reset) = reset_change(handle, inputs)
    {
        ui.separator();
        if ui.button(tr("Back to automatic")).clicked() {
            change = Some(reset);
            ui.close();
        }
    }
    change
}

/// The outlet: d₂ and b₂ of the main dimensions.
fn trailing_edge_menu(
    ui: &mut Ui,
    g: &ImpellerGeometry,
    inputs: Inputs<'_>,
    units: &DisplayUnits,
) -> Option<Change> {
    ui.strong(tr("Trailing edge"));
    let b = &g.basis;
    let (mut d2, mut b2, mut d2i) = (b.outlet_diameter, b.outlet_width, b.outlet_hub_diameter);
    let diameter = length_field(ui, "d₂", &mut d2, units);
    let width = length_field(ui, "b₂", &mut b2, units);
    // A semi-axial outlet tilts: its hub end and its inclination are inputs too.
    let (mut hub, mut tilt) = (false, false);
    let mut degrees = b.outlet_inclination.to_degrees();
    if inputs.semi_axial {
        hub = length_field(ui, "d₂i", &mut d2i, units);
        tilt = ui
            .horizontal(|ui| {
                ui.label("ε_s");
                ui.add(
                    DragValue::new(&mut degrees)
                        .speed(0.2)
                        .range(0.5..=80.0)
                        .max_decimals(2)
                        .suffix("°"),
                )
                .changed()
            })
            .inner;
    } else {
        ui.weak(tr(
            "A radial outlet stays parallel to the axis; a semi-axial impeller (main \
             dimensions) tilts it.",
        ));
    }
    ui.weak(tr("A main dimension: typing a value sets it by hand."));
    if !(diameter || width || hub || tilt) {
        return None;
    }
    inputs.main(|m| {
        if diameter {
            m.outlet_diameter = Param::Manual(d2.max(1e-4));
        }
        if width {
            m.outlet_width = Param::Manual(b2.max(1e-4));
        }
        if hub {
            m.outlet_hub_diameter = Param::Manual(d2i.max(1e-4));
        }
        if tilt {
            m.outlet_inclination = Param::Manual(degrees.to_radians());
        }
    })
}

/// The edits of the context menu of `target`.
fn edit_menu(
    ui: &mut Ui,
    target: MenuTarget,
    g: &ImpellerGeometry,
    inputs: Inputs<'_>,
    units: &DisplayUnits,
    references: &mut Vec<Reference>,
) -> Option<Change> {
    if target == MenuTarget::Handle(Handle::TrailingEdge) {
        return trailing_edge_menu(ui, g, inputs, units);
    }
    let current = inputs.geometry;
    let contour = &g.contour;
    let mut input = current.clone();
    let m = &mut input.meridional;
    match target {
        MenuTarget::Handle(Handle::AxialLength) => {
            ui.strong(tr("Axial length L"));
            let mut length = contour.axial_length;
            if length_field(ui, "L", &mut length, units) {
                m.axial_length = Param::Manual(length.max(g.basis.outlet_width * 1.01));
            }
        }
        MenuTarget::LeadingEdge => {
            ui.strong(tr("Leading edge"));
            leading_edge_shape(ui, g, m);
            ui.separator();
            leading_edge_extent(ui, g, m, units);
        }
        MenuTarget::Handle(Handle::LeadingEdgeControl(index)) => {
            ui.strong(Handle::LeadingEdgeControl(index).label());
            let mut p = handle_position(Handle::LeadingEdgeControl(index), contour);
            let z = length_field(ui, "z", &mut p.x, units);
            let r = length_field(ui, "r", &mut p.y, units);
            if z || r {
                place_edge_control(index, p, contour, m);
            }
            ui.separator();
            leading_edge_shape(ui, g, m);
        }
        MenuTarget::Handle(handle @ (Handle::LeadingEdgeHub | Handle::LeadingEdgeShroud)) => {
            ui.strong(handle.label());
            let (param, value) = if handle == Handle::LeadingEdgeHub {
                (&mut m.leading_edge_hub, contour.leading_edge.0)
            } else {
                (&mut m.leading_edge_shroud, contour.leading_edge.1)
            };
            let mut percent = value * 100.0;
            // A hub point that follows the shroud point has no position of its own.
            let free = handle == Handle::LeadingEdgeShroud
                || m.leading_edge_extent == EdgeExtent::Independent;
            let changed = free
                && ui
                    .horizontal(|ui| {
                        ui.label(tr("Position along the wall"));
                        ui.add(
                            DragValue::new(&mut percent)
                                .speed(0.2)
                                .range(0.0..=90.0)
                                .max_decimals(2)
                                .suffix(" %"),
                        )
                        .changed()
                    })
                    .inner;
            if changed {
                *param = Param::Manual(percent / 100.0);
            }
            ui.separator();
            leading_edge_shape(ui, g, m);
            ui.separator();
            leading_edge_extent(ui, g, m, units);
        }
        MenuTarget::Handle(handle @ Handle::AxialReference) => {
            ui.strong(handle.label());
            axial_placement(ui, contour, m, units);
        }
        MenuTarget::Handle(handle @ Handle::InletShroud) => {
            ui.strong(handle.label());
            inlet_tilt(ui, contour, m, units);
        }
        MenuTarget::Handle(handle @ Handle::ArcCenter(side, segment)) => {
            ui.strong(handle.label());
            let wall = side.wall(contour);
            let mut radius = wall.segments().get(segment)?.fillet()?.radius;
            if length_field(ui, "R", &mut radius, units) {
                *side.segments(m) = Some(wall::set_radius(wall, segment, radius));
            }
        }
        MenuTarget::Handle(handle) => {
            let (side, at) = handle.wall_point()?;
            let wall = side.wall(contour);
            ui.strong(handle.label());
            let Some(mut p) = wall.point(at) else {
                ui.weak(tr("This point no longer exists."));
                return None;
            };
            let z = length_field(ui, "z", &mut p.x, units);
            let r = length_field(ui, "r", &mut p.y, units);
            if z || r {
                place(side, at, p, contour, m);
            }
            if !side.drawn(&current.meridional) {
                ui.weak(tr("Typing a value turns this wall into drawn segments."));
            }
            if let WallPoint::Control { segment, index } = at
                && side.drawn(&current.meridional)
                && removable(&wall.segments()[segment])
                && ui.button(tr("Remove control point")).clicked()
            {
                *side.segments(m) = Some(wall::remove_control(wall, segment, index));
                ui.close();
            }
            if let WallPoint::Joint { segment } = at {
                ui.separator();
                let mut smooth = wall.smooth()[segment];
                if ui.checkbox(&mut smooth, tr("Tangent joint")).changed() {
                    *side.segments(m) = Some(wall::set_smooth(wall, segment, smooth));
                }
                if ui.button(tr("Join the two segments")).clicked() {
                    *side.segments(m) = Some(wall::merge(wall, segment));
                    ui.close();
                }
            }
        }
        MenuTarget::Segment(side, segment, t) => {
            let wall = side.wall(contour);
            wall.segments().get(segment)?;
            ui.strong(side.title());
            if wall.segments().len() > 1 {
                ui.weak(tf(
                    "segment {} of {}",
                    &[&(segment + 1), &wall.segments().len()],
                ));
            }
            if ui.button(tr("Split here")).clicked() {
                *side.segments(m) = Some(wall::split(wall, segment, t));
                ui.close();
            }
            if let Some(segments) = segment_menu(ui, wall, segment, t, units) {
                *side.segments(m) = Some(segments);
            }
            if let Some(segments) = offset_menu(ui, contour, side, segment, units) {
                *side.segments(m) = Some(segments);
            }
            ui.separator();
            if ui
                .button(tr("Keep a copy to compare"))
                .on_hover_text(tr(
                    "Draws the wall as it is now, dashed, to compare changes",
                ))
                .clicked()
            {
                references.push(Reference {
                    name: tf("{} copy", &[&side.title()]),
                    points: wall.sample(200),
                });
                ui.close();
            }
            if ui
                .button(tr("Copy the wall points"))
                .on_hover_text(tr(
                    "100 lines of z and r, tab separated, to paste elsewhere",
                ))
                .clicked()
            {
                ui.ctx().copy_text(points_text(wall, units));
                ui.close();
            }
            if side.drawn(&current.meridional) {
                ui.separator();
                let (name, hint) = if side.is_wire() {
                    (
                        tr("Back to automatic"),
                        tr("The wire as the values of the properties panel draw it"),
                    )
                } else {
                    (
                        tr("Back to the automatic contour"),
                        tr("One Bézier from the main dimensions, with its tangent lengths"),
                    )
                };
                if ui.button(name).on_hover_text(hint).clicked() {
                    *side.segments(m) = None;
                    ui.close();
                }
            }
        }
        MenuTarget::Empty => {
            ui.weak(tr("Right-click a point or a wall to edit it."));
        }
    }
    (input != *current).then_some(Change::Geometry(input))
}

/// Meridional velocity along hub and shroud: the potential flow against the uniform
/// velocity of the quasi-one-dimensional flow.
fn wall_velocities(
    ui: &mut Ui,
    flow: &PotentialFlow,
    g: &tp_design::impeller::ImpellerGeometry,
    id: ComponentId,
    height: f32,
    units: &tp_units::DisplayUnits,
) {
    let palette = Palette::of(ui);
    let speed = |v: f64| units.to_display(QuantityKind::Velocity, v);
    let symbol = units.get(QuantityKind::Velocity).symbol;
    let wall = |points: Vec<[f64; 2]>| -> PlotPoints<'static> {
        points.into_iter().map(|[t, v]| [t, speed(v)]).collect()
    };
    let quasi: PlotPoints = (0..=100)
        .map(|i| {
            let t = f64::from(i) / 100.0;
            [t, speed(g.channel.cm_at(t))]
        })
        .collect();
    let response = Plot::new(("meridional_velocity", id.0))
        .height(height)
        .x_axis_label("fraction of the wall length (0 inlet, 1 outlet)")
        .y_axis_label(format!("c_m [{symbol}]"))
        .x_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
        .y_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
        .include_y(0.0)
        .allow_scroll(false)
        .legend(Legend::default().position(Corner::RightTop))
        .show(ui, |plot| {
            plot.line(
                Line::new("hub, potential flow", wall(flow.hub_velocity()))
                    .color(palette.hub)
                    .width(1.8),
            );
            plot.line(
                Line::new("shroud, potential flow", wall(flow.shroud_velocity()))
                    .color(palette.shroud)
                    .width(1.8),
            );
            plot.line(
                Line::new("quasi-one-dimensional", quasi)
                    .color(palette.guide)
                    .width(1.2)
                    .style(LineStyle::dashed_loose()),
            );
        });
    super::settle(ui, ("meridional_velocity", id.0), &response.transform);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_project;
    use crate::derived::Derived;

    fn geometry() -> tp_design::impeller::ImpellerGeometry {
        let project = default_project();
        let mut derived = Derived::default();
        derived.update(&project);
        let id = project.components()[0].id();
        derived.geometry(id).unwrap().clone()
    }

    #[test]
    fn drawn_walls_offer_their_points_and_move_them_exactly() {
        let g = geometry();
        let mut input = tp_design::impeller::ImpellerGeometryInput::default();
        input.meridional.hub_segments = Some(wall::split(&g.contour.hub, 0, 0.5));
        let drawn = tp_design::impeller::build(&g.basis, &input).unwrap();
        let points: Vec<Handle> = handles(&drawn.contour, &input.meridional)
            .into_iter()
            .filter(|h| matches!(h, Handle::Point(Side::Hub, _)))
            .collect();
        // One joint and two control points on each side of it.
        assert_eq!(points.len(), 5);
        assert!(!handles(&drawn.contour, &input.meridional).contains(&Handle::HubInlet));
        // The split keeps the hub where it was.
        let (a, b) = (g.contour.hub.sample(50), drawn.contour.hub.sample(50));
        assert!(a.first().unwrap().distance(*b.first().unwrap()) < 1e-12);
        let joint = Handle::Point(Side::Hub, WallPoint::Joint { segment: 0 });
        let to = handle_position(joint, &drawn.contour) + DVec2::new(0.001, 0.002);
        let mut moved = input.meridional.clone();
        apply_drag(joint, to, &drawn.contour, &drawn.basis, &mut moved);
        input.meridional = moved;
        let after = tp_design::impeller::build(&drawn.basis, &input).unwrap();
        assert!(handle_position(joint, &after.contour).distance(to) < 1e-12);
    }

    #[test]
    fn dimensions_measure_their_points_and_set_them_exactly() {
        let g = geometry();
        let lines = dimension_lines(&g.contour, &g.basis, true, None);
        let value = |lines: &[DimensionLine], d: Dimension| {
            lines.iter().find(|l| l.dimension == d).map(|l| l.value)
        };
        assert_eq!(
            value(&lines, Dimension::AxialLength),
            Some(g.contour.axial_length)
        );
        let b2 = value(&lines, Dimension::OutletWidth).unwrap();
        assert!((b2 - g.basis.outlet_width).abs() < 1e-9, "{b2}");
        // The main dimensions only; the hovered point adds its own.
        let b = &g.basis;
        for (d, expected) in [
            (Dimension::HubDiameter, b.hub_diameter),
            (Dimension::InletDiameter, b.inlet_diameter),
            (Dimension::OutletDiameter, b.outlet_diameter),
        ] {
            assert_eq!(value(&lines, d), Some(expected), "{d:?}");
        }
        assert_eq!(lines.len(), 5);
        // d₂ reaches from the axis to the shroud end, d₁ to the shroud inlet.
        let d2 = lines
            .iter()
            .find(|l| l.dimension == Dimension::OutletDiameter)
            .unwrap();
        assert!((d2.to.y - g.contour.shroud.end().y).abs() < 1e-12 && d2.from.y == 0.0);
        let d1 = lines
            .iter()
            .find(|l| l.dimension == Dimension::InletDiameter)
            .unwrap();
        assert!(d1.from.x < 0.0, "d₁ stands left of the inlet");
        let hovered = dimension_lines(&g.contour, &g.basis, false, Some(Handle::HubInlet));
        let p = handle_position(Handle::HubInlet, &g.contour);
        assert_eq!(value(&hovered, Dimension::Z(Handle::HubInlet)), Some(p.x));
        assert_eq!(value(&hovered, Dimension::R(Handle::HubInlet)), Some(p.y));

        // A typed value moves the joint of a drawn wall.
        let mut input = ImpellerGeometryInput::default();
        input.meridional.hub_segments = Some(wall::split(&g.contour.hub, 0, 0.5));
        let main = CentrifugalPumpInput::default();
        let inputs = Inputs {
            geometry: &input,
            main: Some(&main),
            semi_axial: false,
        };
        let drawn = tp_design::impeller::build(&g.basis, &input).unwrap();
        let joint = Handle::Point(Side::Hub, WallPoint::Joint { segment: 0 });
        let Some(Change::Geometry(moved)) =
            set_dimension(Dimension::R(joint), 0.03, &drawn, inputs)
        else {
            panic!("a geometry change");
        };
        let after = tp_design::impeller::build(&drawn.basis, &moved).unwrap();
        let p = handle_position(joint, &after.contour);
        assert!((p.y - 0.03).abs() < 1e-12);
        assert!((p.x - handle_position(joint, &drawn.contour).x).abs() < 1e-12);
        let Some(Change::Geometry(longer)) =
            set_dimension(Dimension::AxialLength, 0.05, &drawn, inputs)
        else {
            panic!("a geometry change");
        };
        assert_eq!(longer.meridional.axial_length, Param::Manual(0.05));
        // The diameters and b₂ set the main dimensions by hand.
        let main_change = |d: Dimension, v: f64| match set_dimension(d, v, &drawn, inputs) {
            Some(Change::Main(m)) => m,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            main_change(Dimension::OutletWidth, 0.02).outlet_width,
            Param::Manual(0.02)
        );
        assert_eq!(
            main_change(Dimension::OutletDiameter, 0.2).outlet_diameter,
            Param::Manual(0.2)
        );
        assert_eq!(
            main_change(Dimension::InletDiameter, 0.08).inlet_diameter,
            Param::Manual(0.08)
        );
        assert_eq!(
            main_change(Dimension::HubDiameter, 0.02).hub_diameter,
            Param::Manual(0.02)
        );
        // Without pump main dimensions there is nothing to set.
        let geometry_only = Inputs {
            geometry: &input,
            main: None,
            semi_axial: false,
        };
        assert_eq!(
            set_dimension(Dimension::OutletDiameter, 0.2, &drawn, geometry_only),
            None
        );
    }

    #[test]
    fn the_trailing_edge_sets_d2_and_handles_go_back_to_automatic() {
        let g = geometry();
        let input = ImpellerGeometryInput::default();
        let main = CentrifugalPumpInput::default();
        let inputs = Inputs {
            geometry: &input,
            main: Some(&main),
            semi_axial: false,
        };
        // Dragged 5 mm outwards, the trailing edge sets d₂ 10 mm larger, by hand.
        let edge = handle_position(Handle::TrailingEdge, &g.contour);
        let Some(Change::Main(dragged)) = drag_change(
            Handle::TrailingEdge,
            edge + DVec2::new(0.003, 0.005),
            &g,
            inputs,
        ) else {
            panic!("a main-dimensions change");
        };
        let d2 = dragged.outlet_diameter.manual_value().unwrap();
        assert!((d2 - (g.basis.outlet_diameter + 0.01)).abs() < 1e-9, "{d2}");
        assert!(!is_manual(Handle::TrailingEdge, inputs));
        let after = Inputs {
            geometry: &input,
            main: Some(&dragged),
            semi_axial: false,
        };
        assert!(is_manual(Handle::TrailingEdge, after));
        assert_eq!(
            reset_change(Handle::TrailingEdge, after),
            Some(Change::Main(main.clone()))
        );

        // A dragged tangent is manual until it is reset; automatic ones have no reset.
        let Some(Change::Geometry(moved)) =
            drag_change(Handle::ShroudInlet, DVec2::new(10.0, 10.0), &g, inputs)
        else {
            panic!("a geometry change");
        };
        let moved_inputs = Inputs {
            geometry: &moved,
            main: Some(&main),
            semi_axial: false,
        };
        assert!(is_manual(Handle::ShroudInlet, moved_inputs));
        assert_eq!(
            reset_change(Handle::ShroudInlet, moved_inputs),
            Some(Change::Geometry(input.clone()))
        );
        for handle in handles(&g.contour, &input.meridional) {
            assert!(!is_manual(handle, inputs), "{handle:?}");
            assert_eq!(reset_change(handle, inputs), None, "{handle:?}");
        }
        // The points of a drawn wall are its own: nothing to reset them to.
        let joint = Handle::Point(Side::Hub, WallPoint::Joint { segment: 0 });
        assert!(is_manual(joint, inputs));
        assert_eq!(reset_change(joint, inputs), None);
    }

    #[test]
    fn the_outlet_width_is_dimensioned_across_an_inclined_outlet() {
        let g = geometry();
        let mut basis = g.basis;
        basis.outlet_inclination = 25f64.to_radians();
        basis.outlet_hub_diameter = basis.outlet_diameter * 0.8;
        let input = MeridionalInput::default();
        let contour = MeridionalContour::build(&basis, &input).unwrap();
        let lines = dimension_lines(&contour, &basis, true, None);
        let b2 = lines
            .iter()
            .find(|l| l.dimension == Dimension::OutletWidth)
            .unwrap();
        assert_eq!(b2.value, basis.outlet_width);
        assert!((b2.from.distance(b2.to) - basis.outlet_width).abs() < 1e-12);
        // The far end lines up with the hub end along the outlet direction.
        let along = contour.hub.end_direction();
        assert!((b2.to - contour.hub.end()).perp_dot(along).abs() < 1e-9);
        // The ends themselves are further apart than b₂.
        assert!(contour.hub.end().distance(contour.shroud.end()) > basis.outlet_width * 1.01);
    }

    #[test]
    fn pasted_points_are_read_in_any_common_layout() {
        let units = DisplayUnits::default();
        let mm = 1.0 / units.to_display(QuantityKind::Length, 1.0);
        let expect = [
            DVec2::new(1.5 * mm, 20.0 * mm),
            DVec2::new(3.0 * mm, 22.25 * mm),
        ];
        for text in [
            "1.5 20\n3 22.25",
            "# z r\n1.5\t20\n\n3\t22.25\n",
            "1,5;20\n3;22,25",
            "1.5,20\n3,22.25",
            "1,5 20\n3 22,25",
        ] {
            let points = parse_points(text, &units);
            assert_eq!(points.len(), 2, "{text:?}");
            for (p, q) in points.iter().zip(expect) {
                assert!(p.distance(q) < 1e-15, "{text:?}: {p} vs {q}");
            }
        }
        assert!(parse_points("abc\n1 2 3", &units).is_empty());
        // What is copied from a wall reads back as the same points.
        let g = geometry();
        let back = parse_points(&points_text(&g.contour.hub, &units), &units);
        assert_eq!(back.len(), 100);
        for (p, q) in back.iter().zip(g.contour.hub.sample(100)) {
            assert!(p.distance(q) < 1e-8);
        }
    }

    #[test]
    fn dragged_points_snap_to_the_grid() {
        let p = DVec2::new(0.012_34, 0.056_78);
        assert_eq!(snap(p, 0.0), p);
        let q = snap(p, 0.001);
        assert!((q.x - 0.012).abs() < 1e-15 && (q.y - 0.057).abs() < 1e-15);
    }

    #[test]
    fn dragging_a_handle_to_its_own_position_keeps_the_input() {
        let g = geometry();
        let mut input = MeridionalInput::default();
        for handle in handles(&g.contour, &MeridionalInput::default()) {
            let at = handle_position(handle, &g.contour);
            apply_drag(handle, at, &g.contour, &g.basis, &mut input);
        }
        let close = |p: Param<f64>, v: f64, tol: f64| {
            let got = p.manual_value().unwrap();
            assert!((got - v).abs() < tol, "{got} vs {v}");
        };
        let d = tp_design::impeller::meridian::DEFAULT_TANGENT;
        close(input.hub_inlet_tangent, d, 1e-9);
        close(input.hub_outlet_tangent, d, 1e-9);
        close(input.shroud_inlet_tangent, d, 1e-9);
        close(input.shroud_outlet_tangent, d, 1e-9);
        close(
            input.leading_edge_hub,
            g.contour.leading_edge.0,
            1.0 / 400.0,
        );
        close(
            input.leading_edge_shroud,
            g.contour.leading_edge.1,
            1.0 / 400.0,
        );
        close(input.axial_length, g.contour.axial_length, 1e-12);
    }

    #[test]
    fn drags_are_clamped_to_valid_inputs() {
        let g = geometry();
        let mut input = MeridionalInput::default();
        let far = DVec2::new(10.0, 10.0);
        apply_drag(Handle::HubInlet, far, &g.contour, &g.basis, &mut input);
        assert_eq!(input.hub_inlet_tangent, Param::Manual(0.95));
        apply_drag(
            Handle::AxialLength,
            DVec2::ZERO,
            &g.contour,
            &g.basis,
            &mut input,
        );
        assert!(input.axial_length.manual_value().unwrap() > g.basis.outlet_width);
        apply_drag(
            Handle::LeadingEdgeHub,
            far,
            &g.contour,
            &g.basis,
            &mut input,
        );
        assert!(input.leading_edge_hub.manual_value().unwrap() <= 0.9);
    }
}
