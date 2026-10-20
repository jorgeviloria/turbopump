//! Input widgets for quantities with units and for automatic/manual parameters.
//!
//! Values are stored in SI and shown in the project's display units. The parameter
//! rows are meant for a three-column [`egui::Grid`]: label, value and "auto" toggle.

use std::ops::RangeInclusive;

use eframe::egui::{self, DragValue, Ui};
use tp_design::Param;
use tp_units::{DisplayUnits, QuantityKind, decimals_for};

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
    ui.label(label).on_hover_text(hint);
    let mut changed = false;
    match param {
        Param::Auto => match effective {
            Some(value) => {
                let mut shown = units.to_display(kind, value);
                ui.add_enabled(false, drag(&mut shown, kind, units));
            }
            None => {
                ui.weak("—");
            }
        },
        Param::Manual(value) => {
            changed |= quantity(ui, value, kind, units, range.clone());
        }
    }
    changed |= auto_toggle(ui, param, || {
        effective
            .unwrap_or(*range.start())
            .clamp(*range.start(), *range.end())
    });
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
    ui.label(label).on_hover_text(hint);
    let mut changed = false;
    match param {
        Param::Auto => match effective {
            Some(mut value) => {
                ui.add_enabled(false, DragValue::new(&mut value));
            }
            None => {
                ui.weak("—");
            }
        },
        Param::Manual(value) => {
            changed |= ui
                .add(DragValue::new(value).speed(0.05).range(range.clone()))
                .changed();
        }
    }
    changed |= auto_toggle(ui, param, || {
        effective
            .unwrap_or(*range.start())
            .clamp(*range.start(), *range.end())
    });
    ui.end_row();
    changed
}

fn auto_toggle<T: Copy>(
    ui: &mut Ui,
    param: &mut Param<T>,
    manual_start: impl FnOnce() -> T,
) -> bool {
    let mut auto = param.is_auto();
    let response = ui.checkbox(&mut auto, "auto").on_hover_text(
        "Automatic: the program proposes the value from a correlation and updates it when \
         earlier inputs change. Clear it to fix your own value.",
    );
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
pub fn value_row(ui: &mut Ui, label: &str, value: impl Into<egui::WidgetText>) {
    ui.label(label);
    ui.label(value);
    ui.end_row();
}

/// A label that marks manual values, for reports.
pub fn manual_tag<T>(param: &Param<T>) -> &'static str {
    match param {
        Param::Auto => "",
        Param::Manual(_) => " (manual)",
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

#[cfg(test)]
mod glyph_tests {
    use eframe::egui::{self, FontId};

    /// Every non-ASCII symbol the editor writes must exist in egui's default fonts,
    /// or it is drawn as an empty box.
    #[test]
    fn symbols_used_by_the_editor_have_glyphs() {
        let symbols = "°±²³·éαβγδηθνξρψ–—•…₁₂−≤≥○⚙⟲⏺⏴⏵✚🗙⚑";
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        let missing: String = ctx.fonts_mut(|fonts| {
            symbols
                .chars()
                .filter(|&c| !fonts.has_glyph(&FontId::proportional(14.0), c))
                .collect()
        });
        assert!(missing.is_empty(), "no glyph for {missing:?}");
    }
}
