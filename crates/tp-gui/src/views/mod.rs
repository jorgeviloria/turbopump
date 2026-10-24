//! The views of the active stage, one at a time in the main area.

pub(crate) mod blade;
pub(crate) mod blade_edges;
pub(crate) mod cross_section;
pub(crate) mod front;
pub(crate) mod meridian_editor;
pub(crate) mod meridional;
pub(crate) mod performance;
pub(crate) mod report;

use eframe::egui::{self, Color32, Id, Stroke, Ui, vec2};
use egui_plot::PlotTransform;

/// Line colors shared by the plots, tuned for light and dark themes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub hub: Color32,
    pub shroud: Color32,
    pub leading_edge: Color32,
    pub trailing_edge: Color32,
    pub guide: Color32,
    pub handle: Color32,
    pub handle_active: Color32,
    pub volute: Color32,
    pub stator: Color32,
}

impl Palette {
    pub fn of(ui: &Ui) -> Self {
        if ui.visuals().dark_mode {
            Self {
                hub: Color32::from_rgb(120, 170, 235),
                shroud: Color32::from_rgb(235, 140, 110),
                leading_edge: Color32::from_rgb(240, 200, 80),
                trailing_edge: Color32::from_rgb(190, 140, 235),
                guide: Color32::from_gray(90),
                handle: Color32::from_rgb(90, 200, 160),
                handle_active: Color32::WHITE,
                volute: Color32::from_rgb(170, 190, 120),
                stator: Color32::from_rgb(110, 200, 200),
            }
        } else {
            Self {
                hub: Color32::from_rgb(30, 80, 160),
                shroud: Color32::from_rgb(190, 70, 40),
                leading_edge: Color32::from_rgb(200, 130, 0),
                trailing_edge: Color32::from_rgb(120, 60, 170),
                guide: Color32::from_gray(185),
                handle: Color32::from_rgb(0, 140, 110),
                handle_active: Color32::BLACK,
                volute: Color32::from_rgb(90, 120, 30),
                stator: Color32::from_rgb(0, 120, 130),
            }
        }
    }

    /// Color of blade section `i` of `n`, from hub to shroud.
    pub fn span(&self, i: usize, n: usize) -> Color32 {
        let t = if n > 1 {
            i as f32 / (n - 1) as f32
        } else {
            0.0
        };
        self.hub.lerp_to_gamma(self.shroud, t)
    }
}

/// Asks for one more frame when a plot's bounds moved. egui_plot lays out its axes
/// from the previous frame, so after the data change the labels would otherwise lag
/// until the next input event.
pub(crate) fn settle(
    ui: &Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    transform: &PlotTransform,
) {
    let key = Id::new(id).with("settled_bounds");
    let b = transform.bounds();
    let current = [b.min()[0], b.min()[1], b.max()[0], b.max()[1]];
    let previous = ui.ctx().data(|d| d.get_temp::<[f64; 4]>(key));
    if previous != Some(current) {
        ui.ctx().data_mut(|d| d.insert_temp(key, current));
        ui.ctx().request_repaint();
    }
}

/// Grid steps of 1, 2 or 5 times a power of ten, about a quarter of the visible range
/// apart, so that every plot labels a few values whatever its height.
pub(crate) fn nice_grid(input: egui_plot::GridInput) -> [f64; 3] {
    let range = (input.bounds.1 - input.bounds.0).abs().max(f64::EPSILON);
    let target = range / 4.0;
    let power = 10f64.powf(target.log10().floor());
    let mantissa = target / power;
    let step = if mantissa < 1.5 {
        1.0
    } else if mantissa < 3.5 {
        2.0
    } else if mantissa < 7.5 {
        5.0
    } else {
        10.0
    } * power;
    [step / 5.0, step, step * 5.0]
}

/// Message shown instead of a view when the geometry is missing.
pub(crate) fn unavailable(ui: &mut Ui, reason: &str) {
    ui.centered_and_justified(|ui| {
        ui.colored_label(ui.visuals().warn_fg_color, crate::i18n::message(reason));
    });
}

/// Symbols of the shared legend.
#[derive(Clone, Copy)]
pub(crate) enum Mark {
    Circle,
    Diamond,
    Square,
    /// A vertical dashed line, as for a marked flow.
    Dashed,
    /// A vertical dotted line.
    Dotted,
    /// A horizontal solid line, as for a curve.
    Line,
    /// A horizontal dashed line.
    DashedLine,
}

/// A small drawn symbol and its name, as in a plot legend.
pub(crate) fn marker(ui: &mut Ui, mark: Mark, color: Color32, text: &str) {
    // The mark and its words as one piece: a wrapping legend moves it whole to the next
    // line rather than splitting it.
    let width = 14.0 + ui.spacing().item_spacing.x + text_width(ui, text, false);
    whole(ui, width, |ui| marker_piece(ui, mark, color, text));
    ui.add_space(10.0);
}

/// Width of `text` in the body font, bold or not.
pub(crate) fn text_width(ui: &Ui, text: &str, bold: bool) -> f32 {
    let mut font = egui::TextStyle::Body.resolve(ui.style());
    if bold {
        font.family = crate::theme::bold_family();
    }
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x
}

/// Adds a piece `width` wide to a wrapping row in one go: on a new line when it does
/// not fit in what is left of this one.
pub(crate) fn whole(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    if ui.available_size_before_wrap().x < width && ui.cursor().left() > ui.max_rect().left() + 1.0
    {
        ui.end_row();
    }
    ui.horizontal(add);
}

fn marker_piece(ui: &mut Ui, mark: Mark, color: Color32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    match mark {
        Mark::Circle => {
            painter.circle_filled(c, 4.5, color);
        }
        Mark::Square => {
            painter.rect_filled(egui::Rect::from_center_size(c, vec2(8.0, 8.0)), 0.0, color);
        }
        Mark::Diamond => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + vec2(0.0, -5.5),
                    c + vec2(5.5, 0.0),
                    c + vec2(0.0, 5.5),
                    c + vec2(-5.5, 0.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Mark::Dashed | Mark::Dotted => {
            let (dash, gap) = match mark {
                Mark::Dashed => (3.0, 2.0),
                _ => (1.0, 2.0),
            };
            let mut y = rect.top() + 1.0;
            while y < rect.bottom() - 1.0 {
                let end = (y + dash).min(rect.bottom() - 1.0);
                painter.line_segment(
                    [egui::pos2(c.x, y), egui::pos2(c.x, end)],
                    Stroke::new(1.5, color),
                );
                y = end + gap;
            }
        }
        Mark::Line => {
            painter.line_segment(
                [egui::pos2(rect.left(), c.y), egui::pos2(rect.right(), c.y)],
                Stroke::new(2.0, color),
            );
        }
        Mark::DashedLine => {
            for k in 0..3 {
                let x = rect.left() + 5.0 * k as f32;
                painter.line_segment(
                    [
                        egui::pos2(x, c.y),
                        egui::pos2((x + 3.0).min(rect.right()), c.y),
                    ],
                    Stroke::new(2.0, color),
                );
            }
        }
    }
    ui.add(egui::Label::new(text).extend());
}
