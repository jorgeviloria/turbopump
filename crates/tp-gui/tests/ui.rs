//! Interaction tests of the editor. They drive the real application through its
//! accessibility tree and need no GPU (the 3D view shows a notice instead).

use eframe::egui::{self, Key, Modifiers};
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
    harness.get_by_label_contains(ACCEPT).click();
    harness.run();
}

/// Opens the settings window, picks `language` on its language page (`page` in the
/// current language) and closes it with the button labelled `close`.
fn choose_language(
    harness: &mut Harness<'_, TurbopumpApp>,
    page: &str,
    language: &str,
    close: &str,
) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label_contains(page).click();
    harness.run();
    harness.get_by_label_contains(language).click();
    harness.run();
    harness.get_by_label(close).click();
    harness.run();
}

#[test]
fn starts_on_the_main_dimensions_of_the_default_pump() {
    let harness = harness();
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::MainDimensions)
    ));
    // The tree lists the step, and the report shows the specific speed of the pump.
    assert!(harness.get_all_by_label("Main dimensions").next().is_some());
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
    // The component bar and the tree both open it.
    harness
        .get_all_by_label("Global setup")
        .next()
        .unwrap()
        .click();
    harness.run();
    // The rotation is a combo box showing "clockwise".
    harness.get_by_value("clockwise").click();
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
    harness.get_by_label_contains("Reset to automatic").click();
    harness.run();
    assert!(manual(&harness).is_none());
}

#[test]
fn every_view_draws_without_a_gpu() {
    let mut harness = harness();
    // One view at a time fills the main area.
    for view in View::ALL {
        harness.state_mut().focus_view(view);
        harness.run();
        match view {
            View::View3d => {
                harness.get_by_label("The 3D view needs the wgpu renderer.");
            }
            View::Report => {
                harness.get_by_label("Blade sections");
            }
            View::Performance => {
                // Key numbers on top; the rest folded below.
                harness.get_by_label("Best efficiency");
                harness.get_by_label("Forces");
                harness.get_by_label_contains("More results");
            }
            _ => {}
        }
    }
}

#[test]
fn the_properties_panel_opens_with_a_step_and_folds() {
    let mut harness = harness();
    assert!(harness.state().sidebar_open());
    harness.get_by_label_contains("Impeller 1 · Main dimensions");
    harness.key_press(Key::Escape);
    harness.run();
    // Escape folds the panel to a strip, which opens it again.
    harness.run();
    assert!(!harness.state().sidebar_open());
    harness.get_by_label("Show the properties").click();
    harness.run();
    assert!(harness.state().sidebar_open());
    harness.key_press(Key::Escape);
    harness.run();
    harness.run();
    // Picking a step in the tree opens it again, and the view follows the step.
    // The toolbar and the tree both list the step.
    harness
        .get_all_by_label("Blade properties")
        .next()
        .unwrap()
        .click();
    harness.run();
    assert!(harness.state().sidebar_open());
    harness.get_by_label_contains("Impeller 1 · Blade properties");
    assert_eq!(harness.state().view(), View::Blade);
}

#[test]
fn the_volute_is_designed_after_the_impeller() {
    let mut harness = harness();
    // Impeller up to the blade edges.
    for _ in 0..6 {
        accept(&mut harness);
    }
    // Then every volute step, from the tree.
    harness.get_by_label("Volute setup").click();
    harness.run();
    for step in tp_project::VOLUTE_STEPS {
        assert!(matches!(harness.state().selection(), Selection::Step(_, s) if s == step));
        accept(&mut harness);
        let project = harness.state().document().project();
        assert_eq!(
            project.components()[1].status(step),
            StepStatus::Done,
            "{step}"
        );
    }
    // The report and the 3D view include the volute.
    harness.state_mut().focus_view(View::Report);
    harness.run();
    harness.get_by_label_contains(": volute");
}

#[test]
fn a_diffuser_goes_between_impeller_and_volute() {
    use tp_project::{ComponentKind, VANED_STEPS};
    let mut harness = harness();
    harness.get_by_label("Add component").click();
    harness.run();
    harness
        .get_by_label_contains("Stator (vaned diffuser)")
        .click();
    harness.run();
    let kinds: Vec<ComponentKind> = harness
        .state()
        .document()
        .project()
        .components()
        .iter()
        .map(|c| c.kind())
        .collect();
    assert_eq!(
        kinds,
        [
            ComponentKind::Impeller,
            ComponentKind::Stator { vaned: true },
            ComponentKind::Volute
        ]
    );
    for step in VANED_STEPS {
        assert!(matches!(harness.state().selection(), Selection::Step(_, s) if s == step));
        accept(&mut harness);
        let project = harness.state().document().project();
        assert_eq!(
            project.components()[1].status(step),
            StepStatus::Done,
            "{step}"
        );
    }
    harness.state_mut().focus_view(View::Report);
    harness.run();
    harness.get_by_label_contains(": vaned diffuser");
}

#[test]
fn the_toolbar_accepts_steps_and_adds_components() {
    use tp_project::ComponentKind;
    let mut harness = harness();
    // The tab of the selected component accepts its step.
    harness.get_by_label("Accept").click();
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
    // The home tab adds a vaneless diffuser, which goes before the outlet volute.
    harness.get_by_label("Home tab").click();
    harness.run();
    harness.get_by_label("Vaneless diffuser").click();
    harness.run();
    let kinds: Vec<ComponentKind> = harness
        .state()
        .document()
        .project()
        .components()
        .iter()
        .map(|c| c.kind())
        .collect();
    assert_eq!(
        kinds,
        [
            ComponentKind::Impeller,
            ComponentKind::Stator { vaned: false },
            ComponentKind::Volute
        ]
    );
    // The new component is selected, and the left panel can show the 3D model tree.
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(id, _) if id == harness.state().document().project().components()[1].id()
    ));
    harness.get_by_label_contains("3D model").click();
    harness.run();
    harness.get_by_label("Side walls");
    harness.get_by_label("Casing");
}

#[test]
fn the_interface_switches_to_spanish_and_back() {
    let mut harness = harness();
    harness.get_by_label("View tab").click();
    harness.run();
    choose_language(&mut harness, "Language", "Español", "Cerrar");
    // The step editor, the tree and the toolbar now read in Spanish.
    harness.get_by_label_contains("Aceptar paso");
    assert!(
        harness
            .get_all_by_label("Dimensiones principales")
            .next()
            .is_some()
    );
    harness.get_by_label("Vista tab");
    assert!(harness.query_by_label_contains(ACCEPT).is_none());
    // Every view draws in Spanish, and the steps still go forward.
    for view in View::ALL {
        harness.state_mut().focus_view(view);
        harness.run();
    }
    harness.state_mut().focus_view(View::Report);
    harness.run();
    harness.get_by_label("Informe");
    harness.get_by_label_contains("Aceptar paso").click();
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
    choose_language(&mut harness, "Idioma", "English", "Close");
    harness.get_by_label_contains(ACCEPT);
}

#[test]
fn the_settings_window_changes_theme_size_and_units() {
    let mut harness = harness();
    // From the View tab of the toolbar.
    harness.get_by_label("View tab").click();
    harness.run();
    harness.get_all_by_label("Settings").next().unwrap().click();
    harness.run();
    harness.get_by_label_contains("Dark").click();
    harness.run();
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);
    harness.get_by_label("125 %").click();
    harness.run();
    harness.get_by_label("100 %").click();
    harness.run();

    // The units belong to the project: one undo step each.
    harness.get_by_label_contains("Units").click();
    harness.run();
    harness.get_by_label("US customary").click();
    harness.run();
    let units =
        |h: &Harness<'_, TurbopumpApp>| h.state().document().project().display_units.clone();
    assert_eq!(units(&harness), tp_units::DisplayUnits::us());
    harness.get_by_label("Close").click();
    harness.run();
    harness.get_by_label("US units");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(units(&harness), tp_units::DisplayUnits::si());

    // The status bar opens the units page; a single unit makes the set custom.
    harness.get_by_label("SI units").click();
    harness.run();
    harness.get_by_value("mm").click();
    harness.run();
    harness.get_by_label("in").click();
    harness.run();
    let custom = units(&harness);
    assert_eq!(custom.get(tp_units::QuantityKind::Length).symbol, "in");
    assert_eq!(custom.get(tp_units::QuantityKind::Head).symbol, "m");
    harness.get_by_label("Custom");
    harness.key_press(Key::Escape);
    harness.run();
    harness.get_by_label("Custom units");
}

#[test]
fn cross_sections_are_defined_round_the_spiral() {
    use tp_design::volute::{SectionShape, SideShape};
    let mut harness = harness();
    for _ in 0..6 {
        accept(&mut harness);
    }
    harness.get_by_label("Volute setup").click();
    harness.run();
    accept(&mut harness);
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::CrossSection)
    ));
    assert_eq!(harness.state().view(), View::CrossSection);
    let input = |harness: &Harness<'_, TurbopumpApp>| {
        let project = harness.state().document().project();
        project.volute_input(project.components()[1].id()).unwrap()
    };
    // The −z side becomes radius based, still symmetric.
    harness.get_by_value("Trapezoid").click();
    harness.run();
    harness.get_by_label("Radius based").click();
    harness.run();
    let sections = input(&harness).cross_section.sections;
    assert!(
        matches!(
            &sections[0].shape,
            SectionShape::Sides {
                left: SideShape::RadiusBased(_),
                right: None
            }
        ),
        "{sections:?}"
    );
    // A second definition, halfway round, with its own +z side.
    harness.get_by_label_contains("Add section").click();
    harness.run();
    let sections = input(&harness).cross_section.sections;
    assert_eq!(sections.len(), 2);
    assert!((sections[1].angle - std::f64::consts::PI).abs() < 1e-9);
    harness
        .get_all_by_value("symmetric")
        .last()
        .unwrap()
        .click();
    harness.run();
    harness.get_by_label("Rectangular").click();
    harness.run();
    let sections = input(&harness).cross_section.sections;
    assert!(matches!(
        &sections[1].shape,
        SectionShape::Sides {
            right: Some(SideShape::Rectangular { .. }),
            ..
        }
    ));
    // Both shape the design, which still builds.
    let project = harness.state().document().project();
    assert!(project.compute_volute(project.components()[1].id()).is_ok());
}

#[test]
fn internal_sections_share_their_bend() {
    use tp_design::volute::{BendWall, SectionShape};
    let mut harness = harness();
    for _ in 0..6 {
        accept(&mut harness);
    }
    harness.get_by_label("Volute setup").click();
    harness.run();
    accept(&mut harness);
    let input = |harness: &Harness<'_, TurbopumpApp>| {
        let project = harness.state().document().project();
        project.volute_input(project.components()[1].id()).unwrap()
    };
    harness.get_by_value("Trapezoid").click();
    harness.run();
    harness.get_by_label("Round, asymmetric, internal").click();
    harness.run();
    // A second internal definition, then a sharp inner wall on the selected one: both
    // share the bend.
    harness.get_by_label_contains("Add section").click();
    harness.run();
    harness
        .get_all_by_value("quarter ellipse")
        .next()
        .unwrap()
        .click();
    harness.run();
    harness.get_by_label("sharp corner").click();
    harness.run();
    let sections = input(&harness).cross_section.sections;
    assert_eq!(sections.len(), 2);
    for d in &sections {
        let SectionShape::RoundInternal(round) = &d.shape else {
            panic!("{d:?}");
        };
        assert_eq!(round.inner_bend, BendWall::Sharp);
    }
    // The design builds, with its bend.
    let project = harness.state().document().project();
    let g = project
        .compute_volute(project.components()[1].id())
        .unwrap();
    assert!(g.bend.is_some());
}

#[test]
fn file_menu_offers_the_drawings_as_svg_dxf_and_dwg() {
    let mut harness = harness();
    harness.get_by_label("File").click();
    harness.run();
    for view in ["Meridional drawing", "Front view drawing"] {
        for format in ["SVG", "DXF", "DWG"] {
            harness.get_by_label(&format!("{view} ({format})…"));
        }
    }
}

#[test]
fn the_new_project_assistant_builds_the_chosen_components() {
    use tp_project::ComponentKind;
    let mut harness = harness();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    // Machine and global setup keep their defaults; the panel behind has a "Next" too.
    for _ in 0..2 {
        harness
            .get_all_by_label_contains("Next")
            .last()
            .unwrap()
            .click();
        harness.run();
    }
    harness.get_by_value("none").click();
    harness.run();
    harness.get_by_label("vaned").click();
    harness.run();
    harness.get_by_label("Create").click();
    harness.run();
    // The window's first project was never saved: the editor asks before replacing it.
    harness.get_by_label("Don't save").click();
    harness.run();
    let kinds: Vec<ComponentKind> = harness
        .state()
        .document()
        .project()
        .components()
        .iter()
        .map(|c| c.kind())
        .collect();
    assert_eq!(
        kinds,
        [
            ComponentKind::Impeller,
            ComponentKind::Stator { vaned: true },
            ComponentKind::Volute
        ]
    );
}
