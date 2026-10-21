//! The "About" window and the confirmation before discarding unsaved changes. Both
//! share one look: a header with a pictogram and a title, the body, and a row of
//! buttons on the right with the main action last, filled with the accent.

use eframe::egui::{self, Align, Button, Layout, Modal, RichText, ScrollArea, Ui, vec2};
use tp_project::ComponentKind;

use crate::i18n::{tf, tr};
use crate::icons;
use crate::theme::{Palette, bold, component_color, icon};

/// The project page, for the "About" window and the help menu.
pub(crate) const PROJECT_PAGE: &str = "https://github.com/jorgeviloria/turbopump";

/// Software the editor is built with: name, what it does here, license and page.
const BUILT_WITH: &[(&str, &str, &str, &str)] = &[
    (
        "Rust",
        "Language of the whole program",
        "MIT / Apache-2.0",
        "https://www.rust-lang.org",
    ),
    (
        "egui · eframe",
        "Interface and windows",
        "MIT / Apache-2.0",
        "https://github.com/emilk/egui",
    ),
    (
        "egui_plot",
        "Plots and curves",
        "MIT / Apache-2.0",
        "https://github.com/emilk/egui_plot",
    ),
    (
        "wgpu",
        "3D view on the graphics card",
        "MIT / Apache-2.0",
        "https://wgpu.rs",
    ),
    (
        "Open CASCADE",
        "CAD solids, STEP, IGES and STL",
        "LGPL-2.1 with exception",
        "https://dev.opencascade.org",
    ),
    (
        "CoolProp",
        "Fluid properties",
        "MIT",
        "http://www.coolprop.org",
    ),
    (
        "dxf",
        "DXF drawings",
        "MIT",
        "https://github.com/IxMilia/dxf-rs",
    ),
    (
        "acadrust",
        "DWG drawings",
        "MPL-2.0",
        "https://crates.io/crates/acadrust",
    ),
    (
        "Noto Sans",
        "Typeface",
        "OFL-1.1",
        "https://fonts.google.com/noto",
    ),
    ("Phosphor", "Icons", "MIT", "https://phosphoricons.com"),
];

/// The books the design methods come from.
const REFERENCES: &[&str] = &[
    "J. F. Gülich, Centrifugal Pumps, 2nd ed., Springer",
    "A. J. Stepanoff, Centrifugal and Axial Flow Pumps, 2nd ed., Wiley",
    "D. Japikse, W. D. Marscher, R. B. Furst, Centrifugal Pump Design and Performance, Concepts ETI",
    "E. Carnicer Royo, C. Mainar Hasta, Bombas centrífugas, Paraninfo",
];

/// The header of a dialog: a pictogram drawn by `glyph`, the title and a subtitle.
pub(crate) fn header(ui: &mut Ui, glyph: impl FnOnce(&mut Ui), title: &str, subtitle: &str) {
    let p = Palette::of(ui);
    ui.horizontal(|ui| {
        glyph(ui);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(bold(title).size(18.0));
            if !subtitle.is_empty() {
                ui.label(RichText::new(subtitle).color(p.weak));
            }
        });
    });
    ui.add_space(8.0);
}

/// A heading inside a dialog.
fn heading(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui);
    ui.add_space(10.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(p.weak),
    );
    ui.add_space(2.0);
}

/// The main action of a dialog, filled with the accent.
pub(crate) fn primary(ui: &mut Ui, text: &str) -> bool {
    let p = Palette::of(ui);
    ui.add(
        Button::new(RichText::new(text).color(p.on_accent))
            .fill(p.accent)
            .min_size(vec2(88.0, 26.0)),
    )
    .clicked()
}

/// A secondary action of a dialog.
pub(crate) fn secondary(ui: &mut Ui, text: &str) -> bool {
    ui.add(Button::new(text).min_size(vec2(88.0, 26.0)))
        .clicked()
}

/// Shows the "About" window. Returns whether it stays open.
pub(crate) fn about(ctx: &egui::Context) -> bool {
    let mut close = false;
    let response = Modal::new(egui::Id::new("about")).show(ctx, |ui| {
        ui.set_width(560.0);
        let p = Palette::of(ui);
        header(
            ui,
            |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), egui::Sense::hover());
                icons::component(
                    ui.painter(),
                    rect,
                    ComponentKind::Impeller,
                    component_color(ComponentKind::Impeller),
                );
            },
            "Turbopump",
            &tf(
                "Version {} · open-source preliminary design of turbomachinery",
                &[&env!("CARGO_PKG_VERSION")],
            ),
        );
        ui.separator();
        // As tall as the window allows, leaving room for the header and the buttons.
        let room = (ui.ctx().content_rect().height() - 240.0).clamp(160.0, 560.0);
        ScrollArea::vertical().max_height(room).show(ui, |ui| {
            heading(ui, tr("Author"));
            ui.horizontal(|ui| {
                ui.label(bold("Jorge Viloria"));
                ui.label(RichText::new(tr("and the Turbopump contributors")).color(p.weak));
            });
            ui.hyperlink_to(
                format!("{}  {PROJECT_PAGE}", icon::GITHUB_LOGO),
                PROJECT_PAGE,
            );

            heading(ui, tr("Built with"));
            egui::Grid::new("about_built_with")
                .num_columns(3)
                .spacing([16.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    for (name, purpose, license, page) in BUILT_WITH {
                        ui.hyperlink_to(*name, *page);
                        ui.label(tr(purpose));
                        ui.label(RichText::new(*license).color(p.weak).small());
                        ui.end_row();
                    }
                });

            heading(ui, tr("Methods and references"));
            for book in REFERENCES {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("•").color(p.weak));
                    ui.label(*book);
                });
            }

            heading(ui, tr("License"));
            ui.label(tr(
                "Free software: you may use, study, change and share it under the MIT or the \
                 Apache-2.0 license, at your choice. It comes without any warranty.",
            ));
            ui.label(
                RichText::new("© 2026 Jorge Viloria and the Turbopump contributors")
                    .color(p.weak)
                    .small(),
            );
        });
        ui.add_space(6.0);
        ui.separator();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close |= primary(ui, tr("Close"));
            if secondary(ui, &format!("{}  {}", icon::BOOK_OPEN, tr("Project page"))) {
                ui.ctx().open_url(egui::OpenUrl::new_tab(PROJECT_PAGE));
            }
        });
    });
    !close && !response.should_close()
}

/// What the user chose before unsaved changes are lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Save,
    Discard,
    Cancel,
}

/// Asks whether to save the changes to project `name` before `quitting` the editor
/// or opening another project. Returns the choice once made; closing the window,
/// or Esc, cancels.
pub(crate) fn confirm_discard(ctx: &egui::Context, name: &str, quitting: bool) -> Option<Choice> {
    let mut choice = None;
    let response = Modal::new(egui::Id::new("confirm_discard")).show(ctx, |ui| {
        ui.set_width(440.0);
        let warning = ui.visuals().warn_fg_color;
        header(
            ui,
            |ui| {
                ui.label(RichText::new(icon::WARNING).size(36.0).color(warning));
            },
            &tf("Save the changes to “{}”?", &[&name]),
            tr(if quitting {
                "If you close without saving, the changes are lost."
            } else {
                "If you open another project without saving, the changes are lost."
            }),
        );
        ui.separator();
        ui.horizontal(|ui| {
            if secondary(ui, tr("Don't save")) {
                choice = Some(Choice::Discard);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary(ui, tr("Save")) {
                    choice = Some(Choice::Save);
                }
                if secondary(ui, tr("Cancel")) {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        // Enter saves, as the highlighted button promises.
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            choice = Some(Choice::Save);
        }
    });
    if choice.is_none() && response.should_close() {
        choice = Some(Choice::Cancel);
    }
    choice
}
