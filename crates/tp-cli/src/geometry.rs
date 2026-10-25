//! `turbopump geometry`: meridional contour, meridional flow and blades of the
//! impellers of a pump project.

use std::path::{Path, PathBuf};

use clap::Args;
use tp_design::Param;
use tp_design::impeller::ImpellerGeometryInput;
use tp_design::impeller::blade::{
    BetaDistribution, BladeShape, EdgeShape, InletVelocity, LoadingShape, MeridionalFlow, WrapMode,
};
use tp_design::impeller::{BladeLoading, ImpellerGeometry};
use tp_io::dwg::{self, DwgVersion};
use tp_math::DVec2;
use tp_project::{ComponentId, ComponentKind, Project};
use tp_units::{DisplayUnits, QuantityKind, parse_quantity_or};

use crate::design::err;
use crate::{CliResult, Outcome, show};

#[derive(Args)]
pub(crate) struct GeometryArgs {
    /// Project file (.tpj). Inputs and step status are saved back into it.
    file: PathBuf,
    /// Only this component id (default: every impeller).
    #[arg(long)]
    component: Option<u32>,
    /// Fix an input as KEY=VALUE, or KEY=auto. Keys: axial-length, hub-in, hub-out,
    /// shroud-in, shroud-out (Bézier tangent fractions), le-hub, le-shroud (leading-edge
    /// position as a fraction of the contour), le-points (Z,R;Z,R;… from hub to shroud:
    /// the leading edge through those points, its ends taken to the walls), spans, flow
    /// (potential or quasi-1d: the meridional flow the sections are built on),
    /// inlet-velocity (local, or uniform: c1m = Q_La/A1 at every leading edge), wrap
    /// (auto or an angle: the wrap every section shares; free: each its own; linear, or
    /// linear:HUB,SHROUD: from the hub wrap to the shroud wrap), shape (free-form,
    /// cylindrical, circular-arc or straight), points, beta (linear, or smooth:IN,OUT),
    /// le and te (edge shapes: cut, round, ellipse:RATIO, taper[:LENGTH,NOSE,RATIO[,ASYM]],
    /// bezier[:START,PULL] or bezier:START_P,START_S,PULL_P,PULL_S,TIP, lengths in blade
    /// thicknesses; at the hub, or on every span), le-shroud-shape and te-shroud-shape
    /// (the shape at the shroud, blended linearly from the hub; uniform: none),
    /// e-le, e-te (thickness), and for each side (front or rear) the seal and its wear
    /// rings: front-seal and rear-seal (on or off), front-seal-d, front-seal-length,
    /// front-seal-s (a length or auto), front-impeller-ring and front-casing-ring (on or
    /// off), front-impeller-ring-e and front-casing-ring-e (radial thickness).
    #[arg(long = "set", value_name = "KEY=VALUE")]
    settings: Vec<String>,
    /// Write impeller.txt (points), impeller.stl (mesh), and the meridional and front
    /// drawings as SVG, DXF and DWG into this folder.
    #[arg(long, value_name = "DIR")]
    export: Option<PathBuf>,
    /// DWG format of the exported drawings: 2000, 2004, 2007, 2010, 2013 or 2018.
    #[arg(
        long,
        value_name = "VERSION",
        default_value = "2018",
        requires = "export"
    )]
    dwg_version: DwgVersion,
    /// Compute and print without saving the project.
    #[arg(long)]
    dry_run: bool,
}

pub(crate) fn run(args: GeometryArgs) -> CliResult {
    let mut project =
        Project::load(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let units = project.display_units.clone();
    let impellers: Vec<(ComponentId, String)> = project
        .components()
        .iter()
        .filter(|c| c.kind() == ComponentKind::Impeller)
        .filter(|c| args.component.is_none_or(|n| c.id().0 == n))
        .map(|c| (c.id(), c.name.clone()))
        .collect();
    if impellers.is_empty() {
        return Err(match args.component {
            Some(n) => format!("no impeller with id #{n}"),
            None => "the project has no impeller".into(),
        });
    }

    let mut warned = false;
    for (id, name) in &impellers {
        let mut input = project.impeller_geometry_input(*id).map_err(err)?;
        // The leading-edge points need the walls the other settings give.
        let (points, others): (Vec<&String>, Vec<&String>) = args.settings.iter().partition(|s| {
            s.split_once('=')
                .is_some_and(|(k, _)| k.trim() == "le-points")
        });
        for setting in others {
            apply(&mut input, &units, setting)?;
        }
        project
            .set_impeller_geometry(*id, input.clone())
            .map_err(err)?;
        if let Some(setting) = points.last() {
            let (_, value) = setting.split_once('=').expect("partitioned on '='");
            let points = parse_points(value, &units)?;
            let contour = project.compute_impeller_geometry(*id).map_err(err)?.contour;
            let off = input
                .meridional
                .set_leading_edge_points(&contour, &points)
                .map_err(|e| format!("le-points: {e}"))?;
            if off > 1e-4 {
                println!(
                    "note: the leading-edge ends were {} from the walls and were moved onto them",
                    show(&units, QuantityKind::Length, off)
                );
            }
            project
                .set_impeller_geometry(*id, input.clone())
                .map_err(err)?;
        }
        let geometry = project.design_impeller_geometry(*id).map_err(err)?;
        print_report(name, *id, &geometry, &input, &units);
        warned |= !geometry.warnings.is_empty();

        if let Some(dir) = &args.export {
            let dir = if impellers.len() > 1 {
                dir.join(format!("impeller-{}", id.0))
            } else {
                dir.clone()
            };
            export(&dir, &geometry, args.dwg_version)?;
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

fn export(dir: &Path, g: &ImpellerGeometry, dwg_version: DwgVersion) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("impeller.txt".into(), tp_io::points::geometry_txt(g).into()),
        (
            "impeller.stl".into(),
            tp_io::stl::ascii("impeller", &tp_io::mesh::impeller_triangles(g)).into(),
        ),
    ];
    for (name, drawing) in [
        ("meridional", tp_io::drawing::meridional(g)),
        ("front", tp_io::drawing::front_view(g)),
    ] {
        files.push((format!("{name}.svg"), tp_io::svg::write(&drawing).into()));
        files.push((format!("{name}.dxf"), tp_io::dxf::write(&drawing).into()));
        let dwg = dwg::write(&drawing, dwg_version).map_err(|e| format!("{name}.dwg: {e}"))?;
        files.push((format!("{name}.dwg"), dwg));
    }
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("Wrote {}", path.display());
    }
    Ok(())
}

/// An edge shape: cut, round, ellipse:RATIO, taper[:LENGTH,NOSE,RATIO[,ASYMMETRY]] or
/// bezier[:START,PULL] (symmetric) or bezier:START_P,START_S,PULL_P,PULL_S,TIP, lengths
/// in blade thicknesses.
pub(crate) fn parse_edge(value: &str) -> Result<EdgeShape, String> {
    let bad = || {
        format!(
            "edge must be cut, round, ellipse:RATIO, taper[:LENGTH,NOSE,RATIO[,ASYMMETRY]] or \
             bezier[:START,PULL | :START_P,START_S,PULL_P,PULL_S,TIP], got {value:?}"
        )
    };
    let (kind, numbers) = value.split_once(':').unwrap_or((value, ""));
    let numbers: Vec<f64> = numbers
        .split(',')
        .filter(|n| !n.trim().is_empty())
        .map(|n| n.trim().parse().map_err(|_| bad()))
        .collect::<Result<_, _>>()?;
    let shape = match (kind.trim(), numbers.as_slice()) {
        ("cut", []) => EdgeShape::Cut,
        ("round", []) => EdgeShape::Ellipse { ratio: 1.0 },
        ("ellipse", &[ratio]) => EdgeShape::Ellipse { ratio },
        ("taper", []) => EdgeShape::TAPER,
        ("taper", &[length, nose, ratio]) => EdgeShape::Taper {
            length,
            nose,
            ratio,
            asymmetry: 0.0,
        },
        ("taper", &[length, nose, ratio, asymmetry]) => EdgeShape::Taper {
            length,
            nose,
            ratio,
            asymmetry,
        },
        ("bezier", []) => EdgeShape::BEZIER,
        ("bezier", &[start, pull]) => EdgeShape::Bezier {
            start: [start; 2],
            pull: [pull; 2],
            tip: 0.0,
            symmetric: true,
        },
        ("bezier", &[sp, ss, pp, ps, tip]) => EdgeShape::Bezier {
            start: [sp, ss],
            pull: [pp, ps],
            tip,
            symmetric: false,
        },
        _ => return Err(bad()),
    };
    shape.validate()?;
    Ok(shape)
}

/// A shroud edge shape, or `uniform` for the hub shape on every span.
fn parse_shroud_edge(value: &str) -> Result<Option<EdgeShape>, String> {
    if value.trim() == "uniform" {
        Ok(None)
    } else {
        parse_edge(value).map(Some)
    }
}

/// How an edge shape reads in the summary.
fn edge_text(shape: EdgeShape) -> String {
    match shape.resolved() {
        EdgeShape::Cut => "blunt".into(),
        EdgeShape::Ellipse { ratio } => format!("ellipse {ratio}:1"),
        EdgeShape::Taper {
            length,
            nose,
            ratio,
            asymmetry,
        } => {
            format!("taper over {length} e to a {nose} e nose of {ratio}:1, asymmetry {asymmetry}")
        }
        EdgeShape::Bezier {
            start, pull, tip, ..
        } => format!(
            "Bézier, {} e / {} e long, pull {} / {}, tip {tip}",
            start[0], start[1], pull[0], pull[1]
        ),
    }
}

/// Points `Z,R;Z,R;…` (bare numbers in the display length unit) as (z, r) in m.
fn parse_points(value: &str, units: &DisplayUnits) -> Result<Vec<DVec2>, String> {
    let unit = units.get(QuantityKind::Length);
    value
        .split(';')
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let (z, r) = p
                .split_once(',')
                .ok_or_else(|| format!("le-points: {p:?} is not Z,R"))?;
            let read = |text: &str| {
                parse_quantity_or(text.trim(), QuantityKind::Length, unit)
                    .map_err(|e| format!("le-points: {e}"))
            };
            Ok(DVec2::new(read(z)?, read(r)?))
        })
        .collect()
}

fn parse_beta(value: &str) -> Result<BetaDistribution, String> {
    if value == "linear" {
        return Ok(BetaDistribution::Linear);
    }
    let bad = || {
        format!(
            "beta must be linear, smooth:IN,OUT or loading[:L,F,A,S/L,F,A,S] (hub/shroud \
             leading-edge loading, end of the front part, start of the rear part, middle \
             slope), got {value:?}"
        )
    };
    if let Some(rest) = value.strip_prefix("loading") {
        if rest.is_empty() {
            return Ok(BetaDistribution::loading());
        }
        let shape = |text: &str| -> Result<LoadingShape, String> {
            let v: Vec<f64> = text
                .split(',')
                .map(|x| x.trim().parse().map_err(|_| bad()))
                .collect::<Result<_, _>>()?;
            let [leading, fore, aft, slope] = v[..] else {
                return Err(bad());
            };
            Ok(LoadingShape {
                leading,
                fore,
                aft,
                slope,
            })
        };
        let (hub, shroud) = rest
            .strip_prefix(':')
            .and_then(|r| r.split_once('/'))
            .ok_or_else(bad)?;
        return Ok(BetaDistribution::Loading {
            hub: shape(hub)?,
            shroud: shape(shroud)?,
        });
    }
    let rest = value.strip_prefix("smooth").ok_or_else(bad)?;
    if rest.is_empty() {
        return Ok(BetaDistribution::Smooth {
            inlet: 0.3,
            outlet: 0.3,
        });
    }
    let (a, b) = rest
        .strip_prefix(':')
        .and_then(|r| r.split_once(','))
        .ok_or_else(bad)?;
    Ok(BetaDistribution::Smooth {
        inlet: a.trim().parse().map_err(|_| bad())?,
        outlet: b.trim().parse().map_err(|_| bad())?,
    })
}

/// Applies one `KEY=VALUE` override to the geometry inputs.
fn apply(
    input: &mut ImpellerGeometryInput,
    units: &DisplayUnits,
    setting: &str,
) -> Result<(), String> {
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
    let count = || -> Result<Param<u32>, String> {
        if auto {
            Ok(Param::Auto)
        } else {
            value
                .parse()
                .map(Param::Manual)
                .map_err(|_| format!("{key} must be a whole number, got {value:?}"))
        }
    };
    use QuantityKind::{Dimensionless, Length};
    let m = &mut input.meridional;
    match key {
        "axial-length" => m.axial_length = quantity(Length)?,
        "hub-in" => m.hub_inlet_tangent = quantity(Dimensionless)?,
        "hub-out" => m.hub_outlet_tangent = quantity(Dimensionless)?,
        "shroud-in" => m.shroud_inlet_tangent = quantity(Dimensionless)?,
        "shroud-out" => m.shroud_outlet_tangent = quantity(Dimensionless)?,
        "le-hub" => m.leading_edge_hub = quantity(Dimensionless)?,
        "le-shroud" => m.leading_edge_shroud = quantity(Dimensionless)?,
        "spans" => input.properties.spans = count()?,
        "wrap" => match value {
            "free" => input.mean_lines.wrap = WrapMode::Free,
            _ if value == "linear" || value.starts_with("linear:") => {
                input.mean_lines.wrap = WrapMode::Linear;
                let (hub, shroud) = match value.strip_prefix("linear:") {
                    None => (Param::Auto, Param::Auto),
                    Some(ends) => {
                        let (hub, shroud) = ends.split_once(',').ok_or_else(|| {
                            format!("wrap: linear:HUB,SHROUD needs two angles, got {value:?}")
                        })?;
                        let angle = |text: &str| {
                            parse_quantity_or(
                                text.trim(),
                                QuantityKind::Angle,
                                units.get(QuantityKind::Angle),
                            )
                            .map(Param::Manual)
                            .map_err(|e| format!("wrap: {e}"))
                        };
                        (angle(hub)?, angle(shroud)?)
                    }
                };
                input.mean_lines.wrap_angle = hub;
                input.mean_lines.wrap_angle_shroud = shroud;
            }
            _ => {
                input.mean_lines.wrap = WrapMode::Common;
                input.mean_lines.wrap_angle = quantity(QuantityKind::Angle)?;
            }
        },
        "flow" => {
            input.properties.meridional_flow = match value {
                "potential" | "auto" => MeridionalFlow::Potential,
                "quasi-1d" => MeridionalFlow::QuasiOneDimensional,
                _ => return Err(format!("flow must be potential or quasi-1d, got {value:?}")),
            }
        }
        "inlet-velocity" => {
            input.properties.inlet_velocity = match value {
                "local" | "auto" => InletVelocity::Local,
                "uniform" => InletVelocity::Uniform,
                _ => {
                    return Err(format!(
                        "inlet-velocity must be local or uniform, got {value:?}"
                    ));
                }
            }
        }
        "shape" => {
            input.properties.shape = match value {
                "free-form" | "auto" => BladeShape::FreeForm,
                "cylindrical" => BladeShape::Cylindrical,
                "circular-arc" => BladeShape::CircularArc,
                "straight" => BladeShape::Straight,
                _ => {
                    return Err(format!(
                        "shape must be free-form, cylindrical, circular-arc or straight, got \
                         {value:?}"
                    ));
                }
            }
        }
        "points" => input.mean_lines.points = count()?,
        "beta" => input.mean_lines.beta = parse_beta(value)?,
        "e-le" => input.profiles.thickness_leading = quantity(Length)?,
        "e-te" => input.profiles.thickness_trailing = quantity(Length)?,
        "le" => input.edges.leading = parse_edge(value)?,
        "te" => input.edges.trailing = parse_edge(value)?,
        "le-shroud-shape" => input.edges.leading_shroud = parse_shroud_edge(value)?,
        "te-shroud-shape" => input.edges.trailing_shroud = parse_shroud_edge(value)?,
        _ if seal_key(key).is_some() => apply_seal(&mut m.seals, key, value, &quantity)?,
        _ => {
            return Err(format!(
                "unknown key {key:?}; valid keys: front-seal, rear-seal (and -d, -length, -s), \
                 front-impeller-ring, front-casing-ring, rear-impeller-ring, rear-casing-ring \
                 (and -e), axial-length, hub-in, hub-out, shroud-in, \
                 shroud-out, le-hub, le-shroud, le-points, spans, flow, inlet-velocity, wrap, shape, points, beta, e-le, e-te, le, te, le-shroud-shape, te-shroud-shape"
            ));
        }
    }
    Ok(())
}

/// The side and the setting of a seal key such as `front-seal-d`, or `None` if `key` is
/// not one.
fn seal_key(key: &str) -> Option<(bool, &str)> {
    let (front, rest) = if let Some(rest) = key.strip_prefix("front-") {
        (true, rest)
    } else {
        (false, key.strip_prefix("rear-")?)
    };
    const SETTINGS: [&str; 8] = [
        "seal",
        "seal-d",
        "seal-length",
        "seal-s",
        "impeller-ring",
        "impeller-ring-e",
        "casing-ring",
        "casing-ring-e",
    ];
    SETTINGS.contains(&rest).then_some((front, rest))
}

/// Applies a seal or wear-ring key (`front-seal-d`, `rear-casing-ring`, …).
fn apply_seal(
    seals: &mut tp_design::impeller::seals::Seals,
    key: &str,
    value: &str,
    quantity: &dyn Fn(QuantityKind) -> Result<Param<f64>, String>,
) -> Result<(), String> {
    let (front, setting) = seal_key(key).ok_or_else(|| format!("unknown key {key:?}"))?;
    let seal = if front {
        &mut seals.front
    } else {
        &mut seals.rear
    };
    let on = || match value.to_ascii_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("{key} must be on or off, got {value:?}")),
    };
    match setting {
        "seal" => seal.enabled = on()?,
        "seal-d" => seal.diameter = quantity(QuantityKind::Length)?,
        "seal-length" => seal.length = quantity(QuantityKind::Length)?,
        "seal-s" => seal.clearance = quantity(QuantityKind::Length)?,
        "impeller-ring" => seal.impeller_ring.enabled = on()?,
        "impeller-ring-e" => seal.impeller_ring.thickness = quantity(QuantityKind::Length)?,
        "casing-ring" => seal.casing_ring.enabled = on()?,
        _ => seal.casing_ring.thickness = quantity(QuantityKind::Length)?,
    }
    Ok(())
}

fn print_report(
    name: &str,
    id: ComponentId,
    g: &ImpellerGeometry,
    input: &ImpellerGeometryInput,
    units: &DisplayUnits,
) {
    use QuantityKind::*;
    let s = |kind, value| show(units, kind, value);
    println!("{name} ({id}): geometry");
    println!();
    let qos = &g.channel.quasi_orthogonals;
    let (inlet, outlet) = (&qos[0], &qos[qos.len() - 1]);
    println!(
        "Meridional  L {}   hub {}   shroud {}",
        s(Length, g.contour.axial_length),
        s(Length, g.contour.hub_length()),
        s(Length, g.contour.shroud_length())
    );
    for (label, seal) in [
        ("Front seal", &g.contour.seals.front),
        ("Rear seal ", &g.contour.seals.rear),
    ] {
        let Some(seal) = seal else { continue };
        let ring = |e: Option<f64>| e.map_or("none".to_string(), |e| s(Length, e));
        println!(
            "{label}  d {}   L {}   s {}   impeller ring {}   casing ring {}",
            s(Length, 2.0 * seal.radius),
            s(Length, seal.length),
            s(Length, seal.clearance),
            ring(seal.impeller_ring),
            ring(seal.casing_ring)
        );
    }
    println!(
        "Flow        cm {} → {}   area {} → {}   max local rise of cm {:.1} %",
        s(Velocity, inlet.cm),
        s(Velocity, outlet.cm),
        s(Area, inlet.area),
        s(Area, outlet.area),
        g.channel.max_cm_rise() * 100.0
    );
    println!();
    println!(
        "Blade sections ({} blades, β2B {})",
        g.blade.count,
        s(Angle, g.blade.outlet_blade_angle)
    );
    println!(
        "  {:>5} {:>11} {:>11} {:>9} {:>9} {:>9} {:>11}",
        "span", "r_LE", "cm_LE", "β1", "β1B", "wrap", "length"
    );
    for section in &g.blade.sections {
        let le = &section.leading_edge;
        println!(
            "  {:>5.2} {:>11} {:>11} {:>9} {:>9} {:>9} {:>11}",
            section.span,
            s(Length, le.position.y),
            s(Velocity, le.cm),
            s(Angle, le.flow_angle),
            s(Angle, le.blade_angle),
            s(Angle, section.wrap_angle()),
            s(Length, section.mean_line_length())
        );
    }
    let loading = &g.blade_loading;
    println!();
    println!(
        "Blade loading  ξ_eff {:.3} / ξ_al {:.3} = {:.2}  (band {:.2}–{:.2}, Gülich eqs. 7.1.10–11)",
        loading.effective,
        loading.allowable,
        loading.ratio(),
        1.0 - BladeLoading::BAND,
        1.0 + BladeLoading::BAND
    );
    let edges = [
        ("hub", Some(input.edges.leading)),
        ("shroud", input.edges.leading_shroud),
    ];
    for (span, shape) in edges {
        let Some(shape) = shape else { continue };
        let at = if input.edges.leading_shroud.is_some() {
            format!(" at the {span}")
        } else {
            String::new()
        };
        match shape.min_pressure_coefficient() {
            Some(cp) => println!(
                "Leading edge   {}{at}, c_p,min at shockless entry {cp:.2} (Gülich eq. 7.1a)",
                edge_text(shape)
            ),
            None => println!("Leading edge   blunt{at}"),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn set(input: &mut ImpellerGeometryInput, setting: &str) -> Result<(), String> {
        apply(input, &DisplayUnits::si(), setting)
    }

    #[test]
    fn front_seal_keys_set_the_seal() {
        let mut input = ImpellerGeometryInput::default();
        for s in [
            "front-seal-d=120 mm",
            "front-seal-length=15 mm",
            "front-seal-s=auto",
            "front-impeller-ring-e=5 mm",
            "rear-casing-ring-e=6 mm",
        ] {
            set(&mut input, s).unwrap();
        }
        let seals = &input.meridional.seals;
        assert_eq!(seals.front.diameter, Param::Manual(0.12));
        assert_eq!(seals.front.length, Param::Manual(0.015));
        assert!(seals.front.clearance.is_auto());
        assert_eq!(seals.front.impeller_ring.thickness, Param::Manual(0.005));
        assert_eq!(seals.rear.casing_ring.thickness, Param::Manual(0.006));
        assert!(seals.rear.diameter.is_auto());
    }

    #[test]
    fn ring_switches_accept_on_and_off() {
        let mut input = ImpellerGeometryInput::default();
        for s in [
            "front-seal=OFF",
            "rear-seal=Off",
            "front-impeller-ring=off",
            "rear-casing-ring=off",
        ] {
            set(&mut input, s).unwrap();
        }
        let seals = &input.meridional.seals;
        assert!(!seals.front.enabled && !seals.rear.enabled);
        assert!(!seals.front.impeller_ring.enabled && seals.front.casing_ring.enabled);
        assert!(!seals.rear.casing_ring.enabled && seals.rear.impeller_ring.enabled);
        set(&mut input, "front-seal=on").unwrap();
        assert!(input.meridional.seals.front.enabled);
    }

    #[test]
    fn an_unknown_switch_value_is_an_error() {
        let mut input = ImpellerGeometryInput::default();
        let e = set(&mut input, "front-seal=x").unwrap_err();
        assert_eq!(e, "front-seal must be on or off, got \"x\"");
        assert!(set(&mut input, "front-seal-q=1").is_err());
    }
}
