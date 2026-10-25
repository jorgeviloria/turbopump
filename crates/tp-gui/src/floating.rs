//! The look of the panels that float over the views: the properties of a step and the
//! scene of the 3D view. One frame and one header for both, so they read as a family.

use eframe::egui::{self, Frame, RichText, Stroke, Style, Ui};

use crate::theme::{Palette, bold};

/// Corner radius of a floating panel.
pub(crate) const RADIUS: u8 = 10;
/// Height of its header.
pub(crate) const HEADER: f32 = 32.0;

/// The frame of a floating panel in `style`: window fill, a thin border, rounded
/// corners and the shadow of popups.
pub(crate) fn frame(style: &Style) -> Frame {
    let p = Palette::for_dark(style.visuals.dark_mode);
    Frame::new()
        .fill(style.visuals.window_fill)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(RADIUS)
        .shadow(style.visuals.popup_shadow)
}

/// The header of a floating panel: an icon and a title on the left, `actions` from the
/// right edge leftwards, and a rule under it. Returns the response of the header row,
/// which senses drags when `draggable`.
pub(crate) fn header(
    ui: &mut Ui,
    glyph: &str,
    title: &str,
    draggable: bool,
    actions: impl FnOnce(&mut Ui),
) -> egui::Response {
    let p = Palette::of(ui);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), HEADER),
        if draggable {
            egui::Sense::drag()
        } else {
            egui::Sense::hover()
        },
    );
    let response = if draggable {
        response.on_hover_cursor(egui::CursorIcon::Grab)
    } else {
        response
    };
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new(glyph).size(15.0).color(p.weak));
            ui.add(
                egui::Label::new(bold(title).size(14.0))
                    .truncate()
                    .selectable(false),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
        });
    });
    ui.painter()
        .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.border));
    ui.add_space(4.0);
    response
}
