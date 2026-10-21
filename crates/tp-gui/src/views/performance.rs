//! Predicted characteristic of the active impeller: head, efficiency, power and the
//! required NPSH against flow, with the estimated recirculation range dashed and the
//! losses of the volute after the impeller.

use eframe::egui::{Color32, DragValue, RichText, Ui};
use egui_plot::{
    Corner, HLine, Legend, Line, LineStyle, MarkerShape, Plot, PlotPoints, Points, VLine,
};
use tp_design::Param;
use tp_design::performance::{Collector, PerformanceInput, PerformancePoint, PumpPerformance};
use tp_project::{ComponentId, Project};
use tp_units::{DisplayUnits, QuantityKind};

use super::{Palette, unavailable};
use crate::derived::Derived;
use crate::i18n::{tf, tr};
use crate::messages::{self, Severity};
use crate::theme;

/// What the cached curve was computed from.
type Key = (u64, ComponentId, PerformanceInput);

/// Settings and the last curve, between frames.
#[derive(Debug, Default)]
pub(crate) struct PerformanceView {
    onset: Param<f64>,
    shutoff: Param<f64>,
    max_flow: Param<f64>,
    cache: Option<(Key, Result<PumpPerformance, String>)>,
}

fn collector_name(c: Collector) -> &'static str {
    tr(match c {
        Collector::Volute => "volute",
        Collector::VanedDiffuser => "vaned diffuser",
        Collector::VanelessDiffuser => "vaneless diffuser",
    })
}

/// An automatic or manual setting on one line: label, value and "auto".
fn setting(ui: &mut Ui, label: &str, hint: &str, param: &mut Param<f64>, auto_value: f64) {
    ui.label(label).on_hover_text(hint);
    match param {
        Param::Auto => {
            let mut shown = auto_value;
            ui.add_enabled(false, DragValue::new(&mut shown).max_decimals(3));
        }
        Param::Manual(value) => {
            ui.add(
                DragValue::new(value)
                    .speed(0.005)
                    .range(0.05..=3.0)
                    .max_decimals(3),
            );
        }
    }
    let mut auto = param.is_auto();
    if ui.checkbox(&mut auto, tr("auto")).changed() {
        *param = if auto {
            Param::Auto
        } else {
            Param::Manual(auto_value)
        };
    }
    ui.add_space(10.0);
}

impl PerformanceView {
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        project: &Project,
        derived: &Derived,
        id: ComponentId,
        units: &DisplayUnits,
    ) {
        let Ok(mut input) = project.performance_input(id) else {
            unavailable(ui, tr("The performance prediction needs a pump impeller."));
            return;
        };
        input.recirculation_onset = self.onset;
        input.shutoff_coefficient = self.shutoff;
        input.max_flow_ratio = self.max_flow;
        let key = (derived.revision(), id, input.clone());
        if self.cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let curve = project
                .compute_performance(id, &input)
                .map_err(|e| e.to_string());
            self.cache = Some((key, curve));
        }
        let Some((_, curve)) = &self.cache else {
            return;
        };
        let curve = match curve {
            Ok(c) => c.clone(),
            Err(e) => {
                unavailable(ui, e);
                return;
            }
        };

        // Settings.
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(tf("Collector: {}", &[&collector_name(input.collector)]))
                    .color(Palette::of(ui).guide),
            )
            .on_hover_text(tr(
                "From the component after the impeller; it sets the shut-off head",
            ));
            ui.add_space(10.0);
            let q_opt = curve.design.flow;
            setting(
                ui,
                tr("Recirculation below Q/Q_opt"),
                tr(
                    "Flow ratio where recirculation sets in at the impeller inlet; below it \
                 the curve is an estimate",
                ),
                &mut self.onset,
                curve.recirculation_flow / q_opt,
            );
            setting(
                ui,
                tr("Shut-off ψ₀"),
                tr("Pressure coefficient 2gH₀/u₂² against a closed valve (Gülich eqs. 3.27–3.28)"),
                &mut self.shutoff,
                curve.shutoff_coefficient,
            );
            setting(
                ui,
                tr("Up to Q/Q_opt"),
                tr("Largest flow of the curve"),
                &mut self.max_flow,
                curve.max_flow / q_opt,
            );
        });

        // Key numbers.
        let s = |kind, si| units.show(kind, si);
        use QuantityKind::{Head, Power, VolumeFlow};
        ui.horizontal_wrapped(|ui| {
            let line = |ui: &mut Ui, title: &str, text: String| {
                ui.label(theme::bold(title));
                ui.label(text);
                ui.add_space(12.0);
            };
            line(
                ui,
                tr("Design"),
                format!(
                    "{} · {} · {} · η {:.1} %",
                    s(VolumeFlow, curve.design.flow),
                    s(Head, curve.design.head),
                    s(Power, curve.design.power),
                    100.0 * curve.design.efficiency
                ),
            );
            line(
                ui,
                tr("Best efficiency"),
                format!(
                    "{} ({:.0} %) · η {:.1} %",
                    s(VolumeFlow, curve.best.flow),
                    100.0 * curve.best.flow / curve.design.flow,
                    100.0 * curve.best.efficiency
                ),
            );
            line(
                ui,
                tr("Shut-off"),
                tf(
                    "{} · {} (estimate)",
                    &[&s(Head, curve.shutoff_head), &s(Power, curve.shutoff_power)],
                ),
            );
            line(
                ui,
                tr("NPSH₃"),
                tf(
                    "{} at design · available {}",
                    &[
                        &s(Head, curve.npsh_required),
                        &s(Head, curve.npsh_available),
                    ],
                ),
            );
            if let Some(v) = &curve.volute {
                line(
                    ui,
                    tr("Volute"),
                    tf(
                        "{} lost at design ({} % of the hydraulic losses)",
                        &[
                            &s(Head, v.design.total()),
                            &format!("{:.0}", 100.0 * v.share),
                        ],
                    ),
                );
            }
        });
        for w in &curve.warnings {
            messages::card(ui, Severity::Warning, w);
        }
        ui.add_space(4.0);

        self.plots(ui, &curve, id, units);
    }

    fn plots(&self, ui: &mut Ui, curve: &PumpPerformance, id: ComponentId, units: &DisplayUnits) {
        let palette = Palette::of(ui);
        let dark = ui.visuals().dark_mode;
        let error = ui.visuals().error_fg_color;
        let flow = |p: &PerformancePoint| units.to_display(QuantityKind::VolumeFlow, p.flow);
        let symbol = |kind| units.get(kind).symbol;
        let onset = units.to_display(QuantityKind::VolumeFlow, curve.recirculation_flow);
        let spacing = ui.spacing().item_spacing.y;
        let height = ((ui.available_height() - 3.0 * spacing) / 4.0).max(100.0);
        let colors = if dark {
            [
                Color32::from_rgb(120, 170, 235),
                Color32::from_rgb(110, 200, 120),
                Color32::from_rgb(235, 150, 90),
                Color32::from_rgb(190, 140, 230),
            ]
        } else {
            [
                Color32::from_rgb(30, 80, 160),
                Color32::from_rgb(28, 135, 64),
                Color32::from_rgb(196, 100, 30),
                Color32::from_rgb(120, 60, 170),
            ]
        };

        // Each plot: a value per point, its axis label and color.
        type Value<'a> = &'a dyn Fn(&PerformancePoint) -> f64;
        let head: Value = &|p| units.to_display(QuantityKind::Head, p.head);
        let efficiency: Value = &|p| 100.0 * p.efficiency;
        let power: Value = &|p| units.to_display(QuantityKind::Power, p.power);
        let npsh: Value = &|p| units.to_display(QuantityKind::Head, p.npsh_required);
        let head_unit = symbol(QuantityKind::Head);
        let plots: [(&str, String, &str, Value, Color32); 4] = [
            (
                "curve_head",
                format!("H [{}]", symbol(QuantityKind::Head)),
                tr("head"),
                head,
                colors[0],
            ),
            (
                "curve_efficiency",
                "η [%]".into(),
                tr("efficiency"),
                efficiency,
                colors[1],
            ),
            (
                "curve_power",
                format!("P [{}]", symbol(QuantityKind::Power)),
                tr("power"),
                power,
                colors[2],
            ),
            (
                "curve_npsh",
                format!("NPSH₃ [{head_unit}]"),
                tr("NPSH₃"),
                npsh,
                colors[3],
            ),
        ];
        let last = plots.len() - 1;
        for (index, (name, label, legend, value, color)) in plots.into_iter().enumerate() {
            let mut plot = Plot::new((name, id.0))
                .height(height)
                .y_axis_label(label)
                .link_axis("curve_plots", [true, false])
                .link_cursor("curve_plots", [true, false])
                .y_axis_min_width(44.0)
                .x_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
                .y_grid_spacer(egui_plot::uniform_grid_spacer(super::nice_grid))
                .include_y(0.0)
                .allow_scroll(false);
            // Head: every item; efficiency: its two curves; power: none; NPSH: its own.
            match index {
                0 => plot = plot.legend(Legend::default().position(Corner::LeftBottom)),
                1 => plot = plot.legend(Legend::default().position(Corner::RightBottom)),
                3 => plot = plot.legend(Legend::default().position(Corner::LeftTop)),
                _ => {}
            }
            // Items with an empty name stay out of the legend.
            let named = |text: &'static str| {
                if index == 0 {
                    tr(text).to_owned()
                } else {
                    String::new()
                }
            };
            if index == last {
                plot = plot.x_axis_label(format!("Q [{}]", symbol(QuantityKind::VolumeFlow)));
            }
            let response = plot.show(ui, |plot| {
                let series = |estimated: bool| -> PlotPoints<'static> {
                    curve
                        .points
                        .iter()
                        .filter(|p| {
                            p.estimated == estimated
                                || (p.flow - curve.recirculation_flow).abs()
                                    <= 1e-9 * curve.design.flow
                        })
                        .map(|p| [flow(p), value(p)])
                        .collect()
                };
                let main = if index == 2 {
                    String::new()
                } else {
                    legend.to_owned()
                };
                plot.line(Line::new(main, series(false)).color(color).width(2.0));
                plot.line(
                    Line::new(
                        if index == 0 {
                            tf("{}, estimate", &[&legend])
                        } else {
                            String::new()
                        },
                        series(true),
                    )
                    .color(color)
                    .width(1.5)
                    .style(LineStyle::dashed_loose()),
                );
                if index == 0 {
                    let theoretical: PlotPoints = curve
                        .points
                        .iter()
                        .filter(|p| p.theoretical_head.is_finite())
                        .map(|p| {
                            [
                                flow(p),
                                units.to_display(QuantityKind::Head, p.theoretical_head),
                            ]
                        })
                        .collect();
                    plot.line(
                        Line::new(tr("theoretical head"), theoretical)
                            .color(palette.guide)
                            .width(1.0)
                            .style(LineStyle::dashed_dense()),
                    );
                }
                if index == 3 {
                    plot.hline(
                        HLine::new(
                            tr("NPSH available"),
                            units.to_display(QuantityKind::Head, curve.npsh_available),
                        )
                        .color(palette.guide)
                        .style(LineStyle::dashed_dense()),
                    );
                    if curve.volute.is_some() {
                        let loss: PlotPoints = curve
                            .points
                            .iter()
                            // The loss model says nothing in the recirculation range.
                            .filter(|p| !p.estimated && p.volute_loss.is_finite())
                            .map(|p| [flow(p), units.to_display(QuantityKind::Head, p.volute_loss)])
                            .collect();
                        plot.line(
                            Line::new(tr("volute loss"), loss)
                                .color(palette.volute)
                                .width(1.5)
                                .style(LineStyle::dotted_dense()),
                        );
                    }
                    if let Some(q) = curve.cavitation_flow {
                        let x = units.to_display(QuantityKind::VolumeFlow, q);
                        let y = units.to_display(QuantityKind::Head, curve.npsh_available);
                        plot.points(
                            Points::new(tr("NPSH₃ reaches the available"), vec![[x, y]])
                                .shape(MarkerShape::Cross)
                                .radius(6.0)
                                .color(error),
                        );
                    }
                }
                if index == 1 {
                    let hydraulic: PlotPoints = curve
                        .points
                        .iter()
                        .filter(|p| p.hydraulic_efficiency.is_finite())
                        .map(|p| [flow(p), 100.0 * p.hydraulic_efficiency])
                        .collect();
                    plot.line(
                        Line::new(tr("hydraulic efficiency"), hydraulic)
                            .color(color.gamma_multiply(0.6))
                            .width(1.0)
                            .style(LineStyle::dashed_dense()),
                    );
                }
                plot.vline(
                    VLine::new(named("onset of recirculation"), onset)
                        .color(palette.guide)
                        .style(LineStyle::dashed_loose()),
                );
                plot.points(
                    Points::new(
                        named("design point"),
                        vec![[flow(&curve.design), value(&curve.design)]],
                    )
                    .shape(MarkerShape::Circle)
                    .radius(4.5)
                    .color(palette.handle),
                );
                plot.points(
                    Points::new(
                        named("best efficiency"),
                        vec![[flow(&curve.best), value(&curve.best)]],
                    )
                    .shape(MarkerShape::Diamond)
                    .radius(5.0)
                    .color(palette.handle_active),
                );
            });
            super::settle(ui, (name, id.0), &response.transform);
        }
    }
}
