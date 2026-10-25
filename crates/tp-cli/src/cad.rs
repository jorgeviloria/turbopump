//! `turbopump cad`: CAD solids of the impeller and its flow domain, exported to STEP,
//! IGES and STL.

use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use tp_cad::{BalanceHoleOptions, CadOptions, ImpellerCad, Shape};
use tp_project::{
    ComponentId, ComponentKind, DesignStep, Project, StepStatus, VANED_STEPS, VANELESS_STEPS,
    VOLUTE_STEPS,
};
use tp_units::{DisplayUnits, QuantityKind, parse_quantity_or};

use crate::design::err;
use crate::{CliResult, Outcome, show};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Step,
    Iges,
    Stl,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Format::Step => "step",
            Format::Iges => "igs",
            Format::Stl => "stl",
        }
    }
}

#[derive(Args)]
pub(crate) struct CadArgs {
    /// Project file (.tpj); the geometry step must be complete.
    file: PathBuf,
    /// Only this component id (default: every impeller).
    #[arg(long)]
    component: Option<u32>,
    /// Output folder (default: next to the project, named after it with "-cad").
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// File formats to write.
    #[arg(long, value_enum, value_delimiter = ',', default_value = "step")]
    format: Vec<Format>,
    /// Wall thickness of hub disk and shroud, over the project's automatic material
    /// (default: the project's, max(3 mm, 0.02·d2) unless set).
    #[arg(long)]
    wall: Option<String>,
    /// Bore diameter of the hub (default: hub diameter at the eye).
    #[arg(long)]
    bore: Option<String>,
    /// Outer diameter of the boss around the bore (default: the bore plus a wall of a
    /// quarter of it, no thinner than the wall thickness).
    #[arg(long)]
    boss: Option<String>,
    /// Fillet radius where the blades meet hub and shroud (default: half the blade
    /// thickness; 0 for sharp edges).
    #[arg(long)]
    fillet: Option<String>,
    /// Drill one balance hole per channel inside the rear seal, sized by Gülich
    /// Chap. 9.2.2.
    #[arg(long)]
    balance_holes: bool,
    /// Material density for the masses of the impeller and the volute casing.
    #[arg(long, default_value = "7850 kg/m3")]
    density: String,
}

fn length(text: &Option<String>, units: &DisplayUnits) -> Result<Option<f64>, String> {
    text.as_deref()
        .map(|t| {
            parse_quantity_or(t, QuantityKind::Length, units.get(QuantityKind::Length))
                .map_err(|e| e.to_string())
        })
        .transpose()
}

fn write(shape: &Shape, path: &Path, format: Format, deflection: f64) -> Result<(), String> {
    match format {
        Format::Step => tp_cad::write_step(shape, path),
        Format::Iges => tp_cad::write_iges(shape, path),
        Format::Stl => tp_cad::write_stl(shape, path, deflection),
    }
    .map_err(|e| e.to_string())?;
    println!("Wrote {}", path.display());
    Ok(())
}

pub(crate) fn run(args: CadArgs) -> CliResult {
    let project = Project::load(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let units = project.display_units.clone();
    let options = CadOptions {
        fillet_radius: length(&args.fillet, &units)?,
        ..CadOptions::default()
    };
    // Wall, bore and boss given here replace the project's for the automatic material.
    let material = [
        length(&args.wall, &units)?,
        length(&args.bore, &units)?,
        length(&args.boss, &units)?,
    ];
    let density = parse_quantity_or(
        &args.density,
        QuantityKind::Density,
        units.get(QuantityKind::Density),
    )
    .map_err(|e| e.to_string())?;

    let impellers: Vec<(ComponentId, String)> = project
        .components()
        .iter()
        .filter(|c| c.kind() == ComponentKind::Impeller)
        .filter(|c| args.component.is_none_or(|n| c.id().0 == n))
        .map(|c| (c.id(), c.name.clone()))
        .collect();
    let volutes: Vec<(ComponentId, String)> = project
        .components()
        .iter()
        .filter(|c| c.kind() == ComponentKind::Volute)
        .filter(|c| args.component.is_none_or(|n| c.id().0 == n))
        .map(|c| (c.id(), c.name.clone()))
        .collect();
    let stators: Vec<(ComponentId, String, bool)> = project
        .components()
        .iter()
        .filter_map(|c| match c.kind() {
            ComponentKind::Stator { vaned } => Some((c.id(), c.name.clone(), vaned)),
            _ => None,
        })
        .filter(|(id, _, _)| args.component.is_none_or(|n| id.0 == n))
        .collect();
    if impellers.is_empty() && volutes.is_empty() && stators.is_empty() {
        return Err(match args.component {
            Some(n) => format!("no component with id #{n}"),
            None => "the project has no impeller".into(),
        });
    }
    let base = args.out.clone().unwrap_or_else(|| {
        let stem = args
            .file
            .file_stem()
            .map_or("project".into(), |s| s.to_string_lossy().into_owned());
        args.file.with_file_name(format!("{stem}-cad"))
    });

    let mut warned = false;
    for (id, name) in &impellers {
        let component = project.component(*id).expect("listed above");
        if component.status(DesignStep::BladeEdges) != StepStatus::Done {
            return Err(format!(
                "{name}: complete the geometry first (`turbopump geometry {}`)",
                args.file.display()
            ));
        }
        let geometry = if material.iter().any(Option::is_some) {
            let mut input = project.impeller_geometry_input(*id).map_err(err)?;
            let m = &mut input.meridional.material;
            for (value, param) in material.iter().zip([
                &mut m.wall_thickness,
                &mut m.bore_diameter,
                &mut m.boss_diameter,
            ]) {
                if let Some(v) = value {
                    *param = tp_design::Param::Manual(*v);
                }
            }
            let mut adjusted = project.clone();
            adjusted.set_impeller_geometry(*id, input).map_err(err)?;
            adjusted.compute_impeller_geometry(*id).map_err(err)?
        } else {
            project.compute_impeller_geometry(*id).map_err(err)?
        };
        for warning in &geometry.contour.material_warnings {
            eprintln!("warning: {warning}");
        }
        let options = CadOptions {
            balance_holes: if args.balance_holes {
                let holes = balance_holes(&project, *id, &geometry)?;
                if holes.is_none() {
                    eprintln!("warning: balance holes need the rear seal; they are left out");
                    warned = true;
                }
                holes
            } else {
                None
            },
            ..options
        };
        println!("{name} ({id}): building CAD solids…");
        let cad = tp_cad::build_impeller(&geometry, &options).map_err(|e| e.to_string())?;
        warned |= !report(&cad, density, &units);
        // The inlet duct ahead of the impeller, built on the same contour.
        let duct_before = project
            .components()
            .iter()
            .take_while(|c| c.id() != *id)
            .last()
            .filter(|c| c.kind() == ComponentKind::InletDuct)
            .map(|c| c.id());
        let duct = match duct_before {
            Some(duct) => {
                let input = project.duct_input(duct).map_err(err)?;
                let built =
                    tp_design::impeller::duct::build(&geometry.contour, &geometry.basis, &input);
                match tp_cad::build_inlet_duct(&built) {
                    Ok(shape) => Some(shape),
                    Err(e) => {
                        eprintln!("warning: {e}");
                        warned = true;
                        None
                    }
                }
            }
            None => None,
        };

        let dir = if impellers.len() > 1 {
            base.join(format!("impeller-{}", id.0))
        } else {
            base.clone()
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // STL chordal deflection: 1/2000 of the outlet diameter, in mm.
        let deflection = geometry.basis.outlet_diameter * 1e3 / 2000.0;
        for &format in &args.format {
            let chambers = cad.side_chambers.as_ref().map(|s| ("side-chambers", s));
            let duct = duct.as_ref().map(|s| ("inlet-duct", s));
            // Every wear ring is a file of its own, the chambers designed or not.
            let rings = cad.rings.iter().map(|r| (r.kind.file_stem(), &r.solid));
            for (stem, shape) in [("impeller", &cad.impeller), ("fluid", &cad.fluid)]
                .into_iter()
                .chain(chambers)
                .chain(duct)
                .chain(rings)
            {
                let path = dir.join(format!("{stem}.{}", format.extension()));
                write(shape, &path, format, deflection)?;
            }
        }
        // One flow domain for CFD: with the side chambers and the volute after them.
        let next = project
            .components()
            .iter()
            .skip_while(|c| c.id() != *id)
            .nth(1)
            .filter(|c| c.kind() == ComponentKind::Volute)
            .map(|c| c.id());
        if let (Some(_), Some(volute)) = (&cad.side_chambers, next) {
            let joined = project.compute_volute(volute).map_err(err).and_then(|vg| {
                let v = tp_cad::build_volute(&vg).map_err(|e| e.to_string())?;
                tp_cad::join_fluid(&cad, &geometry, &v, &vg, duct.as_ref())
                    .map_err(|e| e.to_string())
            });
            match joined {
                Ok((shape, warnings)) => {
                    for w in warnings {
                        eprintln!("warning: {w}");
                    }
                    for &format in &args.format {
                        let path = dir.join(format!("fluid-joined.{}", format.extension()));
                        write(&shape, &path, format, deflection)?;
                    }
                }
                Err(e) => eprintln!("warning: the joined fluid was not built: {e}"),
            }
        }
        println!();
    }

    for (id, name, vaned) in &stators {
        let component = project.component(*id).expect("listed above");
        let steps: &[DesignStep] = if *vaned {
            &VANED_STEPS
        } else {
            &VANELESS_STEPS
        };
        if steps
            .iter()
            .any(|&step| component.status(step) != StepStatus::Done)
        {
            println!(
                "{name} ({id}): not designed yet; run `turbopump diffuser {}` first",
                args.file.display()
            );
            println!();
            warned = true;
            continue;
        }
        let design = project.compute_diffuser(*id).map_err(err)?;
        println!("{name} ({id}): building CAD solids…");
        let cad = tp_cad::build_diffuser(&design).map_err(|e| e.to_string())?;
        println!("  Diffuser flow domain {:.1} cm³", cad.fluid_volume() * 1e6);
        if !cad.fluid.is_valid() || cad.fluid.solid_count() != 1 {
            println!("  warning: the diffuser flow domain is not a single valid solid");
            warned = true;
        }
        std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
        let deflection = design.outlet_diameter * 1e3 / 2000.0;
        for &format in &args.format {
            let mut shapes = vec![(format!("diffuser-{}", id.0), &cad.fluid)];
            if let Some(vanes) = &cad.vanes {
                shapes.push((format!("diffuser-{}-vanes", id.0), vanes));
            }
            for (stem, shape) in shapes {
                let path = base.join(format!("{stem}.{}", format.extension()));
                write(shape, &path, format, deflection)?;
            }
        }
        println!();
    }

    for (id, name) in &volutes {
        let component = project.component(*id).expect("listed above");
        if VOLUTE_STEPS
            .iter()
            .any(|&step| component.status(step) != StepStatus::Done)
        {
            println!(
                "{name} ({id}): not designed yet; run `turbopump volute {}` first",
                args.file.display()
            );
            println!();
            warned = true;
            continue;
        }
        let geometry = project.compute_volute(*id).map_err(err)?;
        println!("{name} ({id}): building CAD solids…");
        let cad = tp_cad::build_volute(&geometry).map_err(|e| e.to_string())?;
        println!("  Volute flow domain {:.1} cm³", cad.fluid_volume() * 1e6);
        let casing = tp_cad::build_volute_casing(&geometry, &cad).map_err(|e| e.to_string())?;
        let material = casing.volume();
        println!(
            "  Volute casing      {:.1} cm³, {} at {}, wall {}{}",
            material * 1e6,
            format_mass(material * density),
            show(&units, QuantityKind::Density, density),
            show(&units, QuantityKind::Length, geometry.design.wall_thickness),
            if casing.flange {
                ", with its flange"
            } else {
                ""
            }
        );
        for w in &casing.warnings {
            println!("  warning: {w}");
            warned = true;
        }
        for (label, shape) in [("flow domain", &cad.fluid), ("casing", &casing.solid)] {
            if !shape.is_valid() || shape.solid_count() != 1 {
                println!("  warning: the volute {label} is not a single valid solid");
                warned = true;
            }
        }
        std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
        let stem = if volutes.len() > 1 {
            format!("volute-{}", id.0)
        } else {
            "volute".to_owned()
        };
        let deflection = geometry.basis.impeller_diameter * 1e3 / 2000.0;
        for &format in &args.format {
            let path = base.join(format!("{stem}.{}", format.extension()));
            write(&cad.fluid, &path, format, deflection)?;
            let path = base.join(format!("{stem}-casing.{}", format.extension()));
            write(&casing.solid, &path, format, deflection)?;
        }
        println!();
    }
    Ok(if warned {
        Outcome::Warnings
    } else {
        Outcome::Ok
    })
}

/// One balance hole per channel, sized for the rear seal drawn in `geometry` (Gülich
/// Chap. 9.2.2); `None` when the contour has no rear seal for the holes to open into.
fn balance_holes(
    project: &Project,
    id: ComponentId,
    geometry: &tp_design::impeller::ImpellerGeometry,
) -> Result<Option<BalanceHoleOptions>, String> {
    Ok(project
        .balance_hole_size(id, geometry)?
        .map(|size| BalanceHoleOptions {
            count: size.count,
            diameter: size.diameter,
            seal_diameter: size.seal_diameter,
        }))
}

/// Prints volumes and checks; returns whether every solid is valid.
fn report(cad: &ImpellerCad, density: f64, units: &DisplayUnits) -> bool {
    let impeller = cad.impeller_volume();
    let fluid = cad.fluid_volume();
    println!(
        "  Wall thickness   {}",
        show(units, QuantityKind::Length, cad.wall_thickness)
    );
    println!(
        "  Impeller         {:.1} cm³, {} at {}",
        impeller * 1e6,
        format_mass(impeller * density),
        show(units, QuantityKind::Density, density)
    );
    println!("  Flow domain      {:.1} cm³", fluid * 1e6);
    for ring in &cad.rings {
        let volume = cad.ring_volume(ring.kind).unwrap_or(0.0);
        println!(
            "  {:<19} {:.1} cm³, {} (at the impeller density)",
            ring.kind.label(),
            volume * 1e6,
            format_mass(volume * density)
        );
    }
    if let Some(vanes) = &cad.back_vanes {
        let volume = vanes.volume() * 1e-9;
        println!(
            "  Back vanes       {:.1} cm³, {} (part of the impeller)",
            volume * 1e6,
            format_mass(volume * density)
        );
    }
    let length = |v: f64| show(units, QuantityKind::Length, v);
    println!(
        "  Hub              bore {}, boss {}",
        length(cad.bore_diameter),
        length(cad.boss_diameter)
    );
    if cad.fillet_radius > 0.0 {
        println!(
            "  Fillets          {} at the blade roots",
            length(cad.fillet_radius)
        );
    }
    if let Some(h) = &cad.balance_holes {
        println!(
            "  Balance holes    {} × {} on {} (Gülich Chap. 9.2.2)",
            h.count,
            length(h.diameter),
            length(h.pitch_diameter)
        );
    }
    for w in &cad.warnings {
        println!("  warning: {w}");
    }
    let checks = [
        ("blade".to_string(), &cad.blade),
        ("flow domain".to_string(), &cad.fluid),
        ("impeller".to_string(), &cad.impeller),
    ]
    .into_iter()
    .chain(
        cad.rings
            .iter()
            .map(|r| (r.kind.label().to_lowercase(), &r.solid)),
    );
    let mut ok = true;
    for (label, shape) in checks {
        if !shape.is_valid() || shape.solid_count() != 1 {
            println!(
                "  warning: the {label} is not a single valid solid ({} solids)",
                shape.solid_count()
            );
            ok = false;
        }
    }
    if ok {
        println!(
            "  Solids           valid (blade, flow domain, impeller and wear rings are single solids)"
        );
    }
    ok && cad.warnings.is_empty()
}

fn format_mass(kg: f64) -> String {
    if kg < 1.0 {
        format!("{:.0} g", kg * 1e3)
    } else {
        format!("{kg:.2} kg")
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use tp_project::{DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType};

    use super::*;

    fn pump() -> (Project, ComponentId) {
        let setup = GlobalSetup::new(
            MachineType::Pump,
            DesignPoint {
                flow: FlowRate::Volume(100.0 / 3600.0),
                energy: EnergyTransfer::Head(32.0),
                speed: 2900.0 * PI / 30.0,
            },
        );
        let mut project = Project::new("t", MachineType::Pump, setup).unwrap();
        let id = project
            .add_component(ComponentKind::Impeller, None)
            .unwrap();
        (project, id)
    }

    #[test]
    fn balance_holes_follow_the_rear_seal_of_the_contour() {
        let (mut project, id) = pump();
        let mut input = project.impeller_geometry_input(id).unwrap();
        input.meridional.seals.rear.diameter = tp_design::Param::Manual(0.09);
        project.set_impeller_geometry(id, input).unwrap();
        let geometry = project.compute_impeller_geometry(id).unwrap();
        let rear = geometry.contour.seals.rear.unwrap();
        let holes = balance_holes(&project, id, &geometry).unwrap().unwrap();
        assert!((holes.seal_diameter - 2.0 * rear.radius).abs() < 1e-12);
    }

    #[test]
    fn balance_holes_without_a_rear_seal_are_left_out() {
        let (mut project, id) = pump();
        let mut input = project.impeller_geometry_input(id).unwrap();
        input.meridional.seals.rear.enabled = false;
        project.set_impeller_geometry(id, input).unwrap();
        let geometry = project.compute_impeller_geometry(id).unwrap();
        assert!(balance_holes(&project, id, &geometry).unwrap().is_none());
    }
}
