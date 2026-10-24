//! Property rows and input widgets for quantities with units and for automatic/manual
//! parameters.
//!
//! Values are stored in SI and shown in the project's display units. Every group of
//! properties is a [`props`] grid of two columns: the label, with an ⓘ that tells
//! where the value comes from and, at its right end, just before the input, the mark
//! of the parameters that can be automatic: a hollow dot while the program proposes
//! the value, a filled one once it is set by hand, as the points of the meridian
//! editor; and the input or the computed value. All grids of the panel share the
//! column widths, so the values line up.

use std::hash::Hash;
use std::ops::RangeInclusive;

use crate::i18n::{tf, tr};
use crate::theme::{Palette, icon};
use eframe::egui::{self, Align, DragValue, Layout, RichText, Sense, Ui, vec2};
use tp_design::Param;
use tp_units::{DisplayUnits, QuantityKind, decimals_for};

/// Room for the automatic/manual dot at the right end of the label cell.
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
        // The value column reaches the right edge; a hair of slack keeps rounding from
        // nudging the panel.
        let free = (width - GAP - 2.0).max(160.0);
        let label = (free * 0.52).clamp(100.0, 360.0);
        Self {
            label,
            value: free - label,
        }
    }

    fn id() -> egui::Id {
        egui::Id::new("tp_property_columns")
    }

    /// Where a panel keeps the columns all its grids share.
    fn panel_id() -> egui::Id {
        egui::Id::new("tp_panel_columns")
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
    let columns = ui
        .data(|d| d.get_temp::<Columns>(Columns::panel_id()))
        .unwrap_or_else(|| Columns::for_width(ui.available_width()));
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
            // Lists and text fields as wide as the numeric fields, which leave the
            // rest of the column to their unit: all line up on both edges.
            let field = columns.value - UNIT_WIDTH - 4.0;
            spacing.combo_width = field;
            spacing.text_edit_width = field - 2.0 * spacing.button_padding.x;
            add(ui);
        });
}

/// Columns of a [`table`]: the label, and the value field with its unit.
const TABLE_COLUMNS: Columns = Columns {
    label: 280.0,
    value: 200.0,
};

/// A striped table of rows for documents and dialogs, such as the report: labels, then
/// values in fields of one width with their units after them, whatever the property
/// panel drew before. Build the rows with [`value_row`] and the other row helpers.
pub fn table(ui: &mut Ui, id: impl Hash + std::fmt::Debug, add: impl FnOnce(&mut Ui)) {
    let columns = TABLE_COLUMNS;
    ui.data_mut(|d| d.insert_temp(Columns::id(), columns));
    egui::Grid::new(id)
        .num_columns(2)
        .striped(true)
        .spacing([GAP, 4.0])
        .min_row_height(ROW_HEIGHT)
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            let spacing = ui.spacing_mut();
            spacing.interact_size.x = columns.value;
            let field = columns.value - UNIT_WIDTH - 4.0;
            spacing.combo_width = field;
            spacing.text_edit_width = field - 2.0 * spacing.button_padding.x;
            add(ui);
        });
    ui.data_mut(|d| d.remove::<Columns>(Columns::id()));
}

/// Draws `add` with every grid of rows in it on the same columns, worked out from the
/// width of `ui`: labels, fields, lists and units line up down the whole panel, whatever
/// frames or indents a group sits in.
pub fn shared_columns<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    // The scroll bar floats over the content: a little room under it.
    let columns = Columns::for_width(ui.available_width() - 4.0);
    ui.data_mut(|d| d.insert_temp(Columns::panel_id(), columns));
    let inner = add(ui);
    ui.data_mut(|d| d.remove::<Columns>(Columns::panel_id()));
    inner
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
    label_cell(ui, text, hint, None, None);
}

/// The automatic/manual mark of a parameter: a hollow dot while automatic, a filled
/// one when set by hand. A click switches it. Screen readers see the check box
/// "auto", ticked while automatic.
pub(crate) fn auto_dot(ui: &mut Ui, auto: &mut bool) -> egui::Response {
    let p = Palette::of(ui);
    let (rect, mut response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    if response.clicked() {
        *auto = !*auto;
        response.mark_changed();
    }
    let center = rect.center();
    let painter = ui.painter();
    if response.hovered() {
        painter.circle_filled(center, 8.0, p.hover);
    }
    if *auto {
        painter.circle_stroke(center, 4.0, egui::Stroke::new(1.6, p.weak));
    } else {
        painter.circle_filled(center, 4.5, p.accent);
    }
    response
}

/// Where the property rows and the drawings tell each other what the pointer is on, by
/// the row's label: the row under it, the dimension under it.
fn hover_id(what: &str) -> egui::Id {
    egui::Id::new(("hover_link", what))
}

/// The label of the property row under the pointer, this frame or the last.
pub(crate) fn hovered_row(ctx: &egui::Context) -> Option<String> {
    let now = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<(u64, String)>(hover_id("row")))
        .filter(|(pass, _)| now <= pass + 1)
        .map(|(_, label)| label)
}

/// Tells the property rows which of them a drawing has under the pointer.
pub(crate) fn set_drawing_hover(ctx: &egui::Context, label: Option<String>) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| match label {
        Some(label) => {
            d.insert_temp(hover_id("drawing"), (pass, label));
        }
        None => d.remove::<(u64, String)>(hover_id("drawing")),
    });
}

/// Whether a drawing has the row `label` under the pointer, this frame or the last.
pub(crate) fn drawing_hovers(ctx: &egui::Context, label: &str) -> bool {
    let now = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<(u64, String)>(hover_id("drawing")))
        .is_some_and(|(pass, l)| now <= pass + 1 && l == label)
}

/// The label cell; with `auto`, the automatic/manual dot at its right end, just
/// before the input. With `shown`, what the row shows: a bar in the margin marks it
/// when it changed since the step was accepted. Returns the dot's response.
fn label_cell(
    ui: &mut Ui,
    text: &str,
    hint: &str,
    auto: Option<&mut bool>,
    shown: Option<&Shown<'_>>,
) -> Option<egui::Response> {
    let columns = Columns::of(ui);
    let width = columns.label;
    let was = shown.and_then(|v| crate::steps::changed_since_accepted(ui, text, v));
    // The whole row, label and value: shaded while a drawing points at its dimension,
    // and published while the pointer is on it.
    let row = egui::Rect::from_min_size(
        ui.cursor().min,
        vec2(columns.label + GAP + columns.value, ROW_HEIGHT),
    );
    if drawing_hovers(ui.ctx(), text) {
        let p = Palette::of(ui);
        ui.painter()
            .rect_filled(row.expand2(vec2(4.0, 1.0)), 4, p.hover);
    }
    if ui.rect_contains_pointer(row) {
        let pass = ui.ctx().cumulative_pass_nr();
        ui.data_mut(|d| d.insert_temp(hover_id("row"), (pass, text.to_owned())));
    }
    // Room for the ⓘ of the hint and the automatic/manual dot, only where they are.
    let room =
        if hint.is_empty() { 0.0 } else { 18.0 } + if auto.is_some() { AUTO_WIDTH } else { 0.0 };
    let cell = ui.allocate_ui_with_layout(
        vec2(width, ROW_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.scope(|ui| {
                ui.set_max_width(width - room);
                ui.add(egui::Label::new(text).truncate());
            });
            if !hint.is_empty() {
                info(ui, hint);
            }
            auto.map(|auto| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    auto_dot(ui, auto)
                })
                .inner
            })
        },
    );
    if let Some(was) = was {
        let p = Palette::of(ui);
        let r = cell.response.rect;
        let bar = egui::Rect::from_min_max(
            egui::pos2(r.left() - 8.0, r.top() + 3.0),
            egui::pos2(r.left() - 5.0, r.bottom() - 3.0),
        );
        ui.painter().rect_filled(bar, 1, p.stale);
        let label = tf("{} changed since the step was accepted", &[&text]);
        let mark = ui.interact(
            bar.expand(3.0),
            ui.id().with(("changed", text)),
            Sense::hover(),
        );
        mark.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
        mark.on_hover_text(tf(
            "Changed since the step was accepted, when it was {}",
            &[&was],
        ));
    }
    cell.inner
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
    let text: egui::WidgetText = text.into();
    let plain = text.text().to_owned();
    match split_unit(&plain) {
        Some((number, unit)) => {
            let number = number.to_owned();
            with_unit(ui, unit, |ui| value_box(ui, number.into()))
        }
        None => with_unit(ui, "", |ui| value_box(ui, text)),
    };
}

/// A number and the unit after it ("3123 mm²" → "3123", "mm²"), so the unit can go
/// outside the field like that of an input.
fn split_unit(text: &str) -> Option<(&str, &str)> {
    let (number, unit) = text.rsplit_once(' ')?;
    let digits = number.trim_start_matches(['±', '+', ' ']);
    (digits.parse::<f64>().is_ok()
        && !unit.is_empty()
        && !unit.starts_with(|c: char| c.is_ascii_digit()))
    .then_some((number, unit))
}

/// The flat box of a computed value, as wide as an input.
fn value_box(ui: &mut Ui, text: egui::WidgetText) -> egui::Response {
    let p = Palette::of(ui);
    let width = ui.spacing().interact_size.x;
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
        })
        .response
}

/// Width of the unit after a numeric field: room for the longest symbols, such as
/// mm²/s, lb/ft³ or Btu/lb.
const UNIT_WIDTH: f32 = 44.0;

/// A numeric field and, after it and outside it, its `unit` in the weak color, as
/// engineering programs show them. The field leaves room for a unit even when there is
/// none, so the fields of a column line up and their units start at one edge.
pub fn with_unit(
    ui: &mut Ui,
    unit: &str,
    field: impl FnOnce(&mut Ui) -> egui::Response,
) -> egui::Response {
    let full = ui.spacing().interact_size.x;
    let weak = Palette::of(ui).weak;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.spacing_mut().interact_size.x = (full - UNIT_WIDTH - 4.0).max(36.0);
        let response = field(ui);
        ui.allocate_ui_with_layout(
            vec2(UNIT_WIDTH, ROW_HEIGHT - 2.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(UNIT_WIDTH);
                if !unit.is_empty() {
                    ui.add(egui::Label::new(RichText::new(unit).color(weak)).truncate());
                }
            },
        );
        response
    })
    .inner
}

/// A field and its `unit` right after it, in the weak color, where no column is kept
/// (menus and editors on the drawings).
pub fn then_unit(
    ui: &mut Ui,
    unit: &str,
    field: impl FnOnce(&mut Ui) -> egui::Response,
) -> egui::Response {
    let response = field(ui);
    let weak = Palette::of(ui).weak;
    ui.label(RichText::new(unit).color(weak));
    response
}

/// Keeps a list to the width of its column however long its text, which a combo box
/// would otherwise stretch to the edge of the panel.
pub fn list<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.spacing().combo_width;
    ui.data_mut(|d| d.remove::<String>(list_text_id()));
    let inner = ui.allocate_ui_with_layout(
        vec2(width, ROW_HEIGHT - 2.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_max_width(width);
            add(ui)
        },
    );
    // A choice cut to fit shows whole on hover.
    if let Some(text) = ui.data_mut(|d| d.remove_temp::<String>(list_text_id())) {
        let font = egui::TextStyle::Button.resolve(ui.style());
        let room = width
            - ui.spacing().icon_width
            - ui.spacing().icon_spacing
            - 2.0 * ui.spacing().button_padding.x;
        let needed = ui
            .painter()
            .layout_no_wrap(text.clone(), font, egui::Color32::WHITE)
            .size()
            .x;
        if needed > room {
            inner.response.on_hover_text(text);
        }
    }
    inner.inner
}

/// The value of a wire drawn by hand: how many segments it has and a button back to
/// the automatic one at the right, both within the width of a field so the row keeps to
/// the columns of the others. Returns whether the button was clicked.
pub fn drawn_wire(ui: &mut Ui, count: &str) -> bool {
    let width = ui.spacing().combo_width;
    let mut clicked = false;
    ui.allocate_ui_with_layout(
        vec2(width + UNIT_WIDTH + 4.0, ROW_HEIGHT - 2.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.allocate_ui_with_layout(
                vec2(width, ROW_HEIGHT - 2.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_width(width);
                    ui.add(egui::Label::new(count).extend());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        clicked = ui.button(tr("Automatic")).clicked();
                    });
                },
            );
        },
    );
    clicked
}

fn list_text_id() -> egui::Id {
    egui::Id::new("tp_list_text")
}

/// The text a list shows, noted so that [`list`] can show it whole on hover when it does
/// not fit.
pub fn noted(ui: &Ui, text: impl Into<egui::WidgetText>) -> egui::WidgetText {
    let text = text.into();
    ui.data_mut(|d| d.insert_temp(list_text_id(), text.text().to_owned()));
    text
}

/// The unit of `kind` to show after a field: none for a plain number.
fn unit_of(kind: QuantityKind, units: &DisplayUnits) -> &'static str {
    match units.get(kind).symbol {
        "-" => "",
        symbol => symbol,
    }
}

/// Drag value showing `display`, already in display units; its unit goes after it.
fn drag(display: &mut f64) -> DragValue<'_> {
    let magnitude = display.abs();
    let speed = if magnitude > 1e-9 {
        magnitude * 0.003
    } else {
        0.01
    };
    let decimals = decimals_for(*display);
    DragValue::new(display)
        .speed(speed)
        .min_decimals(decimals.min(2))
        .max_decimals(decimals + 1)
        .update_while_editing(false)
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
    quantity_response(ui, si, kind, units, range).changed()
}

/// [`quantity`], returning the response of its field.
fn quantity_response(
    ui: &mut Ui,
    si: &mut f64,
    kind: QuantityKind,
    units: &DisplayUnits,
    range: RangeInclusive<f64>,
) -> egui::Response {
    let mut shown = units.to_display(kind, *si);
    let range = display_range(&range, kind, units);
    let response = with_unit(ui, unit_of(kind, units), |ui| {
        ui.add(drag(&mut shown).range(range))
    });
    if response.changed() {
        *si = units.from_display(kind, shown);
    }
    response
}

/// The value of an automatic parameter: its field, in grey, still editable; typing or
/// dragging sets it by hand. Returns the response and the value it shows.
fn automatic<T>(
    ui: &mut Ui,
    add: impl FnOnce(&mut Ui) -> (egui::Response, T),
) -> (egui::Response, T) {
    let weak = Palette::of(ui).weak;
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(weak);
        add(ui)
    })
    .inner
}

/// The menu of a manual value: back to automatic. Returns whether it was chosen.
fn back_to_automatic(response: &egui::Response) -> bool {
    let mut chosen = false;
    response.context_menu(|ui| {
        if ui
            .button(format!(
                "{}  {}",
                icon::ARROW_COUNTER_CLOCKWISE,
                tr("Back to automatic")
            ))
            .clicked()
        {
            chosen = true;
            ui.close();
        }
    });
    chosen
}

/// What a parameter row shows, to compare with what it showed when its step was
/// accepted.
pub(crate) struct Shown<'a> {
    pub value: tp_project::AcceptedValue,
    /// The units a quantity shows in, to compare and write it as shown.
    pub units: Option<&'a DisplayUnits>,
}

impl<'a> Shown<'a> {
    /// A quantity of `kind`, `si` in SI, shown in `units`.
    fn quantity(kind: QuantityKind, si: f64, units: &'a DisplayUnits) -> Self {
        Self {
            value: tp_project::AcceptedValue {
                key: format!("{si:e}"),
                text: units.show(kind, si),
                quantity: Some((kind, si)),
                automatic: false,
            },
            units: Some(units),
        }
    }

    /// Anything else: `text` as shown, compared by `key`.
    fn text(key: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            value: tp_project::AcceptedValue {
                key: key.into(),
                text: text.into(),
                quantity: None,
                automatic: false,
            },
            units: None,
        }
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
    let shown = match param {
        Param::Auto => effective,
        Param::Manual(v) => Some(*v),
    }
    .map(|v| Shown::quantity(kind, v, units));
    let mut changed = auto_toggle(ui, label, hint, param, shown, || {
        effective
            .unwrap_or(*range.start())
            .clamp(*range.start(), *range.end())
    });
    match param {
        Param::Auto => match effective {
            Some(value) => {
                let (response, edited) = automatic(ui, |ui| {
                    let mut v = value;
                    let r = quantity_response(ui, &mut v, kind, units, range.clone());
                    (r, v)
                });
                if response.changed() {
                    *param = Param::Manual(edited);
                    changed = true;
                }
            }
            None => self::value(ui, "—"),
        },
        Param::Manual(value) => {
            let response = quantity_response(ui, value, kind, units, range.clone());
            changed |= response.changed();
            if back_to_automatic(&response) {
                *param = Param::Auto;
                changed = true;
            }
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
    let shown = match param {
        Param::Auto => effective,
        Param::Manual(v) => Some(*v),
    }
    .map(|v| Shown::text(v.to_string(), v.to_string()));
    let mut changed = auto_toggle(ui, label, hint, param, shown, || {
        effective
            .unwrap_or(*range.start())
            .clamp(*range.start(), *range.end())
    });
    match param {
        Param::Auto => match effective {
            Some(value) => {
                let (response, edited) = automatic(ui, |ui| {
                    let mut v = value;
                    let r = with_unit(ui, "", |ui| {
                        ui.add(DragValue::new(&mut v).speed(0.05).range(range.clone()))
                    });
                    (r, v)
                });
                if response.changed() {
                    *param = Param::Manual(edited);
                    changed = true;
                }
            }
            None => self::value(ui, "—"),
        },
        Param::Manual(value) => {
            let response = with_unit(ui, "", |ui| {
                ui.add(DragValue::new(value).speed(0.05).range(range.clone()))
            });
            changed |= response.changed();
            if back_to_automatic(&response) {
                *param = Param::Auto;
                changed = true;
            }
        }
    }
    ui.end_row();
    changed
}

/// A drag value for a ± percentage.
fn percent(value: &mut f64) -> DragValue<'_> {
    DragValue::new(value)
        .speed(0.05)
        .max_decimals(2)
        .prefix("± ")
}

/// One grid row for a fraction shown in percent, like [`param_f64`]. `range` is in
/// fractions.
pub fn param_percent(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    param: &mut Param<f64>,
    effective: Option<f64>,
    range: RangeInclusive<f64>,
) -> bool {
    let shown = match param {
        Param::Auto => effective,
        Param::Manual(v) => Some(*v),
    }
    .map(|v| Shown::text(format!("{v:e}"), format!("± {:.2} %", v * 100.0)));
    let mut changed = auto_toggle(ui, label, hint, param, shown, || {
        effective
            .unwrap_or(*range.start())
            .clamp(*range.start(), *range.end())
    });
    let percent_range = range.start() * 100.0..=range.end() * 100.0;
    match param {
        Param::Auto => match effective {
            Some(value) => {
                let (response, edited) = automatic(ui, |ui| {
                    let mut shown = value * 100.0;
                    let r = with_unit(ui, "%", |ui| {
                        ui.add(percent(&mut shown).range(percent_range.clone()))
                    });
                    (r, shown / 100.0)
                });
                if response.changed() {
                    *param = Param::Manual(edited);
                    changed = true;
                }
            }
            None => self::value(ui, "—"),
        },
        Param::Manual(value) => {
            let mut shown = *value * 100.0;
            let response = with_unit(ui, "%", |ui| {
                ui.add(percent(&mut shown).range(percent_range))
            });
            if response.changed() {
                *value = shown / 100.0;
                changed = true;
            }
            if back_to_automatic(&response) {
                *param = Param::Auto;
                changed = true;
            }
        }
    }
    ui.end_row();
    changed
}

/// One grid row for a parameter chosen from `options`, like [`param_f64`]; `name`
/// writes an option.
pub fn param_choice<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    param: &mut Param<T>,
    effective: Option<T>,
    options: &[T],
    name: impl Fn(T) -> String,
) -> bool {
    let Some(&first) = options.first() else {
        return false;
    };
    // Compared by the English name, so that another language changes nothing.
    let shown = match param {
        Param::Auto => effective,
        Param::Manual(v) => Some(*v),
    }
    .map(|v| {
        let text = name(v);
        Shown::text(crate::i18n::key_of(&text).to_owned(), text)
    });
    let mut changed = auto_toggle(ui, label, hint, param, shown, || effective.unwrap_or(first));
    match param {
        Param::Auto => match effective {
            Some(value) => {
                let weak = Palette::of(ui).weak;
                let mut chosen = None;
                list(ui, |ui| {
                    egui::ComboBox::from_id_salt(("param_choice", label))
                        .selected_text(noted(ui, egui::RichText::new(name(value)).color(weak)))
                        .show_ui(ui, |ui| {
                            for &option in options {
                                if ui.selectable_label(option == value, name(option)).clicked() {
                                    chosen = Some(option);
                                }
                            }
                        })
                });
                if let Some(option) = chosen {
                    *param = Param::Manual(option);
                    changed = true;
                }
            }
            None => self::value(ui, "—"),
        },
        Param::Manual(value) => {
            list(ui, |ui| {
                egui::ComboBox::from_id_salt(("param_choice", label))
                    .selected_text(noted(ui, name(*value)))
                    .show_ui(ui, |ui| {
                        for &option in options {
                            changed |= ui.selectable_value(value, option, name(option)).changed();
                        }
                    })
            });
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
    shown: Option<Shown<'_>>,
    manual_start: impl FnOnce() -> T,
) -> bool {
    let mut auto = param.is_auto();
    // Automatic or by hand is part of what the row shows.
    let shown = shown.map(|mut v| {
        v.value.automatic = auto;
        v
    });
    let Some(response) = label_cell(ui, label, hint, Some(&mut auto), shown.as_ref()) else {
        return false;
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, auto, tr("auto"))
    });
    let response = response.on_hover_text(if auto {
        tr("Automatic: the program proposes the value and updates it when earlier inputs change. Type a value, or click the dot, to set it by hand.")
    } else {
        tr("Set by hand. Click the dot, or right-click the value, to make it automatic again.")
    });
    if response.changed() {
        *param = if auto {
            Param::Auto
        } else {
            Param::Manual(manual_start())
        };
        return true;
    }
    // The section this row is in counts it, or asks for it back to automatic.
    if !param.is_auto()
        && let Some(section) = crate::steps::current_section(ui)
    {
        if crate::steps::section_to_automatic(ui, section) {
            *param = Param::Auto;
            return true;
        }
        crate::steps::count_manual(ui, section);
    }
    false
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
    fn computed_values_give_their_unit_up() {
        assert_eq!(split_unit("3123 mm²"), Some(("3123", "mm²")));
        assert_eq!(split_unit("-0.5 m/s"), Some(("-0.5", "m/s")));
        assert_eq!(split_unit("± 3.00 %"), Some(("± 3.00", "%")));
        assert_eq!(split_unit("1.60 : 1"), None);
        assert_eq!(split_unit("radial"), None);
        assert_eq!(split_unit("7 3"), None);
    }

    #[test]
    fn manual_tag_marks_only_manual_values() {
        assert_eq!(manual_tag(&Param::<f64>::Auto), "");
        assert_eq!(manual_tag(&Param::Manual(1.0)), " (manual)");
    }

    /// A row and a drawing hear each other for this frame and the next, then forget.
    #[test]
    fn the_hover_link_lasts_one_frame() {
        let ctx = egui::Context::default();
        let pass = |ctx: &egui::Context| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
            output.textures_delta.clear();
        };
        pass(&ctx);
        set_drawing_hover(&ctx, Some("Axial length L".into()));
        let now = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(hover_id("row"), (now, "Outlet width b₂".to_owned())));
        assert!(drawing_hovers(&ctx, "Axial length L"));
        assert!(!drawing_hovers(&ctx, "Outlet width b₂"));
        assert_eq!(hovered_row(&ctx).as_deref(), Some("Outlet width b₂"));
        pass(&ctx);
        assert!(drawing_hovers(&ctx, "Axial length L"));
        pass(&ctx);
        assert!(!drawing_hovers(&ctx, "Axial length L"));
        assert!(hovered_row(&ctx).is_none());
        set_drawing_hover(&ctx, None);
        assert!(!drawing_hovers(&ctx, "Axial length L"));
    }
}
