//! Dockable views of the active impeller.

pub(crate) mod blade;
pub(crate) mod front;
pub(crate) mod meridional;
pub(crate) mod report;

use eframe::egui::{Color32, Id, Ui};
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

/// Message shown instead of a view when the geometry is missing.
pub(crate) fn unavailable(ui: &mut Ui, reason: &str) {
    ui.centered_and_justified(|ui| {
        ui.colored_label(ui.visuals().warn_fg_color, reason);
    });
}
