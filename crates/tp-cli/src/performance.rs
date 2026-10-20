//! `turbopump performance`: predicted characteristic (head, power and efficiency
//! against flow) of the impellers of a pump project.

use std::fmt::Write as _;
use std::path::PathBuf;

use clap::Args;
use tp_design::Param;
use tp_design::performance::{Collector, PerformanceInput, PumpPerformance};
use tp_project::{ComponentId, ComponentKind, Project};
use tp_units::{DisplayUnits, QuantityKind, parse_quantity_or};

use crate::design::err;
use crate::{CliResult, Outcome, show};

#[derive(Args)]
pub(crate) struct PerformanceArgs {
    /// Project file (.tpj). It is only read.
    file: PathBuf,
    /// Only this component id (default: every impeller).
    #[arg(long)]
    component: Option<u32>,
    /// Fix a setting as KEY=VALUE, or KEY=auto. Keys: collector (volute, diffuser or
    /// vaneless), onset (Q/Q_opt where recirculation starts), psi0 (shut-off pressure
    /// coefficient), p0 (shut-off power), max (largest Q/Q_opt), points.
    #[arg(long = "set", value_name = "KEY=VALUE")]
    settings: Vec<String>,
    /// Write the curve as CSV in SI units to this file (one impeller only).
    #[arg(long, value_name = "FILE")]
    csv: Option<PathBuf>,
}

pub(crate) fn run(args: PerformanceArgs) -> CliResult {
    let project = Project::load(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
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
    if args.csv.is_some() && impellers.len() > 1 {
        return Err("--csv writes one curve: choose the impeller with --component".into());
    }

    let mut warned = false;
    for (id, name) in &impellers {
        let mut input = project.performance_input(*id).map_err(err)?;
        for setting in &args.settings {
            apply(&mut input, &units, setting)?;
        }
        let curve = project.compute_performance(*id, &input).map_err(err)?;
        print_report(name, *id, &input, &curve, &units);
        warned |= !curve.warnings.is_empty();
        if let Some(path) = &args.csv {
            std::fs::write(path, csv(&curve)).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("Wrote {}", path.display());
        }
    }
    Ok(if warned {
        Outcome::Warnings
    } else {
        Outcome::Ok
    })
}

/// Applies one `KEY=VALUE` setting.
fn apply(input: &mut PerformanceInput, units: &DisplayUnits, setting: &str) -> Result<(), String> {
    let (key, value) = setting
        .split_once('=')
        .ok_or_else(|| format!("--set expects KEY=VALUE, got {setting:?}"))?;
    let (key, value) = (key.trim(), value.trim());
    let auto = value.eq_ignore_ascii_case("auto");
    let number = |kind: QuantityKind| -> Result<Param<f64>, String> {
        if auto {
            return Ok(Param::Auto);
        }
        parse_quantity_or(value, kind, units.get(kind))
            .map(Param::Manual)
            .map_err(|e| format!("{key}: {e}"))
    };
    match key {
        "collector" => {
            input.collector = match value.to_ascii_lowercase().as_str() {
                "volute" => Collector::Volute,
                "diffuser" | "vaned" => Collector::VanedDiffuser,
                "vaneless" => Collector::VanelessDiffuser,
                _ => {
                    return Err(format!(
                        "collector must be volute, diffuser or vaneless, got {value:?}"
                    ));
                }
            }
        }
        "onset" => input.recirculation_onset = number(QuantityKind::Dimensionless)?,
        "psi0" => input.shutoff_coefficient = number(QuantityKind::Dimensionless)?,
        "p0" => input.shutoff_power = number(QuantityKind::Power)?,
        "max" => input.max_flow_ratio = number(QuantityKind::Dimensionless)?,
        "points" => {
            input.points = value
                .parse()
                .map_err(|_| format!("points must be a whole number, got {value:?}"))?;
        }
        _ => {
            return Err(format!(
                "unknown key {key:?}; valid keys: collector, onset, psi0, p0, max, points"
            ));
        }
    }
    Ok(())
}

fn collector_name(c: Collector) -> &'static str {
    match c {
        Collector::Volute => "volute",
        Collector::VanedDiffuser => "vaned diffuser",
        Collector::VanelessDiffuser => "vaneless diffuser",
    }
}

fn print_report(
    name: &str,
    id: ComponentId,
    input: &PerformanceInput,
    c: &PumpPerformance,
    units: &DisplayUnits,
) {
    use QuantityKind::*;
    let s = |kind, value| show(units, kind, value);
    let q_opt = c.design.flow;
    println!(
        "{name} ({id}): predicted characteristic, {} collector",
        collector_name(input.collector)
    );
    println!();
    println!(
        "Design     Q {}   H {}   P {}   η {:.3}",
        s(VolumeFlow, c.design.flow),
        s(Head, c.design.head),
        s(Power, c.design.power),
        c.design.efficiency
    );
    println!(
        "Best       Q {} ({:.0} %)   H {}   P {}   η {:.3}",
        s(VolumeFlow, c.best.flow),
        100.0 * c.best.flow / q_opt,
        s(Head, c.best.head),
        s(Power, c.best.power),
        c.best.efficiency
    );
    println!(
        "Shut-off   H {} (ψ0 {:.3}, Gülich eqs. 3.27–3.28)   P {} (estimate)",
        s(Head, c.shutoff_head),
        c.shutoff_coefficient,
        s(Power, c.shutoff_power)
    );
    println!(
        "Range      recirculation below {} ({:.0} %)   largest flow {} ({:.0} %)   NPSH3 at design {}",
        s(VolumeFlow, c.recirculation_flow),
        100.0 * c.recirculation_flow / q_opt,
        s(VolumeFlow, c.max_flow),
        100.0 * c.max_flow / q_opt,
        s(Head, c.npsh_required)
    );
    println!();
    println!(
        "  {:>6} {:>14} {:>12} {:>12} {:>7} {:>7}",
        "Q/Qopt", "Q", "H", "P", "η", "η_h"
    );
    for p in &c.points {
        let mark = if p.estimated { "~" } else { " " };
        let hydraulic = if p.hydraulic_efficiency.is_finite() {
            format!("{:.3}", p.hydraulic_efficiency)
        } else {
            "—".into()
        };
        println!(
            "{mark} {:>6.3} {:>14} {:>12} {:>12} {:>7.3} {:>7}",
            p.flow / q_opt,
            s(VolumeFlow, p.flow),
            s(Head, p.head),
            s(Power, p.power),
            p.efficiency,
            hydraulic
        );
    }
    println!("  ~ estimate in the recirculation range (Gülich Chap. 4.3)");
    if !c.warnings.is_empty() {
        println!();
        println!("Warnings");
        for w in &c.warnings {
            println!("  warning: {w}");
        }
    }
    println!();
}

/// The curve as CSV in SI units.
fn csv(c: &PumpPerformance) -> String {
    let mut out = String::from(
        "flow_m3_s,head_m,theoretical_head_m,power_w,efficiency,hydraulic_efficiency,estimated\n",
    );
    for p in &c.points {
        let opt = |v: f64| {
            if v.is_finite() {
                format!("{v}")
            } else {
                String::new()
            }
        };
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{}",
            p.flow,
            p.head,
            opt(p.theoretical_head),
            p.power,
            p.efficiency,
            opt(p.hydraulic_efficiency),
            p.estimated
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_parse_and_reject_unknown_keys() {
        let units = DisplayUnits::si();
        let mut input = PerformanceInput::default();
        apply(&mut input, &units, "collector=diffuser").unwrap();
        apply(&mut input, &units, "onset=0.5").unwrap();
        apply(&mut input, &units, "p0=3 kW").unwrap();
        apply(&mut input, &units, "points=21").unwrap();
        assert_eq!(input.collector, Collector::VanedDiffuser);
        assert_eq!(input.recirculation_onset, Param::Manual(0.5));
        assert_eq!(input.shutoff_power, Param::Manual(3000.0));
        assert_eq!(input.points, 21);
        apply(&mut input, &units, "onset=auto").unwrap();
        assert!(input.recirculation_onset.is_auto());
        assert!(apply(&mut input, &units, "speed=3").is_err());
        assert!(apply(&mut input, &units, "collector=pipe").is_err());
    }
}
