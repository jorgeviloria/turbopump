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
        harness.get_by_label_contains("Accept step").click();
        harness.run();
        save(&mut harness, &format!("main-{suffix}"));
        for view in [
            View::Report,
            View::Blade,
            View::FrontView,
            View::Performance,
        ] {
            harness.state_mut().focus_view(view);
            harness.run();
            let name = view.title().to_lowercase().replace(' ', "-");
            save(&mut harness, &format!("{name}-{suffix}"));
        }
        // The home tab of the toolbar and the tree of the 3D model.
        harness.get_by_label("Home tab").click();
        harness.get_by_label_contains("3D model").click();
        harness.state_mut().focus_view(View::View3d);
        harness.run();
        save(&mut harness, &format!("home-{suffix}"));
        // The global setup: rows with a value and its kind side by side.
        harness
            .get_all_by_label("Global setup")
            .next()
            .unwrap()
            .click();
        harness.run();
        save(&mut harness, &format!("setup-{suffix}"));
    }
}

/// The picture in the README: impeller and volute designed, the spiral step selected.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn documentation() {
    let mut harness = harness(egui::Theme::Light);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.get_by_label_contains("Volute setup").click();
    harness.run();
    for _ in 0..2 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.state_mut().focus_view(View::View3d);
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "editor");
}

/// The whole stage: impeller and volute designed, the spiral step selected.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn volute() {
    let mut harness = harness(egui::Theme::Dark);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.get_by_label_contains("Volute setup").click();
    harness.run();
    for _ in 0..2 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "volute-spiral");
    harness.state_mut().focus_view(View::FrontView);
    harness.run();
    save(&mut harness, "volute-front");
}

/// The cross-section step: the default trapezoid, then a radius-based section with a
/// second definition of line segments, its corners ready to drag.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn cross_sections() {
    let mut harness = harness(egui::Theme::Light);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.get_by_label_contains("Volute setup").click();
    harness.run();
    harness.get_by_label_contains("Accept step").click();
    harness.run();
    harness.state_mut().focus_view(View::CrossSection);
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "cross-section-trapezoid");
    harness.get_by_value("Trapezoid").click();
    harness.run();
    harness.get_by_label("Radius based").click();
    harness.run();
    save(&mut harness, "cross-section-radius");
    harness.get_by_label_contains("Add section").click();
    harness.run();
    harness
        .get_all_by_value("Radius based")
        .last()
        .unwrap()
        .click();
    harness.run();
    harness.get_by_label("Line segments").click();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "cross-section-segments");
}

/// An internal volute: its bend and neck in the cross-section view, the meridional view
/// and 3D.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn internal_volute() {
    let mut harness = harness(egui::Theme::Light);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.get_by_label_contains("Volute setup").click();
    harness.run();
    harness.get_by_label_contains("Accept step").click();
    harness.run();
    harness.get_by_value("Trapezoid").click();
    harness.run();
    harness.get_by_label("Round, asymmetric, internal").click();
    harness.state_mut().focus_view(View::CrossSection);
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "internal-section");
    harness.get_by_label_contains("Properties").click();
    harness.state_mut().focus_view(View::Meridional);
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "internal-meridional");
    harness.state_mut().focus_view(View::View3d);
    harness.run();
    save(&mut harness, "internal-3d");
}

/// A stage with a vaned diffuser between impeller and volute.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn diffuser() {
    let mut harness = harness(egui::Theme::Dark);
    harness.get_by_label("Add component").click();
    harness.run();
    harness
        .get_by_label_contains("Stator (vaned diffuser)")
        .click();
    harness.run();
    for _ in 0..2 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.state_mut().focus_view(View::FrontView);
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "diffuser-front");
    harness.state_mut().focus_view(View::View3d);
    harness.run();
    save(&mut harness, "diffuser-3d");
}

/// The meridional view with the potential flow: its stream lines and the velocity
/// along hub and shroud.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn potential_flow() {
    let mut harness = harness(egui::Theme::Light);
    harness.state_mut().focus_view(View::Meridional);
    harness.run();
    harness.get_by_label("potential flow").click();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "potential-flow");
}

/// The properties folded to a strip: the view takes the whole window.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn properties_folded() {
    let mut harness = harness(egui::Theme::Light);
    harness.state_mut().focus_view(View::View3d);
    harness.run();
    harness.key_press(egui::Key::Escape);
    for _ in 0..10 {
        harness.run();
    }
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "properties-folded");
}

/// The interface in Spanish: the step editor, the report and the curves.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn spanish() {
    let mut harness = harness(egui::Theme::Light);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    harness.get_by_label_contains("Language").click();
    harness.run();
    harness.get_by_label_contains("Español").click();
    harness.run();
    harness.get_by_label("Cerrar").click();
    harness.run();
    harness.get_by_label_contains("Aceptar paso").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "es-main");
    for (view, name) in [
        (View::Report, "es-report"),
        (View::Performance, "es-curves"),
        (View::Meridional, "es-meridional"),
        (View::View3d, "es-3d"),
    ] {
        harness.state_mut().focus_view(view);
        harness.run();
        save(&mut harness, name);
    }
}

/// The settings window, page by page, in both themes.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn settings() {
    for (theme, suffix) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let mut harness = harness(theme);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        for (page, name) in [
            ("Appearance", "appearance"),
            ("Language", "language"),
            ("Units", "units"),
            ("Behavior", "behavior"),
        ] {
            harness
                .get_all_by_label_contains(page)
                .next()
                .unwrap()
                .click();
            harness.run();
            harness.remove_cursor();
            harness.run();
            save(&mut harness, &format!("settings-{name}-{suffix}"));
        }
    }
}

/// A double volute: front view and 3D, chosen in the volute-setup step.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn double_volute() {
    let mut harness = harness(egui::Theme::Light);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.get_by_label_contains("Volute setup").click();
    harness.run();
    harness.get_by_value("single").click();
    harness.run();
    harness.get_by_label("double").click();
    harness.run();
    harness.state_mut().focus_view(View::FrontView);
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "double-volute-front");
    // Without the properties panel the rib nose shows on the right.
    harness.get_by_label_contains("Properties").click();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "double-volute-rib");
    harness.state_mut().focus_view(View::View3d);
    harness.run();
    save(&mut harness, "double-volute-3d");
}

/// A tangential discharge chosen in the diffuser step, then back to the default
/// centerline one, with the rounded cutwater of the default design.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn radial_discharge() {
    let mut harness = harness(egui::Theme::Light);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.get_by_label_contains("Volute setup").click();
    harness.run();
    for _ in 0..3 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    let front = |harness: &mut Harness<'_, TurbopumpApp>, name: &str| {
        harness.state_mut().focus_view(View::FrontView);
        harness.key_press(egui::Key::Escape);
        for _ in 0..10 {
            harness.run();
        }
        harness.remove_cursor();
        harness.run();
        save(harness, name);
    };
    harness.get_by_value("radial").click();
    harness.run();
    harness
        .get_by_label("tangential, opening to both sides")
        .click();
    harness.run();
    front(&mut harness, "tangential-discharge-front");
    harness.get_by_label_contains("Properties").click();
    harness.run();
    harness
        .get_by_value("tangential, opening to both sides")
        .click();
    harness.run();
    harness.get_by_label("radial").click();
    harness.run();
    front(&mut harness, "radial-discharge-front");
    harness.state_mut().focus_view(View::View3d);
    harness.run();
    save(&mut harness, "radial-discharge-3d");
}

/// The spiral step with tangent circular arcs and the cutwater compensation, and the
/// front view that draws them, from a project saved with those inputs.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn spiral_development() {
    let mut harness = harness(egui::Theme::Light);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    let mut project = harness.state().document().project().clone();
    let volute = project
        .components()
        .iter()
        .find(|c| c.kind() == tp_project::ComponentKind::Volute)
        .map(|c| c.id())
        .unwrap();
    let mut input = project.volute_input(volute).unwrap();
    input.spiral.law = tp_design::volute::AreaLaw::Stepanoff;
    input.spiral.arcs.enabled = true;
    input.spiral.compensation.enabled = true;
    input.spiral.cutwater_correction = false;
    project.set_volute(volute, input).unwrap();
    let path = output("spiral-development").with_extension("tpj");
    tp_project::Document::new(project).save_as(&path).unwrap();
    // Tall enough to show the whole step.
    let mut harness = Harness::builder()
        .with_size([1440.0, 2100.0])
        .with_pixels_per_point(1.0)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
    harness.run();
    harness.get_by_label_contains("Spiral development").click();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "spiral-development");
    harness.state_mut().focus_view(View::FrontView);
    harness.get_by_label_contains("Properties").click();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "spiral-development-front");
}

/// The view cube with the pointer on its nearest corner and on its top face.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn view_cube() {
    for (theme, suffix) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let mut harness = harness(theme);
        harness.state_mut().focus_view(View::View3d);
        harness.run();
        for (pos, name) in [
            (egui::pos2(954.0, 238.0), "corner"),
            (egui::pos2(954.0, 216.0), "face"),
        ] {
            harness.hover_at(pos);
            harness.run();
            save(&mut harness, &format!("cube-{name}-{suffix}"));
        }
    }
}
