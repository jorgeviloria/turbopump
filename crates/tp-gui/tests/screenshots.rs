//! Renders the editor with wgpu and saves PNG screenshots to `target/screenshots/`, for
//! checking the look by eye. They need a GPU, so they only run when asked:
//!
//! ```text
//! cargo test -p tp-gui --test screenshots -- --ignored
//! ```

use std::path::PathBuf;

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use tp_gui::{Scope, TurbopumpApp, View};

fn output(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{name}.png"))
}

/// Saves the window without the pointer kittest paints, as a real screenshot would.
fn save(harness: &mut Harness<'_, TurbopumpApp>, name: &str) {
    harness.remove_cursor();
    harness.run_steps(4);
    save_hovering(harness, name);
}

/// Saves the window with the pointer, for hover states.
fn save_hovering(harness: &mut Harness<'_, TurbopumpApp>, name: &str) {
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
        // The 3D tab of the toolbar and the scene floating over the view.
        harness.state_mut().focus_view(View::View3d);
        harness.run();
        save(&mut harness, &format!("home-{suffix}"));
        // The global setup: rows with a value and its kind side by side.
        harness.get_by_label_contains("TARGET").click();
        harness.run();
        save(&mut harness, &format!("setup-{suffix}"));
        // The design flow folded to its rail.
        harness.get_by_label("Fold the design flow").click();
        harness.run();
        save(&mut harness, &format!("flow-rail-{suffix}"));
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
    harness.get_by_label("Volute 1").click();
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

/// The scene's color picker over a parallel view with a translucent casing, and the
/// PNG picture the 3D tab saves of that view.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn scene_colors_and_picture() {
    let mut harness = harness(egui::Theme::Light);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.get_by_label("Volute 1").click();
    harness.run();
    for _ in 0..2 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.state_mut().set_sidebar_open(false);
    harness.get_by_label("3D tab").click();
    harness.run();
    harness.get_by_label("Orthographic").click();
    harness.run();
    // Glass to opaque to translucent.
    for _ in 0..2 {
        harness.get_by_label("Opacity of Casing").click();
        harness.run();
    }
    harness.get_by_label("Color of Blades").click();
    harness.run_steps(4);
    save(&mut harness, "scene-colors");
    harness.key_press(egui::Key::Escape);
    harness.run();
    let ctx = harness.ctx.clone();
    let png = harness
        .state_mut()
        .picture_of_3d_view(&ctx)
        .expect("a picture of the 3D view");
    assert!(png.starts_with(b"\x89PNG"));
    let path = output("picture-3d");
    std::fs::write(&path, png).unwrap();
    println!("wrote {}", path.display());
}

/// The button that completes every step, shown on the impeller's header, and the
/// impeller completed with it.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn complete_all() {
    let mut harness = harness(egui::Theme::Light);
    let button = harness
        .get_by_label("Complete every step of Impeller 1")
        .rect()
        .center();
    harness.hover_at(button);
    harness.run_steps(40);
    save_hovering(&mut harness, "complete-all-hover");
    click_at(&mut harness, button);
    save(&mut harness, "complete-all");
}

/// The main dimensions accepted, then a value set by hand: its row marked in the margin.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn changed_since_accepted() {
    let mut harness = harness(egui::Theme::Light);
    harness.get_by_label_contains("Accept step").click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowUp);
    harness.run();
    harness.run();
    harness.get_all_by_label("auto").nth(1).unwrap().click();
    harness.run();
    harness.run();
    save(&mut harness, "changed-since-accepted");
}

/// The pointer on the row of the axial length L in the properties: its dimension
/// stands out in the meridian.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn row_and_dimension() {
    let mut harness = harness(egui::Theme::Light);
    harness.state_mut().focus_view(View::Meridian);
    harness.run();
    // Only the section of L open, so its row is in sight.
    harness.get_by_label("More actions of the step").click();
    harness.run();
    harness.get_by_label_contains("Fold every section").click();
    harness.run();
    harness.get_all_by_label("Contour").last().unwrap().click();
    harness.run();
    let row = harness
        .get_all_by_label("Axial length L")
        .next()
        .unwrap()
        .rect()
        .center();
    harness.hover_at(row);
    harness.run_steps(4);
    save_hovering(&mut harness, "row-and-dimension");
    // The other way: the value of L in the drawing shades its row.
    harness.hover_at(egui::pos2(683.0, 751.0));
    harness.run_steps(4);
    save_hovering(&mut harness, "dimension-and-row");
}

/// The same link in the front view: the row of d₂ lights its diameter, and the text of
/// the diameter shades its row.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn front_dimensions() {
    let mut harness = harness(egui::Theme::Light);
    harness.state_mut().focus_view(View::FrontView);
    harness.run();
    let row = harness
        .get_all_by_label("Outlet diameter d₂")
        .next()
        .unwrap()
        .rect()
        .center();
    harness.hover_at(row);
    harness.run_steps(4);
    save_hovering(&mut harness, "front-row-and-dimension");
    // The drawing comes before the properties, whose summary also names d₂.
    let text = harness
        .get_all_by_label_contains("d₂ ")
        .next()
        .unwrap()
        .rect()
        .center();
    harness.hover_at(text);
    harness.run_steps(4);
    save_hovering(&mut harness, "front-dimension-and-row");
}

/// The meridional cut with an opaque casing: the cut faces of the volute, the blades
/// and the inlet duct are capped, as seen and facing the camera.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn cut_caps() {
    let mut harness = stage_in_3d();
    harness.get_by_label("Opacity of Casing").click();
    harness.run();
    harness.get_by_label("Meridional cut").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "cut-caps");
    // From the side, the cut turned to face the camera: the meridional section.
    harness.get_by_label("Views").click();
    harness.run();
    harness.get_by_label("From the side").click();
    harness.run_steps(40);
    harness.get_by_label_contains("Face the camera").click();
    harness.remove_cursor();
    harness.run_steps(4);
    save(&mut harness, "cut-caps-facing");
}

/// The Views menu of the 3D tab, a view saved from the side and brought back.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn views_menu() {
    let mut harness = stage_in_3d();
    let open = |h: &mut Harness<'_, TurbopumpApp>| {
        h.get_by_label("Views").click();
        h.run();
    };
    open(&mut harness);
    harness.get_by_label("From the side").click();
    harness.run_steps(40);
    open(&mut harness);
    harness.get_by_label_contains("Save this view").click();
    harness.run();
    open(&mut harness);
    harness.get_by_label("View 1");
    save(&mut harness, "views-menu");
    harness.get_by_label("From the inlet").click();
    harness.run_steps(40);
    open(&mut harness);
    harness.get_by_label("View 1").click();
    harness.run_steps(40);
    save(&mut harness, "view-1");
    open(&mut harness);
    harness.get_by_label("Forget View 1").click();
    harness.run();
    open(&mut harness);
    assert!(harness.query_by_label("View 1").is_none());
}

/// A primary click at `pos`, with the pointer left there.
fn click_at(harness: &mut Harness<'_, TurbopumpApp>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
    }
}

/// The stage with its impeller and volute designed, in the 3D view, properties folded.
fn stage_in_3d() -> Harness<'static, TurbopumpApp> {
    let mut harness = harness(egui::Theme::Light);
    for _ in 0..6 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.get_by_label("Volute 1").click();
    harness.run();
    for _ in 0..2 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.state_mut().set_sidebar_open(false);
    harness.get_by_label("3D tab").click();
    harness.run();
    harness
}

/// Phase C of the 3D view: a part selected and another under the pointer, the
/// meridional cut, and a distance measured on the cut model.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn inspect_3d() {
    let mut harness = stage_in_3d();
    let impeller = egui::pos2(960.0, 560.0);
    let casing = egui::pos2(960.0, 700.0);
    click_at(&mut harness, impeller);
    harness.hover_at(casing);
    harness.run_steps(4);
    save_hovering(&mut harness, "pick");

    // A right-click on the casing offers to hide it.
    harness.hover_at(casing);
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: casing,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
    }
    save(&mut harness, "part-menu");
    harness
        .get_all_by_label_contains("Hide")
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.ends_with("  Hide"))
        })
        .expect("the menu's Hide")
        .click();
    harness.run();
    let casing_shown = harness
        .get_all_by_label("Casing")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::CheckBox)
        .and_then(|n| n.accesskit_node().toggled());
    assert_eq!(casing_shown, Some(egui::accesskit::Toggled::False));
    harness.get_by_label("Scene menu").click();
    harness.run();
    harness.get_by_label("Show all").click();
    harness.run();

    harness.get_by_label("Meridional cut").click();
    harness.run();
    harness.get_by_label_contains("Show the plane").click();
    harness.run();
    save(&mut harness, "cut");
    harness.get_by_label_contains("Show the plane").click();
    harness.run();

    harness.get_by_label("Measure").click();
    harness.run();
    click_at(&mut harness, egui::pos2(900.0, 520.0));
    // The casing on the far side, which the cut leaves.
    let far = egui::pos2(1205.0, 520.0);
    harness.hover_at(far);
    harness.run_steps(4);
    save_hovering(&mut harness, "measure-rubber");
    click_at(&mut harness, far);
    save(&mut harness, "measure");
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
    harness.get_by_label("Volute 1").click();
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

/// The cutwater step opens the cutwater page of the front view: the plane by plane
/// drawing with its points to drag, the measures, the body in 3D and the incidence over
/// the flow range.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn cutwater() {
    for (theme, suffix) in [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")] {
        let mut harness = harness(theme);
        for _ in 0..6 {
            harness.get_by_label_contains("Accept step").click();
            harness.run();
        }
        harness.get_by_label("Volute 1").click();
        harness.run();
        // Setup, cross section, spiral and diffuser: the cutwater step is next.
        for _ in 0..4 {
            harness.get_by_label_contains("Accept step").click();
            harness.run();
        }
        harness.remove_cursor();
        harness.run();
        save(&mut harness, &format!("cutwater-{suffix}"));
    }
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
    harness.get_by_label("Volute 1").click();
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
    harness.get_by_label("Volute 1").click();
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
    let open = harness.state().sidebar_open();
    harness.state_mut().set_sidebar_open(!open);
    harness.state_mut().set_meridian_scope(Scope::Stage);
    harness.state_mut().focus_view(View::Meridian);
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
    harness.state_mut().set_meridian_scope(Scope::Stage);
    harness.state_mut().focus_view(View::Meridian);
    harness.run();
    harness.get_by_label_contains("Analysis").click();
    harness.run();
    harness.get_by_label("potential flow").click();
    harness.run();
    harness.get_by_label_contains("Analysis").click();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "potential-flow");
    // The color map of c_m with its iso-lines and vectors, the blades blocking the flow.
    harness.get_by_label_contains("Analysis").click();
    harness.run();
    for option in [
        "c_m iso-lines",
        "c_m color map",
        "velocity vectors",
        "blade blockage",
    ] {
        harness.get_by_label(option).click();
        harness.run();
    }
    harness.key_press(egui::Key::Escape);
    harness.remove_cursor();
    harness.run();
    harness.run();
    save(&mut harness, "potential-maps");
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
        (View::Meridian, "es-meridian"),
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
    harness.get_by_label("Volute 1").click();
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
    let open = harness.state().sidebar_open();
    harness.state_mut().set_sidebar_open(!open);
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
    harness.get_by_label("Volute 1").click();
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
    let open = harness.state().sidebar_open();
    harness.state_mut().set_sidebar_open(!open);
    harness.run();
    harness
        .get_by_value("tangential, opening to both sides")
        .click();
    harness.run();
    harness.get_by_label("radial").click();
    // The panel reopens with the discharge rows changed: its grids take a few frames
    // to settle.
    harness.run_steps(8);
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
    // The volute folds with the accordion: open it, then its step.
    harness.get_by_label("Volute 1").click();
    harness.run();
    harness.get_by_label_contains("Spiral development").click();
    harness.remove_cursor();
    harness.run();
    save(&mut harness, "spiral-development");
    harness.state_mut().focus_view(View::FrontView);
    let open = harness.state().sidebar_open();
    harness.state_mut().set_sidebar_open(!open);
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
            save_hovering(&mut harness, &format!("cube-{name}-{suffix}"));
        }
    }
}

/// The "About" window and the question before discarding unsaved changes.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn dialogs() {
    for (theme, suffix) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let mut harness = harness(theme);
        harness.get_all_by_label("Help").next().unwrap().click();
        harness.run();
        harness.get_by_label_contains("About").click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        save(&mut harness, &format!("about-{suffix}"));
        // The references and the license, further down.
        harness.hover_at(egui::pos2(480.0, 300.0));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -600.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        harness.run();
        harness.remove_cursor();
        harness.run();
        save(&mut harness, &format!("about-end-{suffix}"));
        harness.get_by_label("Close").click();
        harness.run();
        // An accepted step leaves unsaved changes; a new project asks first.
        harness.get_by_label_contains("Accept step").click();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        // The pages of the assistant.
        for step in ["project", "point", "fluid", "components"] {
            harness.remove_cursor();
            harness.run();
            save(&mut harness, &format!("new-{step}-{suffix}"));
            if step != "components" {
                // The panel behind has a "Next" too; the dialog is drawn last.
                harness
                    .get_all_by_label_contains("Next")
                    .last()
                    .unwrap()
                    .click();
                harness.run();
            }
        }
        harness.get_by_label("Create").click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        save(&mut harness, &format!("unsaved-{suffix}"));
    }
}

/// A semi-axial impeller (800 m³/h, 12 m, 1450 rpm, nq ≈ 106): its main dimensions,
/// meridional section and 3D model.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn semi_axial() {
    use std::f64::consts::PI;
    use tp_project::{
        ComponentKind, DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType, Project,
    };
    let setup = GlobalSetup::new(
        MachineType::Pump,
        DesignPoint {
            flow: FlowRate::Volume(800.0 / 3600.0),
            energy: EnergyTransfer::Head(12.0),
            speed: 1450.0 * PI / 30.0,
        },
    );
    let mut project = Project::new("Mixed flow", MachineType::Pump, setup).unwrap();
    project
        .add_component(ComponentKind::Impeller, None)
        .unwrap();
    let path = std::env::temp_dir().join("turbopump-semi-axial.tpj");
    project.save(&path).unwrap();
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.5)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
    harness.run();
    save(&mut harness, "semi-axial-main");
    harness.get_by_label_contains("Accept step").click();
    harness.run();
    harness.state_mut().set_meridian_scope(Scope::Stage);
    harness.state_mut().focus_view(View::Meridian);
    harness.run();
    save(&mut harness, "semi-axial-meridional");
    harness.state_mut().set_meridian_scope(Scope::Channel);
    harness.run();
    save(&mut harness, "semi-axial-editor");
    harness.state_mut().focus_view(View::View3d);
    harness.run();
    save(&mut harness, "semi-axial-3d");
}

#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn meridional_segments() {
    use tp_design::impeller::wall::{self, SegmentKind};
    let mut project = tp_gui::default_project();
    let id = project.components()[0].id();
    let g = project.compute_impeller_geometry(id).unwrap();
    let mut input = project.impeller_geometry_input(id).unwrap();
    let rebuild = |w: &wall::Wall, s: &[wall::SegmentInput]| {
        wall::Wall::build(w.start(), w.end(), s).unwrap()
    };
    // Hub: an axial straight run, then two Béziers joined smoothly.
    let hub = rebuild(&g.contour.hub, &wall::split(&g.contour.hub, 0, 0.25));
    let hub = rebuild(&hub, &wall::set_kind(&hub, 0, SegmentKind::Line));
    let hub = rebuild(&hub, &wall::split(&hub, 1, 0.5));
    input.meridional.hub_segments = Some(hub.to_input());
    // Shroud: two Béziers meeting at a corner.
    let shroud = rebuild(&g.contour.shroud, &wall::split(&g.contour.shroud, 0, 0.55));
    let shroud = rebuild(&shroud, &wall::set_smooth(&shroud, 0, false));
    let at = wall::WallPoint::Control {
        segment: 1,
        index: 0,
    };
    let moved = shroud.point(at).unwrap() + tp_math::DVec2::new(-0.004, 0.006);
    input.meridional.shroud_segments = Some(wall::move_point(&shroud, at, moved));
    // A leading edge bowed upstream.
    input.meridional.leading_edge_shape = tp_design::impeller::meridian::LeadingEdgeShape::Bezier {
        controls: [
            tp_math::DVec2::new(0.33, 0.12),
            tp_math::DVec2::new(0.67, 0.12),
        ],
    };
    project.set_impeller_geometry(id, input).unwrap();
    let path = std::env::temp_dir().join("turbopump-meridional-segments.tpj");
    project.save(&path).unwrap();
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.5)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
    harness.run();
    harness.state_mut().set_meridian_scope(Scope::Stage);
    harness.state_mut().focus_view(View::Meridian);
    harness.run();
    save(&mut harness, "meridional-segments");
    harness.state_mut().set_meridian_scope(Scope::Channel);
    harness.run();
    save(&mut harness, "meridian-editor");
}

/// The meridian editor on the automatic contour of a new impeller.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn meridian_editor() {
    for (theme, suffix) in [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")] {
        let mut harness = harness(theme);
        harness.state_mut().focus_view(View::Meridian);
        harness.run();
        save(&mut harness, &format!("editor-{suffix}"));
    }
    // The analysis menu open, with the charts under the diagram.
    let mut harness = harness(egui::Theme::Light);
    harness.state_mut().focus_view(View::Meridian);
    harness.run();
    harness.get_by_label_contains("Analysis").click();
    harness.run();
    harness.get_by_label("area and curvature charts").click();
    harness.get_by_label("curvature maximum").click();
    harness.run();
    save(&mut harness, "editor-analysis");
    // The closed material of hub disk and shroud cover.
    harness.get_by_label_contains("Analysis").click();
    harness.run();
    harness.get_by_label("Materials").click();
    harness.run();
    harness.run();
    save(&mut harness, "editor-materials");
    // The side chambers, designed.
    harness
        .get_all_by_label("Side chambers")
        .next()
        .unwrap()
        .click();
    harness.run();
    harness
        .get_all_by_label("Design the side chambers")
        .next()
        .unwrap()
        .click();
    harness.run();
    harness.run();
    save(&mut harness, "editor-chambers");
}

/// The inlet duct with the side chambers: in the chambers scope of the Meridian view
/// and in the 3D view, impeller and volute designed.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn inlet_duct() {
    let mut project = tp_gui::default_project();
    let id = project.components()[0].id();
    let mut input = project.impeller_geometry_input(id).unwrap();
    input.meridional.chambers.enabled = true;
    input.meridional.inlet_duct.enabled = true;
    project.set_impeller_geometry(id, input).unwrap();
    let path = std::env::temp_dir().join("turbopump-inlet-duct.tpj");
    project.save(&path).unwrap();
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.5)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
    harness.run();
    for _ in 0..2 {
        harness.get_by_label_contains("Accept step").click();
        harness.run();
    }
    harness.state_mut().focus_view(View::Meridian);
    harness.state_mut().set_meridian_scope(Scope::Chambers);
    harness.run();
    harness.get_by_label("Fit all").click();
    harness.run();
    save(&mut harness, "inlet-duct-meridian");
    harness
        .get_by_label("Complete every step of Impeller 1")
        .click();
    harness.run();
    harness.get_by_label("Volute 1").click();
    harness.run();
    harness
        .get_by_label("Complete every step of Volute 1")
        .click();
    harness.run();
    harness.state_mut().set_sidebar_open(false);
    harness.get_by_label("3D tab").click();
    harness.run();
    harness.get_by_label("Meridional cut").click();
    harness.run_steps(4);
    save(&mut harness, "inlet-duct-3d");
}

/// The segment types of a drawn wall: the hub as lines and an arc, the shroud as a
/// B-spline with its menu open.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn meridian_segment_kinds() {
    use tp_design::impeller::wall::{self, SegmentKind};
    let mut project = tp_gui::default_project();
    let id = project.components()[0].id();
    // An inlet line leaning 15° from the normal to the axis, before the walls are drawn.
    let mut input = project.impeller_geometry_input(id).unwrap();
    input.meridional.inlet_tilt =
        tp_design::impeller::meridian::InletTilt::Angle(15f64.to_radians());
    project.set_impeller_geometry(id, input).unwrap();
    let g = project.compute_impeller_geometry(id).unwrap();
    let mut input = project.impeller_geometry_input(id).unwrap();
    let rebuild = |w: &wall::Wall, s: &[wall::SegmentInput]| {
        wall::Wall::build(w.start(), w.end(), s).unwrap()
    };
    let hub = rebuild(
        &g.contour.hub,
        &wall::set_kind(&g.contour.hub, 0, SegmentKind::Arc),
    );
    input.meridional.hub_segments = Some(hub.to_input());
    let shroud = wall::set_kind(&g.contour.shroud, 0, SegmentKind::BSpline);
    let shroud = rebuild(&g.contour.shroud, &shroud);
    input.meridional.shroud_segments = Some(wall::insert_knot(&shroud, 0, 0.6));
    // A leading edge parallel to the axis, its hub point following the shroud point.
    input.meridional.leading_edge_shroud = tp_design::Param::Manual(0.15);
    input.meridional.leading_edge_extent = tp_design::impeller::meridian::EdgeExtent::Radial(0.0);
    project.set_impeller_geometry(id, input).unwrap();
    let path = std::env::temp_dir().join("turbopump-meridian-kinds.tpj");
    project.save(&path).unwrap();
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.5)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
    harness.run();
    harness.state_mut().focus_view(View::Meridian);
    harness.run();
    save(&mut harness, "meridian-kinds");
}

/// A mixed-flow impeller designed by its center line and a cross-section distribution.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn center_line_mode() {
    use std::f64::consts::PI;
    use tp_design::impeller::meridian::DesignMode;
    use tp_project::{
        ComponentKind, DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType, Project,
    };
    let setup = GlobalSetup::new(
        MachineType::Pump,
        DesignPoint {
            flow: FlowRate::Volume(800.0 / 3600.0),
            energy: EnergyTransfer::Head(12.0),
            speed: 1450.0 * PI / 30.0,
        },
    );
    let mut project = Project::new("Mixed flow", MachineType::Pump, setup).unwrap();
    let id = project
        .add_component(ComponentKind::Impeller, None)
        .unwrap();
    let mut input = project.impeller_geometry_input(id).unwrap();
    input.meridional.design_mode = DesignMode::CenterLine;
    input.meridional.area_progression = vec![[0.0, 0.0], [0.4, 0.55], [1.0, 1.0]];
    project.set_impeller_geometry(id, input).unwrap();
    let path = std::env::temp_dir().join("turbopump-center-line.tpj");
    project.save(&path).unwrap();
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.5)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
    harness.run();
    harness.state_mut().focus_view(View::Meridian);
    harness.run();
    harness.run();
    save(&mut harness, "editor-center-line");
}

/// The blade edges in true shape: a Bézier leading edge that leans toward the shroud and
/// a tapered trailing edge.
#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn blade_edges() {
    use tp_design::impeller::blade::EdgeShape;
    let mut project = tp_gui::default_project();
    let id = project.components()[0].id();
    let mut input = project.impeller_geometry_input(id).unwrap();
    input.edges.leading = EdgeShape::BEZIER;
    input.edges.leading_shroud = Some(EdgeShape::Bezier {
        start: [2.5, 1.6],
        pull: [0.7, 0.5],
        tip: 0.35,
        symmetric: false,
    });
    input.edges.trailing = EdgeShape::Taper {
        length: 4.0,
        nose: 0.5,
        ratio: 1.6,
        asymmetry: 0.3,
    };
    project.set_impeller_geometry(id, input).unwrap();
    let path = std::env::temp_dir().join("turbopump-blade-edges.tpj");
    project.save(&path).unwrap();
    for (theme, suffix) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .with_pixels_per_point(1.5)
            .with_theme(theme)
            .wgpu()
            .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
        harness.run();
        harness
            .get_all_by_label("Blade edges")
            .next()
            .unwrap()
            .click();
        harness.run();
        harness.run();
        save(&mut harness, &format!("blade-edges-{suffix}"));
    }
}
