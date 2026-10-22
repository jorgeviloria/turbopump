//! Meridian editor: the meridional diagram with its dimensions, a table with every point
//! of hub, shroud and leading edge, and the progressions of cross-section area and wall
//! curvature, the two things a good meridian keeps smooth.

use eframe::egui::{self, ComboBox, DragValue, Grid, RichText, ScrollArea, Ui};
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};
use tp_design::Param;
use tp_design::impeller::guide;
use tp_design::impeller::meridian::{DEFAULT_TANGENT, LeadingEdgeShape};
use tp_design::impeller::wall::{self, SegmentKind, WallPoint};
use tp_design::impeller::{ImpellerGeometry, ImpellerGeometryInput};
use tp_math::DVec2;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::meridional::{
    Fit, Handle, MeridionalView, Reference, Scene, Side, handle_position, leading_edge_shape,
    parse_points, place, place_edge_control,
};
use super::{Palette, unavailable};
use crate::app::EditCtx;
use crate::i18n::{tf, tr};
use crate::theme::icon;

/// Width of the table of points.
const PANEL_WIDTH: f32 = 310.0;

impl MeridionalView {
    pub(crate) fn show_editor(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        id: ComponentId,
        stator: Option<ComponentId>,
        volute: Option<ComponentId>,
        cover: f32,
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
                recover(ui, cx, id, e);
                return;
            }
        };
        let units = cx.units();
        // On opening, frame the meridian with its dimensions, which the plot leaves out
        // of its own bounds.
        if !self.framed {
            self.fit = Some(Fit::Meridian);
            self.framed = true;
        }
        let current = cx
            .doc
            .project()
            .impeller_geometry_input(id)
            .unwrap_or_default();

        let room = ui.available_size();
        // Clear of the floating properties panel, as the other views.
        let right = (ui.max_rect().max.x - cover).max(ui.max_rect().min.x + 320.0);
        let width = right - ui.cursor().min.x;
        ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
            |ui| {
                let d = &mut self.editor;
                ui.checkbox(&mut d.dimensions, tr("dimensions"))
                    .on_hover_text(tr(
                        "L, b₂ and the joints of the drawn walls, with guide lines at d_n, d₁ \
                     and d₂. Click a value to type it.",
                    ));
                ui.checkbox(&mut d.curvature, tr("curvature maximum"))
                    .on_hover_text(tr(
                        "Point of smallest radius of curvature of hub and shroud",
                    ));
                ui.checkbox(&mut d.area_circles, tr("area circles"))
                    .on_hover_text(tr("Circles as wide as the channel normal to the flow"));
                ui.checkbox(&mut d.stream_lines, tr("stream lines"));
                ui.checkbox(&mut d.quasi_orthogonals, tr("quasi-orthogonals"));
                ui.checkbox(&mut d.adjacent, tr("adjacent components"));
                ui.separator();
                if ui
                    .button(format!("{} {}", icon::FRAME_CORNERS, tr("Fit meridian")))
                    .clicked()
                {
                    self.fit = Some(Fit::Meridian);
                }
                if ui
                    .button(format!("{} {}", icon::ARROWS_OUT, tr("Fit all")))
                    .clicked()
                {
                    self.fit = Some(Fit::All);
                }
                ui.separator();
                // Grid steps in display units; dragged points snap to them.
                let scale = units.to_display(QuantityKind::Length, 1.0);
                let unit = units.get(QuantityKind::Length).symbol;
                let step = self.editor.snap * scale;
                let name = |step: f64| {
                    if step > 0.0 {
                        format!("{step} {unit}")
                    } else {
                        tr("free").to_owned()
                    }
                };
                ui.label(tr("grid"))
                    .on_hover_text(tr("Dragged points snap to this grid"));
                ComboBox::from_id_salt("meridian_grid")
                    .width(70.0)
                    .selected_text(name(step))
                    .show_ui(ui, |ui| {
                        for option in [0.0, 0.01, 0.1, 0.5, 1.0] {
                            if ui
                                .selectable_label((step - option).abs() < 1e-12, name(option))
                                .clicked()
                            {
                                self.editor.snap = option / scale;
                            }
                        }
                    });
                ui.weak(icon::RULER)
                    .on_hover_text(tr("Hold Alt and drag on the diagram to measure a distance"));
                if let Some(p) = self.pointer {
                    ui.separator();
                    ui.monospace(format!(
                        "z {}   r {}",
                        units.show(QuantityKind::Length, p.x),
                        units.show(QuantityKind::Length, p.y)
                    ));
                }
            },
        );

        let room = egui::vec2(room.x, ui.available_height());
        let panel = PANEL_WIDTH.min(width * 0.5);
        let plot_width = width - panel - 8.0;
        let charts = (room.y * 0.27).clamp(120.0, 240.0);
        let diagram = (room.y - charts - 8.0).max(150.0);
        let display = self.editor;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(plot_width);
                self.diagram(
                    ui,
                    cx,
                    &Scene {
                        id,
                        g,
                        current: &current,
                        stator,
                        volute,
                        potential: None,
                        display,
                        size: egui::vec2(plot_width, diagram),
                    },
                );
                ui.add_space(8.0);
                progressions(ui, id, g, &units, plot_width, charts);
            });
            ui.vertical(|ui| {
                ui.set_width(panel);
                // It scrolls sideways too rather than run under the properties panel.
                ScrollArea::both()
                    .id_salt(("meridian_points", id.0))
                    .max_height(room.y)
                    .show(ui, |ui| {
                        guide_panel(ui, g, &units);
                        ui.separator();
                        if let Some((input, label)) = points_panel(ui, g, &current, &units) {
                            cx.edit(label, |p| p.set_impeller_geometry(id, input));
                        }
                        ui.separator();
                        references_panel(
                            ui,
                            &mut self.references,
                            &mut self.reference_text,
                            &units,
                        );
                    });
            });
        });
    }
}

/// The meridian against Gülich's recommendations for radial impellers.
fn guide_panel(ui: &mut Ui, g: &ImpellerGeometry, units: &DisplayUnits) {
    let check = guide::guide(&g.contour, &g.basis);
    let palette = Palette::of(ui);
    let length = |v: f64| units.show(QuantityKind::Length, v);
    let mark = |ui: &mut Ui, ok: bool| {
        if ok {
            ui.colored_label(palette.handle, icon::CHECK_CIRCLE);
        } else {
            ui.colored_label(ui.visuals().warn_fg_color, icon::WARNING);
        }
    };
    ui.label(RichText::new(tr("Check against Gülich")).strong())
        .on_hover_text(tr(
            "Recommendations for the meridional section of radial impellers: Gülich, \
             Centrifugal Pumps, chap. 7.2.2.1 and eq. 7.2",
        ));
    // Ranges with the unit once: "17.37–23.16 mm".
    let scale = units.to_display(QuantityKind::Length, 1.0);
    let unit = units.get(QuantityKind::Length).symbol;
    let span = |a: f64, b: f64| format!("{:.2}–{:.2} {unit}", a * scale, b * scale);
    Grid::new("meridian_guide")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.weak("");
            ui.weak(tr("actual"));
            ui.weak(tr("recommended"));
            ui.end_row();
            let rows = [
                (
                    "z_E",
                    tr("Axial extension to the shroud end: (d₂ₐ − d₁)(n_q/74)^1.07, eq. 7.2"),
                    check.axial_extension,
                    false,
                ),
                (
                    "R_Ds",
                    tr("Smallest radius of curvature of the shroud: (0.6 to 0.8)·b₁"),
                    check.shroud_radius,
                    true,
                ),
                (
                    "g₁",
                    tr(
                        "Nearly axial run of the shroud from the inlet, up to a 10° turn: \
                         (0.2 to 0.3)·b₁",
                    ),
                    check.inlet_run,
                    true,
                ),
            ];
            for (name, hint, c, range) in rows {
                ui.horizontal(|ui| {
                    mark(ui, c.ok);
                    ui.label(name)
                })
                .inner
                .on_hover_text(hint);
                ui.label(length(c.actual));
                ui.label(if range {
                    span(c.recommended.0, c.recommended.1)
                } else {
                    length(c.recommended.0)
                });
                ui.end_row();
            }
            ui.horizontal(|ui| {
                mark(ui, check.leading_edge_before_bend);
                ui.label(tr("leading edge"))
            })
            .inner
            .on_hover_text(tr(
                "On the shroud it should sit before the front shroud curves, not where \
                 the curvature is high",
            ));
            ui.label(if check.leading_edge_before_bend {
                tr("before the bend")
            } else {
                tr("in the bend")
            });
            ui.label(tr("before the bend"));
            ui.end_row();
            ui.label("     ε_Ds").on_hover_text(tr(
                "Shroud angle at the outlet from the radial direction. Often 0 below \
                 n_q = 20, about 15–20° for radial impellers of higher n_q",
            ));
            ui.label(format!("{:.1}°", check.shroud_outlet_angle.to_degrees()));
            ui.label(if g.basis.nq < 20.0 { "0°" } else { "0–20°" });
            ui.end_row();
        });
}

/// Curves to compare with: pasted points, or copies kept from the wall menu.
fn references_panel(
    ui: &mut Ui,
    references: &mut Vec<Reference>,
    text: &mut String,
    units: &DisplayUnits,
) {
    let unit = units.get(QuantityKind::Length).symbol;
    ui.label(RichText::new(tr("Reference curves")).strong())
        .on_hover_text(tr(
            "Dashed curves to compare with: points pasted here, or a copy of a wall kept \
             from its right-click menu",
        ));
    let mut remove = None;
    for (i, reference) in references.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui
                .small_button(icon::X)
                .on_hover_text(tr("Remove this curve"))
                .clicked()
            {
                remove = Some(i);
            }
            ui.label(&reference.name);
            ui.weak(tf("{} points", &[&reference.points.len()]));
        });
    }
    if let Some(i) = remove {
        references.remove(i);
    }
    ui.add(
        egui::TextEdit::multiline(text)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text(tf("z r per line, in {}", &[&unit])),
    );
    let points = parse_points(text, units);
    if ui
        .add_enabled(points.len() >= 2, egui::Button::new(tr("Add as a curve")))
        .clicked()
    {
        references.push(Reference {
            name: tf("curve {}", &[&(references.len() + 1)]),
            points,
        });
        text.clear();
    }
}

/// The reason there is no geometry and, when a drawn wall may be the cause, a way back
/// to the automatic contour; undo works too.
fn recover(ui: &mut Ui, cx: &mut EditCtx<'_>, id: ComponentId, reason: &str) {
    let current = cx
        .doc
        .project()
        .impeller_geometry_input(id)
        .unwrap_or_default();
    let m = &current.meridional;
    if m.hub_segments.is_none() && m.shroud_segments.is_none() {
        unavailable(ui, reason);
        return;
    }
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.35);
        ui.colored_label(ui.visuals().warn_fg_color, crate::i18n::message(reason));
        ui.add_space(8.0);
        if ui
            .button(tr("Back to the automatic contour"))
            .on_hover_text(tr(
                "Hub and shroud as one Bézier each; undo brings them back",
            ))
            .clicked()
        {
            let mut input = current.clone();
            input.meridional.hub_segments = None;
            input.meridional.shroud_segments = None;
            cx.edit("edit wall segments", |p| p.set_impeller_geometry(id, input));
        }
        ui.weak(tr("Or undo the last change."));
    });
}

/// Cross-section area along the channel and curvature along the walls.
fn progressions(
    ui: &mut Ui,
    id: ComponentId,
    g: &ImpellerGeometry,
    units: &DisplayUnits,
    width: f32,
    height: f32,
) {
    let palette = Palette::of(ui);
    let unit = units.get(QuantityKind::Length).symbol;
    // Curvature in 1/display unit.
    let per_unit = 1.0 / units.to_display(QuantityKind::Length, 1.0);
    let half = (width - 8.0) / 2.0;
    ui.horizontal_top(|ui| {
        let qos = &g.channel.quasi_orthogonals;
        let inlet = qos[0].area;
        Plot::new(("meridian_area", id.0))
            .width(half)
            .height(height)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .x_axis_label(tr("position along the channel"))
            .y_axis_label("A/A₁")
            .legend(Legend::default().position(Corner::LeftTop))
            .show(ui, |plot| {
                let area: PlotPoints = qos.iter().map(|q| [q.t, q.area / inlet]).collect();
                plot.line(
                    Line::new(tr("cross-section area"), area)
                        .color(palette.stator)
                        .width(1.8),
                );
            });
        Plot::new(("meridian_curvature", id.0))
            .width(half)
            .height(height)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .x_axis_label(tr("fraction of the wall length"))
            .y_axis_label(format!("κ [1/{unit}]"))
            .legend(Legend::default().position(Corner::LeftTop))
            .show(ui, |plot| {
                for (name, wall, color) in [
                    (tr("hub"), &g.contour.hub, palette.hub),
                    (tr("shroud"), &g.contour.shroud, palette.shroud),
                ] {
                    let profile = wall.curvature(60);
                    let length = profile.last().map_or(1.0, |p| p.0).max(f64::EPSILON);
                    let points: PlotPoints = profile
                        .iter()
                        .map(|&(s, kappa, _)| [s / length, kappa * per_unit])
                        .collect();
                    plot.line(Line::new(name, points).color(color).width(1.8));
                }
            });
    });
}

/// A length typed in display units; the new value in m when it changed.
fn coordinate(ui: &mut Ui, value: f64, units: &DisplayUnits) -> Option<f64> {
    let scale = units.to_display(QuantityKind::Length, 1.0);
    let mut shown = value * scale;
    ui.add(DragValue::new(&mut shown).speed(0.1).max_decimals(3))
        .changed()
        .then_some(shown / scale)
}

/// A point that cannot move: its coordinates as text.
fn fixed(ui: &mut Ui, p: DVec2, units: &DisplayUnits) {
    ui.weak(units.show(QuantityKind::Length, p.x));
    ui.weak(units.show(QuantityKind::Length, p.y));
}

/// The (z, r) cells of a movable wall point; true when it moved.
fn wall_point(
    ui: &mut Ui,
    g: &ImpellerGeometry,
    side: Side,
    at: WallPoint,
    m: &mut tp_design::impeller::meridian::MeridionalInput,
    units: &DisplayUnits,
) -> bool {
    let p = handle_position(Handle::Point(side, at), &g.contour);
    let z = coordinate(ui, p.x, units);
    let r = coordinate(ui, p.y, units);
    if z.is_none() && r.is_none() {
        return false;
    }
    let to = DVec2::new(z.unwrap_or(p.x), r.unwrap_or(p.y).max(0.0));
    place(side, at, to, &g.contour, m);
    true
}

/// Every point of the meridian in a table: hub and shroud segment by segment, and the
/// leading edge. Returns the new input and the label of the edit when something changed.
fn points_panel(
    ui: &mut Ui,
    g: &ImpellerGeometry,
    current: &ImpellerGeometryInput,
    units: &DisplayUnits,
) -> Option<(ImpellerGeometryInput, &'static str)> {
    let contour = &g.contour;
    let mut input = current.clone();
    let m = &mut input.meridional;
    let mut label = "edit point";
    // Narrow number fields leave room for the joint buttons.
    ui.spacing_mut().interact_size.x = 52.0;
    let unit = units.get(QuantityKind::Length).symbol;

    ui.label(RichText::new(tr("Points of the meridian")).strong());
    ui.weak(tf(
        "Coordinates in {}: z from the inlet plane, r from the axis.",
        &[&unit],
    ));
    ui.add_space(4.0);
    Grid::new("meridian_length").num_columns(3).show(ui, |ui| {
        ui.label(tr("Axial length L"));
        if let Some(l) = coordinate(ui, contour.axial_length, units) {
            m.axial_length = Param::Manual(l.max(g.basis.outlet_width * 1.01));
        }
        if !m.axial_length.is_auto()
            && ui
                .small_button(tr("automatic"))
                .on_hover_text(tr("b₂ + (d₁ − d_n)/2"))
                .clicked()
        {
            m.axial_length = Param::Auto;
        }
        ui.end_row();
    });

    for side in [Side::Hub, Side::Shroud] {
        ui.separator();
        let wall = match side {
            Side::Hub => &contour.hub,
            Side::Shroud => &contour.shroud,
        };
        ui.label(
            RichText::new(match side {
                Side::Hub => tr("Hub"),
                Side::Shroud => tr("Shroud"),
            })
            .strong(),
        );
        let drawn = match side {
            Side::Hub => current.meridional.hub_segments.is_some(),
            Side::Shroud => current.meridional.shroud_segments.is_some(),
        };
        if !drawn {
            ui.weak(tr("Automatic: one Bézier from the main dimensions."));
            Grid::new(("meridian_auto", side as u8))
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    let (inlet, outlet) = match side {
                        Side::Hub => (&mut m.hub_inlet_tangent, &mut m.hub_outlet_tangent),
                        Side::Shroud => (&mut m.shroud_inlet_tangent, &mut m.shroud_outlet_tangent),
                    };
                    for (name, param) in
                        [(tr("inlet tangent"), inlet), (tr("outlet tangent"), outlet)]
                    {
                        ui.label(name);
                        let mut value = param.resolve(|| DEFAULT_TANGENT);
                        if ui
                            .add(
                                DragValue::new(&mut value)
                                    .speed(0.005)
                                    .range(0.05..=0.95)
                                    .max_decimals(3),
                            )
                            .on_hover_text(tr(
                                "Length of the Bézier tangent as a fraction of the distance to \
                                 the corner",
                            ))
                            .changed()
                        {
                            *param = Param::Manual(value);
                        }
                        ui.label("");
                        ui.end_row();
                    }
                });
            if ui
                .button(tr("Draw by segments"))
                .on_hover_text(tr(
                    "Turn this wall into segments you can split, join and type point by point",
                ))
                .clicked()
            {
                *side_segments(side, m) = Some(wall.to_input());
                label = "edit wall segments";
            }
            continue;
        }
        let n = wall.segments().len();
        let mut operation = None;
        Grid::new(("meridian_points", side as u8))
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                ui.weak("");
                ui.weak("z");
                ui.weak("r");
                ui.weak("");
                ui.end_row();
                ui.label(tr("inlet"));
                fixed(ui, wall.start(), units);
                ui.label("");
                ui.end_row();
                for (segment, curve) in wall.segments().iter().enumerate() {
                    let kind = if curve.degree() == 1 {
                        SegmentKind::Line
                    } else {
                        SegmentKind::Bezier
                    };
                    let mut chosen = kind;
                    let name = match kind {
                        SegmentKind::Bezier => tr("Bézier"),
                        SegmentKind::Line => tr("Straight line"),
                    };
                    ComboBox::from_id_salt(("segment_kind", side as u8, segment))
                        .width(100.0)
                        .selected_text(RichText::new(format!("{} · {name}", segment + 1)).italics())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut chosen, SegmentKind::Bezier, tr("Bézier"));
                            ui.selectable_value(
                                &mut chosen,
                                SegmentKind::Line,
                                tr("Straight line"),
                            );
                        });
                    if chosen != kind {
                        operation = Some(wall::set_kind(wall, segment, chosen));
                    }
                    if ui
                        .small_button(icon::SCISSORS)
                        .on_hover_text(tr("Split this segment in two"))
                        .clicked()
                    {
                        operation = Some(wall::split(wall, segment, 0.5));
                    }
                    ui.label("");
                    ui.label("");
                    ui.end_row();
                    let inner = curve.control_points().len().saturating_sub(2);
                    for index in 0..inner {
                        ui.label(tr("control point"));
                        let at = WallPoint::Control { segment, index };
                        wall_point(ui, g, side, at, m, units);
                        ui.label("");
                        ui.end_row();
                    }
                    if segment + 1 < n {
                        ui.label(tf("joint {}", &[&(segment + 1)]));
                        let at = WallPoint::Joint { segment };
                        wall_point(ui, g, side, at, m, units);
                        ui.horizontal(|ui| {
                            let mut smooth = wall.smooth()[segment];
                            if ui
                                .checkbox(&mut smooth, "")
                                .on_hover_text(tr("Tangent joint"))
                                .changed()
                            {
                                operation = Some(wall::set_smooth(wall, segment, smooth));
                            }
                            if ui
                                .small_button(icon::LINK)
                                .on_hover_text(tr("Join the two segments"))
                                .clicked()
                            {
                                operation = Some(wall::merge(wall, segment));
                            }
                        });
                        ui.end_row();
                    }
                }
                ui.label(tr("outlet"));
                fixed(ui, wall.end(), units);
                ui.label("");
                ui.end_row();
            });
        if let Some(segments) = operation {
            *side_segments(side, m) = Some(segments);
            label = "edit wall segments";
        }
        if ui.button(tr("Back to the automatic contour")).clicked() {
            *side_segments(side, m) = None;
            label = "edit wall segments";
        }
    }

    ui.separator();
    ui.label(RichText::new(tr("Leading edge")).strong());
    leading_edge_shape(ui, g, m);
    Grid::new("meridian_leading_edge")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            for (name, param, value) in [
                (
                    tr("on the hub"),
                    &mut m.leading_edge_hub,
                    contour.leading_edge.0,
                ),
                (
                    tr("on the shroud"),
                    &mut m.leading_edge_shroud,
                    contour.leading_edge.1,
                ),
            ] {
                ui.label(name);
                let mut percent = value * 100.0;
                if ui
                    .add(
                        DragValue::new(&mut percent)
                            .speed(0.2)
                            .range(0.0..=90.0)
                            .max_decimals(2)
                            .suffix(" %"),
                    )
                    .on_hover_text(tr("Position along the wall"))
                    .changed()
                {
                    *param = Param::Manual(percent / 100.0);
                }
                ui.label("");
                ui.end_row();
            }
            if matches!(contour.leading_edge_shape, LeadingEdgeShape::Bezier { .. }) {
                for index in 0..2 {
                    ui.label(tr("control point"));
                    let p = handle_position(Handle::LeadingEdgeControl(index), contour);
                    let z = coordinate(ui, p.x, units);
                    let r = coordinate(ui, p.y, units);
                    if z.is_some() || r.is_some() {
                        let to = DVec2::new(z.unwrap_or(p.x), r.unwrap_or(p.y));
                        place_edge_control(index, to, contour, m);
                    }
                    ui.end_row();
                }
            }
        });

    (input != *current).then_some((input, label))
}

fn side_segments(
    side: Side,
    m: &mut tp_design::impeller::meridian::MeridionalInput,
) -> &mut Option<Vec<tp_design::impeller::wall::SegmentInput>> {
    match side {
        Side::Hub => &mut m.hub_segments,
        Side::Shroud => &mut m.shroud_segments,
    }
}
