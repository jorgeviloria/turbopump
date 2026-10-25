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
    // The design flow opens it.
    harness
        .get_all_by_label("Design point and fluid")
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
    // The dot of the first parameter (hydraulic efficiency) sets it by hand.
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
    // The menu of the panel puts the whole step back to automatic.
    harness.get_by_label("More actions of the step").click();
    harness.run();
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
    harness.get_by_label("Impeller 1 › Sizing");
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
    harness.get_by_label("Impeller 1 › Blading");
    assert_eq!(harness.state().view(), View::Blade);
}

#[test]
fn the_volute_is_designed_after_the_impeller() {
    let mut harness = harness();
    // Impeller up to the blade edges.
    for _ in 0..6 {
        accept(&mut harness);
    }
    // Then every volute step: Alt+↓ goes on from the impeller to the volute.
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
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
fn the_panel_accepts_steps_and_the_column_adds_components() {
    use tp_project::ComponentKind;
    let mut harness = harness();
    accept(&mut harness);
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
    // The + of the component column adds a vaneless diffuser, which goes before the
    // outlet volute.
    harness.get_by_label("Add component").click();
    harness.run();
    harness.get_by_label_contains("Vaneless stator").click();
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
    // The new component is selected, and the scene of the 3D view lists its parts.
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(id, _) if id == harness.state().document().project().components()[1].id()
    ));
    harness.state_mut().focus_view(View::View3d);
    harness.run();
    harness.get_by_label("Side walls");
    harness.get_by_label("Casing");
}

/// The labels of the property rows in sight that are cut short: their text, wider than
/// their cell. Row labels are one line of body text in the label column of the panel,
/// at the right of the 1440 px window.
fn cut_labels(h: &Harness<'_, TurbopumpApp>) -> Vec<String> {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT;
    let font = egui::TextStyle::Body.resolve(&h.ctx.style_of(h.ctx.theme()));
    let line = h.ctx.fonts_mut(|f| f.row_height(&font));
    h.query_all_by_role(Role::Label)
        .filter(|n| (960.0..1030.0).contains(&n.rect().left()))
        .filter(|n| (n.rect().height() - line).abs() < 1.0)
        .filter_map(|n| {
            let text = n.accesskit_node().label().or(n.accesskit_node().value())?;
            let natural = h.ctx.fonts_mut(|f| {
                f.layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            (natural > n.rect().width() + 1.0).then_some(text)
        })
        .collect()
}

/// The row labels cut short anywhere in the design, and in the settings of the curves.
fn cut_labels_everywhere(harness: &mut Harness<'_, TurbopumpApp>) -> Vec<String> {
    let mut cut = std::collections::BTreeSet::new();
    harness.state_mut().focus_view(View::Meridian);
    harness.run();
    for _ in 0..16 {
        harness.key_press_modifiers(Modifiers::ALT, Key::ArrowUp);
    }
    harness.run();
    for _ in 0..16 {
        harness.run();
        cut.extend(cut_labels(harness));
        harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
        harness.run();
    }
    harness.state_mut().focus_view(View::Performance);
    harness.run();
    harness.run();
    cut.extend(cut_labels(harness));
    cut.into_iter().collect()
}

#[test]
fn no_row_label_of_the_properties_is_cut_short() {
    let mut harness = harness();
    let english = cut_labels_everywhere(&mut harness);
    choose_language(&mut harness, "Language", "Español", "Cerrar");
    let spanish = cut_labels_everywhere(&mut harness);
    choose_language(&mut harness, "Idioma", "English", "Close");
    assert!(
        english.is_empty(),
        "cut short in English:\n{}",
        english.join("\n")
    );
    assert!(
        spanish.is_empty(),
        "cut short in Spanish:\n{}",
        spanish.join("\n")
    );
}

#[test]
fn the_report_shows_its_values_whole_and_clear_of_the_panel() {
    let mut harness = harness();
    harness.get_by_label("Report tab").click();
    harness.run();
    harness.run();
    let panel = panel_left(&harness);
    let in_report = |name: &str| {
        harness
            .get_all_by_label(name)
            .map(|n| n.rect())
            .find(|r| r.left() > 380.0 && r.right() < panel)
            .unwrap_or_else(|| panic!("{name} is not in the report, clear of the panel"))
    };
    // A value sits centred in a field as wide as those of the panel, its unit after
    // the field: well apart, not squeezed into a few characters.
    for (value, unit) in [("998.2", "kg/m³"), ("0.02339", "bar"), ("2900", "rpm")] {
        let (value, unit) = (in_report(value), in_report(unit));
        assert!(
            unit.left() - value.right() > 40.0,
            "{value:?} then {unit:?}"
        );
    }
}

#[test]
fn the_curves_take_their_settings_in_the_properties_panel() {
    use egui_kittest::kittest::NodeT;
    let mut harness = harness();
    harness.state_mut().focus_view(View::Performance);
    harness.run();
    let page = |harness: &mut Harness<'_, TurbopumpApp>, name: &str| {
        harness
            .get_all_by_label(name)
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .unwrap()
            .click();
        harness.run();
    };
    // The settings, in sections of rows like those of a step, without its footer.
    for row in ["Curve", "Losses and forces", "Liquid", "Static head H_stat"] {
        harness.get_by_label(row);
    }
    assert!(harness.query_by_label_contains("Accept step").is_none());
    // The step is one click away, and the curves stay in the view.
    page(&mut harness, "Step");
    harness.get_by_label_contains("Accept step");
    harness.get_by_label("Best efficiency");
    // Folded on the step, the panel opens on the settings from the Settings tool.
    harness.key_press(Key::Escape);
    harness.run();
    harness.run();
    assert!(!harness.state().sidebar_open());
    harness.get_by_label("Settings").click();
    harness.run();
    assert!(harness.state().sidebar_open());
    harness.get_by_label("Static head H_stat");
    page(&mut harness, "Step");
    page(&mut harness, "Curve settings");
    harness.get_by_label("Static head H_stat");
}

#[test]
fn the_tabs_of_the_toolbar_are_the_views() {
    let mut harness = harness();
    harness.get_by_label("Curves tab").click();
    harness.run();
    assert_eq!(harness.state().view(), View::Performance);
    // The properties panel shows the settings of the prediction; the Settings tool
    // under the tab gives the step back, and shows them again.
    harness.get_by_label("Operation in the system");
    harness.get_by_label("Settings").click();
    harness.run();
    assert!(harness.query_by_label("Operation in the system").is_none());
    harness.get_by_label_contains("Accept step");
    harness.get_by_label("Settings").click();
    harness.run();
    harness.get_by_label("Operation in the system");
    // The Meridian tab opens on the impeller channel with its step in the panel.
    harness.get_by_label("Meridian tab").click();
    harness.run();
    assert_eq!(harness.state().view(), View::Meridian);
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::MeridionalContour)
    ));
    harness.get_by_label("Impeller channel");
    harness.get_by_label("Stage").click();
    harness.run();
    // Folded, the toolbar leaves the tabs, which still pick the view.
    harness
        .get_by_label("Hide the tools; the tabs still pick the view")
        .click();
    harness.run();
    assert!(harness.query_by_label("Stage").is_none());
    harness.get_by_label("3D tab").click();
    harness.run();
    assert_eq!(harness.state().view(), View::View3d);
}

#[test]
fn the_interface_switches_to_spanish_and_back() {
    let mut harness = harness();
    choose_language(&mut harness, "Language", "Español", "Cerrar");
    // The step editor, the tree and the toolbar now read in Spanish.
    harness.get_by_label_contains("Aceptar paso");
    assert!(
        harness
            .get_all_by_label("Dimensiones principales")
            .next()
            .is_some()
    );
    harness.get_by_label("Meridiano tab");
    assert!(harness.query_by_label_contains(ACCEPT).is_none());
    // Every view draws in Spanish, and the steps still go forward.
    for view in View::ALL {
        harness.state_mut().focus_view(view);
        harness.run();
    }
    harness.state_mut().focus_view(View::Report);
    harness.run();
    harness.get_by_label("Informe tab");
    // The editor moved the panel to the meridional contour: back to the first step.
    harness
        .get_all_by_label("Dimensiones principales")
        .last()
        .unwrap()
        .click();
    harness.run();
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
    // From the gear of the toolbar.
    harness
        .get_by_label("Settings: theme, language, units…")
        .click();
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
    {
        // The list of the length unit, not the units shown after the fields.
        use egui_kittest::kittest::NodeT;
        harness
            .get_all_by_value("mm")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::ComboBox)
            .unwrap()
            .click();
    }
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
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
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
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
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
    // A new window has nothing to save.
    assert!(!harness.state().document().is_dirty());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    // Project, design point and fluid keep their defaults; the panel behind has a
    // "Next" too.
    for _ in 0..3 {
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
    // The window's first project is untouched: it is replaced without asking.
    harness.get_by_label("Create").click();
    harness.run();
    assert!(harness.query_by_label("Don't save").is_none());
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

#[test]
fn the_objective_strip_shows_the_target_and_the_prediction() {
    let mut harness = harness();
    harness.get_by_label_contains("TARGET");
    harness.get_by_label_contains("PREDICTED · IMPELLER 1");
    // The default pump is predicted within the head tolerance of its class (G3).
    let head = format!("% {}", egui_phosphor::regular::CHECK_CIRCLE);
    harness.get_by_label_contains(&head).click();
    harness.run();
    assert_eq!(harness.state().view(), View::Performance);
}

#[test]
fn a_tight_head_tolerance_flags_the_prediction() {
    let mut project = tp_gui::default_project();
    project.acceptance.head_tolerance = tp_design::Param::Manual(0.001);
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_eframe(move |_| TurbopumpApp::with_document(tp_project::Document::new(project)));
    harness.run();
    harness.get_by_label_contains(&format!("% {}", egui_phosphor::regular::WARNING));
}

#[test]
fn the_acceptance_settings_leave_the_steps_as_they_are() {
    let mut harness = harness();
    accept(&mut harness);
    harness.get_by_label_contains("TARGET").click();
    harness.run();
    assert_eq!(harness.state().selection(), Selection::Setup);
    // The sections above fold out of the way; the quality class is then the first
    // automatic parameter in sight.
    for section in ["Project", "Design point", "Fluid and inlet conditions"] {
        // The heading of the section, after the path at the top of the panel.
        harness.get_all_by_label(section).last().unwrap().click();
        harness.run();
    }
    harness.get_all_by_label("auto").next().unwrap().click();
    harness.run();
    assert_eq!(
        harness.state().document().project().acceptance.class,
        tp_design::Param::Manual(tp_design::acceptance::QualityClass::G3)
    );
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
}

/// The row of a step in the design flow (the heading of the panel may show its name).
fn row<'a>(
    harness: &'a Harness<'_, TurbopumpApp>,
    step: &'a str,
) -> Option<egui_kittest::Node<'a>> {
    use egui_kittest::kittest::NodeT;
    harness
        .query_all_by_label(step)
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
}

#[test]
fn the_design_flow_keeps_the_active_component_open() {
    let mut harness = harness();
    // The impeller is active: its steps show, the volute's do not.
    row(&harness, "Blade properties").unwrap();
    assert!(row(&harness, "Cross section").is_none());
    // Its caret folds it, and opening the volute keeps it folded.
    harness.get_by_label("Fold Impeller 1").click();
    harness.run();
    assert!(row(&harness, "Blade properties").is_none());
    harness.get_by_label("Volute 1").click();
    harness.run();
    row(&harness, "Cross section").unwrap();
    assert!(row(&harness, "Blade properties").is_none());
    // The caret folds the active volute until another step opens.
    harness.get_by_label("Fold Volute 1").click();
    harness.run();
    assert!(row(&harness, "Cross section").is_none());
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
    harness.run();
    row(&harness, "Cross section").unwrap();
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::CrossSection)
    ));
    // Pinned, the impeller stays open while the volute is active.
    harness.get_all_by_label("Pin open").next().unwrap().click();
    harness.run();
    row(&harness, "Blade properties").unwrap();
    row(&harness, "Cross section").unwrap();
}

#[test]
fn the_design_flow_folds_to_a_rail() {
    let mut harness = harness();
    harness.get_by_label("Fold the design flow").click();
    harness.run();
    assert!(harness.query_by_label("Blade properties").is_none());
    // The rail opens a component at its next step.
    harness.get_by_label("Volute 1").click();
    harness.run();
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::VoluteSetup)
    ));
    harness.get_by_label("Show the design flow").click();
    harness.run();
    harness.get_by_label("Cross section");
}

#[test]
fn the_scene_shows_hides_and_isolates_the_parts() {
    let mut harness = harness();
    harness.get_by_label("3D tab").click();
    harness.run();
    use egui_kittest::kittest::NodeT;
    // The eye of a part: the check box with its name (a toolbar group may share it).
    fn eye<'a>(h: &'a Harness<'_, TurbopumpApp>, part: &'a str) -> egui_kittest::Node<'a> {
        h.get_all_by_label(part)
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::CheckBox)
            .unwrap_or_else(|| panic!("no eye for {part}"))
    }
    let shown = |h: &Harness<'_, TurbopumpApp>, part: &str| {
        eye(h, part).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    };
    assert!(shown(&harness, "Casing"));
    // The eye hides a part.
    eye(&harness, "Casing").click();
    harness.run();
    assert!(!shown(&harness, "Casing"));
    // Isolating the blades hides every other part; again, everything comes back.
    harness.get_by_label("Isolate Blades").click();
    harness.run();
    assert!(shown(&harness, "Blades"));
    assert!(!shown(&harness, "Hub (disk)"));
    harness.get_by_label("Isolate Blades").click();
    harness.run();
    assert!(shown(&harness, "Hub (disk)"));
    assert!(!shown(&harness, "Casing"), "as it was before");
    // One blade or all of them.
    harness.get_by_label("All blades").click();
    harness.run();
    assert!(harness.query_by_label("All blades").is_some());
    // The menu shows everything; the panel folds to a pill and opens again.
    harness.get_by_label("Scene menu").click();
    harness.run();
    harness.get_by_label("Show all").click();
    harness.run();
    assert!(shown(&harness, "Casing"));
    harness.get_by_label("Fold the scene").click();
    harness.run();
    assert!(harness.query_by_label("Casing").is_none());
    harness.get_by_label("Show the scene").click();
    harness.run();
    eye(&harness, "Casing");
}

/// Whether a toggle button is on, however its role reports it.
fn is_on(node: &egui_kittest::Node<'_>) -> bool {
    use egui_kittest::kittest::NodeT;
    let node = node.accesskit_node();
    node.is_selected() == Some(true) || node.toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_scene_sets_opacity_and_color_and_the_camera_goes_parallel() {
    use egui_kittest::kittest::NodeT;
    let mut harness = harness();
    harness.get_by_label("3D tab").click();
    harness.run();
    // The casing starts as glass; the button steps to opaque, then translucent.
    assert!(is_on(&harness.get_by_label("Opacity of Casing")));
    harness.get_by_label("Opacity of Casing").click();
    harness.run();
    assert!(!is_on(&harness.get_by_label("Opacity of Casing")));
    harness.get_by_label("Opacity of Casing").click();
    harness.run();
    assert!(is_on(&harness.get_by_label("Opacity of Casing")));
    // The swatch opens the color and the three opacities; the color is the default one.
    harness.get_by_label("Color of Blades").click();
    harness.run();
    assert!(
        harness
            .get_by_label("Default color")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_label("Glass").click();
    harness.run();
    assert!(is_on(&harness.get_by_label("Opacity of Blades")));
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Default color").is_none());
    // Parallel projection, on and off.
    harness.get_by_label("Orthographic").click();
    harness.run();
    assert!(is_on(&harness.get_by_label("Orthographic")));
    harness.get_by_label("Orthographic").click();
    harness.run();
    assert!(!is_on(&harness.get_by_label("Orthographic")));
}

#[test]
fn the_3d_tab_cuts_and_measures() {
    use egui_kittest::kittest::NodeT;
    let mut harness = harness();
    harness.get_by_label("3D tab").click();
    harness.run();
    // The cut's own buttons wait for the cut.
    let disabled = |h: &Harness<'_, TurbopumpApp>, label: &str| {
        h.get_by_label_contains(label)
            .accesskit_node()
            .is_disabled()
    };
    assert!(disabled(&harness, "Other half"));
    harness.get_by_label("Meridional cut").click();
    harness.run();
    assert!(is_on(&harness.get_by_label("Meridional cut")));
    assert!(!disabled(&harness, "Other half"));
    harness.get_by_label_contains("Other half").click();
    harness.run();
    harness.get_by_label("Meridional cut").click();
    harness.run();
    assert!(!is_on(&harness.get_by_label("Meridional cut")));
    assert!(disabled(&harness, "Open toward the view"));
    harness.get_by_label("Measure").click();
    harness.run();
    assert!(is_on(&harness.get_by_label("Measure")));
}

#[test]
fn folded_sections_still_save_the_step() {
    let mut harness = harness();
    // The last section of the main dimensions folds; an edit above it still counts.
    harness.get_by_label("Inlet and cavitation").click();
    harness.run();
    harness.get_all_by_label("auto").next().unwrap().click();
    harness.run();
    let project = harness.state().document().project();
    let id = project.components()[0].id();
    assert!(
        project
            .pump_impeller_input(id)
            .unwrap()
            .hydraulic_efficiency
            .manual_value()
            .is_some()
    );
}

#[test]
fn command_enter_accepts_the_step_and_goes_on() {
    let mut harness = harness();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::MeridionalContour)
    ));
}

#[test]
fn every_step_of_a_component_completes_at_once() {
    use tp_project::{GEOMETRY_STEPS, VOLUTE_STEPS};
    let mut harness = harness();
    let component = |h: &Harness<'_, TurbopumpApp>, i: usize| {
        h.state().document().project().components()[i].clone()
    };
    // The button beside the pin of the impeller accepts its six steps.
    harness
        .get_by_label("Complete every step of Impeller 1")
        .click();
    harness.run();
    let impeller = component(&harness, 0);
    assert_eq!(
        impeller.status(DesignStep::MainDimensions),
        StepStatus::Done
    );
    for step in GEOMETRY_STEPS {
        assert_eq!(impeller.status(step), StepStatus::Done, "{step}");
    }
    assert_eq!(
        harness.state().selection(),
        Selection::Step(impeller.id(), DesignStep::BladeEdges)
    );
    // One undo takes them all back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Pending
    );
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run();
    // Done, the button has nothing left to do.
    use egui_kittest::kittest::NodeT;
    assert!(
        harness
            .get_by_label("Complete every step of Impeller 1")
            .accesskit_node()
            .is_disabled()
    );
    // ⌘⇧↩ completes the component of the selected step: the volute.
    harness.get_by_label("Volute 1").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter);
    harness.run();
    let volute = component(&harness, 1);
    for step in VOLUTE_STEPS {
        assert_eq!(volute.status(step), StepStatus::Done, "{step}");
    }
}

#[test]
fn a_section_counts_its_values_by_hand_and_makes_them_automatic() {
    let mut harness = harness();
    let manual = |h: &Harness<'_, TurbopumpApp>| {
        let project = h.state().document().project();
        let id = project.components()[0].id();
        let input = project.pump_impeller_input(id).unwrap();
        [
            input.hydraulic_efficiency.manual_value().is_some(),
            input.volumetric_efficiency.manual_value().is_some(),
        ]
    };
    // Two efficiencies by hand: their section counts them (the frame after).
    for i in 0..2 {
        harness.get_all_by_label("auto").nth(i).unwrap().click();
        harness.run();
    }
    harness.run();
    assert_eq!(manual(&harness), [true, true]);
    // One click puts both back, as one edit.
    harness.get_by_label("All 2 to automatic").click();
    harness.run();
    assert_eq!(manual(&harness), [false, false]);
    harness.run();
    assert!(harness.query_by_label_contains("to automatic").is_none());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(manual(&harness), [true, true]);
}

#[test]
fn rows_changed_since_the_step_was_accepted_are_marked() {
    let mut harness = harness();
    let marks = |h: &Harness<'_, TurbopumpApp>| {
        h.query_all_by_label_contains("changed since the step was accepted")
            .count()
    };
    accept(&mut harness);
    // Back on the accepted step: nothing changed yet.
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowUp);
    harness.run();
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
    assert_eq!(marks(&harness), 0);
    // A value set by hand outdates the step and marks its row.
    harness.get_all_by_label("auto").next().unwrap().click();
    harness.run();
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Stale
    );
    assert_eq!(marks(&harness), 1);
    // Accepted again, the mark goes.
    accept(&mut harness);
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowUp);
    harness.run();
    harness.run();
    assert_eq!(marks(&harness), 0);
}

/// Opens `project` from a file, as after closing the editor and starting it again.
fn reopened(project: &tp_project::Project, name: &str) -> Harness<'static, TurbopumpApp> {
    let path = std::env::temp_dir().join(format!("turbopump-{name}.tpj"));
    project.save(&path).unwrap();
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_eframe(move |cc| TurbopumpApp::new(cc, Some(&path)));
    harness.run();
    harness
}

#[test]
fn what_an_accepted_step_showed_is_kept_in_the_project_file() {
    let mut harness = harness();
    accept(&mut harness);
    let mut project = harness.state().document().project().clone();
    let rows = project.components()[0]
        .accepted(DesignStep::MainDimensions)
        .expect("kept as the step was accepted");
    assert!(rows.keys().any(|k| k.ends_with("Outlet diameter d₂")));
    // Undoing the accept forgets them with it.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(
        harness.state().document().project().components()[0]
            .accepted(DesignStep::MainDimensions)
            .is_none()
    );
    // A higher head, out of sight and after a restart, outdates the step: the rows
    // that moved are marked though the step never showed again while done.
    let mut setup = project.setup().clone();
    setup.design_point.energy = tp_project::EnergyTransfer::Head(40.0);
    project.set_setup(setup).unwrap();
    let mut harness = reopened(&project, "accepted-rows");
    harness
        .get_all_by_label("Main dimensions")
        .find(|n| n.rect().left() < 400.0)
        .unwrap()
        .click();
    harness.run();
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Stale
    );
    harness.get_by_label("Outlet diameter d₂ changed since the step was accepted");
}

#[test]
fn folded_sections_keep_their_rows_as_the_step_is_accepted() {
    // What the rows of the main dimensions showed as they were accepted.
    let accepted = |fold: bool| {
        let mut harness = harness();
        if fold {
            harness.get_by_label("More actions of the step").click();
            harness.run();
            harness.get_by_label_contains("Fold every section").click();
            harness.run();
            harness.run();
            assert!(harness.query_by_label("Outlet diameter d₂").is_none());
        }
        accept(&mut harness);
        let rows = harness.state().document().project().components()[0]
            .accepted(DesignStep::MainDimensions)
            .cloned()
            .unwrap();
        rows.into_keys().collect::<Vec<_>>()
    };
    let open = accepted(false);
    assert!(open.iter().any(|k| k.ends_with("Outlet diameter d₂")));
    assert_eq!(accepted(true), open);
}

#[test]
fn other_units_or_language_mark_no_row() {
    let mut harness = harness();
    accept(&mut harness);
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowUp);
    harness.run();
    harness.get_by_label("SI units").click();
    harness.run();
    harness.get_by_label("US customary").click();
    harness.run();
    harness.get_by_label("Close").click();
    harness.run();
    choose_language(&mut harness, "Language", "Español", "Cerrar");
    // One value set by hand outdates the step: only its row is marked.
    harness.get_all_by_label("auto").next().unwrap().click();
    harness.run();
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Stale
    );
    let marks = harness
        .query_all_by_label_contains("cambió desde que se aceptó el paso")
        .count();
    choose_language(&mut harness, "Idioma", "English", "Close");
    assert_eq!(marks, 1);
}

#[test]
fn the_views_menu_names_the_views_of_a_pump() {
    use egui_kittest::kittest::NodeT;
    let mut harness = harness();
    harness.get_by_label("3D tab").click();
    harness.run();
    harness.get_by_label("Views").click();
    harness.run();
    for name in [
        "From the inlet",
        "From behind",
        "From the side",
        "From the other side",
    ] {
        harness.get_by_label(name);
    }
    // Without a drawn view (no GPU here) there is no camera to save.
    assert!(
        harness
            .get_by_label_contains("Save this view")
            .accesskit_node()
            .is_disabled()
    );
}

/// Where every labelled widget is, by label (the first of a name).
fn layout(h: &Harness<'_, TurbopumpApp>) -> std::collections::BTreeMap<String, egui::Rect> {
    use egui_kittest::kittest::NodeT;
    let mut map = std::collections::BTreeMap::new();
    for node in h.get_all_by_label_contains("") {
        if let Some(label) = node.accesskit_node().label()
            && !label.is_empty()
        {
            map.entry(label).or_insert_with(|| node.rect());
        }
    }
    map
}

/// The labelled widgets that moved between two layouts, with how far.
fn moved(
    before: &std::collections::BTreeMap<String, egui::Rect>,
    after: &std::collections::BTreeMap<String, egui::Rect>,
) -> Vec<String> {
    before
        .iter()
        .filter_map(|(label, a)| {
            let b = after.get(label)?;
            let shift = (a.min - b.min).length().max((a.max - b.max).length());
            (shift > 0.5).then(|| format!("{label}: {a:?} -> {b:?}"))
        })
        .collect()
}

#[test]
fn nothing_moves_when_the_pointer_rests_on_the_objective() {
    let mut harness = harness();
    harness.run();
    let before = layout(&harness);
    let target = harness
        .get_all_by_label_contains("TARGET")
        .next()
        .unwrap()
        .rect()
        .center();
    harness.hover_at(target);
    harness.run_steps(30);
    let after = layout(&harness);
    let shifted = moved(&before, &after);
    assert!(shifted.is_empty(), "{}", shifted.join("\n"));
}

/// Hovers every labelled widget in turn: no other widget may move.
fn nothing_moves_on_hover(harness: &mut Harness<'_, TurbopumpApp>) {
    harness.remove_cursor();
    harness.run();
    let settled = layout(harness);
    harness.run_steps(20);
    let baseline = layout(harness);
    let settling = moved(&settled, &baseline);
    assert!(
        settling.is_empty(),
        "still moving with nothing touched:\n{}",
        settling
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut problems = Vec::new();
    for (label, rect) in &baseline {
        harness.hover_at(rect.center());
        harness.run_steps(3);
        for line in moved(&baseline, &layout(harness)) {
            problems.push(format!("hovering {label:?}: {line}"));
        }
        harness.remove_cursor();
        harness.run_steps(3);
    }
    problems.dedup();
    assert!(
        problems.is_empty(),
        "{} shifts, first ones:\n{}",
        problems.len(),
        problems
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn nothing_moves_on_hover_in_any_view() {
    let mut harness = harness();
    nothing_moves_on_hover(&mut harness);
    for tab in ["3D tab", "Meridian tab", "Curves tab"] {
        harness.get_by_label(tab).click();
        harness.run();
        nothing_moves_on_hover(&mut harness);
    }
}

#[test]
fn nothing_moves_when_a_toggle_is_selected() {
    let mut harness = harness();
    harness.get_by_label("3D tab").click();
    harness.run();
    for toggle in [
        "Orthographic",
        "Opacity of Casing",
        "Opacity of Blades",
        "Shaded",
        "Meridional cut",
        "Pin open",
    ] {
        harness.remove_cursor();
        harness.run();
        let before = layout(&harness);
        harness.get_all_by_label(toggle).next().unwrap().click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        let after = layout(&harness);
        let shifted: Vec<String> = moved(&before, &after)
            .into_iter()
            .filter(|line| !line.starts_with(toggle))
            .collect();
        assert!(shifted.is_empty(), "{toggle}:\n{}", shifted.join("\n"));
    }
}

/// The left edge of what the properties panel shows: its labels start there.
fn panel_left(h: &Harness<'_, TurbopumpApp>) -> f32 {
    let crumb = h
        .get_all_by_label_contains("›")
        .filter(|n| n.rect().left() > 700.0)
        .map(|n| n.rect().left())
        .fold(f32::INFINITY, f32::min);
    // The menu of a step, or of the settings of the curves.
    let _ = h.get_by_label_contains("More actions of the");
    crumb
}

#[test]
fn the_properties_panel_keeps_its_width_in_every_view_and_step() {
    let mut harness = harness();
    // Every place of the design, in every view.
    let mut widths = Vec::new();
    for tab in [
        "Meridian tab",
        "Blade tab",
        "Front tab",
        "Section tab",
        "3D tab",
        "Curves tab",
        "Report tab",
    ] {
        harness.get_by_label(tab).click();
        harness.run();
        for _ in 0..14 {
            harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
            harness.run();
            harness.get_by_label(tab).click();
            harness.run();
            widths.push((tab, panel_left(&harness)));
        }
    }
    let first = widths[0].1;
    let off: Vec<String> = widths
        .iter()
        .filter(|(_, x)| x.is_finite() && (x - first).abs() > 0.5)
        .map(|(tab, x)| format!("{tab}: {x}"))
        .collect();
    assert!(
        off.is_empty(),
        "panel left at {first}, but\n{}",
        off.join("\n")
    );
}

/// The inputs of the properties panel: numeric fields, lists and text fields.
fn panel_inputs(h: &Harness<'_, TurbopumpApp>) -> Vec<(String, egui::Rect)> {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT;
    h.query_all_by_role(Role::SpinButton)
        .chain(h.query_all_by_role(Role::ComboBox))
        .chain(h.query_all_by_role(Role::TextInput))
        .filter(|n| {
            matches!(
                n.accesskit_node().role(),
                Role::SpinButton | Role::ComboBox | Role::TextInput
            ) && n.rect().left() > 1000.0
        })
        .map(|n| {
            let name = n.accesskit_node().label().or(n.accesskit_node().value());
            (name.unwrap_or_default(), n.rect())
        })
        .collect()
}

#[test]
fn the_inputs_of_the_properties_line_up() {
    let mut harness = harness();
    // The design point, the main dimensions and the meridional contour.
    let mut checked = 0;
    for _ in 0..3 {
        let inputs = panel_inputs(&harness);
        if let Some((_, first)) = inputs.first() {
            let off: Vec<String> = inputs
                .iter()
                .filter(|(_, r)| {
                    (r.left() - first.left()).abs() > 1.0 || (r.right() - first.right()).abs() > 1.0
                })
                .map(|(name, r)| format!("{name}: {r:?} (first {first:?})"))
                .collect();
            assert!(off.is_empty(), "{}", off.join("\n"));
            checked += inputs.len();
        }
        harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
        harness.run();
        harness.run();
    }
    assert!(checked > 10, "{checked}");
}

#[test]
fn warnings_appearing_do_not_move_the_form() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT;
    // The meridional contour without warnings, and with the inlet line leaning 15°,
    // which warns that the meridional velocity rises.
    let contour_top = |tilted: bool| {
        let mut project = tp_gui::default_project();
        let id = project.components()[0].id();
        project.design_step(id, DesignStep::MainDimensions).unwrap();
        if tilted {
            let mut input = project.impeller_geometry_input(id).unwrap();
            input.meridional.inlet_tilt =
                tp_design::impeller::meridian::InletTilt::Angle(15f64.to_radians());
            project.set_impeller_geometry(id, input).unwrap();
        }
        let path = std::env::temp_dir().join(format!("turbopump-warnings-{tilted}.tpj"));
        project.save(&path).unwrap();
        let mut harness = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
        harness.run();
        // Its row in the design flow.
        harness
            .get_all_by_label("Meridional contour")
            .find(|n| n.rect().left() < 400.0)
            .unwrap()
            .click();
        harness.run();
        harness.run();
        let warned = harness.query_by_label("Warnings of the step").is_some();
        assert_eq!(
            warned, tilted,
            "the tilted inlet warns, the straight one does not"
        );
        harness
            .get_all_by_label("Contour")
            .filter(|n| n.rect().left() > 1000.0 && n.accesskit_node().role() != Role::Label)
            .map(|n| n.rect().top())
            .reduce(f32::min)
            .unwrap()
    };
    assert_eq!(contour_top(false), contour_top(true));
}

#[test]
fn the_footer_buttons_keep_their_place() {
    let mut harness = harness();
    let back = |h: &Harness<'_, TurbopumpApp>| h.get_by_label_contains("Back").rect();
    // Pending main dimensions: Accept.
    let pending = back(&harness);
    accept(&mut harness);
    // A later step that waits for the earlier ones: Accept, disabled.
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowDown);
    harness.run();
    harness.run();
    let blocked = back(&harness);
    // The accepted step: Next.
    for _ in 0..2 {
        harness.key_press_modifiers(Modifiers::ALT, Key::ArrowUp);
        harness.run();
    }
    harness.run();
    assert_eq!(
        status(&harness, DesignStep::MainDimensions),
        StepStatus::Done
    );
    let done = back(&harness);
    assert_eq!(pending, done, "Back moved between Accept and Next");
    assert_eq!(pending, blocked, "Back moved for a blocked step");
}

/// The right edge of the properties panel's content: its header actions end there.
fn panel_right(h: &Harness<'_, TurbopumpApp>) -> f32 {
    h.get_by_label_contains("More actions of the")
        .rect()
        .right()
}

#[test]
fn the_fields_reach_the_right_edge_of_the_panel() {
    let mut harness = harness();
    let check = |h: &Harness<'_, TurbopumpApp>| {
        let right = panel_right(h);
        // The units follow the fields, and the unit column ends at the panel's edge.
        for (name, r) in panel_inputs(h) {
            assert!(
                right - r.right() < 42.0 && right - r.right() > 15.0,
                "{name} ends at {} but the panel at {right}",
                r.right()
            );
        }
    };
    check(&harness);
    // Wider by dragging its left edge: the fields follow.
    let edge = harness.get_by_label("More actions of the step").rect();
    let left = harness
        .get_all_by_label_contains("›")
        .find(|n| n.rect().left() > 700.0)
        .unwrap()
        .rect()
        .left()
        - 14.0;
    let from = egui::pos2(left, edge.center().y + 300.0);
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(120.0, 0.0));
    harness.run();
    harness.drop_at(from - egui::vec2(120.0, 0.0));
    harness.run();
    harness.run();
    check(&harness);
}

#[test]
fn nothing_moves_on_hover_in_the_other_views() {
    let mut harness = harness();
    for tab in ["Blade tab", "Front tab", "Section tab", "Report tab"] {
        harness.get_by_label(tab).click();
        harness.run();
        nothing_moves_on_hover(&mut harness);
    }
}

#[test]
fn nothing_moves_on_hover_in_the_dialogs() {
    let mut harness = harness();
    // Every page of the settings.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    for page in ["Appearance", "Language", "Units", "Behavior"] {
        harness
            .get_all_by_label_contains(page)
            .next()
            .unwrap()
            .click();
        harness.run();
        nothing_moves_on_hover(&mut harness);
    }
    harness.get_by_label("Close").click();
    harness.run();
    // About.
    harness.get_all_by_label("Help").next().unwrap().click();
    harness.run();
    harness.get_by_label_contains("About").click();
    harness.run();
    nothing_moves_on_hover(&mut harness);
    harness.get_by_label("Close").click();
    harness.run();
    // The export of CAD solids, once the stage is designed.
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
    harness
        .get_all_by_label_contains("CAD solids")
        .find(|n| n.rect().left() < 400.0)
        .unwrap()
        .click();
    harness.run();
    harness.get_by_label_contains("Export CAD solids");
    nothing_moves_on_hover(&mut harness);
    harness.key_press(Key::Escape);
    harness.run();
    // A new project.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    nothing_moves_on_hover(&mut harness);
}

#[test]
fn a_blade_loading_sets_the_beta_distribution() {
    use tp_design::impeller::blade::BetaDistribution;
    let mut harness = harness();
    for _ in 0..3 {
        accept(&mut harness);
    }
    assert!(matches!(
        harness.state().selection(),
        Selection::Step(_, DesignStep::BladeMeanLines)
    ));
    // The quality of the linear distribution is already shown.
    harness.get_by_label("Deceleration w₂/w₁ (shroud)");
    harness.get_by_value("linear").click();
    harness.run();
    harness.get_by_label("by blade loading").click();
    harness.run();
    let project = harness.state().document().project();
    let id = project.components()[0].id();
    let beta = project.impeller_geometry_input(id).unwrap().mean_lines.beta;
    assert_eq!(beta, BetaDistribution::loading());
    // Hub and shroud each get their four rows, and the blade is still built.
    assert_eq!(harness.get_all_by_label("Middle slope").count(), 2);
    harness.get_by_label("Secondary-flow drive ΔC_p");
}

/// A harness on the meridional contour of a saved copy of the default pump, after
/// `prepare` has changed its inputs.
fn on_the_contour(
    name: &str,
    prepare: impl FnOnce(&mut tp_project::Project, tp_project::ComponentId),
) -> Harness<'static, TurbopumpApp> {
    let mut project = tp_gui::default_project();
    let id = project.components()[0].id();
    project.design_step(id, DesignStep::MainDimensions).unwrap();
    prepare(&mut project, id);
    let path = std::env::temp_dir().join(format!("turbopump-{name}.tpj"));
    project.save(&path).unwrap();
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_eframe(|cc| TurbopumpApp::new(cc, Some(&path)));
    harness.run();
    harness
        .get_all_by_label("Meridional contour")
        .find(|n| n.rect().left() < 400.0)
        .unwrap()
        .click();
    harness.run();
    harness.run();
    harness
}

fn seals(h: &Harness<'_, TurbopumpApp>) -> tp_design::impeller::seals::Seals {
    let project = h.state().document().project();
    let id = project.components()[0].id();
    project
        .impeller_geometry_input(id)
        .unwrap()
        .meridional
        .seals
}

#[test]
fn the_material_step_shows_both_seal_cards() {
    let harness = on_the_contour("seal-cards", |_, _| {});
    for text in [
        "Front seal",
        "Rear seal",
        "Seal diameter",
        "Seal length",
        "Radial clearance",
        "Impeller ring",
        "Casing ring",
    ] {
        assert!(
            harness.get_all_by_label(text).next().is_some(),
            "no {text:?}"
        );
    }
    // Two cards: each has its own rows.
    assert_eq!(harness.get_all_by_label("Seal diameter").count(), 2);
}

#[test]
fn toggling_a_seal_keeps_its_manual_values() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT;
    use tp_design::Param;
    let mut harness = on_the_contour("seal-toggle", |project, id| {
        let mut input = project.impeller_geometry_input(id).unwrap();
        input.meridional.seals.rear.diameter = Param::Manual(0.09);
        project.set_impeller_geometry(id, input).unwrap();
    });
    let toggle = |h: &mut Harness<'_, TurbopumpApp>| {
        h.get_all_by_label("Rear seal")
            .find(|n| n.accesskit_node().role() == Role::CheckBox)
            .unwrap()
            .click_accesskit();
        h.run();
        h.run();
    };
    toggle(&mut harness);
    assert!(!seals(&harness).rear.enabled);
    // A switched-off seal hides its rows.
    assert_eq!(harness.get_all_by_label("Seal diameter").count(), 1);
    toggle(&mut harness);
    let rear = seals(&harness).rear;
    assert!(rear.enabled);
    assert_eq!(rear.diameter, Param::Manual(0.09));
    assert_eq!(harness.get_all_by_label("Seal diameter").count(), 2);
}

#[test]
fn seal_cards_do_not_move_on_hover() {
    let mut harness = on_the_contour("seal-hover", |_, _| {});
    nothing_moves_on_hover(&mut harness);
}

#[test]
fn a_double_entry_impeller_hides_the_rear_seal() {
    use tp_design::MainDimensionsInput;
    use tp_design::pump::Suction;
    let harness = on_the_contour("seal-double", |project, id| {
        let mut input = project.pump_impeller_input(id).unwrap();
        input.suction = Suction::Double;
        project
            .set_main_dimensions(id, MainDimensionsInput::CentrifugalPump(input))
            .unwrap();
        project.design_step(id, DesignStep::MainDimensions).unwrap();
    });
    assert!(harness.get_all_by_label("Front seal").next().is_some());
    assert!(harness.query_by_label("Rear seal").is_none());
}
