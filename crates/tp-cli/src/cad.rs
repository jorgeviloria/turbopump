//! `turbopump cad`: CAD solids of the impeller and its flow domain, exported to STEP,
//! IGES and STL.

use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use tp_cad::{CadOptions, ImpellerCad, Shape};
use tp_project::{ComponentId, ComponentKind, DesignStep, Project, StepStatus};
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
    /// Wall thickness of hub disk and shroud (default: max(3 mm, 0.02·d2)).
    #[arg(long)]
    wall: Option<String>,
    /// Bore diameter of the hub (default: hub diameter at the eye).
    #[arg(long)]
    bore: Option<String>,
    /// Material density for the impeller mass.
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
        wall_thickness: length(&args.wall, &units)?,
        bore_diameter: length(&args.bore, &units)?,
        ..CadOptions::default()
    };
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
    if impellers.is_empty() {
        return Err(match args.component {
            Some(n) => format!("no impeller with id #{n}"),
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
        let geometry = project.compute_impeller_geometry(*id).map_err(err)?;
        println!("{name} ({id}): building CAD solids…");
        let cad = tp_cad::build_impeller(&geometry, &options).map_err(|e| e.to_string())?;
        warned |= !report(&cad, density, &units);

        let dir = if impellers.len() > 1 {
            base.join(format!("impeller-{}", id.0))
        } else {
            base.clone()
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // STL chordal deflection: 1/2000 of the outlet diameter, in mm.
        let deflection = geometry.basis.outlet_diameter * 1e3 / 2000.0;
        for &format in &args.format {
            for (stem, shape) in [("impeller", &cad.impeller), ("fluid", &cad.fluid)] {
                let path = dir.join(format!("{stem}.{}", format.extension()));
                write(shape, &path, format, deflection)?;
            }
        }
        println!();
    }
    Ok(if warned {
        Outcome::Warnings
    } else {
        Outcome::Ok
    })
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
    let checks = [
        ("blade", &cad.blade),
        ("flow domain", &cad.fluid),
        ("impeller", &cad.impeller),
    ];
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
        println!("  Solids           valid (blade, flow domain and impeller are single solids)");
    }
    ok
}

fn format_mass(kg: f64) -> String {
    if kg < 1.0 {
        format!("{:.0} g", kg * 1e3)
    } else {
        format!("{kg:.2} kg")
    }
}
