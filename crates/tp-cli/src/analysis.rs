//! `turbopump analysis`: flow analysis of the impellers of a pump project: the
//! two-dimensional potential flow in the meridional section, the relative
//! velocities on the blade faces and the blade-quality indicators drawn from them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::Args;
use tp_design::impeller::ImpellerGeometry;
use tp_design::impeller::loading::{BladeVelocities, blade_velocities};
use tp_design::impeller::potential::{self, PotentialFlow, PotentialMesh};
use tp_design::impeller::quality::{DE_HALLER, blade_quality};
use tp_project::{ComponentId, ComponentKind, Project};
use tp_units::{DisplayUnits, QuantityKind};

use crate::design::err;
use crate::{CliResult, Outcome, show};

#[derive(Args)]
pub(crate) struct AnalysisArgs {
    /// Project file (.tpj). It is only read.
    file: PathBuf,
    /// Only this component id (default: every impeller).
    #[arg(long)]
    component: Option<u32>,
    /// Write potential-walls.csv and blade-velocities.csv (SI units) into this folder.
    #[arg(long, value_name = "DIR")]
    csv: Option<PathBuf>,
}

pub(crate) fn run(args: AnalysisArgs) -> CliResult {
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
        return Err("--csv writes one impeller: choose it with --component".into());
    }

    let mut warned = false;
    for (id, name) in &impellers {
        let g = project.compute_impeller_geometry(*id).map_err(err)?;
        let flow = potential::solve(&g.contour, g.basis.flow, PotentialMesh::default())
            .map_err(|e| format!("{name}: {e}"))?;
        let velocities = blade_velocities(&g);
        print_report(name, *id, &g, &flow, &velocities, &units);
        warned |= !velocities.warnings.is_empty();
        if let Some(dir) = &args.csv {
            write_csv(dir, &g, &flow, &velocities)?;
        }
    }
    Ok(if warned {
        Outcome::Warnings
    } else {
        Outcome::Ok
    })
}

fn print_report(
    name: &str,
    id: ComponentId,
    g: &ImpellerGeometry,
    flow: &PotentialFlow,
    v: &BladeVelocities,
    units: &DisplayUnits,
) {
    let s = |value| show(units, QuantityKind::Velocity, value);
    println!("{name} ({id}): flow analysis");
    println!();
    println!(
        "Potential flow   mesh {} × {} in the channel, {} iterations",
        flow.channel.1 - flow.channel.0 + 1,
        flow.across,
        flow.iterations
    );
    println!(
        "  {:>8} {:>12} {:>12} {:>12}",
        "fraction", "c_m hub", "c_m shroud", "quasi-1D"
    );
    let (hub, shroud) = (flow.hub_velocity(), flow.shroud_velocity());
    let n = hub.len();
    for k in 0..=8 {
        let i = (k * (n - 1)) / 8;
        let t = hub[i][0];
        println!(
            "  {:>8.3} {:>12} {:>12} {:>12}",
            t,
            s(hub[i][1]),
            s(shroud[i][1]),
            s(g.channel.cm_at(t))
        );
    }
    println!();
    println!(
        "Blade faces      relative velocity over u₂ = {}",
        s(v.outlet_speed)
    );
    println!(
        "  {:>6} {:>9} {:>9} {:>11} {:>11}",
        "span", "w_SS max", "w_PS min", "w_LE mean", "w_TE mean"
    );
    let u2 = v.outlet_speed;
    for sec in &v.sections {
        let max = sec
            .suction
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let min = sec.pressure.iter().copied().fold(f64::INFINITY, f64::min);
        println!(
            "  {:>6.2} {:>9.3} {:>9.3} {:>11.3} {:>11.3}",
            sec.span,
            max / u2,
            min / u2,
            sec.mean[0] / u2,
            sec.mean[sec.mean.len() - 1] / u2
        );
    }
    let q = blade_quality(g, v);
    println!();
    println!("Blade quality    with the flow following the blades");
    println!(
        "  {:>6} {:>8} {:>9} {:>9} {:>6} {:>9} {:>9}",
        "span", "w2/w1", "w_SS/w2", "Δw/w̄ max", "at m̄", "Δw/w̄ LE", "w_PS/u2"
    );
    for sec in &q.sections {
        println!(
            "  {:>6.2} {:>8.3} {:>9.3} {:>9.3} {:>6.2} {:>9.3} {:>9.3}",
            sec.span,
            sec.deceleration,
            sec.diffusion,
            sec.peak_loading,
            sec.peak_at,
            sec.leading_loading,
            sec.pressure_minimum
        );
    }
    if let Some(outer) = q.outer() {
        println!(
            "  de Haller (shroud) w2/w1 = {:.3}, {} {DE_HALLER}",
            outer.deceleration,
            if outer.deceleration >= DE_HALLER {
                "at least"
            } else {
                "below"
            }
        );
    }
    println!(
        "  secondary-flow drive ΔC_p (suction face, shroud − hub) = {:.3} at m̄ {:.2}",
        q.secondary_flow, q.secondary_at
    );
    if !v.warnings.is_empty() {
        println!();
        println!("Warnings");
        for w in &v.warnings {
            println!("  warning: {w}");
        }
    }
    println!();
}

fn write_csv(
    dir: &Path,
    g: &ImpellerGeometry,
    flow: &PotentialFlow,
    v: &BladeVelocities,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut walls = String::from("fraction,cm_hub_m_s,cm_shroud_m_s,cm_quasi1d_m_s\n");
    for (h, s) in flow.hub_velocity().iter().zip(flow.shroud_velocity()) {
        let _ = writeln!(
            walls,
            "{},{},{},{}",
            h[0],
            h[1],
            s[1],
            g.channel.cm_at(h[0])
        );
    }
    let mut faces = String::from("span,position,cm_m_s,w_mean_m_s,w_suction_m_s,w_pressure_m_s\n");
    for sec in &v.sections {
        for k in 0..sec.position.len() {
            let _ = writeln!(
                faces,
                "{},{},{},{},{},{}",
                sec.span, sec.position[k], sec.cm[k], sec.mean[k], sec.suction[k], sec.pressure[k]
            );
        }
    }
    for (file, text) in [
        ("potential-walls.csv", walls),
        ("blade-velocities.csv", faces),
    ] {
        let path = dir.join(file);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("Wrote {}", path.display());
    }
    Ok(())
}
