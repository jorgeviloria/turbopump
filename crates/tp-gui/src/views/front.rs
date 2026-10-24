//! Front view seen from the inlet looking downstream: blade outlines at hub and shroud,
//! and the outlet, eye and hub circles. A second page shapes the cutwater of the volute
//! (see [`super::cutwater`]).

use std::f64::consts::PI;

use eframe::egui::{self, Ui};
use egui_plot::{Corner, Legend, Line, LineStyle, Plot, PlotPoints, Points};
use tp_math::DVec3;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::cutwater::CutwaterPage;
use super::{Palette, unavailable};
use crate::app::{EditCtx, Selection};
use crate::derived::Derived;
use crate::i18n::tr;
use tp_project::DesignStep;

/// The pages of the front view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FrontPage {
    /// The whole stage from the inlet.
    #[default]
    Plan,
    /// The cutwater of the volute, plane by plane.
    Cutwater,
}

/// The front view between frames.
#[derive(Debug, Default)]
pub(crate) struct FrontView {
    pub page: FrontPage,
    pub cutwater: CutwaterPage,
    /// The step the page last followed.
    followed: Option<Selection>,
}

impl FrontView {
    /// The view, clear of the `cover` px the floating properties panel hides on the
    /// right. The cutwater step opens the cutwater page, the other steps the plan.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditCtx<'_>,
        id: ComponentId,
        stator: Option<ComponentId>,
        volute: Option<ComponentId>,
        units: &DisplayUnits,
        cover: f32,
    ) {
        let selection = *cx.selection;
        if self.followed != Some(selection) {
            self.followed = Some(selection);
            if let Selection::Step(_, step) = selection {
                self.page = if step == DesignStep::CutWater {
                    self.cutwater.frame = true;
                    FrontPage::Cutwater
                } else {
                    FrontPage::Plan
                };
            }
        }
        match (self.page, volute) {
            (FrontPage::Cutwater, Some(v)) => {
                let area = ui.available_rect_before_wrap();
                let right = (area.max.x - cover).max(area.min.x + 480.0);
                let clear = egui::Rect::from_min_max(area.min, egui::pos2(right, area.max.y));
                ui.scope_builder(egui::UiBuilder::new().max_rect(clear), |ui| {
                    self.cutwater.show(ui, cx, id, v, units);
                });
            }
            _ => plan(ui, cx.derived, id, stator, volute, units),
        }
    }
}

/// Looking along +z the x axis points left, as in the SVG drawing.
fn view(x: f64, y: f64, scale: f64) -> [f64; 2] {
    [-x * scale, y * scale]
}

fn circle(r: f64, scale: f64) -> PlotPoints<'static> {
    (0..=180)
        .map(|i| {
            let a = 2.0 * PI * f64::from(i) / 180.0;
            view(r * a.cos(), r * a.sin(), scale)
        })
        .collect()
}

/// The stage from the inlet.
pub(crate) fn plan(
    ui: &mut Ui,
    derived: &Derived,
    id: ComponentId,
    stator: Option<ComponentId>,
    volute: Option<ComponentId>,
    units: &DisplayUnits,
) {
    let stator = stator.and_then(|s| derived.diffuser(s));
    let volute = volute.and_then(|v| derived.volute(v));
    let Some(g) = derived.geometry(id) else {
        let reason = derived
            .impeller(id)
            .and_then(|r| r.geometry.as_ref().err())
            .map_or_else(
                || tr("No geometry yet.").to_owned(),
                |e| crate::i18n::message(e),
            );
        unavailable(ui, &reason);
        return;
    };
    let palette = Palette::of(ui);
    let scale = units.to_display(QuantityKind::Length, 1.0);
    let unit = units.get(QuantityKind::Length).symbol;
    let blade = &g.blade;
    let sign = blade.rotation_sign;
    let outline = |s: &tp_design::impeller::blade::BladeSection| -> Vec<DVec3> {
        let mut points = s.pressure_side(sign);
        points.extend(s.suction_side(sign).into_iter().rev());
        points.push(points[0]);
        points
    };
    let outlines = [
        (
            tr("hub section"),
            blade.sections.first().map(outline),
            palette.hub,
        ),
        (
            tr("shroud section"),
            blade.sections.last().map(outline),
            palette.shroud,
        ),
    ];

    let response = Plot::new(("front", id.0))
        .data_aspect(1.0)
        .x_axis_label(format!("x [{unit}]"))
        .y_axis_label(format!("y [{unit}]"))
        .y_axis_min_width(40.0)
        .legend(Legend::default().position(Corner::LeftTop))
        .show(ui, |plot| {
            let b = &g.basis;
            plot.line(
                Line::new(tr("outlet d₂"), circle(b.outlet_diameter / 2.0, scale))
                    .color(palette.guide)
                    .width(1.5),
            );
            for (name, d) in [
                (tr("eye d₁"), b.inlet_diameter),
                (tr("hub d_n"), b.hub_diameter),
            ] {
                plot.line(
                    Line::new(name, circle(d / 2.0, scale))
                        .color(palette.guide)
                        .style(LineStyle::dashed_loose())
                        .width(1.0),
                );
            }
            if let Some(d) = stator {
                for r in [d.inlet_diameter / 2.0, d.outlet_diameter / 2.0] {
                    plot.line(
                        Line::new(tr("diffuser"), circle(r, scale))
                            .color(palette.stator)
                            .style(LineStyle::dashed_loose())
                            .width(1.0),
                    );
                }
                if let Some(vanes) = &d.vanes {
                    for k in 0..vanes.count {
                        let mut outline = vanes.geometry.outline(k, d.rotation_sign);
                        outline.push(outline[0]);
                        let points: PlotPoints =
                            outline.iter().map(|p| view(p.x, p.y, scale)).collect();
                        plot.line(
                            Line::new(tr("diffuser vanes"), points)
                                .color(palette.stator)
                                .width(1.5),
                        );
                    }
                }
            }
            if let Some(v) = volute {
                // Outer wall of the spiral, from the start to the throat; in a double
                // volute it goes on around the outer channel.
                let outer: Vec<[f64; 2]> =
                    v.outer_wall.iter().map(|p| view(p.x, p.y, scale)).collect();
                plot.line(
                    Line::new(tr("volute"), PlotPoints::from(outer))
                        .color(palette.volute)
                        .width(2.0),
                );
                // The inner wall where the cutwater compensation raises it off r'_z.
                if !v.inner_wall.is_empty() {
                    let inner: Vec<[f64; 2]> =
                        v.inner_wall.iter().map(|p| view(p.x, p.y, scale)).collect();
                    plot.line(
                        Line::new(tr("cutwater compensation"), PlotPoints::from(inner))
                            .color(palette.volute)
                            .width(1.5),
                    );
                }
                // Circular arcs of the outer wall: each center joined to the ends of its
                // arc, so tangent arcs share the line through their meeting point.
                if let Some(arcs) = &v.design.development.arcs {
                    for arc in &arcs.arcs {
                        let at = |p| {
                            let q = v.plane_point(p);
                            view(q.x, q.y, scale)
                        };
                        plot.line(
                            Line::new(
                                tr("circular arcs"),
                                vec![at(arc.start), at(arc.center), at(arc.end)],
                            )
                            .color(palette.guide)
                            .width(1.0),
                        );
                        plot.points(
                            Points::new(tr("circular arcs"), vec![at(arc.center)])
                                .color(palette.guide)
                                .radius(2.5),
                        );
                    }
                }
                // The rib of a double volute, with its nose and tail.
                if let Some(rib) = &v.rib {
                    let mut outline = rib.outline();
                    outline.push(outline[0]);
                    let points: PlotPoints =
                        outline.iter().map(|p| view(p.x, p.y, scale)).collect();
                    plot.line(
                        Line::new(tr("volute"), points)
                            .color(palette.volute)
                            .width(1.5),
                    );
                }
                plot.line(
                    Line::new(
                        tr("cutwater d_z"),
                        circle(v.design.cutwater_diameter / 2.0, scale),
                    )
                    .color(palette.volute)
                    .style(LineStyle::dashed_loose())
                    .width(1.0),
                );
                // Side walls of the discharge diffuser, across its centerline.
                let mut walls = (Vec::new(), Vec::new());
                for n in &v.nozzle {
                    let across = DVec3::new(-n.tangent.y, n.tangent.x, 0.0);
                    let along = |p: &DVec3| (*p - n.center).dot(across);
                    let low = n.points.iter().min_by(|a, b| along(a).total_cmp(&along(b)));
                    let high = n.points.iter().max_by(|a, b| along(a).total_cmp(&along(b)));
                    if let (Some(l), Some(h)) = (low, high) {
                        walls.0.push(view(l.x, l.y, scale));
                        walls.1.push(view(h.x, h.y, scale));
                    }
                }
                if let (Some(&a), Some(&b)) = (walls.0.last(), walls.1.last()) {
                    plot.line(
                        Line::new(tr("volute"), vec![a, b])
                            .color(palette.volute)
                            .width(2.0),
                    );
                }
                plot.line(
                    Line::new(tr("volute"), walls.0)
                        .color(palette.volute)
                        .width(2.0),
                );
                plot.line(
                    Line::new(tr("volute"), walls.1)
                        .color(palette.volute)
                        .width(2.0),
                );
                // The designed cutwater: its rounded body, which runs into the walls, or
                // the tip of the wedge.
                let nose = v.cutwater_outline();
                if !nose.is_empty() {
                    let points: Vec<[f64; 2]> =
                        nose.iter().map(|p| view(p.x, p.y, scale)).collect();
                    plot.line(
                        Line::new(tr("cutwater"), PlotPoints::from(points))
                            .color(palette.volute)
                            .width(2.0),
                    );
                } else if v.cutwater.angle > 0.0 {
                    let tip = v.cutwater.tip;
                    plot.points(
                        Points::new(tr("cutwater"), vec![view(tip.x, tip.y, scale)])
                            .color(palette.volute)
                            .radius(3.0),
                    );
                }
            }
            for i in 0..blade.count {
                let angle = 2.0 * PI * f64::from(i) / f64::from(blade.count);
                let (sin, cos) = angle.sin_cos();
                for (name, points, color) in &outlines {
                    let Some(points) = points else { continue };
                    let rotated: PlotPoints = points
                        .iter()
                        .map(|p| view(cos * p.x - sin * p.y, sin * p.x + cos * p.y, scale))
                        .collect();
                    plot.line(Line::new(*name, rotated).color(*color).width(1.5));
                }
            }
        });
    super::settle(ui, ("front", id.0), &response.transform);
}
