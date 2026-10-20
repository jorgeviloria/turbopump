//! Dockable views of the active impeller.

pub(crate) mod blade;
pub(crate) mod front;
pub(crate) mod meridional;
pub(crate) mod report;

use eframe::egui::{Color32, Ui};

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

/// Message shown instead of a view when the geometry is missing.
pub(crate) fn unavailable(ui: &mut Ui, reason: &str) {
    ui.centered_and_justified(|ui| {
        ui.colored_label(ui.visuals().warn_fg_color, reason);
    });
}
