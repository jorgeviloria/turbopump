//! Renders the editor with wgpu and saves PNG screenshots to `target/screenshots/`, for
//! checking the look by eye. They need a GPU, so they only run when asked:
//!
//! ```text
//! cargo test -p tp-gui --test screenshots -- --ignored
//! ```

use std::path::PathBuf;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use tp_gui::{TurbopumpApp, View};

fn output(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}.png"))
}

fn save(harness: &mut Harness<'_, TurbopumpApp>, name: &str) {
    let image = harness.render().expect("wgpu rendering");
    let path = output(name);
    image.save(&path).unwrap();
    println!("wrote {}", path.display());
}

fn harness(theme: egui::Theme) -> Harness<'static, TurbopumpApp> {
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.5)
        .with_theme(theme)
        .wgpu()
        .build_eframe(|cc| TurbopumpApp::new(cc, None));
    harness.run();
    harness
}

#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn main_window() {
    for (theme, suffix) in [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")] {
        let mut harness = harness(theme);
        // Accept the main dimensions so the tree shows every state.
        harness.get_by_label("Accept step").click();
        harness.run();
        save(&mut harness, &format!("main-{suffix}"));
        for view in [View::Report, View::Blade, View::FrontView] {
            harness.state_mut().focus_view(view);
            harness.run();
            let name = view.title().to_lowercase().replace(' ', "-");
            save(&mut harness, &format!("{name}-{suffix}"));
        }
    }
}

/// The picture in the README: a finished impeller with the meridional step selected.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn documentation() {
    let mut harness = harness(egui::Theme::Dark);
    for _ in 0..6 {
        harness.get_by_label("Accept step").click();
        harness.run();
    }
    harness.get_by_label_contains("Meridional contour").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "editor");
}
