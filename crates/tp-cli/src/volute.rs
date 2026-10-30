//! `turbopump volute`: volute casings of a pump project, after the impeller.

use std::f64::consts::{PI, TAU};
use std::path::{Path, PathBuf};

use clap::Args;
use tp_design::Param;
use tp_design::outlet::Reach;
use tp_design::volute::stress::StressPlace;
use tp_design::volute::{
    AreaLaw, BendWall, BezierBase, BezierSide, CutMeasure, CutwaterShape, DEFAULT_CONE_ANGLE_DEG,
    DiffuserGrowth, DischargeShape, ExternalRound, InnerPosition, InternalRound, OutletShape,
    Progression, RadiusSide, SectionDefinition, SectionShape, SegmentPoint, SideShape,
    StraightWall, VoluteGeometry, VoluteInput, VoluteKind, ZAlignment, ZBend,
};
use tp_project::{ComponentId, ComponentKind, Project, VOLUTE_STEPS};
use tp_units::{DisplayUnits, QuantityKind, parse_quantity_or};

use crate::design::err;
use crate::{CliResult, Outcome, show};

#[derive(Args)]
pub(crate) struct VoluteArgs {
    /// Project file (.tpj). Inputs and step status are saved back into it.
    file: PathBuf,
    /// Only this component id (default: every volute).
    #[arg(long)]
    component: Option<u32>,
    /// Fix an input as KEY=VALUE, or KEY=auto. Keys: flow-factor (Q_Le/Q), dz
    /// (cutwater diameter), b3 (inlet width), e3 (cutwater thickness), shape (the cross
    /// section of the whole spiral: rectangle, trapezoid, round, bezier,
    /// bezier-trapezoid, segments, radius or external, each with optional parameters
    /// after a colon, e.g. trapezoid:30, round:ratio=1.2, radius:main=40mm,cone=30,
    /// segments:points=0/0.08+0.55/1/0.4, external:right,square, internal:neck=10mm,
    /// inner=sharp,gap=5mm (an internal section beside the impeller, through a bend whose
    /// walls are sharp, straight or round, toward +z unless left, its automatic neck long
    /// enough to keep the side gap to the impeller); shape@DEGREES=… defines
    /// it at one wrap angle, blending between angles, and remove, off or on drop or
    /// toggle that definition; shape.right=… or shape@DEGREES.right=… gives the +z side
    /// its own shape, symmetric undoes it), law (auto; the velocity rules momentum
    /// (Pfleiderer, c_u·r^x constant), velocity (constant, throat by angular momentum) or
    /// stepanoff (c3 = K3·√(2gH)); the geometric rules height, area or area_radius
    /// (A/r_c)), wrap (wrap angle of the spiral, 300° to 360°), x (swirl exponent of
    /// momentum), k3 (Stepanoff's factor), cu4 (reference velocity: c_u at r4 = r'z, or
    /// c3 for stepanoff), qi (reference flow), q0 (blind flow at the start), end_height,
    /// end_area and end_area_radius (end values of the geometric rules), progression
    /// (linear or bezier:X1,Y1,X2,Y2), compensation (on or off: the inner radius rises
    /// by comp_e from comp_start to the throat), comp_e, comp_start, arcs (off or the
    /// number of circular arcs of the outer contour), sections, correction (on or off),
    /// length (along the diffuser centerline) and
    /// outlet (discharge diffuser), incidence (cutwater), obliquity (of the rib nose: the
    /// angle of the edge to the pump axis), spiral_start
    /// (where the spiral of a sharp or rounded cutwater starts), wall, fillet
    /// (radius rounding the outside of the casing over the cutwater; 0 for none), type
    /// (single or double),
    /// rib_length (length of the rib of a double volute from its nose, which is always 180° from the cutwater; 180° reaches the throat, less stops the rib short of the discharge diffuser), rib (rib thickness),
    /// rib_comp (on or off: the first partial volute grows by the rib from rib_comp_start
    /// to the nose, so the outer channel starts with the area of the rule), rib_comp_start,
    /// rib_nose (length over thickness of the elliptic nose and tail of the rib, 1 for a
    /// semicircle), channel_ratio (outlet area of the outer channel over the first
    /// throat), discharge (spline:TURN[,START,END], a Bézier centerline turning TURN degrees with shares START and END from 0.1 to 0.9; radial, the default, with the outlet on the pump centerline,
    /// or radial:DEGREES from the line through the pump axis; tangential, opening to both
    /// sides; tangential:inner or tangential:outer with that wall straight), bend (bend radius of a radial discharge), outlet_shape (round, or rectangular[:ASPECT] with the height along the pump axis over the width, the same area as the round outlet and no round flange), z_bend (none, or EXTENT,RADIUS,ANGLE: the centerline of the diffuser runs flat, rounds off with RADIUS through ANGLE degrees and goes on straight, ending EXTENT along the pump axis), eccentricity (−1 to 1 for a tangential diffuser: −1 with the cutwater wall straight, 0 opening to both sides, 1 with the outer wall straight), base_form (0 to 1: how far the inlet chord of the first diffuser section bulges toward the pump axis, so that the diffuser crosses the start of the spiral), height_offset (Δr_C: the diffuser shifted across its centerline, away from the pump axis when positive, to give the cutwater room), z_align (centric, or manual:OFFSET[,quadratic]: the outlet shifted along the pump axis), growth (area of the diffuser from the throat: linear, the default, or matched, which starts with the slope of the spiral), rotation (angle
    /// of the cutwater from the x axis; auto turns the discharge vertically up), cutwater
    /// (tangent, sharp, rounded or simple), nose (radius of a rounded cutwater), cut (how far
    /// back from the tip a simple cutwater cuts the walls) or gap (instead, how wide the gap
    /// between the walls is where it cuts them) and handle (the handles of its curve, as a
    /// share of each wall's cut), height (H, the
    /// outlet face from the pump axis, which sets the diffuser length), dn (on: round the
    /// automatic outlet to the nearest nominal size), flange (on or off), flange_d
    /// (outer diameter), flange_t (thickness), bolt_circle, bolt_hole and bolts, pressure
    /// (casing design pressure over the suction) and allowable (stress of the casing).
    #[arg(long = "set", value_name = "KEY=VALUE")]
    settings: Vec<String>,
    /// Write volute.txt (points) and volute.stl (mesh) into this folder.
    #[arg(long, value_name = "DIR")]
    export: Option<PathBuf>,
    /// Compute and print without saving the project.
    #[arg(long)]
    dry_run: bool,
}

pub(crate) fn run(args: VoluteArgs) -> CliResult {
    let mut project =
        Project::load(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let units = project.display_units.clone();
    let volutes: Vec<(ComponentId, String)> = project
        .components()
        .iter()
        .filter(|c| c.kind() == ComponentKind::Volute)
        .filter(|c| args.component.is_none_or(|n| c.id().0 == n))
        .map(|c| (c.id(), c.name.clone()))
        .collect();
    if volutes.is_empty() {
        return Err(match args.component {
            Some(n) => format!("no volute with id #{n}"),
            None => "the project has no volute".into(),
        });
    }

    let mut warned = false;
    for (id, name) in &volutes {
        let mut input = project.volute_input(*id).map_err(err)?;
        for setting in &args.settings {
            apply(&mut input, &units, setting)?;
        }
        project.set_volute(*id, input).map_err(err)?;
        for step in VOLUTE_STEPS {
            project.design_step(*id, step).map_err(err)?;
        }
        let volute = project.compute_volute(*id).map_err(err)?;
        print_report(name, *id, &volute, &units);
        warned |= !volute.warnings.is_empty();
        if let Some(dir) = &args.export {
            let dir = if volutes.len() > 1 {
                dir.join(format!("volute-{}", id.0))
            } else {
                dir.clone()
            };
            export(&dir, &volute)?;
        }
    }

    if !args.dry_run {
        project.save(&args.file).map_err(err)?;
        println!("Saved {}", args.file.display());
    }
    Ok(if warned {
        Outcome::Warnings
    } else {
        Outcome::Ok
    })
}

fn export(dir: &Path, g: &VoluteGeometry) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let files = [
        ("volute.txt", tp_io::points::volute_txt(g)),
        (
            "volute.stl",
            tp_io::stl::ascii("volute", &tp_io::mesh::volute_triangles(g)),
        ),
    ];
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("Wrote {}", path.display());
    }
    Ok(())
}

/// The parameters after `TYPE:` in a cross-section spec: `name=value` pairs or flags
/// separated by commas, where a bare first value names `positional`.
fn spec_params<'a>(rest: &'a str, positional: &'static str) -> Vec<(&'a str, &'a str)> {
    rest.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .enumerate()
        .map(|(i, item)| match item.split_once('=') {
            Some((name, value)) => (name.trim(), value.trim()),
            None if i == 0 && item.parse::<f64>().is_ok() => (positional, item),
            None => (item, ""),
        })
        .collect()
}

fn number(name: &str, value: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map_err(|_| format!("{name} must be a number, got {value:?}"))
}

/// One side of a cross section from `TYPE[:PARAMS]`.
fn parse_side(spec: &str, units: &DisplayUnits) -> Result<SideShape, String> {
    let (kind, rest) = spec.split_once(':').unwrap_or((spec, ""));
    let degrees = |name: &str, value: &str| number(name, value).map(f64::to_radians);
    let length = |name: &str, value: &str| -> Result<Param<f64>, String> {
        if value.eq_ignore_ascii_case("auto") {
            return Ok(Param::Auto);
        }
        parse_quantity_or(value, QuantityKind::Length, units.get(QuantityKind::Length))
            .map(Param::Manual)
            .map_err(|e| format!("{name}: {e}"))
    };
    let unknown = |name: &str, kind: &str, valid: &str| {
        Err(format!(
            "{kind} sections have no parameter {name:?}; valid: {valid}"
        ))
    };
    match kind {
        "rectangle" | "rectangular" => {
            let mut tangent_angle = 0.0;
            for (name, value) in spec_params(rest, "tangent") {
                match name {
                    "tangent" => tangent_angle = degrees(name, value)?,
                    _ => return unknown(name, kind, "tangent"),
                }
            }
            Ok(SideShape::Rectangular { tangent_angle })
        }
        "trapezoid" | "trapezoidal" => {
            let (mut cone_angle, mut tangent_angle) = (DEFAULT_CONE_ANGLE_DEG.to_radians(), 0.0);
            for (name, value) in spec_params(rest, "cone") {
                match name {
                    "cone" => cone_angle = degrees(name, value)?,
                    "tangent" => tangent_angle = degrees(name, value)?,
                    _ => return unknown(name, kind, "cone, tangent"),
                }
            }
            Ok(SideShape::Trapezoidal {
                cone_angle,
                tangent_angle,
            })
        }
        "round" | "circle" => {
            let mut ellipse_ratio = 1.0;
            for (name, value) in spec_params(rest, "ratio") {
                match name {
                    "ratio" => ellipse_ratio = number(name, value)?,
                    _ => return unknown(name, kind, "ratio"),
                }
            }
            Ok(SideShape::Round { ellipse_ratio })
        }
        "bezier" | "bezier-rectangle" | "bezier-trapezoid" => {
            let mut b = BezierSide {
                base: if kind == "bezier-trapezoid" {
                    BezierBase::Trapezoid
                } else {
                    BezierBase::Rectangle
                },
                ..BezierSide::default()
            };
            let params = spec_params(rest, "cone");
            let absolute = params.iter().any(|(name, _)| *name == "absolute");
            if absolute {
                b.position = InnerPosition::Absolute;
                b.point1 = 0.0;
                b.point3 = 0.0;
            }
            let inner = |name: &str, value: &str| -> Result<f64, String> {
                if absolute {
                    length(name, value)?
                        .manual_value()
                        .ok_or_else(|| format!("{name} needs a length"))
                } else {
                    number(name, value)
                }
            };
            for (name, value) in params {
                match name {
                    "cone" => b.cone_angle = degrees(name, value)?,
                    "tangent" => b.tangent_angle = degrees(name, value)?,
                    "p1" => b.point1 = inner(name, value)?,
                    "p3" => b.point3 = inner(name, value)?,
                    "p2" => {
                        let (u, v) = value
                            .split_once('/')
                            .ok_or_else(|| format!("p2 expects A/B, got {value:?}"))?;
                        b.point2 = [number(name, u)?, number(name, v)?];
                    }
                    "absolute" => {}
                    _ => return unknown(name, kind, "cone, tangent, p1, p3, p2, absolute"),
                }
            }
            Ok(SideShape::Bezier(b))
        }
        "segments" => {
            let mut points = tp_design::volute::section::default_segments();
            for (name, value) in spec_params(rest, "points") {
                match name {
                    "points" => {
                        points = value
                            .split('+')
                            .map(|p| {
                                let v: Vec<f64> = p
                                    .split('/')
                                    .map(|x| number("points", x))
                                    .collect::<Result<_, _>>()?;
                                match v[..] {
                                    [out, up] => Ok(SegmentPoint {
                                        out,
                                        up,
                                        radius: 0.0,
                                    }),
                                    [out, up, radius] => Ok(SegmentPoint { out, up, radius }),
                                    _ => Err(format!(
                                        "each corner is OUT/UP or OUT/UP/RADIUS over H, got {p:?}"
                                    )),
                                }
                            })
                            .collect::<Result<_, _>>()?;
                    }
                    _ => return unknown(name, kind, "points"),
                }
            }
            Ok(SideShape::LineSegments { points })
        }
        "radius" => {
            let mut r = RadiusSide::default();
            for (name, value) in spec_params(rest, "main") {
                match name {
                    "base" => r.base_height = length(name, value)?,
                    "base_radius" => r.base_radius = length(name, value)?,
                    "cone" => r.cone_angle = degrees(name, value)?,
                    "corner" => r.corner_radius = length(name, value)?,
                    "main" => r.main_radius = length(name, value)?,
                    _ => return unknown(name, kind, "base, base_radius, cone, corner, main"),
                }
            }
            Ok(SideShape::RadiusBased(r))
        }
        _ => Err(format!(
            "unknown cross section {kind:?}; valid: rectangle, trapezoid, round, bezier, \
             bezier-trapezoid, segments, radius, external"
        )),
    }
}

/// A whole cross section from `TYPE[:PARAMS]`: a symmetric side or `external`.
fn parse_section(spec: &str, units: &DisplayUnits) -> Result<SectionShape, String> {
    let (kind, rest) = spec.split_once(':').unwrap_or((spec, ""));
    if kind == "internal" {
        return parse_internal(rest, units).map(SectionShape::RoundInternal);
    }
    if kind != "external" {
        return parse_side(spec, units).map(SectionShape::symmetric);
    }
    let mut round = ExternalRound::default();
    for (name, value) in spec_params(rest, "ratio") {
        match name {
            "ratio" => round.ellipse_ratio = number(name, value)?,
            "right" => round.open_to_right = true,
            "square" => round.square_on_top = true,
            "below" => round.strictly_external = false,
            _ => {
                return Err(format!(
                    "external sections have no parameter {name:?}; valid: ratio, right, \
                     square, below"
                ));
            }
        }
    }
    Ok(SectionShape::RoundExternal(round))
}

/// The parameters of an internal section: the ellipse ratio, the side (+z unless `left`),
/// the neck width and the walls and ratios of the bend.
fn parse_internal(rest: &str, units: &DisplayUnits) -> Result<InternalRound, String> {
    let wall = |name: &str, value: &str| match value {
        "sharp" => Ok(BendWall::Sharp),
        "straight" => Ok(BendWall::Straight),
        "round" => Ok(BendWall::Round),
        _ => Err(format!(
            "{name} must be sharp, straight or round, got {value:?}"
        )),
    };
    let mut round = InternalRound::default();
    for (name, value) in spec_params(rest, "ratio") {
        match name {
            "ratio" => round.ellipse_ratio = number(name, value)?,
            "left" => round.open_to_right = false,
            "right" => round.open_to_right = true,
            "neck" | "gap" => {
                let length = if value.eq_ignore_ascii_case("auto") {
                    Param::Auto
                } else {
                    parse_quantity_or(value, QuantityKind::Length, units.get(QuantityKind::Length))
                        .map(Param::Manual)
                        .map_err(|e| format!("{name}: {e}"))?
                };
                if name == "neck" {
                    round.neck_width = length;
                } else {
                    round.side_gap = length;
                }
            }
            "inner" => round.inner_bend = wall(name, value)?,
            "outer" => round.outer_bend = wall(name, value)?,
            "bend" => round.bend_ratio = number(name, value)?,
            "area" => round.area_ratio = number(name, value)?,
            _ => {
                return Err(format!(
                    "internal sections have no parameter {name:?}; valid: ratio, left, right, \
                     neck, gap, inner, outer, bend, area"
                ));
            }
        }
    }
    Ok(round)
}

/// Applies `shape[@DEGREES][.right]=VALUE`: the cross section of the whole spiral, the
/// definition at one wrap angle, or its +z side.
fn apply_shape(
    input: &mut VoluteInput,
    units: &DisplayUnits,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let rest = &key["shape".len()..];
    let (place, right) = match rest.strip_suffix(".right") {
        Some(place) => (place, true),
        None => (rest, false),
    };
    let angle = match place.strip_prefix('@') {
        Some(degrees) => Some(number("the angle", degrees)?.to_radians()),
        None if place.is_empty() => None,
        None => {
            return Err(format!(
                "unknown key {key:?}; use shape, shape@DEGREES or .right"
            ));
        }
    };
    let sections = &mut input.cross_section.sections;
    let set_right = |shape: &mut SectionShape| -> Result<(), String> {
        let SectionShape::Sides { right: side, .. } = shape else {
            return Err("a round asymmetric section has no sides of its own".into());
        };
        *side = match value {
            "symmetric" => None,
            _ => Some(parse_side(value, units)?),
        };
        Ok(())
    };
    let Some(angle) = angle else {
        if right {
            for d in sections.iter_mut() {
                set_right(&mut d.shape)?;
            }
        } else {
            *sections = vec![SectionDefinition::at_throat(parse_section(value, units)?)];
        }
        return Ok(());
    };
    let existing = sections
        .iter()
        .position(|d| (d.angle - angle).abs() < 0.5f64.to_radians());
    match (value, existing) {
        ("remove", Some(i)) => {
            sections.remove(i);
        }
        ("off" | "on", Some(i)) => sections[i].active = value == "on",
        ("remove" | "off" | "on", None) => {
            return Err(format!(
                "no cross section is defined at {:.1}°",
                angle.to_degrees()
            ));
        }
        (_, Some(i)) if right => set_right(&mut sections[i].shape)?,
        (_, None) if right => {
            return Err(format!(
                "no cross section is defined at {:.1}°",
                angle.to_degrees()
            ));
        }
        (_, Some(i)) => sections[i].shape = parse_section(value, units)?,
        (_, None) => sections.push(SectionDefinition {
            angle,
            active: true,
            shape: parse_section(value, units)?,
        }),
    }
    Ok(())
}

/// Distance from the midplane where the bend of an internal volute ends and its neck
/// starts.
fn bend_end(bend: &tp_design::volute::section::Bend) -> f64 {
    bend.outer.last().map_or(0.0, |neck| neck[0].x.abs())
}

/// Description of a cross section with its main parameters.
fn describe_section(shape: &SectionShape, units: &DisplayUnits) -> String {
    let side = |s: &SideShape| -> String {
        let deg = |a: f64| show(units, QuantityKind::Angle, a);
        match s {
            SideShape::Rectangular { tangent_angle } if *tangent_angle > 0.0 => {
                format!("rectangular, top at {}", deg(*tangent_angle))
            }
            SideShape::Trapezoidal {
                cone_angle,
                tangent_angle,
            } => {
                let top = if *tangent_angle > 0.0 {
                    format!(", top at {}", deg(*tangent_angle))
                } else {
                    String::new()
                };
                format!("trapezoidal, flanks at {}{top}", deg(*cone_angle))
            }
            SideShape::Round { ellipse_ratio } if (*ellipse_ratio - 1.0).abs() > 1e-12 => {
                format!("round, ellipse ratio {ellipse_ratio}")
            }
            SideShape::Bezier(b) if b.base == BezierBase::Trapezoid => {
                format!("Bézier, trapezoid type at {}", deg(b.cone_angle))
            }
            SideShape::LineSegments { points } => {
                format!("line segments, {} corners", points.len())
            }
            SideShape::RadiusBased(r) => format!("radius based, cone at {}", deg(r.cone_angle)),
            other => other.name().to_owned(),
        }
    };
    match shape {
        SectionShape::Sides { left, right: None } => side(left),
        SectionShape::Sides {
            left,
            right: Some(right),
        } => format!("{} (−z) / {} (+z)", side(left), side(right)),
        SectionShape::RoundExternal(r) => format!(
            "round asymmetric toward {}z, ellipse ratio {}{}{}",
            if r.open_to_right { "+" } else { "−" },
            r.ellipse_ratio,
            if r.strictly_external {
                ""
            } else {
                ", also below r'z"
            },
            if r.square_on_top { ", square top" } else { "" }
        ),
        SectionShape::RoundInternal(r) => {
            let wall = |w: BendWall| match w {
                BendWall::Sharp => "sharp",
                BendWall::Straight => "straight",
                BendWall::Round => "round",
            };
            format!(
                "round asymmetric internal toward {}z, ellipse ratio {}, bend {} inside and {} \
                 outside",
                if r.open_to_right { "+" } else { "−" },
                r.ellipse_ratio,
                wall(r.inner_bend),
                wall(r.outer_bend)
            )
        }
    }
}

fn on_off(value: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("expected on or off, got {value:?}")),
    }
}

/// The bend of the diffuser along the pump axis: none, or EXTENT,RADIUS,ANGLE with two
/// lengths and the angle in degrees.
fn parse_z_bend(value: &str, units: &DisplayUnits) -> Result<Option<ZBend>, String> {
    if value == "none" || value == "off" {
        return Ok(None);
    }
    let bad = || format!("z_bend must be none or EXTENT,RADIUS,ANGLE (degrees), got {value:?}");
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    let [extent, radius, angle] = parts[..] else {
        return Err(bad());
    };
    let length = |text: &str| {
        parse_quantity_or(text, QuantityKind::Length, units.get(QuantityKind::Length))
            .map_err(|e| format!("z_bend: {e}"))
    };
    Ok(Some(ZBend {
        extent: length(extent)?,
        radius: length(radius)?,
        angle: angle.parse::<f64>().map_err(|_| bad())?.to_radians(),
    }))
}

/// How the diffuser sits along the pump axis: centric, or manual:OFFSET[,quadratic]
/// with a length for the shift of the outlet.
fn parse_z_alignment(value: &str, units: &DisplayUnits) -> Result<ZAlignment, String> {
    match value {
        "centric" => Ok(ZAlignment::Centric),
        _ => {
            let manual = value.strip_prefix("manual:").ok_or_else(|| {
                format!(
                    "z_align must be centric or manual:OFFSET[,quadratic], got \
                     {value:?}"
                )
            })?;
            let (offset, quadratic) = match manual.strip_suffix(",quadratic") {
                Some(offset) => (offset, true),
                None => (manual, false),
            };
            let offset = parse_quantity_or(
                offset.trim(),
                QuantityKind::Length,
                units.get(QuantityKind::Length),
            )
            .map_err(|e| format!("z_align: {e}"))?;
            Ok(ZAlignment::Manual { offset, quadratic })
        }
    }
}

fn parse_discharge(value: &str) -> Result<DischargeShape, String> {
    let wall = |w| DischargeShape::Tangential { straight_wall: w };
    match value {
        "tangential:inner" => Ok(wall(StraightWall::Inner)),
        "tangential" | "tangential:centered" => Ok(wall(StraightWall::Centered)),
        "tangential:outer" => Ok(wall(StraightWall::Outer)),
        "radial" => Ok(DischargeShape::Radial { axis_angle: 0.0 }),
        _ if value.starts_with("spline") => {
            let bad = || {
                format!(
                    "discharge spline must be spline:TURN[,START,END] with the turn in degrees \
                     and the two shares from 0.1 to 0.9, got {value:?}"
                )
            };
            let numbers: Vec<f64> = value
                .strip_prefix("spline:")
                .ok_or_else(bad)?
                .split(',')
                .map(|n| n.trim().parse::<f64>().map_err(|_| bad()))
                .collect::<Result<_, _>>()?;
            match numbers[..] {
                [turn] => Ok((turn, 0.5, 0.5)),
                [turn, start, end] => Ok((turn, start, end)),
                _ => Err(bad()),
            }
            .map(|(turn, start_share, end_share)| DischargeShape::Spline {
                turn: turn.to_radians(),
                start_share,
                end_share,
            })
        }
        _ => value
            .strip_prefix("radial:")
            .and_then(|d| d.trim().parse::<f64>().ok())
            .map(|degrees| DischargeShape::Radial {
                axis_angle: degrees.to_radians(),
            })
            .ok_or_else(|| {
                format!(
                    "discharge must be tangential[:inner|centered|outer], radial[:DEGREES] \
                     or spline:TURN[,START,END], got {value:?}"
                )
            }),
    }
}

/// A progression of a geometric rule: linear, or bezier:X1,Y1,X2,Y2 with the two inner
/// control points as fractions of the wrap and of the end value.
fn parse_progression(value: &str) -> Result<Progression, String> {
    if value == "linear" {
        return Ok(Progression::Linear);
    }
    let numbers: Option<Vec<f64>> = value
        .strip_prefix("bezier:")
        .map(|v| v.split(',').map(|n| n.trim().parse().ok()).collect())
        .and_then(|n: Option<Vec<f64>>| n.filter(|n| n.len() == 4));
    numbers
        .map(|n| Progression::Bezier {
            points: [[n[0], n[1]], [n[2], n[3]]],
        })
        .ok_or_else(|| format!("progression must be linear or bezier:X1,Y1,X2,Y2, got {value:?}"))
}

/// Applies one `KEY=VALUE` override to the volute inputs.
fn apply(input: &mut VoluteInput, units: &DisplayUnits, setting: &str) -> Result<(), String> {
    let (key, value) = setting
        .split_once('=')
        .ok_or_else(|| format!("--set expects KEY=VALUE, got {setting:?}"))?;
    let (key, value) = (key.trim(), value.trim());
    let auto = value.eq_ignore_ascii_case("auto");
    let quantity = |kind: QuantityKind| -> Result<Param<f64>, String> {
        if auto {
            return Ok(Param::Auto);
        }
        parse_quantity_or(value, kind, units.get(kind))
            .map(Param::Manual)
            .map_err(|e| format!("{key}: {e}"))
    };
    use QuantityKind::{Angle, Area, Dimensionless, Length, Pressure, Velocity, VolumeFlow};
    match key {
        "flow-factor" => input.setup.flow_factor = quantity(Dimensionless)?,
        "dz" => input.setup.cutwater_diameter = quantity(Length)?,
        "b3" => input.setup.inlet_width = quantity(Length)?,
        "e3" => input.setup.cutwater_thickness = quantity(Length)?,
        key if key.starts_with("shape") => apply_shape(input, units, key, value)?,
        "law" => {
            input.spiral.law = match value {
                "auto" => AreaLaw::Auto,
                "momentum" | "pfleiderer" => AreaLaw::AngularMomentum,
                "velocity" => AreaLaw::ConstantVelocity,
                "stepanoff" => AreaLaw::Stepanoff,
                "height" => AreaLaw::Height,
                "area" => AreaLaw::Area,
                "area_radius" => AreaLaw::AreaRadius,
                _ => {
                    return Err(format!(
                        "law must be auto, momentum, velocity, stepanoff, height, area or \
                         area_radius, got {value:?}"
                    ));
                }
            }
        }
        "wrap" => input.spiral.wrap = quantity(Angle)?,
        "x" => input.spiral.exponent = quantity(Dimensionless)?,
        "k3" => input.spiral.velocity_factor = quantity(Dimensionless)?,
        "cu4" => input.spiral.reference_velocity = quantity(Velocity)?,
        "qi" => input.spiral.reference_flow = quantity(VolumeFlow)?,
        "q0" => input.spiral.blind_flow = quantity(VolumeFlow)?,
        "end_height" => input.spiral.end_height = quantity(Length)?,
        "end_area" => input.spiral.end_area = quantity(Area)?,
        "end_area_radius" => input.spiral.end_area_radius = quantity(Length)?,
        "progression" => input.spiral.progression = parse_progression(value)?,
        "compensation" => input.spiral.compensation.enabled = on_off(value)?,
        "comp_e" => input.spiral.compensation.thickness = quantity(Length)?,
        "comp_start" => input.spiral.compensation.start = quantity(Angle)?,
        "arcs" => {
            let arcs = &mut input.spiral.arcs;
            if value == "off" {
                arcs.enabled = false;
            } else {
                arcs.segments = value
                    .parse()
                    .map_err(|_| format!("arcs must be off or a number of arcs, got {value:?}"))?;
                arcs.enabled = true;
            }
        }
        "sections" => {
            input.spiral.sections = if auto {
                Param::Auto
            } else {
                Param::Manual(
                    value
                        .parse()
                        .map_err(|_| format!("sections must be a whole number, got {value:?}"))?,
                )
            }
        }
        "correction" => {
            input.spiral.cutwater_correction = match value {
                "on" => true,
                "off" => false,
                _ => return Err(format!("correction must be on or off, got {value:?}")),
            }
        }
        "length" => input.diffuser.length = quantity(Length)?,
        "height" => input.diffuser.flange_height = quantity(Length)?,
        "dn" => input.diffuser.nominal_size = on_off(value)?,
        "flange" => input.wall.flange.enabled = on_off(value)?,
        "flange_d" => input.wall.flange.outer_diameter = quantity(Length)?,
        "flange_t" => input.wall.flange.thickness = quantity(Length)?,
        "bolt_circle" => input.wall.flange.bolt_circle = quantity(Length)?,
        "bolt_hole" => input.wall.flange.bolt_hole = quantity(Length)?,
        "bolts" => {
            input.wall.flange.bolts = if auto {
                Param::Auto
            } else {
                Param::Manual(
                    value
                        .parse()
                        .map_err(|_| format!("bolts must be a whole number, got {value:?}"))?,
                )
            }
        }
        "outlet" => input.diffuser.outlet_diameter = quantity(Length)?,
        "incidence" => input.cutwater.incidence = quantity(Angle)?,
        "obliquity" => input.cutwater.obliquity = quantity(Angle)?,
        "spiral_start" => input.cutwater.spiral_start = quantity(Angle)?,
        "wall" => input.wall.thickness = quantity(Length)?,
        "fillet" => input.wall.cutwater_fillet = quantity(Length)?,
        "pressure" => input.wall.pressure = quantity(Pressure)?,
        "allowable" => input.wall.allowable_stress = quantity(Pressure)?,
        "type" => {
            input.setup.kind = match value {
                "single" => VoluteKind::Single,
                "double" => VoluteKind::Double,
                _ => return Err(format!("type must be single or double, got {value:?}")),
            }
        }
        "rib_length" | "rib_wrap" => input.setup.rib_length = quantity(Angle)?,
        "rib" => input.setup.rib_thickness = quantity(Length)?,
        "rib_nose" => input.setup.rib_nose_ratio = quantity(Dimensionless)?,
        "channel_ratio" => input.setup.channel_area_ratio = quantity(Dimensionless)?,
        "rib_comp" => input.setup.rib_compensation = on_off(value)?,
        "rib_comp_start" => input.setup.rib_compensation_start = quantity(Angle)?,
        "discharge" => input.diffuser.shape = parse_discharge(value)?,
        "bend" => input.diffuser.bend_radius = quantity(Length)?,
        "height_offset" => input.diffuser.height_offset = quantity(Length)?,
        "base_form" => input.diffuser.base_form = quantity(Dimensionless)?,
        "eccentricity" => input.diffuser.eccentricity = quantity(Dimensionless)?,
        "z_bend" => input.diffuser.z_bend = parse_z_bend(value, units)?,
        "outlet_shape" => {
            input.diffuser.outlet_shape =
                match value {
                    "round" => OutletShape::Round,
                    _ => {
                        let aspect = value
                        .strip_prefix("rectangular:")
                        .or(if value == "rectangular" { Some("1") } else { None })
                        .and_then(|a| a.trim().parse::<f64>().ok())
                        .ok_or_else(|| {
                            format!(
                                "outlet_shape must be round or rectangular[:ASPECT], got {value:?}"
                            )
                        })?;
                        OutletShape::Rectangular { aspect }
                    }
                }
        }
        "z_align" => input.diffuser.z_alignment = parse_z_alignment(value, units)?,
        "growth" => {
            input.diffuser.growth = match value {
                "linear" => DiffuserGrowth::Linear,
                "matched" => DiffuserGrowth::Matched,
                _ => return Err(format!("growth must be linear or matched, got {value:?}")),
            }
        }
        "rotation" => input.diffuser.rotation = quantity(Angle)?,
        "cutwater" => {
            input.cutwater.shape = match value {
                "tangent" => CutwaterShape::Tangent,
                "sharp" => CutwaterShape::Sharp,
                // Rounded is the arc tangent to both walls; fillet is its former name.
                "rounded" | "fillet" => CutwaterShape::Fillet {
                    radius: input.cutwater.shape.radius().unwrap_or(Param::Auto),
                },
                "simple" => match input.cutwater.shape {
                    simple @ CutwaterShape::Simple { .. } => simple,
                    _ => CutwaterShape::Simple {
                        cut: Param::Auto,
                        handle: Param::Auto,
                        measure: CutMeasure::Along,
                    },
                },
                _ => {
                    return Err(format!(
                        "cutwater must be tangent, sharp, rounded or simple, got {value:?}"
                    ));
                }
            }
        }
        "nose" => {
            input.cutwater.shape = CutwaterShape::Fillet {
                radius: quantity(Length)?,
            }
        }
        // The cut along the walls, or across the gap they leave.
        "cut" | "gap" | "handle" => {
            let (mut cut, mut handle, mut measure) = match input.cutwater.shape {
                CutwaterShape::Simple {
                    cut,
                    handle,
                    measure,
                } => (cut, handle, measure),
                _ => (Param::Auto, Param::Auto, CutMeasure::Along),
            };
            match key {
                "cut" => (cut, measure) = (quantity(Length)?, CutMeasure::Along),
                "gap" => (cut, measure) = (quantity(Length)?, CutMeasure::Across),
                _ => handle = quantity(Dimensionless)?,
            }
            input.cutwater.shape = CutwaterShape::Simple {
                cut,
                handle,
                measure,
            };
        }
        _ => {
            return Err(format!(
                "unknown key {key:?}; valid keys: flow-factor, dz, b3, e3, shape, law, \
                 wrap, x, k3, cu4, qi, q0, end_height, end_area, end_area_radius, \
                 progression, compensation, comp_e, comp_start, arcs, \
                 sections, correction, length, outlet, incidence, obliquity, spiral_start, \
                 wall, \
                 fillet, type, \
                 rib_length, rib, \
                 rib_nose, channel_ratio, discharge, bend, growth, height_offset, z_align, \
                 base_form, eccentricity, outlet_shape, z_bend, rotation, cutwater, nose, cut, \
                 gap, handle, height, \
                 dn, flange, flange_d, flange_t, bolt_circle, bolt_hole, bolts, pressure, \
                 allowable"
            ));
        }
    }
    Ok(())
}

fn print_report(name: &str, id: ComponentId, g: &VoluteGeometry, units: &DisplayUnits) {
    use QuantityKind::*;
    let s = |kind, value| show(units, kind, value);
    let d = &g.design;
    let b = &g.basis;
    println!("{name} ({id}): volute");
    println!();
    let definitions = d.cross_sections.definitions();
    let shape = if let [only] = definitions {
        describe_section(&only.shape, units)
    } else {
        definitions
            .iter()
            .map(|def| {
                format!(
                    "{} at {}",
                    describe_section(&def.shape, units),
                    s(Angle, def.angle)
                )
            })
            .collect::<Vec<_>>()
            .join(", blending to ")
    };
    let dev = &d.development;
    let progression = match dev.progression {
        Progression::Linear => "linearly",
        Progression::Bezier { .. } => "along a Bézier curve",
    };
    let law = match d.law {
        AreaLaw::ConstantVelocity => format!(
            "constant velocity {} (Gülich eqs. 7.26–7.27)",
            s(Velocity, dev.velocity)
        ),
        AreaLaw::Stepanoff => match dev.velocity_factor {
            Some(k) => format!(
                "Stepanoff, c3 = K3·√(2gH) = {} with K3 = {k:.3} (eq. 7.1, Fig. 7.5)",
                s(Velocity, dev.velocity)
            ),
            None => format!("Stepanoff, c3 = {}", s(Velocity, dev.velocity)),
        },
        AreaLaw::Height => format!(
            "height growing {progression} to {}",
            s(Length, dev.end_value)
        ),
        AreaLaw::Area => format!("area growing {progression} to {}", s(Area, dev.end_value)),
        AreaLaw::AreaRadius => format!(
            "A/r_c growing {progression} to {}",
            s(Length, dev.end_value)
        ),
        #[allow(clippy::float_cmp)]
        _ if dev.exponent == 1.0 => {
            "conservation of angular momentum (Gülich eqs. 7.24–7.25)".to_owned()
        }
        _ => format!("Pfleiderer, c_u·r^x constant with x = {:.2}", dev.exponent),
    };
    println!(
        "Inlet      Q_Le {}   c2u {}   d2 {}",
        s(VolumeFlow, d.flow),
        s(Velocity, b.cu),
        s(Length, b.impeller_diameter)
    );
    println!(
        "Cutwater   dz {} (min {}, Table 10.2)   e3 {}   b3 {}",
        s(Length, d.cutwater_diameter),
        s(Length, d.min_cutwater_diameter),
        s(Length, d.cutwater_thickness),
        s(Length, d.inlet_width)
    );
    println!(
        "           flow angle α3' {}   cutwater angle α3B {}{}",
        s(Angle, d.cutwater_flow_angle),
        s(Angle, d.cutwater_angle),
        if d.cutwater_obliquity == 0.0 {
            String::new()
        } else {
            format!(
                "   obliquity ε {} (Table 10.2: over 35°)",
                s(Angle, d.cutwater_obliquity)
            )
        }
    );
    let c = &g.cutwater;
    match d.cutwater_shape {
        _ if d.development.start > 0.0 => println!(
            "           blunt cutwater: the casing on r'z from the throat to the start of the \
             spiral at {}",
            s(Angle, g.start_angle)
        ),
        CutwaterShape::Tangent => {
            println!("           tangent cutwater: the spiral starts where it clears the diffuser")
        }
        shape => println!(
            "           {} cutwater: tip at {} ({} from the axis), wedge {}{}   nearest to the axis {}",
            match shape {
                CutwaterShape::Sharp => "sharp",
                CutwaterShape::Simple { .. } => "simple",
                _ => "rounded",
            },
            s(Angle, c.angle),
            s(Length, c.tip.truncate().length()),
            s(Angle, c.wedge),
            if c.radius > 0.0 {
                format!(", nose radius {}", s(Length, c.radius))
            } else {
                String::new()
            },
            s(Length, c.nose_distance)
        ),
    }
    // The walls at the tip, from the circumferential direction.
    if !matches!(d.cutwater_shape, CutwaterShape::Tangent) && d.development.start <= 0.0 {
        println!(
            "           walls at the tip: spiral αIn {}   diffuser αOut {}   mean αAvg {} ({} off the flow angle)   spiral starts at {}",
            s(Angle, c.spiral_angle),
            s(Angle, c.diffuser_angle),
            s(Angle, c.mean_angle()),
            s(Angle, g.wall_misalignment()),
            s(Angle, g.start_angle)
        );
    }
    // Across the inlet width: where the walls cross.
    let slices = &g.cutwater_slices;
    if !slices.is_empty() {
        let range = |f: &dyn Fn(&tp_design::volute::CutwaterSlice) -> f64| {
            slices
                .iter()
                .map(f)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                    (lo.min(x), hi.max(x))
                })
        };
        let (tip_lo, tip_hi) = range(&|c| c.tip.truncate().length());
        let (wedge_lo, wedge_hi) = range(&|c| c.wedge);
        let (near, _) = range(&|c| c.nose_distance);
        println!(
            "           across the width ({} planes): tip {} to {} from the axis, wedge {} to {}, \
             nearest to the axis {}",
            slices.len(),
            s(Length, tip_lo),
            s(Length, tip_hi),
            s(Angle, wedge_lo),
            s(Angle, wedge_hi),
            s(Length, near)
        );
        if let Some(v) = g.cutwater_values() {
            println!(
                "           diameter lower / upper / mean {} / {} / {} (dMin {}), angle {} / {} / {}, \
                 position φC,1 {}",
                s(Length, v.diameter.lower),
                s(Length, v.diameter.upper),
                s(Length, v.diameter.mean),
                s(Length, v.least_diameter),
                s(Angle, v.angle.lower),
                s(Angle, v.angle.upper),
                s(Angle, v.angle.mean),
                s(Angle, v.position)
            );
            for (label, f) in [
                ("cutwater section", v.start),
                ("throat", v.throat),
                ("outlet", v.outlet),
            ] {
                println!(
                    "           {label}: equivalent diameter D {}, area A {}",
                    s(Length, f.equivalent_diameter),
                    s(Area, f.area)
                );
            }
        }
        if c.radius > 0.0 {
            let (thin, _) = range(&|c| c.min_thickness);
            let (join_lo, join_hi) = range(&|c| c.join_length);
            println!(
                "           body: nowhere thinner than {} (the nose {}); it runs into the walls \
                 {} to {} past the tip",
                s(Length, thin),
                s(Length, 2.0 * c.radius),
                s(Length, join_lo),
                s(Length, join_hi)
            );
        }
    }
    println!("Sections   {shape}; {law}");
    if let Some(bend) = &g.bend {
        println!(
            "Bend       neck {} high from {} above r'z, {} long; the spiral starts {} from \
             the midplane",
            s(Length, bend.top - bend.floor),
            s(Length, bend.floor),
            s(Length, (bend.face.abs() - bend_end(bend)).max(0.0)),
            s(Length, bend.face.abs()),
        );
    }
    if let Some(side) = &d.side_gap {
        let gap = match (side.reach, side.gap) {
            (Reach::Eye, _) => "the spiral reaches into the suction eye".to_owned(),
            (_, Some(gap)) => format!("{} to the impeller", s(Length, gap)),
            _ => "the spiral stays outside r2".to_owned(),
        };
        println!(
            "           side gap {} (at least {})",
            gap,
            s(Length, side.required)
        );
    }
    println!(
        "Spiral     wrap {} from {}   Q_i {}   Q0 {}   c_u4 {} at r4 {}",
        s(Angle, dev.wrap),
        s(Angle, dev.start),
        s(VolumeFlow, dev.reference_flow),
        s(VolumeFlow, dev.blind_flow),
        s(Velocity, dev.reference_velocity),
        s(Length, dev.reference_radius)
    );
    if let Some(c) = dev.compensation {
        println!(
            "           cutwater compensation: the inner radius rises {} from {} to the throat",
            s(Length, c.thickness),
            s(Angle, c.start)
        );
    }
    if let Some(arcs) = &dev.arcs {
        println!(
            "           {} circular arcs, at most {} from the spiral:",
            arcs.arcs.len(),
            s(Length, arcs.max_deviation)
        );
        for (k, arc) in arcs.arcs.iter().enumerate() {
            let (from, to) = arc.angles();
            println!(
                "             {}: radius {}, center {} from the axis at {}, {} to {}",
                k + 1,
                s(Length, arc.radius),
                s(Length, arc.center.length()),
                s(Angle, arc.center.y.atan2(arc.center.x).rem_euclid(TAU)),
                s(Angle, from),
                s(Angle, to)
            );
        }
    }
    println!(
        "Throat     area {}   height {}   outer radius {}   correction Δa3 {}",
        s(Area, d.throat.area),
        s(Length, d.throat.height),
        s(Length, d.throat.outer_radius),
        s(Length, d.throat_correction)
    );
    if let Ok(p) = d.end_section() {
        let height = p.outer_radius - p.inner_radius;
        println!(
            "End        r' {}   r {}   z min {}   H {}   B {}   H/B {:.3}   D {}   A {}   A/r_c {}",
            s(Length, p.inner_radius),
            s(Length, p.outer_radius),
            s(Length, p.axial_min),
            s(Length, height),
            s(Length, p.width),
            height / p.width,
            s(Length, (4.0 * p.area / PI).sqrt()),
            s(Area, p.area),
            s(Length, p.area / p.centroid_radius)
        );
    }
    if let Some(rib) = &d.rib {
        println!(
            "Double     inner volute wraps {} from the rib nose at {}   rib {} long {}{}",
            s(Angle, rib.wrap),
            s(Angle, rib.start),
            s(Length, rib.thickness),
            s(Angle, rib.length),
            if rib.is_short() {
                format!(" (ends at {}, before the throat)", s(Angle, rib.end()))
            } else {
                String::new()
            }
        );
        println!(
            "           first throat {} at the rib nose   outer channel {} → {}",
            s(Area, rib.outer_throat.area),
            s(Area, rib.channel_inlet_area),
            s(Area, rib.channel_outlet_area)
        );
        if let Some(start) = rib.compensation_start {
            println!(
                "           rib compensation from {} to the nose: the base rises by the rib",
                s(Angle, start)
            );
        }
        if let Some(r) = &g.rib {
            println!(
                "           {} nose and tail (length {}): tip at {}   nearest to the axis {}",
                if rib.nose_ratio > 1.0 {
                    format!("elliptic {:.2}:1", rib.nose_ratio)
                } else {
                    "semicircular".to_string()
                },
                s(Length, rib.nose_length()),
                s(Angle, r.nose_angle),
                s(Length, r.nose_distance)
            );
        }
    }
    println!(
        "           c3q {}   c3q/c2 {:.2} (not much below 0.8, Table 10.2)   B/H {:.2} (2 to 3)",
        s(Velocity, d.throat_velocity),
        d.deceleration,
        d.throat_aspect
    );
    let dif = &d.diffuser;
    let discharge = match dif.shape {
        DischargeShape::Tangential { straight_wall } => format!(
            "tangential, {} wall straight",
            match straight_wall {
                StraightWall::Inner => "inner",
                StraightWall::Centered => "no",
                StraightWall::Outer => "outer",
            }
        ),
        DischargeShape::Radial { axis_angle } => format!(
            "radial, bend R_N {} (Gülich eq. 7.29) turning {}, axis {} from the line through the pump axis",
            s(Length, dif.bend_radius),
            s(Angle, g.nozzle_bend),
            s(Angle, axis_angle)
        ),
        DischargeShape::Spline {
            turn,
            start_share,
            end_share,
        } => format!(
            "spline (Bézier of degree 4) turning {}, shares {start_share:.2} and {end_share:.2}",
            s(Angle, turn)
        ),
    };
    println!("Diffuser   {discharge}");
    println!(
        "           outlet face {} from the pump axis   cutwater at {} from the x axis",
        s(Length, g.outlet_height),
        s(Angle, g.rotation)
    );
    println!(
        "           length {} (centerline {})   outlet {}   area ratio {:.2}   c_d {}",
        s(Length, dif.length),
        s(Length, g.nozzle_length),
        s(Length, dif.outlet_diameter),
        dif.area_ratio,
        s(Velocity, dif.outlet_velocity)
    );
    println!(
        "           opening {} (allowed {}, Gülich eq. 1.44)   c_p,id {:.2}",
        s(Angle, dif.opening_angle),
        s(Angle, dif.allowable_angle),
        dif.ideal_pressure_recovery
    );
    // Inlet and outlet as equivalent circles, the velocity ratio, and where the outlet
    // stands: C from the pump axis to its axis, φ from the throat to its centre.
    let equivalent = |area: f64| (4.0 * area / std::f64::consts::PI).sqrt();
    println!(
        "           inlet D {} A {}   outlet D {} A {}   c_out/c_in {:.3}",
        s(Length, equivalent(dif.inlet_area)),
        s(Area, dif.inlet_area),
        s(Length, dif.outlet_diameter),
        s(
            Area,
            std::f64::consts::PI * dif.outlet_diameter.powi(2) / 4.0
        ),
        1.0 / dif.area_ratio
    );
    println!(
        "           outlet axis C {} from the pump axis   outlet centre φ {} from the throat",
        s(Length, g.outlet_offset()),
        s(Angle, g.outlet_angle())
    );
    println!(
        "Spiral     {} sections from {} to {}   wall {}{}",
        g.sections.len(),
        s(Angle, g.start_angle),
        s(Angle, d.rib.map_or(std::f64::consts::TAU, |r| r.start)),
        s(Length, d.wall_thickness),
        g.start_range.map_or(String::new(), |(lo, hi)| format!(
            "   (it may start from {} to {}: before the walls cross in every plane)",
            s(Angle, lo),
            s(Angle, hi)
        ))
    );
    if let (Some(_), Some(table)) = (&d.rib, d.free_double()) {
        // Sections the rib cuts: the inner volute and the outer channel are parts of the
        // whole section, sized together.
        let end = table.row_at(std::f64::consts::TAU);
        println!(
            "           whole sections cut by the rib: {} sections; at the throat the rib stands {} above r'z, the whole section {}",
            g.envelope.len(),
            s(Length, end.inner_height),
            s(Length, end.whole_height)
        );
    } else if d.rib.is_some() {
        println!(
            "           inner volute {} sections, outer channel {} sections",
            g.inner_sections.len(),
            g.outer_channel.len()
        );
    }
    let w = &g.wall_stress;
    let place = match w.place {
        StressPlace::Spiral { angle } => format!("the section at {}", s(Angle, angle)),
        StressPlace::Bend => "the bend".to_owned(),
        StressPlace::Straight => "the straight diffuser".to_owned(),
    };
    println!(
        "Stress     σ1 {} round {place}, σ2 {} along it, at {} over the suction (shut-off, Gülich eqs. 3.26 and 3.28)",
        s(Pressure, w.hoop),
        s(Pressure, w.longitudinal),
        s(Pressure, w.pressure)
    );
    println!(
        "           thin torus of the same area (our estimate); allowable {} needs a wall of {}",
        s(Pressure, w.allowable),
        s(Length, w.required_thickness)
    );
    if let Some(f) = &d.flange {
        println!(
            "Flange     {} × {}   {} bolt holes of {} on {}   (our proportions, not a standard)",
            s(Length, f.outer_diameter),
            s(Length, f.thickness),
            f.bolts,
            s(Length, f.bolt_hole),
            s(Length, f.bolt_circle)
        );
    }
    if !g.warnings.is_empty() {
        println!();
        println!("Warnings");
        for w in &g.warnings {
            println!("  warning: {w}");
        }
    }
    println!();
}
