//! `turbopump performance`: predicted characteristic (head, power, efficiency and
//! required NPSH against flow) of the impellers of a pump project, with the losses of
//! the volute after them.

use std::fmt::Write as _;
use std::path::PathBuf;

use clap::Args;
use tp_design::Param;
use tp_design::performance::{Collector, PerformanceInput, PumpPerformance};
use tp_design::radial_thrust::RadialCollector;
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
    /// coefficient), p0 (shut-off power), max (largest Q/Q_opt), points, npsh_inlet
    /// (λ_c of the NPSH₃), npsh_blade (λ_w at the design point), npsh_exponent (steep
    /// rise of NPSH₃), clearances (normal or wide: seal clearances for the radial and
    /// axial forces), seal (annular seal diameter d_sp), shaft_seal (shaft seal
    /// diameter d_D), balance (on or off: balance holes with a rear seal), rear_seal
    /// (its diameter d_s2), k0 (rotation factor of the liquid in the sidewall gaps),
    /// static (static head of the system), system_flow and system_head (a point of the
    /// system curve; default the design point), speed, d2 (trimmed impeller
    /// diameter), trim_exponent (m of the trimming law), continuous (η/η_max that bounds
    /// continuous operation), losses (analysis or statistics: how leakage, disk friction
    /// and mechanical losses are found), seal_clearance, seal_length, seal_roughness and
    /// axial_gap (of the loss analysis), viscosity (kinematic, e.g. 100 cSt; default the
    /// liquid of the project).
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
        "npsh_inlet" => input.npsh_inlet_coefficient = number(QuantityKind::Dimensionless)?,
        "npsh_blade" => input.npsh_blade_coefficient = number(QuantityKind::Dimensionless)?,
        "npsh_exponent" => input.npsh_exponent = number(QuantityKind::Dimensionless)?,
        "clearances" => {
            input.wide_seal_clearances = match value.to_ascii_lowercase().as_str() {
                "normal" | "auto" => false,
                "wide" => true,
                _ => return Err(format!("clearances must be normal or wide, got {value:?}")),
            }
        }
        "seal" => input.axial.seal_diameter = number(QuantityKind::Length)?,
        "shaft_seal" => input.axial.shaft_seal_diameter = number(QuantityKind::Length)?,
        "rear_seal" => input.axial.rear_seal_diameter = number(QuantityKind::Length)?,
        "k0" => input.axial.rotation_factor = number(QuantityKind::Dimensionless)?,
        "balance" => {
            input.axial.balance_holes = match value.to_ascii_lowercase().as_str() {
                "on" | "yes" | "true" => true,
                "off" | "no" | "false" | "auto" => false,
                _ => return Err(format!("balance must be on or off, got {value:?}")),
            }
        }
        "static" => input.operation.static_head = number(QuantityKind::Head)?,
        "system_flow" => input.operation.system_flow = number(QuantityKind::VolumeFlow)?,
        "system_head" => input.operation.system_head = number(QuantityKind::Head)?,
        "speed" => input.operation.speed = number(QuantityKind::RotationalSpeed)?,
        "d2" => input.operation.trimmed_diameter = number(QuantityKind::Length)?,
        "trim_exponent" => input.operation.trim_exponent = number(QuantityKind::Dimensionless)?,
        "continuous" => {
            input.operation.continuous_efficiency = number(QuantityKind::Dimensionless)?
        }
        "losses" => {
            input.losses.statistics = match value.to_ascii_lowercase().as_str() {
                "analysis" | "auto" => false,
                "statistics" => true,
                _ => {
                    return Err(format!(
                        "losses must be analysis or statistics, got {value:?}"
                    ));
                }
            }
        }
        "seal_clearance" => input.losses.seal_clearance = number(QuantityKind::Length)?,
        "seal_length" => input.losses.seal_length = number(QuantityKind::Length)?,
        "seal_roughness" => input.losses.seal_roughness = number(QuantityKind::Length)?,
        "axial_gap" => input.losses.axial_gap = number(QuantityKind::Length)?,
        "viscosity" => input.viscosity = number(QuantityKind::KinematicViscosity)?,
        "points" => {
            input.points = value
                .parse()
                .map_err(|_| format!("points must be a whole number, got {value:?}"))?;
        }
        _ => {
            return Err(format!(
                "unknown key {key:?}; valid keys: collector, onset, psi0, p0, max, points, \
                 npsh_inlet, npsh_blade, npsh_exponent, clearances, seal, shaft_seal, \
                 balance, rear_seal, k0, static, system_flow, system_head, speed, d2, \
                 trim_exponent, continuous, losses, seal_clearance, seal_length, \
                 seal_roughness, axial_gap, viscosity"
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
    let v = &c.viscous;
    if (v.viscosity / tp_design::viscous::WATER_VISCOSITY - 1.0).abs()
        >= tp_design::viscous::VISCOSITY_TOLERANCE
    {
        println!(
            "Liquid     ν {} (Re {:.3e}): water curve × f_Q = f_H {:.3}, f_η {:.3} by loss analysis (Gülich Table 3.10)",
            s(KinematicViscosity, v.viscosity),
            v.reynolds,
            v.head_factor,
            v.efficiency_factor
        );
        println!(
            "           correlation of Figs. 13.8–13.9: f_H {:.3}, f_η {:.3} (Re_mod {:.3e}); NPSH3 × {:.3} at design",
            v.correlation_head_factor,
            v.correlation_efficiency_factor,
            v.modified_reynolds,
            c.design.npsh_required / c.npsh.at(c.design.flow)
        );
    }
    match &c.losses {
        Some(l) => {
            println!(
                "Losses     η {:.3} by losses (statistics {:.3} ± {:.3}, Gülich Table 3.9)   η_v {:.3}",
                c.design.efficiency,
                c.statistical_efficiency,
                c.efficiency_tolerance,
                l.volumetric_efficiency
            );
            println!(
                "           seal d_sp {} s {} L {}: ΔH {} k {:.2} λ {:.3} → leakage {}; disk friction {} + {}; mechanical {}; Gülich Chap. 3.6",
                s(Length, l.seal.diameter),
                s(Length, l.seal.clearance),
                s(Length, l.seal.length),
                s(Head, l.seal_head),
                l.seal_rotation,
                l.seal_flow.friction,
                s(VolumeFlow, l.leakage),
                s(Power, l.front_friction),
                s(Power, l.rear_friction),
                s(Power, l.mechanical)
            );
        }
        None => println!(
            "Losses     from the statistics of the design: η {:.3} (Gülich Table 3.9)",
            c.statistical_efficiency
        ),
    }
    println!(
        "Shut-off   H {} (ψ0 {:.3}, Gülich eqs. 3.27–3.28)   P {} (estimate)",
        s(Head, c.shutoff_head),
        c.shutoff_coefficient,
        s(Power, c.shutoff_power)
    );
    println!(
        "Range      recirculation below {} ({:.0} %)   largest flow {} ({:.0} %)",
        s(VolumeFlow, c.recirculation_flow),
        100.0 * c.recirculation_flow / q_opt,
        s(VolumeFlow, c.max_flow),
        100.0 * c.max_flow / q_opt,
    );
    let n = &c.npsh;
    let ratio = |q: f64| format!("{:.0} %", 100.0 * q / q_opt);
    println!(
        "NPSH3      {} at design (Gülich eq. 6.10, λc {:.2}, λw {:.3}{})   available {}",
        s(Head, c.npsh_required),
        n.inlet_coefficient,
        n.blade_coefficient,
        if n.extrapolated {
            ", extrapolated from Fig. 6.20"
        } else {
            ""
        },
        s(Head, c.npsh_available)
    );
    println!(
        "           steep rise above {} ({}): shockless entry {}, w1 = w1q at {}, collector {}; exponent {:.1}",
        s(VolumeFlow, n.knee_flow),
        ratio(n.knee_flow),
        ratio(n.shockless_flow),
        n.throat_flow.map_or("—".into(), ratio),
        n.collector_flow.map_or("—".into(), ratio),
        n.exponent
    );
    if let Some(q) = c.cavitation_flow {
        println!(
            "           NPSH3 reaches the NPSH available at {} ({})",
            s(VolumeFlow, q),
            ratio(q)
        );
    }
    if let Some(v) = &c.volute {
        let d = &v.design;
        println!(
            "Volute     loses {} at design ({:.0} % of the hydraulic losses): spiral {} (K {:.3}, \
             Cp {:.3}), cone {}",
            s(Head, d.total()),
            100.0 * v.share,
            s(Head, d.spiral_loss),
            d.loss_coefficient,
            d.recovery,
            s(Head, d.cone_loss)
        );
        println!(
            "           λ {:.2}, AR {:.3}, λ6·AR {:.2} (Japikse eqs. 4.6–4.8); least loss at {}",
            d.swirl,
            d.area_ratio,
            d.matching,
            v.matched_flow.map_or("—".into(), ratio)
        );
    }
    if let Some(r) = &c.radial {
        let kind = match r.law.collector {
            RadialCollector::SingleVolute => "single volute".to_owned(),
            RadialCollector::DoubleVolute { wrap } => {
                format!("double volute of {:.0}°", wrap.to_degrees())
            }
            RadialCollector::VanedDiffuser => "vaned diffuser".to_owned(),
        };
        println!(
            "Radial     {} at design (k_R {:.3}), {} at shut-off (k_R {:.3}); {kind}, Gülich Table 9.4",
            s(Force, r.design_force),
            r.law.best_coefficient,
            s(Force, r.shutoff_force),
            r.law.shutoff_coefficient
        );
        println!(
            "           for the mechanical design {} (steady plus unsteady, k_R,tot ≥ 0.15)",
            s(Force, r.design_load)
        );
    }
    let o = &c.operation;
    println!(
        "System     H = {} + R·Q² through {} at {}   (Gülich eqs. 11.1–11.3)",
        s(Head, o.system.static_head),
        s(Head, o.system_head),
        s(VolumeFlow, o.system_flow)
    );
    println!(
        "           speed {} ({:.3} of design)   d2' {} (d2'/d2 {:.3}, Δη {:.3}, limit {:.2}, Gülich eqs. 4.15–4.23)",
        s(RotationalSpeed, o.speed),
        o.speed_ratio,
        s(Length, o.diameter),
        o.trim_ratio,
        o.efficiency_drop,
        o.trim_limit
    );
    match &o.point {
        Some(p) => println!(
            "           operating point Q {} ({:.0} % of best)   H {}   η {:.3}   P {}   NPSH3 {}",
            s(VolumeFlow, p.flow),
            100.0 * p.flow / o.best.flow,
            s(Head, p.head),
            p.efficiency,
            s(Power, p.power),
            s(Head, p.npsh_required)
        ),
        None => println!("           no operating point: the curves do not cross"),
    }
    println!(
        "           continuous operation {} to {} (η ≥ 0.8·η_max, Gülich Chap. 11.6)",
        s(VolumeFlow, o.continuous.0),
        s(VolumeFlow, o.continuous.1)
    );
    let design_speed = o.speed / o.speed_ratio;
    let full_diameter = o.diameter / o.trim_ratio;
    println!(
        "           through the system point: d2' {}   or {} at full diameter",
        o.required_trim
            .map_or("— (needs a larger impeller)".into(), |t| format!(
                "{} ({t:.3})",
                s(Length, t * full_diameter)
            )),
        o.required_speed
            .map_or("—".into(), |r| s(RotationalSpeed, r * design_speed))
    );
    let a = &c.axial;
    let f = &a.design;
    let toward = |force: f64| {
        if force >= 0.0 {
            "toward the suction side"
        } else {
            "toward the pressure side"
        }
    };
    if a.law.eyes > 1.5 {
        println!(
            "Axial      about {} of either sign at design; double-entry impeller, Gülich eq. T9.2.15",
            s(Force, f.total)
        );
    } else {
        println!(
            "Axial      {} at design {}, {} at shut-off; largest {}",
            s(Force, f.total.abs()),
            toward(f.total),
            s(Force, a.shutoff.total),
            s(Force, a.largest)
        );
        println!(
            "           shrouds {} (k {:.2} front, {:.2} rear{}), momentum {}, shaft {}; Gülich Table 9.2",
            s(Force, f.hydraulic),
            f.front_rotation,
            f.rear_rotation,
            if a.law.rear_seal_diameter.is_some() {
                ", balance holes"
            } else {
                ""
            },
            s(Force, f.momentum),
            s(Force, f.shaft)
        );
        println!(
            "           seals d_sp {} and d_D {}; rough estimate {} to {} (eq. T9.2.12)",
            s(Length, a.law.seal_diameter),
            s(Length, a.law.shaft_seal_diameter),
            s(Force, a.estimate.0),
            s(Force, a.estimate.1)
        );
    }
    println!();
    let volute = c.volute.is_some();
    print!(
        "  {:>6} {:>14} {:>12} {:>12} {:>7} {:>7} {:>10}",
        "Q/Qopt", "Q", "H", "P", "η", "η_h", "NPSH3"
    );
    if volute {
        print!(" {:>10}", "volute");
    }
    println!();
    for p in &c.points {
        let mark = if p.estimated { "~" } else { " " };
        let hydraulic = if p.hydraulic_efficiency.is_finite() {
            format!("{:.3}", p.hydraulic_efficiency)
        } else {
            "—".into()
        };
        print!(
            "{mark} {:>6.3} {:>14} {:>12} {:>12} {:>7.3} {:>7} {:>10}",
            p.flow / q_opt,
            s(VolumeFlow, p.flow),
            s(Head, p.head),
            s(Power, p.power),
            p.efficiency,
            hydraulic,
            s(Head, p.npsh_required)
        );
        if volute {
            print!(" {:>10}", s(Head, p.volute_loss));
        }
        println!();
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
        "flow_m3_s,head_m,theoretical_head_m,power_w,efficiency,hydraulic_efficiency,estimated,\
         npsh3_m,volute_loss_m,radial_force_n,axial_force_n\n",
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
            "{},{},{},{},{},{},{},{},{},{},{}",
            p.flow,
            p.head,
            opt(p.theoretical_head),
            p.power,
            p.efficiency,
            opt(p.hydraulic_efficiency),
            p.estimated,
            opt(p.npsh_required),
            opt(p.volute_loss),
            opt(p.radial_force),
            opt(p.axial_force)
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
        apply(&mut input, &units, "npsh_exponent=3").unwrap();
        apply(&mut input, &units, "clearances=wide").unwrap();
        assert!(input.wide_seal_clearances);
        apply(&mut input, &units, "balance=on").unwrap();
        apply(&mut input, &units, "seal=90 mm").unwrap();
        apply(&mut input, &units, "k0=0.45").unwrap();
        apply(&mut input, &units, "static=10 m").unwrap();
        apply(&mut input, &units, "d2=160 mm").unwrap();
        apply(&mut input, &units, "speed=1450 rpm").unwrap();
        apply(&mut input, &units, "losses=statistics").unwrap();
        apply(&mut input, &units, "viscosity=100 cSt").unwrap();
        assert!(matches!(input.viscosity, Param::Manual(v) if (v - 1e-4).abs() < 1e-12));
        apply(&mut input, &units, "seal_clearance=0.3 mm").unwrap();
        assert!(input.losses.statistics);
        assert_eq!(input.losses.seal_clearance, Param::Manual(0.3e-3));
        assert!(apply(&mut input, &units, "losses=guess").is_err());
        assert_eq!(input.operation.static_head, Param::Manual(10.0));
        assert_eq!(input.operation.trimmed_diameter, Param::Manual(0.16));
        assert!(matches!(input.operation.speed, Param::Manual(w) if (w - 151.84).abs() < 0.01));
        assert!(input.axial.balance_holes);
        assert_eq!(input.axial.seal_diameter, Param::Manual(0.09));
        assert_eq!(input.axial.rotation_factor, Param::Manual(0.45));
        assert!(apply(&mut input, &units, "balance=maybe").is_err());
        assert!(apply(&mut input, &units, "clearances=tight").is_err());
        assert_eq!(input.npsh_exponent, Param::Manual(3.0));
        assert_eq!(input.collector, Collector::VanedDiffuser);
        assert_eq!(input.recirculation_onset, Param::Manual(0.5));
        assert_eq!(input.shutoff_power, Param::Manual(3000.0));
        assert_eq!(input.points, 21);
        apply(&mut input, &units, "onset=auto").unwrap();
        assert!(input.recirculation_onset.is_auto());
        assert!(apply(&mut input, &units, "rpm=3").is_err());
        assert!(apply(&mut input, &units, "collector=pipe").is_err());
    }
}
