//! The Meridian view: the meridional section of the impeller over the whole view, in
//! one of four scopes (the stage, the impeller channel, its materials or its side
//! chambers), with the points to drag and the dimensions to type. The inputs of the
//! step sit in the properties panel and the tools in the Meridian tab of the toolbar.

use eframe::egui::{self, Grid, RichText, Ui};
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};
use tp_design::impeller::ImpellerGeometry;
use tp_design::impeller::guide;
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use super::meridional::{MeridionalView, Reference, Scene, Scope, parse_points, wall_velocities};
use super::{Palette, unavailable};
use crate::app::EditCtx;
use crate::i18n::{tf, tr};
use crate::theme::icon;

impl MeridionalView {
    /// The view, clear of the `cover` px the floating properties panel hides on the
    /// right.
    pub(crate) fn show(
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
        // On opening, and in a new scope, frame the drawing with its dimensions, which
        // the plot leaves out of its own bounds.
        if !self.framed || self.shown_scope != Some(self.scope) {
            self.fit = Some(self.scope.fit());
            self.framed = true;
            self.shown_scope = Some(self.scope);
        }
        let current = cx
            .doc
            .project()
            .impeller_geometry_input(id)
            .unwrap_or_default();
        let mut display = self.display;
        display.page = self.scope.page();
        if self.scope == Scope::Stage {
            display.adjacent = true;
            display.legend = true;
        }
        let flow = display
            .potential
            .then(|| self.potential(derived.revision(), id, g).cloned())
            .flatten();

        // The side chambers are drawn once they are designed; the switch is also in the
        // panel.
        if self.scope == Scope::Chambers && !current.meridional.chambers.enabled {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(tr("The side chambers are not designed yet."))
                        .color(ui.visuals().weak_text_color()),
                );
                if ui.button(tr("Design the side chambers")).clicked() {
                    let mut input = current.clone();
                    input.meridional.chambers.enabled = true;
                    cx.edit("edit meridional contour", |p| {
                        p.set_impeller_geometry(id, input)
                    });
                }
            });
            ui.add_space(4.0);
        }

        // Clear of the floating properties panel, as the other views.
        let area = ui.available_rect_before_wrap();
        let right = (area.max.x - cover).max(area.min.x + 320.0);
        let width = right - area.min.x;
        let below = |share: f32, min: f32, max: f32| (area.height() * share).clamp(min, max);
        let velocities = if flow.is_some() {
            below(0.3, 120.0, 260.0)
        } else {
            0.0
        };
        let charts = if display.charts {
            below(0.25, 120.0, 220.0)
        } else {
            0.0
        };
        let diagram = (area.height() - velocities - charts - 8.0).max(150.0);
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
                size: egui::vec2(width, diagram),
            },
        );
        match &flow {
            Some(Ok(flow)) => {
                let rect =
                    egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, velocities));
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    wall_velocities(ui, flow, g, id, velocities, &units);
                });
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("Potential flow: {e}"));
            }
            None => {}
        }
        if display.charts {
            ui.add_space(8.0);
            progressions(ui, id, g, &units, width, charts);
        }
    }
}

/// The meridian against Gülich's recommendations for radial impellers, as a table for
/// the properties panel.
pub(crate) fn guide_table(ui: &mut Ui, g: &ImpellerGeometry, units: &DisplayUnits) {
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
pub(crate) fn references_panel(
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
            .desired_width(260.0)
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
    let drawn = [
        &m.hub_segments,
        &m.shroud_segments,
        &m.hub_material,
        &m.shroud_material,
        &m.center_line,
    ];
    if drawn.iter().all(|s| s.is_none()) {
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
                "Hub and shroud as one Bézier each, and the automatic material; undo brings \
                 them back",
            ))
            .clicked()
        {
            let mut input = current.clone();
            let m = &mut input.meridional;
            m.hub_segments = None;
            m.shroud_segments = None;
            m.hub_material = None;
            m.shroud_material = None;
            m.center_line = None;
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
