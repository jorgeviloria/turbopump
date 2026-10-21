//! Property rows and input widgets for quantities with units and for automatic/manual
//! parameters.
//!
//! Values are stored in SI and shown in the project's display units. Every group of
//! properties is a [`props`] grid of two columns: the label, with an ⓘ that tells
//! where the value comes from and, at its right end, just before the input, the check
//! box for "automatic" of the parameters that have one; and the input or the computed
//! value. All grids of the panel share the column widths, so the values line up.

use std::hash::Hash;
use std::ops::RangeInclusive;

use crate::i18n::tr;
use crate::theme::{Palette, icon};
use eframe::egui::{self, Align, DragValue, Layout, RichText, Sense, Ui, vec2};
use tp_design::Param;
use tp_units::{DisplayUnits, QuantityKind, decimals_for};

/// Room for the "automatic" check box at the right end of the label cell.
const AUTO_WIDTH: f32 = 20.0;
/// Space between the columns.
const GAP: f32 = 8.0;
/// Height of a property row.
const ROW_HEIGHT: f32 = 22.0;

/// Widths of the label and value columns of the property grids.
#[derive(Debug, Clone, Copy)]
struct Columns {
    label: f32,
    value: f32,
}

impl Columns {
    /// The columns for a panel `width` points wide.
    fn for_width(width: f32) -> Self {
        // A little slack: rows exactly as wide as the panel would nudge its width
        // through rounding, and the columns would take frames to settle.
        let free = (width - GAP - 12.0).max(160.0);
        let label = (free * 0.56).clamp(100.0, 260.0);
        Self {
            label,
            value: free - label,
        }
    }

    fn id() -> egui::Id {
        egui::Id::new("tp_property_columns")
    }

    /// The columns of the grid being drawn.
    fn of(ui: &Ui) -> Self {
        ui.data(|d| d.get_temp(Self::id()))
            .unwrap_or_else(|| Self::for_width(ui.available_width()))
    }
}

/// A grid of property rows. Inputs, combo boxes and text fields in it fill the value
/// column; build the rows with [`label`], [`param_f64`], [`param_u32`] and [`value_row`].
pub fn props(ui: &mut Ui, id: impl Hash + std::fmt::Debug, add: impl FnOnce(&mut Ui)) {
    let columns = Columns::for_width(ui.available_width());
    ui.data_mut(|d| d.insert_temp(Columns::id(), columns));
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([GAP, 4.0])
        .min_row_height(ROW_HEIGHT)
        .show(ui, |ui| {
            // Long texts end in "…" rather than widen their column.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            let spacing = ui.spacing_mut();
            spacing.interact_size.x = columns.value;
            spacing.combo_width = columns.value;
            spacing.text_edit_width = columns.value - 2.0 * spacing.button_padding.x;
            add(ui);
        });
}

/// An ⓘ that shows `text` when the pointer rests on it.
pub fn info(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui);
    let response = ui.add(
        egui::Label::new(RichText::new(icon::INFO).size(13.0).color(p.weak)).sense(Sense::hover()),
    );
    if response.hovered() {
        ui.painter().text(
            response.rect.center(),
            egui::Align2::CENTER_CENTER,
            icon::INFO,
            egui::FontId::proportional(13.0),
            p.accent,
        );
    }
    response.on_hover_ui(|ui| {
        ui.set_max_width(320.0);
        ui.label(text);
    });
}

/// The label cell of a property row, with an ⓘ for `hint` when there is one, and the
/// empty "automatic" cell after it: the input or value comes next.
pub fn label(ui: &mut Ui, text: &str, hint: &str) {
    label_cell(ui, text, hint, None);
}

/// The label cell; with `auto`, the "automatic" check box at its right end, just
/// before the input. Returns the check box's response.
fn label_cell(
    ui: &mut Ui,
    text: &str,
    hint: &str,
    auto: Option<&mut bool>,
) -> Option<egui::Response> {
    let width = Columns::of(ui).label;
    ui.allocate_ui_with_layout(
        vec2(width, ROW_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing.x = 4.0;
            let room = if hint.is_empty() { 0.0 } else { 18.0 } + AUTO_WIDTH;
            ui.scope(|ui| {
                ui.set_max_width(width - room);
                ui.add(egui::Label::new(text).truncate());
            });
            if !hint.is_empty() {
                info(ui, hint);
            }
            auto.map(|auto| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().interact_size.x = 0.0;
                    ui.add(egui::Checkbox::without_text(auto))
                })
                .inner
            })
        },
    )
    .inner
}

/// A one-line text field as wide as the value column, or as the half of it that
/// [`halves`] left.
pub fn text_field(ui: &mut Ui, text: &mut String) -> egui::Response {
    let width = ui.spacing().text_edit_width + 2.0 * ui.spacing().button_padding.x;
    ui.add_sized([width, ROW_HEIGHT - 2.0], egui::TextEdit::singleline(text))
}

/// Splits the value column between two controls side by side, in a horizontal
/// layout inside it.
pub fn halves(ui: &mut Ui) {
    let width = Columns::of(ui).value;
    let half = ((width - ui.spacing().item_spacing.x) / 2.0).floor();
    let spacing = ui.spacing_mut();
    spacing.interact_size.x = half;
    spacing.combo_width = half;
    spacing.text_edit_width = half - 2.0 * spacing.button_padding.x;
    // The pair shows its words whole.
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
}

/// A computed value in the value column: a flat field, so results read apart from
/// the inputs but line up with them.
pub fn value(ui: &mut Ui, text: impl Into<egui::WidgetText>) {
    let p = Palette::of(ui);
    let width = Columns::of(ui).value;
    let fill = if ui.visuals().dark_mode {
        egui::Color32::from_white_alpha(9)
    } else {
        p.strip.gamma_multiply(0.7)
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_width(width - 12.0);
            ui.set_min_height(ROW_HEIGHT - 4.0);
            // Centred, like the text of the inputs above and below.
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add(egui::Label::new(text).truncate());
            });
        });
}

/// Drag value showing `display` (already in display units) with its unit symbol.
fn drag<'a>(display: &'a mut f64, kind: QuantityKind, units: &DisplayUnits) -> DragValue<'a> {
    let symbol = units.get(kind).symbol;
    let magnitude = display.abs();
    let speed = if magnitude > 1e-9 {
        magnitude * 0.003
    } else {
        0.01
    };
    let decimals = decimals_for(*display);
    let widget = DragValue::new(display)
        .speed(speed)
        .min_decimals(decimals.min(2))
        .max_decimals(decimals + 1)
        .update_while_editing(false);
    if symbol == "-" {
        widget
    } else {
        widget.suffix(format!(" {symbol}"))
    }
}

/// Range in SI converted to display units.
fn display_range(
    range: &RangeInclusive<f64>,
    kind: QuantityKind,
    units: &DisplayUnits,
) -> RangeInclusive<f64> {
    units.to_display(kind, *range.start())..=units.to_display(kind, *range.end())
}

/// An editable quantity stored in SI. Returns whether the value changed.
pub fn quantity(
    ui: &mut Ui,
    si: &mut f64,
    kind: QuantityKind,
    units: &DisplayUnits,
    range: RangeInclusive<f64>,
) -> bool {
    let mut shown = units.to_display(kind, *si);
    let response = ui.add(drag(&mut shown, kind, units).range(display_range(&range, kind, units)));
    if response.changed() {
        *si = units.from_display(kind, shown);
        true
    } else {
        false
    }
}

/// One grid row for a real-valued parameter: label, value and "auto" toggle.
///
/// `effective` is the value the design used (the automatic one in automatic mode);
/// switching to manual starts from it. Returns whether the parameter changed.
#[allow(clippy::too_many_arguments)]
pub fn param_f64(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    param: &mut Param<f64>,
    kind: QuantityKind,
    effective: Option<f64>,
    units: &DisplayUnits,
    range: RangeInclusive<f64>,
) -> bool {
    let mut changed = auto_toggle(ui, label, hint, param, || {
        effective
            .unwrap_or(*range.start())
            .clamp(*range.start(), *range.end())
    });
    match param {
        Param::Auto => match effective {
            Some(value) => {
                let mut shown = units.to_display(kind, value);
                ui.add_enabled(false, drag(&mut shown, kind, units));
            }
            None => value(ui, "—"),
        },
        Param::Manual(value) => {
            changed |= quantity(ui, value, kind, units, range.clone());
        }
    }
    ui.end_row();
    changed
}

/// One grid row for a whole-number parameter, like [`param_f64`].
pub fn param_u32(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    param: &mut Param<u32>,
    effective: Option<u32>,
    range: RangeInclusive<u32>,
) -> bool {
    let mut changed = auto_toggle(ui, label, hint, param, || {
        effective
            .unwrap_or(*range.start())
            .clamp(*range.start(), *range.end())
    });
    match param {
        Param::Auto => match effective {
            Some(mut value) => {
                ui.add_enabled(false, DragValue::new(&mut value));
            }
            None => value(ui, "—"),
        },
        Param::Manual(value) => {
            changed |= ui
                .add(DragValue::new(value).speed(0.05).range(range.clone()))
                .changed();
        }
    }
    ui.end_row();
    changed
}

/// The label cell of a parameter, with its "automatic" check box. Returns whether
/// the parameter changed.
fn auto_toggle<T: Copy>(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    param: &mut Param<T>,
    manual_start: impl FnOnce() -> T,
) -> bool {
    let mut auto = param.is_auto();
    let Some(response) = label_cell(ui, label, hint, Some(&mut auto)) else {
        return false;
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, auto, tr("auto"))
    });
    let response = response.on_hover_text(tr(
        "Automatic: the program proposes the value from a correlation and updates it when \
         earlier inputs change. Clear it to fix your own value.",
    ));
    if response.changed() {
        *param = if auto {
            Param::Auto
        } else {
            Param::Manual(manual_start())
        };
        true
    } else {
        false
    }
}

/// One grid row with a computed value.
pub fn value_row(ui: &mut Ui, label: &str, text: impl Into<egui::WidgetText>) {
    value_info(ui, label, "", text);
}

/// One grid row with a computed value and an ⓘ that tells where it comes from.
pub fn value_info(ui: &mut Ui, label: &str, hint: &str, text: impl Into<egui::WidgetText>) {
    self::label(ui, label, hint);
    value(ui, text);
    ui.end_row();
}

/// A label that marks manual values, for reports.
pub fn manual_tag<T>(param: &Param<T>) -> &'static str {
    match param {
        Param::Auto => "",
        Param::Manual(_) => tr(" (manual)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_ranges_follow_the_unit() {
        let si = DisplayUnits::si();
        let r = display_range(&(0.001..=0.5), QuantityKind::Length, &si);
        assert!((r.start() - 1.0).abs() < 1e-9 && (r.end() - 500.0).abs() < 1e-9);
        let us = DisplayUnits::us();
        let t = display_range(&(273.15..=373.15), QuantityKind::Temperature, &us);
        assert!((t.start() - 32.0).abs() < 1e-9 && (t.end() - 212.0).abs() < 1e-9);
    }

    #[test]
    fn manual_tag_marks_only_manual_values() {
        assert_eq!(manual_tag(&Param::<f64>::Auto), "");
        assert_eq!(manual_tag(&Param::Manual(1.0)), " (manual)");
    }
}
