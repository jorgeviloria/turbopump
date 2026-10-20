//! Predicted characteristic of the active impeller: head, efficiency and power against
//! flow, with the estimated recirculation range dashed.

use eframe::egui::{Color32, DragValue, RichText, Ui};
use egui_plot::{Corner, Legend, Line, LineStyle, MarkerShape, Plot, PlotPoints, Points, VLine};
use tp_design::Param;
use tp_design::performance::{Collector, PerformanceInput, PerformancePoint, PumpPerformance};
use tp_project::{ComponentId, Project};
use tp_units::{DisplayUnits, QuantityKind};

use super::{Palette, unavailable};
use crate::derived::Derived;
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
    match c {
        Collector::Volute => "volute",
        Collector::VanedDiffuser => "vaned diffuser",
        Collector::VanelessDiffuser => "vaneless diffuser",
    }
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
    if ui.checkbox(&mut auto, "auto").changed() {
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
            unavailable(ui, "The performance prediction needs a pump impeller.");
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
                RichText::new(format!("Collector: {}", collector_name(input.collector)))
                    .color(Palette::of(ui).guide),
            )
            .on_hover_text("From the component after the impeller; it sets the shut-off head");
            ui.add_space(10.0);
            let q_opt = curve.design.flow;
            setting(
                ui,
                "Recirculation below Q/Q_opt",
                "Flow ratio where recirculation sets in at the impeller inlet; below it \
                 the curve is an estimate",
                &mut self.onset,
                curve.recirculation_flow / q_opt,
            );
            setting(
                ui,
                "Shut-off ψ₀",
                "Pressure coefficient 2gH₀/u₂² against a closed valve (Gülich eqs. 3.27–3.28)",
                &mut self.shutoff,
                curve.shutoff_coefficient,
            );
            setting(
                ui,
                "Up to Q/Q_opt",
                "Largest flow of the curve",
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
                "Design",
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
                "Best efficiency",
                format!(
                    "{} ({:.0} %) · η {:.1} %",
                    s(VolumeFlow, curve.best.flow),
                    100.0 * curve.best.flow / curve.design.flow,
                    100.0 * curve.best.efficiency
                ),
            );
            line(
                ui,
                "Shut-off",
                format!(
                    "{} · {} (estimate)",
                    s(Head, curve.shutoff_head),
                    s(Power, curve.shutoff_power)
                ),
            );
            line(ui, "NPSH3", s(Head, curve.npsh_required));
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
        let flow = |p: &PerformancePoint| units.to_display(QuantityKind::VolumeFlow, p.flow);
        let symbol = |kind| units.get(kind).symbol;
        let onset = units.to_display(QuantityKind::VolumeFlow, curve.recirculation_flow);
        let spacing = ui.spacing().item_spacing.y;
        let height = ((ui.available_height() - 2.0 * spacing) / 3.0).max(110.0);
        let colors = if dark {
            [
                Color32::from_rgb(120, 170, 235),
                Color32::from_rgb(110, 200, 120),
                Color32::from_rgb(235, 150, 90),
            ]
        } else {
            [
                Color32::from_rgb(30, 80, 160),
                Color32::from_rgb(28, 135, 64),
                Color32::from_rgb(196, 100, 30),
            ]
        };

        // Each plot: a value per point, its axis label and color.
        type Value<'a> = &'a dyn Fn(&PerformancePoint) -> f64;
        let head: Value = &|p| units.to_display(QuantityKind::Head, p.head);
        let efficiency: Value = &|p| 100.0 * p.efficiency;
        let power: Value = &|p| units.to_display(QuantityKind::Power, p.power);
        let plots: [(&str, String, &str, Value, Color32); 3] = [
            (
                "curve_head",
                format!("H [{}]", symbol(QuantityKind::Head)),
                "head",
                head,
                colors[0],
            ),
            (
                "curve_efficiency",
                "η [%]".into(),
                "efficiency",
                efficiency,
                colors[1],
            ),
            (
                "curve_power",
                format!("P [{}]", symbol(QuantityKind::Power)),
                "power",
                power,
                colors[2],
            ),
        ];
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
            // Head: every item; efficiency: its two curves; power: none.
            match index {
                0 => plot = plot.legend(Legend::default().position(Corner::LeftBottom)),
                1 => plot = plot.legend(Legend::default().position(Corner::RightBottom)),
                _ => {}
            }
            // Items with an empty name stay out of the legend.
            let named = |text: &str| {
                if index == 0 {
                    text.to_owned()
                } else {
                    String::new()
                }
            };
            if index == 2 {
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
                    Line::new(named(&format!("{legend}, estimate")), series(true))
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
                        Line::new("theoretical head", theoretical)
                            .color(palette.guide)
                            .width(1.0)
                            .style(LineStyle::dashed_dense()),
                    );
                }
                if index == 1 {
                    let hydraulic: PlotPoints = curve
                        .points
                        .iter()
                        .filter(|p| p.hydraulic_efficiency.is_finite())
                        .map(|p| [flow(p), 100.0 * p.hydraulic_efficiency])
                        .collect();
                    plot.line(
                        Line::new("hydraulic efficiency", hydraulic)
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
