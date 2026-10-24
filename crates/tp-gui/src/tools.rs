//! The tools of each view, shown in the toolbar under its tab: modes, what to draw,
//! framing and exports. The values of a design step stay in the properties panel.

use eframe::egui::{self, Popup, Ui};
use tp_units::{DisplayUnits, QuantityKind};

use crate::export::{
    CAD_AVAILABLE, DrawingExport, DrawingFormat, DrawingView, TextExport, VoluteExport,
};
use crate::i18n::{tf, tr};
use crate::ribbon::{Command, Icon, big_button, export_items, group, small_button, stack};
use crate::theme::icon;
use crate::viewer::{CameraView, NamedView, Viewer};
use crate::views::blade::{BladePage, BladeView};
use crate::views::meridian_editor::references_panel;
use crate::views::meridional::{Fit, MeridionalView, Scope};
use crate::views::performance::PerformanceView;

/// What the tools need to know about the project.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ToolsCx {
    pub has_geometry: bool,
    pub has_volute: bool,
}

/// A small button that switches `value`. Returns whether it did.
fn toggle(ui: &mut Ui, glyph: &str, label: &str, hint: &str, value: &mut bool) -> bool {
    let response = small_button(ui, glyph, label, true, *value);
    let clicked = if hint.is_empty() {
        response.clicked()
    } else {
        response.on_hover_text(hint).clicked()
    };
    if clicked {
        *value = !*value;
    }
    clicked
}

/// A large button that opens a menu below it.
fn menu_button(
    ui: &mut Ui,
    glyph: &'static str,
    label: &str,
    enabled: bool,
    hint: &str,
    add: impl FnOnce(&mut Ui),
) {
    let response = big_button(ui, Icon::Glyph(glyph), label, enabled, false, None);
    let response = if hint.is_empty() {
        response
    } else {
        response.on_hover_text(hint)
    };
    Popup::menu(&response).show(|ui| {
        ui.set_min_width(220.0);
        add(ui);
    });
}

/// A large help button: its text shows on hover.
fn help(ui: &mut Ui, label: &str, text: &str) {
    group(ui, tr("Help"), |ui| {
        big_button(ui, Icon::Glyph(icon::QUESTION), label, true, false, None).on_hover_ui(|ui| {
            ui.set_max_width(340.0);
            ui.label(text);
        });
    });
}

/// The drawings of one view, in every format.
fn drawings(ui: &mut Ui, view: DrawingView, enabled: bool) -> Option<Command> {
    let mut command = None;
    group(ui, tr("Export"), |ui| {
        menu_button(ui, icon::EXPORT, tr("Drawing"), enabled, "", |ui| {
            for format in [DrawingFormat::Svg, DrawingFormat::Dxf, DrawingFormat::Dwg] {
                let kind = DrawingExport { view, format };
                if ui.button(kind.menu_label()).clicked() {
                    command = Some(Command::ExportDrawing(kind));
                }
            }
        });
    });
    command
}

/// The Meridian tab: scope, what to show, framing, snaps, drawings and help.
pub(crate) fn meridian(
    ui: &mut Ui,
    view: &mut MeridionalView,
    units: &DisplayUnits,
    cx: ToolsCx,
) -> Option<Command> {
    group(ui, tr("Scope"), |ui| {
        for scope in Scope::ALL {
            if big_button(
                ui,
                Icon::Glyph(scope.icon()),
                scope.title(),
                true,
                view.scope == scope,
                None,
            )
            .on_hover_text(scope.hint())
            .clicked()
            {
                view.set_scope(scope);
            }
        }
    });
    let stage = view.scope == Scope::Stage;
    group(ui, tr("Show"), |ui| {
        let d = &mut view.display;
        stack(ui, |ui| {
            toggle(
                ui,
                icon::RULER,
                tr("Dimensions"),
                tr("The main dimensions d_n, d₁, d₂, b₂ and L. Click a value to type it."),
                &mut d.dimensions,
            );
            ui.add_enabled_ui(!stage, |ui| {
                toggle(
                    ui,
                    icon::CIRCLES_THREE,
                    tr("Adjacent components"),
                    tr("The diffuser or volute after the impeller; the Stage scope always shows them"),
                    &mut d.adjacent,
                );
            });
            toggle(ui, icon::LIST_CHECKS, tr("Legend"), "", &mut d.legend);
        });
        stack(ui, |ui| {
            let charts = d.charts;
            let response = small_button(
                ui,
                icon::EYE,
                &format!("{}  {}", tr("Analysis"), icon::CARET_DOWN),
                true,
                false,
            );
            Popup::menu(&response)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_min_width(240.0);
                    ui.checkbox(&mut d.stream_lines, tr("stream lines"));
                    ui.checkbox(&mut d.quasi_orthogonals, tr("quasi-orthogonals"));
                    ui.checkbox(&mut d.curvature, tr("curvature maximum"))
                        .on_hover_text(tr(
                            "Point of smallest radius of curvature of hub and shroud",
                        ));
                    ui.checkbox(&mut d.area_circles, tr("area circles"))
                        .on_hover_text(tr("Circles as wide as the channel normal to the flow"));
                    ui.checkbox(&mut d.charts, tr("area and curvature charts"))
                        .on_hover_text(tr(
                            "Cross-section area along the channel and curvature along the \
                             walls, under the diagram: both should change smoothly",
                        ));
                    ui.separator();
                    ui.checkbox(&mut d.potential, tr("potential flow"))
                        .on_hover_text(tr(
                            "Two-dimensional potential flow between hub and shroud: its stream \
                             lines, and the meridional velocity along the walls against the \
                             quasi-one-dimensional flow of the design",
                        ));
                    ui.indent("potential_view", |ui| {
                        ui.add_enabled_ui(d.potential, |ui| {
                            let v = &mut d.potential_view;
                            ui.checkbox(&mut v.stream_lines, tr("stream lines"));
                            ui.checkbox(&mut v.iso_lines, tr("c_m iso-lines"));
                            ui.checkbox(&mut v.map, tr("c_m color map"));
                            ui.checkbox(&mut v.vectors, tr("velocity vectors"));
                            ui.checkbox(&mut v.blades, tr("blade blockage"))
                                .on_hover_text(tr(
                                    "Solve with the thickness of the blades: among them only the \
                                     open part of the circumference carries the flow (Gülich eqs. \
                                     3.1.7 and 3.2.3)",
                                ));
                        });
                    });
                });
            // The diagram changes height with the charts: frame it again.
            if d.charts != charts {
                view.fit = Some(Fit::Meridian);
            }
            let response = small_button(
                ui,
                icon::STACK,
                &format!("{}  {}", tr("References"), icon::CARET_DOWN),
                true,
                !view.references.is_empty(),
            )
            .on_hover_text(tr(
                "Curves drawn for comparison: pasted points or copies of a wall",
            ));
            Popup::menu(&response)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_min_width(280.0);
                    references_panel(ui, &mut view.references, &mut view.reference_text, units);
                });
        });
    });
    group(ui, tr("Fit"), |ui| {
        if big_button(
            ui,
            Icon::Glyph(icon::FRAME_CORNERS),
            tr("Fit scope"),
            true,
            false,
            None,
        )
        .on_hover_text(tr("Frame what the scope edits, with its dimensions"))
        .clicked()
        {
            view.fit = Some(view.scope.fit());
        }
        if big_button(
            ui,
            Icon::Glyph(icon::ARROWS_OUT),
            tr("Fit all"),
            true,
            false,
            None,
        )
        .on_hover_text(tr("Frame everything drawn"))
        .clicked()
        {
            view.fit = Some(Fit::All);
        }
    });
    group(ui, tr("Snap"), |ui| {
        stack(ui, |ui| {
            let d = &mut view.display;
            // Grid steps in display units; dragged points snap to them.
            let scale = units.to_display(QuantityKind::Length, 1.0);
            let unit = units.get(QuantityKind::Length).symbol;
            let step = d.snap * scale;
            let name = |step: f64| {
                if step > 0.0 {
                    format!("{step} {unit}")
                } else {
                    tr("free").to_owned()
                }
            };
            ui.horizontal(|ui| {
                ui.label(format!("{}  {}", icon::GRID_FOUR, tr("grid")))
                    .on_hover_text(tr("Dragged points snap to this grid"));
                egui::ComboBox::from_id_salt("meridian_grid")
                    .width(70.0)
                    .selected_text(name(step))
                    .show_ui(ui, |ui| {
                        for option in [0.0, 0.01, 0.1, 0.5, 1.0] {
                            if ui
                                .selectable_label((step - option).abs() < 1e-12, name(option))
                                .clicked()
                            {
                                d.snap = option / scale;
                            }
                        }
                    });
            });
            toggle(
                ui,
                icon::MAGNET,
                tr("point snap"),
                tr(
                    "A dragged point takes the z or r of another point near it; hold Ctrl (Cmd) to drag freely",
                ),
                &mut d.snap_points,
            );
            toggle(
                ui,
                icon::MAGNET_STRAIGHT,
                tr("line snap"),
                tr("A dragged point lands on the line through two other points near it"),
                &mut d.snap_lines,
            );
        });
    });
    let command = drawings(ui, DrawingView::Meridional, cx.has_geometry);
    help(
        ui,
        tr("How to edit"),
        tr(
            "Drag a point to shape the meridian: hollow points are automatic, filled ones set by hand. Double-click a point to make it automatic again. Right-click a point or a wall for exact values, segments and joints. Click a dimension to type it. Hold Alt and drag to measure a distance. Drag the background to pan, scroll to zoom, double-click to fit.",
        ),
    );
    command
}

/// The Blade tab: plots or edges, and the exports of the blades.
pub(crate) fn blade(ui: &mut Ui, view: &mut BladeView, cx: ToolsCx) -> Option<Command> {
    let mut command = None;
    group(ui, tr("Page"), |ui| {
        for (page, glyph, label, hint) in [
            (
                BladePage::Plots,
                icon::CHART_LINE,
                tr("Blade plots"),
                tr("Angles, thickness and loading along the mean line"),
            ),
            (
                BladePage::Edges,
                icon::PENCIL_RULER,
                tr("Edges"),
                tr("The shapes of the leading and trailing edges at the hub and the shroud"),
            ),
        ] {
            if big_button(ui, Icon::Glyph(glyph), label, true, view.page == page, None)
                .on_hover_text(hint)
                .clicked()
            {
                view.page = page;
            }
        }
    });
    group(ui, tr("Export"), |ui| {
        for kind in TextExport::ALL {
            let label = match kind {
                TextExport::Points => tr("Points (TXT)"),
                TextExport::Mesh => tr("Mesh (STL)"),
            };
            if big_button(
                ui,
                Icon::Glyph(icon::EXPORT),
                label,
                cx.has_geometry,
                false,
                None,
            )
            .on_hover_text(kind.menu_label())
            .clicked()
            {
                command = Some(Command::ExportText(kind));
            }
        }
    });
    if view.page == BladePage::Edges {
        help(
            ui,
            tr("How to edit"),
            tr(
                "Drag the green points: the shape goes to the values of the panel. Solid lines are the suction face, dashed ones the pressure face.",
            ),
        );
    }
    command
}

/// The Front tab: the drawings of the front view.
pub(crate) fn front(ui: &mut Ui, cx: ToolsCx) -> Option<Command> {
    drawings(ui, DrawingView::Front, cx.has_geometry)
}

/// The Section tab: the exports of the volute and how to edit a section.
pub(crate) fn section(ui: &mut Ui, cx: ToolsCx) -> Option<Command> {
    let mut command = None;
    group(ui, tr("Export"), |ui| {
        for kind in VoluteExport::ALL {
            let label = match kind {
                VoluteExport::Points => tr("Points (TXT)"),
                VoluteExport::Mesh => tr("Mesh (STL)"),
            };
            if big_button(
                ui,
                Icon::Glyph(icon::EXPORT),
                label,
                cx.has_volute,
                false,
                None,
            )
            .on_hover_text(kind.menu_label())
            .clicked()
            {
                command = Some(Command::ExportVolute(kind));
            }
        }
    });
    help(
        ui,
        tr("How to edit"),
        tr(
            "Drag the round handles to shape Bézier and line-segment sides; right-click to add or remove a corner. Ctrl (⌘) keeps a corner's distance to the previous one, Alt keeps it on its line; add Shift for the next one. Drag the background to pan, scroll to zoom, double-click to fit.",
        ),
    );
    command
}

/// The 3D tab: camera, style and the exports of the model. What is shown, in which
/// color and how see-through, is in the scene over the view.
pub(crate) fn view3d(ui: &mut Ui, viewer: &mut Viewer, cx: ToolsCx) -> Option<Command> {
    let mut command = None;
    group(ui, tr("Camera"), |ui| {
        if big_button(
            ui,
            Icon::Glyph(icon::CORNERS_OUT),
            tr("Fit"),
            true,
            false,
            None,
        )
        .on_hover_text(tr("Fit the model in the view (double-click)"))
        .clicked()
        {
            command = Some(Command::Camera(CameraView::Fit));
        }
        if big_button(
            ui,
            Icon::Glyph(icon::HOUSE),
            tr("Home view"),
            true,
            false,
            None,
        )
        .on_hover_text(tr(
            "Oblique, from the inlet side; the view cube has the others",
        ))
        .clicked()
        {
            command = Some(Command::Camera(CameraView::Iso));
        }
        menu_button(
            ui,
            icon::CAMERA_ROTATE,
            tr("Views"),
            true,
            tr("Views named after the pump, and the ones you save"),
            |ui| {
                for view in NamedView::ALL {
                    if ui.button(view.name()).clicked() {
                        viewer.apply(CameraView::Named(view));
                    }
                }
                ui.separator();
                let mut remove = None;
                for index in 0..viewer.saved.len() {
                    ui.horizontal(|ui| {
                        let name = tf("View {}", &[&(index + 1)]);
                        if ui.button(&name).clicked() {
                            viewer.apply(CameraView::Saved(index));
                        }
                        let delete = ui
                            .small_button(icon::TRASH)
                            .on_hover_text(tf("Forget {}", &[&name]));
                        delete.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                tf("Forget {}", &[&name]),
                            )
                        });
                        if delete.clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    viewer.saved.remove(index);
                }
                if ui
                    .add_enabled(
                        viewer.has_camera(),
                        egui::Button::new(format!("{}  {}", icon::PLUS, tr("Save this view"))),
                    )
                    .on_hover_text(tr(
                        "Keeps the direction, the zoom, the centre and the projection; remembered between sessions",
                    ))
                    .clicked()
                {
                    viewer.save_view();
                }
            },
        );
        if big_button(
            ui,
            Icon::Glyph(icon::PERSPECTIVE),
            tr("Orthographic"),
            true,
            viewer.orthographic,
            None,
        )
        .on_hover_text(tr(
            "Parallel projection: sizes do not shrink with depth, as on a drawing",
        ))
        .clicked()
        {
            viewer.orthographic = !viewer.orthographic;
        }
    });
    let options = &mut viewer.options;
    group(ui, tr("Style"), |ui| {
        let styles = [
            (icon::SPHERE, tr("Shaded"), false, false),
            (icon::POLYGON, tr("With edges"), true, false),
            (icon::CUBE_TRANSPARENT, tr("Wireframe"), false, true),
        ];
        for (glyph, label, edges, wireframe) in styles {
            let selected = options.wireframe == wireframe && (wireframe || options.edges == edges);
            if big_button(ui, Icon::Glyph(glyph), label, true, selected, None).clicked() {
                options.edges = edges;
                options.wireframe = wireframe;
            }
        }
    });
    group(ui, tr("Inspect"), |ui| {
        let cut = viewer.cut.is_some();
        if big_button(
            ui,
            Icon::Glyph(icon::SCISSORS),
            tr("Meridional cut"),
            true,
            cut,
            None,
        )
        .on_hover_text(tr(
            "Cut the model through its axis and remove the half toward the camera",
        ))
        .clicked()
        {
            viewer.set_cut(!cut);
        }
        stack(ui, |ui| {
            if small_button(ui, icon::CROSSHAIR, tr("Face the camera"), cut, false)
                .on_hover_text(tr("Turn the cut to the side the camera looks from"))
                .clicked()
            {
                viewer.aim_cut();
            }
            if small_button(ui, icon::ARROWS_LEFT_RIGHT, tr("Other half"), cut, false)
                .on_hover_text(tr("Remove the other half instead"))
                .clicked()
            {
                viewer.flip_cut();
            }
            let shown = viewer.show_cut_plane;
            if small_button(ui, icon::SQUARE, tr("Show the plane"), cut, shown)
                .on_hover_text(tr("Outline the cutting plane over the model"))
                .clicked()
            {
                viewer.show_cut_plane = !shown;
            }
        });
        stack(ui, |ui| {
            let mut percent = viewer.cut_offset * 100.0;
            ui.add_enabled(
                cut,
                egui::Label::new(tf(
                    "Position {} %",
                    &[&if percent.abs() < 0.5 {
                        "0".to_owned()
                    } else {
                        format!("{percent:+.0}")
                    }],
                )),
            );
            let slider = ui.add_enabled(
                cut,
                egui::Slider::new(&mut percent, -100.0..=100.0).show_value(false),
            );
            slider.widget_info(|| {
                egui::WidgetInfo::slider(cut, f64::from(percent), tr("Position of the cut"))
            });
            if slider
                .on_hover_text(tr(
                    "Moves the cut parallel to itself, in percent of the model's radius; 0 through the axis",
                ))
                .changed()
            {
                viewer.cut_offset = percent / 100.0;
            }
        });
        let measuring = viewer.measuring();
        if big_button(
            ui,
            Icon::Glyph(icon::RULER),
            tr("Measure"),
            cx.has_geometry,
            measuring,
            None,
        )
        .on_hover_text(tr(
            "Click two points of the model for the distance between them and its radial and axial parts; near a corner of the mesh the point snaps to it",
        ))
        .clicked()
        {
            viewer.set_measuring(!measuring);
        }
    });
    group(ui, tr("Export"), |ui| {
        if big_button(
            ui,
            Icon::Glyph(icon::CUBE),
            tr("CAD solids"),
            CAD_AVAILABLE,
            false,
            None,
        )
        .on_hover_text(tr("STEP, IGES or STL solids with Open CASCADE"))
        .on_disabled_hover_text(tr("This build has no CAD support."))
        .clicked()
        {
            command = Some(Command::ExportCad);
        }
        menu_button(ui, icon::EXPORT, tr("Meshes"), cx.has_geometry, "", |ui| {
            if ui.button(TextExport::Mesh.menu_label()).clicked() {
                command = Some(Command::ExportText(TextExport::Mesh));
            }
            if cx.has_volute && ui.button(VoluteExport::Mesh.menu_label()).clicked() {
                command = Some(Command::ExportVolute(VoluteExport::Mesh));
            }
        });
        if big_button(
            ui,
            Icon::Glyph(icon::CAMERA),
            tr("Picture"),
            cx.has_geometry,
            false,
            None,
        )
        .on_hover_text(tr(
            "A PNG of the model as the view shows it, twice as sharp, without the cube, axes or scale",
        ))
        .clicked()
        {
            command = Some(Command::Picture3d);
        }
    });
    help(
        ui,
        tr("Gestures"),
        tr(
            "Drag: rotate\nRight-drag or Shift-drag: pan\nScroll or pinch: zoom\nDouble-click: fit\nClick a part: select it; right-click: hide, isolate, opacity\nEsc: clear the measure, then the selection\nView cube: click a face, edge or corner",
        ),
    );
    command
}

/// The Curves tab: the settings of the prediction and its other results.
pub(crate) fn curves(ui: &mut Ui, view: &mut PerformanceView) -> Option<Command> {
    group(ui, tr("Show"), |ui| {
        if big_button(
            ui,
            Icon::Glyph(icon::SLIDERS_HORIZONTAL),
            tr("Settings"),
            true,
            view.settings_open,
            None,
        )
        .on_hover_text(tr("Recirculation, shut-off, liquid and operation in the system: they shape the curve but are seldom changed"))
        .clicked()
        {
            view.settings_open = !view.settings_open;
        }
        if big_button(
            ui,
            Icon::Glyph(icon::LIST_CHECKS),
            tr("More results"),
            true,
            view.details_open,
            None,
        )
        .on_hover_text(tr(
            "Losses, forces and the rest of the results; the report has them too",
        ))
        .clicked()
        {
            view.details_open = !view.details_open;
        }
    });
    None
}

/// The Report tab: every export of the project.
pub(crate) fn report(ui: &mut Ui, cx: ToolsCx) -> Option<Command> {
    let mut command = None;
    group(ui, tr("Export"), |ui| {
        menu_button(ui, icon::EXPORT, tr("Export"), true, "", |ui| {
            command = export_items(ui, cx.has_geometry, cx.has_volute);
        });
    });
    command
}
