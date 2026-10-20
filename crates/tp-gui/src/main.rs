//! `turbopump-gui`: the graphical editor. Takes an optional project file to open.

use std::path::PathBuf;

use eframe::egui;
use tp_gui::TurbopumpApp;

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Turbopump")
            .with_app_id("turbopump")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Turbopump",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(egui::ThemePreference::Light);
            Ok(Box::new(TurbopumpApp::new(cc, path.as_deref())))
        }),
    )
}
