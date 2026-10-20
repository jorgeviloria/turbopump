//! Interaction tests of the editor. They drive the real application through its
//! accessibility tree and need no GPU (the 3D view shows a notice instead).

use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use tp_gui::{Selection, TurbopumpApp, View};
use tp_project::{DesignStep, StepStatus};

const ACCEPT: &str = "Accept step";

fn harness() -> Harness<'static, TurbopumpApp> {
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_eframe(|cc| TurbopumpApp::new(cc, None));
    harness.run();
    harness
}

fn status(harness: &Harness<'_, TurbopumpApp>, step: DesignStep) -> StepStatus {
    harness.state().document().project().components()[0].status(step)
}

fn accept(harness: &mut Harness<'_, TurbopumpApp>) {
    harness.get_by_label(ACCEPT).click();
    harness.run();
}

#[test]
fn starts_on_the_main_dimensions_of_the_default_pump() {
    let harness = harness();
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::MainDimensions)
    ));
    // The report and the step panel show the specific speed of the default pump.
    harness.get_by_label("Main dimensions");
    harness.get_by_label("35.92");
}

#[test]
fn accepting_steps_walks_through_the_impeller() {
    let mut harness = harness();
    for step in [
        DesignStep::MainDimensions,
        DesignStep::MeridionalContour,
        DesignStep::BladeProperties,
        DesignStep::BladeMeanLines,
        DesignStep::BladeProfiles,
        DesignStep::BladeEdges,
    ] {
        assert!(matches!(harness.state().selection(), Selection::Step(_, s) if s == step));
        accept(&mut harness);
        assert_eq!(status(&harness, step), StepStatus::Done, "{step}");
    }
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::ModelSettings)
    ));
}

#[test]
fn undo_and_redo_shortcuts() {
    let mut harness = harness();
    accept(&mut harness);
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Pending
    );
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
}

#[test]
fn changing_the_global_setup_makes_done_steps_outdated() {
    let mut harness = harness();
    accept(&mut harness);
    harness.get_by_label("⚙ Global setup").click();
    harness.run();
    harness.get_by_label("counter-clockwise").click();
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Stale
    );
    assert!(harness.state().document().is_dirty());
}

#[test]
fn manual_values_can_be_reset_to_automatic() {
    let mut harness = harness();
    // Clear the "auto" box of the first parameter (hydraulic efficiency).
    harness.get_all_by_label("auto").next().unwrap().click();
    harness.run();
    let manual = |harness: &Harness<'_, TurbopumpApp>| {
        let project = harness.state().document().project();
        let id = project.components()[0].id();
        project
            .pump_impeller_input(id)
            .unwrap()
            .hydraulic_efficiency
            .manual_value()
    };
    assert!(manual(&harness).is_some());
    harness.get_by_label("Reset to automatic").click();
    harness.run();
    assert!(manual(&harness).is_none());
}

#[test]
fn every_view_draws_without_a_gpu() {
    let mut harness = harness();
    for view in View::ALL {
        harness.state_mut().focus_view(view);
        harness.run();
    }
    harness.get_by_label("The 3D view needs the wgpu renderer.");
    harness.get_by_label("Blade sections");
}
