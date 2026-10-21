//! `turbopump geometry`: meridional contour, meridional flow and blades of the
//! impellers of a pump project.

use std::path::{Path, PathBuf};

use clap::Args;
use tp_design::Param;
use tp_design::impeller::ImpellerGeometryInput;
use tp_design::impeller::blade::{BetaDistribution, EdgeShape};
use tp_design::impeller::{BladeLoading, ImpellerGeometry};
use tp_io::dwg::{self, DwgVersion};
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
    /// position as a fraction of the contour), spans, points, beta (linear, or
    /// smooth:IN,OUT), le and te (cut, round or ellipse:RATIO), e-le, e-te (thickness).
    #[arg(long = "set", value_name = "KEY=VALUE")]
    settings: Vec<String>,
    /// Write impeller.txt (points), impeller.stl (mesh), and the meridional and front
    /// drawings as SVG and DXF into this folder.
    #[arg(long, value_name = "DIR")]
    export: Option<PathBuf>,
    /// Also write the drawings as DWG (2000, 2004, 2007, 2010, 2013 or 2018; default
    /// 2018), with the free ODA File Converter, found where it installs or through
    /// TURBOPUMP_ODAFC.
    #[arg(
        long,
        value_name = "VERSION",
        num_args = 0..=1,
        default_missing_value = "2018",
        requires = "export"
    )]
    dwg: Option<DwgVersion>,
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

    // Look for the converter before computing anything, so a missing one fails fast.
    let to_dwg = match args.dwg {
        Some(version) => {
            let converter = dwg::find_converter().ok_or(dwg::DwgError::NotInstalled);
            Some((version, converter.map_err(|e| e.to_string())?))
        }
        None => None,
    };

    let mut warned = false;
    for (id, name) in &impellers {
        let mut input = project.impeller_geometry_input(*id).map_err(err)?;
        for setting in &args.settings {
            apply(&mut input, &units, setting)?;
        }
        project
            .set_impeller_geometry(*id, input.clone())
            .map_err(err)?;
        let geometry = project.design_impeller_geometry(*id).map_err(err)?;
        print_report(name, *id, &geometry, &input, &units);
        warned |= !geometry.warnings.is_empty();

        if let Some(dir) = &args.export {
            let dir = if impellers.len() > 1 {
                dir.join(format!("impeller-{}", id.0))
            } else {
                dir.clone()
            };
            export(&dir, &geometry, to_dwg.as_ref())?;
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

fn export(
    dir: &Path,
    g: &ImpellerGeometry,
    to_dwg: Option<&(DwgVersion, PathBuf)>,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let meridional = tp_io::drawing::meridional(g);
    let front = tp_io::drawing::front_view(g);
    let drawings = [
        ("meridional", tp_io::dxf::write(&meridional)),
        ("front", tp_io::dxf::write(&front)),
    ];
    let mut files = vec![
        ("impeller.txt".to_owned(), tp_io::points::geometry_txt(g)),
        (
            "impeller.stl".to_owned(),
            tp_io::stl::ascii("impeller", &tp_io::mesh::impeller_triangles(g)),
        ),
        ("meridional.svg".to_owned(), tp_io::svg::write(&meridional)),
        ("front.svg".to_owned(), tp_io::svg::write(&front)),
    ];
    files.extend(
        drawings
            .iter()
            .map(|(name, dxf)| (format!("{name}.dxf"), dxf.clone())),
    );
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("Wrote {}", path.display());
    }
    if let Some((version, converter)) = to_dwg {
        for (name, dxf) in &drawings {
            let path = dir.join(format!("{name}.dwg"));
            dwg::write(converter, dxf, &path, *version)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            println!("Wrote {} (DWG {version})", path.display());
        }
    }
    Ok(())
}

pub(crate) fn parse_edge(value: &str) -> Result<EdgeShape, String> {
    match value {
        "cut" => Ok(EdgeShape::Cut),
        "round" => Ok(EdgeShape::Ellipse { ratio: 1.0 }),
        _ => value
            .strip_prefix("ellipse:")
            .and_then(|r| r.parse().ok())
            .map(|ratio| EdgeShape::Ellipse { ratio })
            .ok_or_else(|| format!("edge must be cut, round or ellipse:RATIO, got {value:?}")),
    }
}

fn parse_beta(value: &str) -> Result<BetaDistribution, String> {
    if value == "linear" {
        return Ok(BetaDistribution::Linear);
    }
    let bad = || format!("beta must be linear or smooth:IN,OUT, got {value:?}");
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
        "points" => input.mean_lines.points = count()?,
        "beta" => input.mean_lines.beta = parse_beta(value)?,
        "e-le" => input.profiles.thickness_leading = quantity(Length)?,
        "e-te" => input.profiles.thickness_trailing = quantity(Length)?,
        "le" => input.edges.leading = parse_edge(value)?,
        "te" => input.edges.trailing = parse_edge(value)?,
        _ => {
            return Err(format!(
                "unknown key {key:?}; valid keys: axial-length, hub-in, hub-out, shroud-in, \
                 shroud-out, le-hub, le-shroud, spans, points, beta, e-le, e-te, le, te"
            ));
        }
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
    match (
        input.edges.leading,
        input.edges.leading.min_pressure_coefficient(),
    ) {
        (EdgeShape::Ellipse { ratio }, Some(cp)) => println!(
            "Leading edge   ellipse {ratio}:1, c_p,min at shockless entry {cp:.2} (Gülich eq. 7.1a)"
        ),
        _ => println!("Leading edge   blunt"),
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
