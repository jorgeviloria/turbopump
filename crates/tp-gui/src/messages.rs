//! Errors and warnings of every component, for the tree, the status bar and the
//! messages panel.

use eframe::egui::{self, Align, Layout, RichText, Sense, Ui};
use tp_project::{ComponentId, Project};

use crate::derived::Derived;
use crate::theme::{Palette, component_color, icon};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn glyph(self) -> &'static str {
        match self {
            Severity::Error => icon::X_CIRCLE,
            Severity::Warning => icon::WARNING,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Message {
    pub component: ComponentId,
    pub severity: Severity,
    pub text: String,
}

fn push(out: &mut Vec<Message>, id: ComponentId, error: Option<&String>, warnings: &[String]) {
    if let Some(e) = error {
        out.push(Message {
            component: id,
            severity: Severity::Error,
            text: e.clone(),
        });
    }
    out.extend(warnings.iter().map(|w| Message {
        component: id,
        severity: Severity::Warning,
        text: w.clone(),
    }));
}

/// The messages of component `id`.
pub(crate) fn of_component(derived: &Derived, id: ComponentId) -> Vec<Message> {
    let mut out = Vec::new();
    if let Some(r) = derived.impeller(id) {
        match &r.main {
            Ok(d) => push(&mut out, id, None, &d.warnings),
            Err(e) => push(&mut out, id, Some(e), &[]),
        }
        // A failed main design also fails the geometry: report it once.
        match (&r.main, &r.geometry) {
            (Ok(_), Err(e)) => push(&mut out, id, Some(e), &[]),
            (_, Ok(g)) => push(&mut out, id, None, &g.warnings),
            _ => {}
        }
    }
    match derived.volute_result(id) {
        Some(Ok(g)) => push(&mut out, id, None, &g.warnings),
        Some(Err(e)) => push(&mut out, id, Some(e), &[]),
        None => {}
    }
    match derived.diffuser_result(id) {
        Some(Ok(d)) => push(&mut out, id, None, &d.warnings),
        Some(Err(e)) => push(&mut out, id, Some(e), &[]),
        None => {}
    }
    out
}

/// The messages of the whole project, from inlet to outlet.
pub(crate) fn all(project: &Project, derived: &Derived) -> Vec<Message> {
    project
        .components()
        .iter()
        .flat_map(|c| of_component(derived, c.id()))
        .collect()
}

/// Number of errors and warnings in `messages`.
pub(crate) fn count(messages: &[Message]) -> (usize, usize) {
    let errors = messages
        .iter()
        .filter(|m| m.severity == Severity::Error)
        .count();
    (errors, messages.len() - errors)
}

/// One message as a tinted card.
pub(crate) fn card(ui: &mut Ui, severity: Severity, text: &str) {
    let p = Palette::of(ui);
    let (fill, color) = match severity {
        Severity::Error => (p.error_bg, ui.visuals().error_fg_color),
        Severity::Warning => (p.warning_bg, ui.visuals().warn_fg_color),
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(severity.glyph()).color(color).size(15.0));
                ui.add(egui::Label::new(RichText::new(text).color(p.text)).wrap());
            });
        });
}

/// The messages panel: every message of the project. Returns the component of the
/// message the user clicked.
pub(crate) fn panel(ui: &mut Ui, project: &Project, messages: &[Message]) -> Option<ComponentId> {
    let p = Palette::of(ui);
    let mut clicked = None;
    if messages.is_empty() {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(icon::CHECK_CIRCLE).color(p.done));
            ui.label(RichText::new("No errors or warnings.").color(p.weak));
        });
        return None;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for m in messages {
            let Some(component) = project.component(m.component) else {
                continue;
            };
            let color = match m.severity {
                Severity::Error => ui.visuals().error_fg_color,
                Severity::Warning => ui.visuals().warn_fg_color,
            };
            let row = ui.horizontal(|ui| {
                ui.label(RichText::new(m.severity.glyph()).color(color));
                ui.label(RichText::new(&component.name).color(component_color(component.kind())));
                ui.label(&m.text);
            });
            let response = ui
                .interact(
                    row.response.rect,
                    row.response.id.with("row"),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Go to this component");
            if response.clicked() {
                clicked = Some(m.component);
            }
        }
    });
    clicked
}
