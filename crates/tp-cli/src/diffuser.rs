//! `turbopump diffuser`: stators of a pump project, vaned or vaneless, after an impeller.

use std::path::PathBuf;

use clap::Args;
use tp_design::Param;
use tp_design::diffuser::{DiffuserDesign, DiffuserInput};
use tp_project::{ComponentId, ComponentKind, Project, VANED_STEPS, VANELESS_STEPS};
use tp_units::{DisplayUnits, QuantityKind, parse_quantity_or};

use crate::design::err;
use crate::geometry::parse_edge;
use crate::{CliResult, Outcome, show};

#[derive(Args)]
pub(crate) struct DiffuserArgs {
    /// Project file (.tpj). Inputs and step status are saved back into it.
    file: PathBuf,
    /// Only this component id (default: every stator).
    #[arg(long)]
    component: Option<u32>,
    /// Fix an input as KEY=VALUE, or KEY=auto. Keys: flow-factor (Q_Le/Q), d3, b3, d4,
    /// b4, incidence; for vaned stators also z (vanes), fa3 (throat factor), e3, e4
    /// (thickness), alpha4 (outlet vane angle), points, le and te (edge shapes, as in
    /// `turbopump geometry`).
    #[arg(long = "set", value_name = "KEY=VALUE")]
    settings: Vec<String>,
    /// Compute and print without saving the project.
    #[arg(long)]
    dry_run: bool,
}

pub(crate) fn run(args: DiffuserArgs) -> CliResult {
    let mut project =
        Project::load(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let units = project.display_units.clone();
    let stators: Vec<(ComponentId, String, bool)> = project
        .components()
        .iter()
        .filter_map(|c| match c.kind() {
            ComponentKind::Stator { vaned } => Some((c.id(), c.name.clone(), vaned)),
            _ => None,
        })
        .filter(|(id, _, _)| args.component.is_none_or(|n| id.0 == n))
        .collect();
    if stators.is_empty() {
        return Err(match args.component {
            Some(n) => format!("no stator with id #{n}"),
            None => "the project has no stator".into(),
        });
    }

    let mut warned = false;
    for (id, name, vaned) in &stators {
        let mut input = project.diffuser_input(*id).map_err(err)?;
        for setting in &args.settings {
            apply(&mut input, &units, setting)?;
        }
        project.set_diffuser(*id, input).map_err(err)?;
        let steps: &[_] = if *vaned {
            &VANED_STEPS
        } else {
            &VANELESS_STEPS
        };
        for &step in steps {
            project.design_step(*id, step).map_err(err)?;
        }
        let design = project.compute_diffuser(*id).map_err(err)?;
        print_report(name, *id, &design, &units);
        warned |= !design.warnings.is_empty();
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

/// Applies one `KEY=VALUE` override to the diffuser inputs.
fn apply(input: &mut DiffuserInput, units: &DisplayUnits, setting: &str) -> Result<(), String> {
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
    use QuantityKind::{Angle, Dimensionless, Length};
    match key {
        "flow-factor" => input.main.flow_factor = quantity(Dimensionless)?,
        "d3" => input.main.inlet_diameter = quantity(Length)?,
        "b3" => input.main.inlet_width = quantity(Length)?,
        "d4" => input.main.outlet_diameter = quantity(Length)?,
        "incidence" => input.main.incidence = quantity(Angle)?,
        "b4" => input.contour.outlet_width = quantity(Length)?,
        "z" => input.properties.vane_count = count()?,
        "fa3" => input.properties.throat_factor = quantity(Dimensionless)?,
        "e3" => input.properties.leading_thickness = quantity(Length)?,
        "alpha4" => input.mean_lines.outlet_angle = quantity(Angle)?,
        "points" => input.mean_lines.points = count()?,
        "e4" => input.profiles.trailing_thickness = quantity(Length)?,
        "le" => input.edges.leading = parse_edge(value)?,
        "te" => input.edges.trailing = parse_edge(value)?,
        _ => {
            return Err(format!(
                "unknown key {key:?}; valid keys: flow-factor, d3, b3, d4, incidence, b4, \
                 z, fa3, e3, alpha4, points, e4, le, te"
            ));
        }
    }
    Ok(())
}

fn print_report(name: &str, id: ComponentId, d: &DiffuserDesign, units: &DisplayUnits) {
    use QuantityKind::*;
    let s = |kind, value| show(units, kind, value);
    let kind = if d.vanes.is_some() {
        "vaned"
    } else {
        "vaneless"
    };
    println!("{name} ({id}): {kind} diffuser");
    println!();
    let minimum = if d.vanes.is_some() {
        format!(" (min {}, Table 10.2)", s(Length, d.min_inlet_diameter))
    } else {
        String::new()
    };
    println!(
        "Size       d3 {}{minimum}   b3 {}   d4 {} ({:.3}·d2)   b4 {}",
        s(Length, d.inlet_diameter),
        s(Length, d.inlet_width),
        s(Length, d.outlet_diameter),
        d.outlet_diameter / d.impeller_diameter,
        s(Length, d.outlet_width)
    );
    for (label, state) in [("Inlet", &d.inlet), ("Outlet", &d.outlet)] {
        println!(
            "{label:<10} cu {}   cm {}   c {}   α {}",
            s(Velocity, state.cu),
            s(Velocity, state.cm),
            s(Velocity, state.velocity()),
            s(Angle, state.angle)
        );
    }
    println!(
        "Recovery   c_p,id {:.2} (no losses)",
        d.ideal_pressure_recovery
    );
    match &d.vanes {
        None => println!(
            "Path       wrap {}   length {}",
            s(Angle, d.wrap_angle),
            s(Length, d.path_length)
        ),
        Some(v) => {
            let c = &v.checks;
            println!(
                "Vanes      z {}   α3' {}   α3B {}   α4B {}   e3 {}   e4 {}",
                v.count,
                s(Angle, v.inlet_flow_angle),
                s(Angle, v.inlet_angle),
                s(Angle, v.outlet_angle),
                s(Length, v.leading_thickness),
                s(Length, v.trailing_thickness)
            );
            println!(
                "Throat     a3 {} (T7.7.10: {})   a4 {}   L3-4 {}",
                s(Length, v.throat),
                s(Length, v.throat_target),
                s(Length, v.outlet_throat),
                s(Length, v.channel_length)
            );
            println!(
                "Criteria   A  A_R {:.2} (optimum {:.2}, c_p,opt {:.2})",
                c.area_ratio, c.optimum_area_ratio, c.optimum_pressure_recovery
            );
            println!("           B  c3q/c2 {:.2} (0.7 to 0.85)", c.deceleration);
            println!("           C  L3-4/a3 {:.1} (2.5 to 6)", c.length_ratio);
            println!("           D  b3/a3 {:.2} (0.8 to 2)", c.aspect_ratio);
            println!(
                "           E  c4²/(2gH) {:.3} (0.02 to 0.04, multistage)",
                c.outlet_energy
            );
            println!(
                "           F  opening {} (allowed {})",
                s(Angle, c.opening_angle),
                s(Angle, c.allowable_angle)
            );
        }
    }
    if !d.warnings.is_empty() {
        println!();
        println!("Warnings");
        for w in &d.warnings {
            println!("  warning: {w}");
        }
    }
    println!();
}
