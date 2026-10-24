//! Step editor of the volute cross sections: the table of definitions round the spiral,
//! each with its −z side and its own or a mirrored +z side, and the properties of the
//! selected one. Its control points are dragged in the cross-section view.

use std::f64::consts::TAU;

use eframe::egui::{self, Button, ComboBox, DragValue, Grid, Id, Ui, vec2};
use tp_design::outlet::Reach;
use tp_design::volute::section::default_segments;
use tp_design::volute::{
    BendWall, BezierBase, BezierSide, DEFAULT_CONE_ANGLE_DEG, ExternalRound, InnerPosition,
    InternalRound, RadiusSide, SectionDefinition, SectionShape, SegmentPoint, SideShape,
    VoluteDesign, VoluteInput,
};
use tp_project::ComponentId;
use tp_units::{DisplayUnits, QuantityKind};

use crate::app::EditCtx;
use crate::i18n::{tf, tr};
use crate::steps::{section, section_info};
use crate::theme::{bold, icon};
use crate::widgets::{info, label, param_f64, props, quantity, value_info, value_row};

/// Key of the definition selected for editing, shared with the cross-section view.
fn selection_key(id: ComponentId) -> Id {
    Id::new(("cross_section_selected", id.0))
}

/// Index of the definition selected in the volute `id`, among `count`.
pub(crate) fn selected(ctx: &egui::Context, id: ComponentId, count: usize) -> usize {
    ctx.data(|d| d.get_temp::<usize>(selection_key(id)))
        .unwrap_or(0)
        .min(count.saturating_sub(1))
}

fn select(ctx: &egui::Context, id: ComponentId, index: usize) {
    ctx.data_mut(|d| d.insert_temp(selection_key(id), index));
}

/// The kinds of shape the tables offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Rectangular,
    Trapezoidal,
    Round,
    External,
    Internal,
    BezierRectangle,
    BezierTrapezoid,
    LineSegments,
    RadiusBased,
}

impl Kind {
    /// Every kind a whole section can be.
    const SECTION: [Kind; 9] = [
        Kind::Rectangular,
        Kind::Trapezoidal,
        Kind::Round,
        Kind::External,
        Kind::Internal,
        Kind::BezierRectangle,
        Kind::BezierTrapezoid,
        Kind::LineSegments,
        Kind::RadiusBased,
    ];

    /// Every kind one side can be.
    const SIDE: [Kind; 7] = [
        Kind::Rectangular,
        Kind::Trapezoidal,
        Kind::Round,
        Kind::BezierRectangle,
        Kind::BezierTrapezoid,
        Kind::LineSegments,
        Kind::RadiusBased,
    ];

    fn label(self) -> &'static str {
        tr(match self {
            Kind::Rectangular => "Rectangular",
            Kind::Trapezoidal => "Trapezoid",
            Kind::Round => "Round",
            Kind::External => "Round, asymmetric",
            Kind::Internal => "Round, asymmetric, internal",
            Kind::BezierRectangle => "Bézier, rectangle type",
            Kind::BezierTrapezoid => "Bézier, trapezoid type",
            Kind::LineSegments => "Line segments",
            Kind::RadiusBased => "Radius based",
        })
    }

    fn hint(self) -> &'static str {
        tr(match self {
            Kind::Rectangular => {
                "Simplest shape; it cannot be cast and grows large, sensible only at low \
                 specific speed"
            }
            Kind::Trapezoidal => {
                "Flanks opening at the cone angle: flatter than a rectangle, with weaker \
                 secondary flow; it cannot be cast"
            }
            Kind::Round => "Elliptic arc through the inlet point: good for stresses",
            Kind::Internal => {
                "Round section beside the impeller, inside the inlet radius, reached through \
                 a bend and a neck: for volutes that must stay small in diameter"
            }
            Kind::External => {
                "Round section growing toward one side: a more favourable secondary flow, \
                 often after mixed-flow impellers"
            }
            Kind::BezierRectangle => "A rectangle with cast radii, shaped by a Bézier curve",
            Kind::BezierTrapezoid => "A trapezoid with cast radii, shaped by a Bézier curve",
            Kind::LineSegments => "Corners you place and round, scaled with the section",
            Kind::RadiusBased => "Radial base, cone and main arc, joined by two radii",
        })
    }

    fn of_side(side: &SideShape) -> Kind {
        match side {
            SideShape::Rectangular { .. } => Kind::Rectangular,
            SideShape::Trapezoidal { .. } => Kind::Trapezoidal,
            SideShape::Round { .. } => Kind::Round,
            SideShape::Bezier(b) if b.base == BezierBase::Trapezoid => Kind::BezierTrapezoid,
            SideShape::Bezier(_) => Kind::BezierRectangle,
            SideShape::LineSegments { .. } => Kind::LineSegments,
            SideShape::RadiusBased(_) => Kind::RadiusBased,
        }
    }

    fn of_section(shape: &SectionShape) -> Kind {
        match shape {
            SectionShape::Sides { left, .. } => Kind::of_side(left),
            SectionShape::RoundExternal(_) => Kind::External,
            SectionShape::RoundInternal(_) => Kind::Internal,
        }
    }

    /// A side of this kind that keeps the cone and tangent angles of `previous`.
    fn side(self, previous: Option<&SideShape>) -> SideShape {
        let default_cone = DEFAULT_CONE_ANGLE_DEG.to_radians();
        let (cone, tangent) = match previous {
            Some(SideShape::Trapezoidal {
                cone_angle,
                tangent_angle,
            }) => (*cone_angle, *tangent_angle),
            Some(SideShape::Rectangular { tangent_angle }) => (default_cone, *tangent_angle),
            Some(SideShape::Bezier(b)) => (b.cone_angle, b.tangent_angle),
            Some(SideShape::RadiusBased(r)) => (r.cone_angle, 0.0),
            _ => (default_cone, 0.0),
        };
        let cone = if cone > 0.0 { cone } else { default_cone };
        match self {
            Kind::Rectangular => SideShape::Rectangular {
                tangent_angle: tangent,
            },
            Kind::Trapezoidal | Kind::External | Kind::Internal => SideShape::Trapezoidal {
                cone_angle: cone,
                tangent_angle: tangent,
            },
            Kind::Round => SideShape::Round { ellipse_ratio: 1.0 },
            Kind::BezierRectangle | Kind::BezierTrapezoid => SideShape::Bezier(BezierSide {
                base: if self == Kind::BezierTrapezoid {
                    BezierBase::Trapezoid
                } else {
                    BezierBase::Rectangle
                },
                cone_angle: cone,
                tangent_angle: tangent,
                ..BezierSide::default()
            }),
            Kind::LineSegments => SideShape::LineSegments {
                points: default_segments(),
            },
            Kind::RadiusBased => SideShape::RadiusBased(RadiusSide {
                cone_angle: cone,
                ..RadiusSide::default()
            }),
        }
    }

    /// `shape` with its −z side (or the whole section) of this kind.
    fn apply(self, shape: &SectionShape) -> SectionShape {
        match (self, shape) {
            (Kind::External, SectionShape::RoundExternal(_)) => shape.clone(),
            (Kind::External, _) => SectionShape::RoundExternal(ExternalRound::default()),
            (Kind::Internal, SectionShape::RoundInternal(_)) => shape.clone(),
            (Kind::Internal, _) => SectionShape::RoundInternal(InternalRound::default()),
            (kind, SectionShape::Sides { left, right }) => SectionShape::Sides {
                left: kind.side(Some(left)),
                right: right.clone(),
            },
            (kind, SectionShape::RoundExternal(_) | SectionShape::RoundInternal(_)) => {
                SectionShape::symmetric(kind.side(None))
            }
        }
    }
}

/// Combo box of `kinds` showing `current`; returns the kind picked.
fn kind_combo(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    current: Option<Kind>,
    kinds: &[Kind],
    none: Option<&str>,
) -> Option<Option<Kind>> {
    let mut picked = None;
    let text = current.map_or_else(|| none.unwrap_or("").to_owned(), |k| k.label().to_owned());
    // Tall enough to list every kind without scrolling.
    ComboBox::from_id_salt(salt)
        .selected_text(text)
        .height(400.0)
        .show_ui(ui, |ui| {
            if let Some(none) = none
                && ui.selectable_label(current.is_none(), none).clicked()
            {
                picked = Some(None);
            }
            for &kind in kinds {
                if ui
                    .selectable_label(current == Some(kind), kind.label())
                    .on_hover_text(kind.hint())
                    .clicked()
                {
                    picked = Some(Some(kind));
                }
            }
        });
    picked
}

/// The kinds of the sides of a definition, short, for the list of definitions.
fn kinds(shape: &SectionShape) -> String {
    match shape {
        SectionShape::Sides { left, right: None } => Kind::of_side(left).label().to_owned(),
        SectionShape::Sides {
            left,
            right: Some(right),
        } => format!(
            "{} / {}",
            Kind::of_side(left).label(),
            Kind::of_side(right).label()
        ),
        other => Kind::of_section(other).label().to_owned(),
    }
}

/// The shape of a definition in words, for reports.
pub(crate) fn describe(shape: &SectionShape, units: &DisplayUnits) -> String {
    let angle = |a: f64| units.show(QuantityKind::Angle, a);
    let side = |s: &SideShape| -> String {
        let kind = Kind::of_side(s).label();
        match s {
            SideShape::Trapezoidal { cone_angle, .. } => {
                tf("{}, flanks at {}", &[&kind, &angle(*cone_angle)])
            }
            SideShape::Bezier(b) if b.base == BezierBase::Trapezoid => {
                tf("{}, flanks at {}", &[&kind, &angle(b.cone_angle)])
            }
            SideShape::RadiusBased(r) => tf("{}, flanks at {}", &[&kind, &angle(r.cone_angle)]),
            _ => kind.to_owned(),
        }
    };
    match shape {
        SectionShape::Sides { left, right: None } => side(left),
        SectionShape::Sides {
            left,
            right: Some(right),
        } => tf("{} (−z) / {} (+z)", &[&side(left), &side(right)]),
        SectionShape::RoundExternal(r) => tf(
            "{}, growing toward {}",
            &[
                &Kind::External.label(),
                &if r.open_to_right { "+z" } else { "−z" },
            ],
        ),
        SectionShape::RoundInternal(r) => tf(
            "{}, growing toward {}",
            &[
                &Kind::Internal.label(),
                &if r.open_to_right { "+z" } else { "−z" },
            ],
        ),
    }
}

/// The wrap angle in the largest gap between `angles`, from the cutwater on.
fn free_angle(angles: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = angles.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mut previous = 0.0;
    let mut best = (0.0, TAU / 2.0);
    for &a in &sorted {
        if a - previous > best.0 {
            best = (a - previous, 0.5 * (a + previous));
        }
        previous = a;
    }
    // Up to the throat, unless something is defined there: then at the throat.
    if TAU - previous >= best.0 {
        best = (TAU - previous, TAU);
    }
    best.1.clamp(1f64.to_radians(), TAU)
}

pub(crate) fn step(
    ui: &mut Ui,
    cx: &mut EditCtx<'_>,
    id: ComponentId,
    input: &mut VoluteInput,
) -> &'static str {
    use QuantityKind::{Angle, Area, Length};
    let units = cx.units();
    let g = cx.derived.volute(id);
    let b3 = g.map(|g| g.design.inlet_width);
    let sections = &mut input.cross_section.sections;
    let count = sections.len();
    let mut current = selected(ui.ctx(), id, count);

    // The definitions, one row each: active, angle and shape; click a row to edit it.
    if section(ui, tr("Sections")) {
        let mut remove = None;
        for (i, d) in sections.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut d.active, "")
                    .on_hover_text(tr("Active: only active definitions shape the spiral"));
                let trash = 26.0;
                let width = ui.available_width() - trash - ui.spacing().item_spacing.x;
                let text = format!("{}    {}", units.show(Angle, d.angle), kinds(&d.shape));
                let row = Button::selectable(current == i, text)
                    .right_text("")
                    .min_size(vec2(width, 24.0));
                if ui
                    .add(row)
                    .on_hover_text(tr("Edit this definition"))
                    .clicked()
                {
                    current = i;
                }
                if ui
                    .add_enabled(count > 1, Button::new(icon::TRASH).frame(false))
                    .on_hover_text(tr("Delete this definition"))
                    .clicked()
                {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            sections.remove(i);
            current = current.min(sections.len() - 1);
        }
        if ui
            .button(format!("{}  {}", icon::PLUS, tr("Add section")))
            .on_hover_text(tr(
                "A new definition in the largest gap round the spiral, as the selected one",
            ))
            .clicked()
        {
            let angles: Vec<f64> = sections.iter().map(|d| d.angle).collect();
            let shape = sections[current].shape.clone();
            sections.push(SectionDefinition {
                angle: free_angle(&angles),
                active: true,
                shape,
            });
            current = sections.len() - 1;
        }
        select(ui.ctx(), id, current);
    }
    let definition = &mut sections[current];

    if section_info(
        ui,
        &tf("Section at {}", &[&units.show(Angle, definition.angle)]),
        tr(
            "Drag the control points of Bézier and line-segment sides in the cross-section \
             view; right-click there to add or remove a corner.",
        ),
    ) {
        props(ui, ("section_definition", current), |ui| {
            label(
                ui,
                tr("Wrap angle φ"),
                tr("Where round the spiral this shape applies, from the start of the spiral"),
            );
            quantity(
                ui,
                &mut definition.angle,
                Angle,
                &units,
                1f64.to_radians()..=TAU,
            );
            ui.end_row();
            let asymmetric = matches!(definition.shape, SectionShape::Sides { right: Some(_), .. });
            label(
                ui,
                if asymmetric {
                    tr("−z side")
                } else {
                    tr("Shape")
                },
                "",
            );
            let now = Kind::of_section(&definition.shape);
            if let Some(Some(kind)) = kind_combo(
                ui,
                ("section_left", current),
                Some(now),
                &Kind::SECTION,
                None,
            ) {
                definition.shape = kind.apply(&definition.shape);
            }
            ui.end_row();
            if let SectionShape::Sides { right, .. } = &mut definition.shape {
                label(
                    ui,
                    tr("+z side"),
                    tr("Symmetric: the same shape as the −z side, mirrored"),
                );
                let now = right.as_ref().map(Kind::of_side);
                if let Some(picked) = kind_combo(
                    ui,
                    ("section_right", current),
                    now,
                    &Kind::SIDE,
                    Some(tr("symmetric")),
                ) {
                    *right = picked.map(|k| k.side(right.as_ref()));
                }
                ui.end_row();
            }
        });
        match &mut definition.shape {
            SectionShape::Sides { left, right } => {
                if right.is_some() {
                    ui.add_space(4.0);
                    ui.label(bold(tr("−z side")));
                }
                side_editor(ui, ("left", current), left, b3, &units);
                if let Some(right) = right {
                    ui.add_space(4.0);
                    ui.label(bold(tr("+z side")));
                    side_editor(ui, ("right", current), right, b3, &units);
                }
            }
            SectionShape::RoundExternal(round) => external_editor(ui, round),
            SectionShape::RoundInternal(round) => {
                internal_editor(ui, round, g.map(|g| &g.design), &units);
                // Every internal definition shares the bend and the neck.
                let shared = *round;
                for d in sections.iter_mut() {
                    if let SectionShape::RoundInternal(other) = &mut d.shape {
                        *other = InternalRound {
                            ellipse_ratio: other.ellipse_ratio,
                            ..shared
                        };
                    }
                }
            }
        }

        if let Some(g) = g {
            let d = &g.design;
            if section(ui, tr("Throat")) {
                props(ui, "volute_shape_results", |ui| {
                    value_row(ui, tr("Height"), units.show(Length, d.throat.height));
                    value_info(
                        ui,
                        tr("Width over height B/H"),
                        tr("Flat sections with B/H = 2 to 3 lose least (Gülich p. 414)"),
                        format!("{:.2}", d.throat_aspect),
                    );
                    value_row(ui, tr("Area A₃q"), units.show(Area, d.throat.area));
                    if let Some(side) = d.side_gap {
                        let text = match (side.reach, side.gap) {
                            (Reach::Eye, _) => tr("into the suction eye").to_owned(),
                            (_, Some(gap)) => tf(
                                "{} (at least {})",
                                &[&units.show(Length, gap), &units.show(Length, side.required)],
                            ),
                            _ => tr("clear: the spiral stays outside r₂").to_owned(),
                        };
                        value_row(ui, tr("Gap to the impeller"), text);
                    }
                });
            }
        }
    }
    "edit volute cross section"
}

/// One grid row for an angle in [0, `max_deg`]°.
fn angle_row(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut f64,
    max_deg: f64,
    units: &DisplayUnits,
) {
    crate::widgets::label(ui, label, hint);
    quantity(
        ui,
        value,
        QuantityKind::Angle,
        units,
        0.0..=max_deg.to_radians(),
    );
    ui.label("");
    ui.end_row();
}

/// One grid row for a plain number.
fn number_row(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
) {
    crate::widgets::label(ui, label, hint);
    ui.add(
        DragValue::new(value)
            .range(range)
            .speed(0.005)
            .max_decimals(3)
            .update_while_editing(false),
    );
    ui.label("");
    ui.end_row();
}

const CONE_HINT: &str = "Angle δ of the flank from the radial direction";
const TANGENT_HINT: &str =
    "Angle λ of the top where it meets the centre line; above 0 the two sides meet in a ridge";

fn side_editor(
    ui: &mut Ui,
    salt: (&str, usize),
    side: &mut SideShape,
    b3: Option<f64>,
    units: &DisplayUnits,
) {
    use tp_design::volute::section::{MAX_CONE_ANGLE_DEG, MAX_TANGENT_ANGLE_DEG};
    let id = format!("section_side_{}_{}", salt.0, salt.1);
    match side {
        SideShape::Rectangular { tangent_angle } => {
            props(ui, &id, |ui| {
                angle_row(
                    ui,
                    tr("Tangent angle λ"),
                    tr(TANGENT_HINT),
                    tangent_angle,
                    MAX_TANGENT_ANGLE_DEG,
                    units,
                );
            });
        }
        SideShape::Trapezoidal {
            cone_angle,
            tangent_angle,
        } => {
            props(ui, &id, |ui| {
                angle_row(
                    ui,
                    tr("Cone angle δ"),
                    tr(CONE_HINT),
                    cone_angle,
                    MAX_CONE_ANGLE_DEG,
                    units,
                );
                angle_row(
                    ui,
                    tr("Tangent angle λ"),
                    tr(TANGENT_HINT),
                    tangent_angle,
                    MAX_TANGENT_ANGLE_DEG,
                    units,
                );
            });
        }
        SideShape::Round { ellipse_ratio } => {
            props(ui, &id, |ui| {
                number_row(
                    ui,
                    tr("Ellipse ratio"),
                    tr("Axial over radial semi-axis, kept as the section grows: 1 for a circle"),
                    ellipse_ratio,
                    0.2..=5.0,
                );
            });
        }
        SideShape::Bezier(b) => bezier_editor(ui, &id, b, b3, units),
        SideShape::LineSegments { points } => segments_editor(ui, &id, points),
        SideShape::RadiusBased(r) => {
            let lengths = b3.map(|b3| r.lengths(b3));
            props(ui, &id, |ui| {
                use QuantityKind::Length;
                param_f64(
                    ui,
                    tr("Base height h"),
                    tr("Radial part from the inlet; automatic: 0.1·b₃"),
                    &mut r.base_height,
                    Length,
                    lengths.map(|l| l.0),
                    units,
                    0.0..=1.0,
                );
                param_f64(
                    ui,
                    tr("Base radius R_B"),
                    tr(
                        "Rounds the base into the cone, limited by their lengths; automatic: 0.1·b₃",
                    ),
                    &mut r.base_radius,
                    Length,
                    lengths.map(|l| l.1),
                    units,
                    0.0..=1.0,
                );
                angle_row(
                    ui,
                    tr("Cone angle δ"),
                    tr(CONE_HINT),
                    &mut r.cone_angle,
                    MAX_CONE_ANGLE_DEG,
                    units,
                );
                param_f64(
                    ui,
                    tr("Corner radius R_C"),
                    tr(
                        "Rounds the cone into the main arc, limited by their lengths; \
                         automatic: 0.3·b₃",
                    ),
                    &mut r.corner_radius,
                    Length,
                    lengths.map(|l| l.2),
                    units,
                    0.0..=1.0,
                );
                param_f64(
                    ui,
                    tr("Main radius R"),
                    tr(
                        "Arc of the top, centred on the centre line; enlarged where it \
                         cannot reach the cone. Automatic: b₃",
                    ),
                    &mut r.main_radius,
                    Length,
                    lengths.map(|l| l.3),
                    units,
                    0.0001..=1.0,
                );
            });
        }
    }
}

fn bezier_editor(ui: &mut Ui, id: &str, b: &mut BezierSide, b3: Option<f64>, units: &DisplayUnits) {
    use tp_design::volute::section::{MAX_CONE_ANGLE_DEG, MAX_TANGENT_ANGLE_DEG};
    props(ui, id, |ui| {
        if b.base == BezierBase::Trapezoid {
            angle_row(
                ui,
                tr("Cone angle δ"),
                tr(CONE_HINT),
                &mut b.cone_angle,
                MAX_CONE_ANGLE_DEG,
                units,
            );
        }
        angle_row(
            ui,
            tr("Tangent angle λ"),
            tr(TANGENT_HINT),
            &mut b.tangent_angle,
            MAX_TANGENT_ANGLE_DEG,
            units,
        );
        label(
            ui,
            tr("Inner point position"),
            tr(
                "Points 1 and 3 as a fraction of the way from the end point to the corner S \
             (relative), or as their distance from S (absolute)",
            ),
        );
        let before = b.position;
        ComboBox::from_id_salt((id, "position"))
            .selected_text(match b.position {
                InnerPosition::Relative => tr("relative"),
                InnerPosition::Absolute => tr("absolute"),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut b.position, InnerPosition::Relative, tr("relative"));
                ui.selectable_value(&mut b.position, InnerPosition::Absolute, tr("absolute"));
            });
        ui.label("");
        ui.end_row();
        if b.position != before {
            let defaults = BezierSide::default();
            (b.point1, b.point3) = match b.position {
                InnerPosition::Relative => (defaults.point1, defaults.point3),
                InnerPosition::Absolute => {
                    let d = 0.25 * b3.unwrap_or(0.02);
                    (d, d)
                }
            };
        }
        for (label, value) in [
            (tr("Point 1"), &mut b.point1),
            (tr("Point 3"), &mut b.point3),
        ] {
            match b.position {
                InnerPosition::Relative => number_row(
                    ui,
                    label,
                    tr("0 at the end point, 1 at the corner S"),
                    value,
                    0.0..=1.0,
                ),
                InnerPosition::Absolute => {
                    crate::widgets::label(ui, label, tr("Distance from the corner S"));
                    quantity(ui, value, QuantityKind::Length, units, 0.0..=1.0);
                    ui.label("");
                    ui.end_row();
                }
            }
        }
        number_row(
            ui,
            tr("Point 2 toward point 0"),
            tr("Point 2 from S, as a fraction of the way to point 0; 0 puts it on S"),
            &mut b.point2[0],
            -1.0..=2.0,
        );
        number_row(
            ui,
            tr("Point 2 toward point 4"),
            tr("Point 2 from S, as a fraction of the way to point 4; 0 puts it on S"),
            &mut b.point2[1],
            -1.0..=2.0,
        );
    });
}

fn segments_editor(ui: &mut Ui, id: &str, points: &mut Vec<SegmentPoint>) {
    ui.horizontal(|ui| {
        ui.label(tr("Corners"));
        info(
            ui,
            tr(
                "Corners from the inlet point, in units of the section height H: out from the \
                 side wall, up from r'_z, and the radius that rounds them (0: sharp).",
            ),
        );
    });
    let mut remove = None;
    Grid::new(id)
        .num_columns(5)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            ui.weak(tr("out/H"));
            ui.weak(tr("up/H"));
            ui.weak(tr("radius/H"));
            ui.label("");
            ui.end_row();
            for (k, p) in points.iter_mut().enumerate() {
                ui.label(format!("{}", k + 1));
                for (value, range) in [
                    (&mut p.out, -1.0..=3.0),
                    (&mut p.up, 0.001..=3.0),
                    (&mut p.radius, 0.0..=1.0),
                ] {
                    ui.add(
                        DragValue::new(value)
                            .range(range)
                            .speed(0.005)
                            .max_decimals(3)
                            .update_while_editing(false),
                    );
                }
                if ui
                    .add(Button::new(icon::TRASH).small())
                    .on_hover_text(tr("Remove this corner"))
                    .clicked()
                {
                    remove = Some(k);
                }
                ui.end_row();
            }
        });
    if let Some(k) = remove {
        points.remove(k);
    }
    if ui
        .button(format!("{}  {}", icon::PLUS, tr("Add corner")))
        .on_hover_text(tr("A corner halfway between the last one and the top"))
        .clicked()
    {
        let last = points.last().map_or((0.0, 0.0), |p| (p.out, p.up));
        // The top lies on the centre line: out = −b₃/(2H), about −0.3 at the throat.
        points.push(SegmentPoint {
            out: 0.5 * (last.0 - 0.3),
            up: 0.5 * (last.1 + 1.0),
            radius: 0.0,
        });
    }
}

fn bend_combo(ui: &mut Ui, salt: &str, wall: &mut BendWall) {
    let text = |w: BendWall| {
        tr(match w {
            BendWall::Sharp => "sharp corner",
            BendWall::Straight => "straight chamfer",
            BendWall::Round => "quarter ellipse",
        })
    };
    ComboBox::from_id_salt(salt)
        .selected_text(text(*wall))
        .show_ui(ui, |ui| {
            for w in [BendWall::Sharp, BendWall::Straight, BendWall::Round] {
                ui.selectable_value(wall, w, text(w));
            }
        });
}

fn internal_editor(
    ui: &mut Ui,
    round: &mut InternalRound,
    design: Option<&VoluteDesign>,
    units: &DisplayUnits,
) {
    // The values the design used: the automatic neck may have grown to keep the gap.
    let neck = design
        .and_then(|d| d.cross_sections.internal())
        .and_then(|r| r.neck_width.manual_value())
        .or_else(|| design.map(|d| 0.5 * d.inlet_width));
    let gap = design.and_then(|d| d.side_gap).map(|g| g.required);
    props(ui, "section_internal", |ui| {
        number_row(
            ui,
            tr("Ellipse ratio"),
            tr("Axial over radial semi-axis, kept as the section grows: 1 for a circle"),
            &mut round.ellipse_ratio,
            0.2..=5.0,
        );
        label(
            ui,
            tr("Open to +z"),
            tr(
                "Toward +z, behind the impeller where its hub disk is; otherwise toward −z, the \
             suction side",
            ),
        );
        ui.checkbox(&mut round.open_to_right, "");
        ui.label("");
        ui.end_row();
        param_f64(
            ui,
            tr("Neck width"),
            tr("Length of the straight neck from the bend to the spiral; automatic: 0.5·b₃"),
            &mut round.neck_width,
            QuantityKind::Length,
            neck,
            units,
            0.0..=1.0,
        );
        param_f64(
            ui,
            tr("Side gap s_ax"),
            tr(
                "Axial gap between the casing wall round the spiral and the impeller beside it; \
                 the automatic neck grows to keep it. Automatic: 0.065·r₂ (Gülich Chap. 9.1)",
            ),
            &mut round.side_gap,
            QuantityKind::Length,
            gap,
            units,
            0.0..=1.0,
        );
        for (label, hint, wall, salt) in [
            (
                tr("Inner bend wall"),
                tr("The wall on the inside of the turn, from the inlet point the flow turns to"),
                &mut round.inner_bend,
                "bend_inner",
            ),
            (
                tr("Outer bend wall"),
                tr("The wall on the outside of the turn, up to the top of the neck"),
                &mut round.outer_bend,
                "bend_outer",
            ),
        ] {
            crate::widgets::label(ui, label, hint);
            bend_combo(ui, salt, wall);
            ui.label("");
            ui.end_row();
        }
        number_row(
            ui,
            tr("Bend ratio"),
            tr(
                "Radial over axial semi-axis of the inner wall of the bend, which spans half the inlet width",
            ),
            &mut round.bend_ratio,
            0.2..=5.0,
        );
        number_row(
            ui,
            tr("Bend area ratio"),
            tr("Height of the neck over the inlet width b₃: the outlet of the bend over its inlet"),
            &mut round.area_ratio,
            0.3..=3.0,
        );
    });
}

fn external_editor(ui: &mut Ui, round: &mut ExternalRound) {
    props(ui, "section_external", |ui| {
        number_row(
            ui,
            tr("Ellipse ratio"),
            tr("Axial over radial semi-axis, kept as the section grows: 1 for a circle"),
            &mut round.ellipse_ratio,
            0.2..=5.0,
        );
        for (value, label, hint) in [
            (
                &mut round.strictly_external,
                tr("Strictly external"),
                tr(
                    "Keep the section outside r'_z; otherwise it also grows inward beside the inlet",
                ),
            ),
            (
                &mut round.open_to_right,
                tr("Open to +z"),
                tr("Grow toward +z instead of −z"),
            ),
            (
                &mut round.square_on_top,
                tr("Square on top"),
                tr("A square corner on the closed side of the top"),
            ),
        ] {
            crate::widgets::label(ui, label, hint);
            ui.checkbox(value, "");
            ui.label("");
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_definitions_go_in_the_largest_gap() {
        assert!((free_angle(&[TAU]) - TAU / 2.0).abs() < 1e-12);
        assert!((free_angle(&[TAU / 2.0]) - TAU).abs() < 1e-12);
        let a = free_angle(&[1.0, 2.0, TAU]);
        assert!((a - 0.5 * (2.0 + TAU)).abs() < 1e-12);
    }

    #[test]
    fn kinds_keep_their_angles_when_switched() {
        let shape = SectionShape::trapezoidal(0.4);
        let bezier = Kind::BezierTrapezoid.apply(&shape);
        assert_eq!(Kind::of_section(&bezier), Kind::BezierTrapezoid);
        let SectionShape::Sides {
            left: SideShape::Bezier(b),
            ..
        } = &bezier
        else {
            panic!("{bezier:?}");
        };
        assert_eq!(b.cone_angle, 0.4);
        assert_eq!(
            Kind::External.apply(&shape),
            SectionShape::RoundExternal(ExternalRound::default())
        );
        for kind in Kind::SECTION {
            assert_eq!(Kind::of_section(&kind.apply(&shape)), kind);
        }
    }
}
