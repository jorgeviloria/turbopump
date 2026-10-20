//! `turbopump` command-line tool.
//!
//! Exit codes follow the batch-mode convention: 0 = success, 1 = completed with
//! warnings, 2 = error.

mod design;
mod geometry;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use tp_project::{
    ComponentKind, DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType, Project,
    Severity, StepStatus,
};
use tp_units::{DisplayUnits, QuantityKind, SpecificSpeed, parse_quantity};

#[derive(Parser)]
#[command(
    name = "turbopump",
    version,
    about = "Open-source preliminary design of turbomachinery"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Specific speed (nq, ns US, ωs, Cordier σ) of a pump stage.
    Nq {
        /// Flow rate with unit, e.g. "100 m3/h".
        #[arg(long)]
        flow: String,
        /// Head with unit, e.g. "32 m".
        #[arg(long)]
        head: String,
        /// Rotational speed with unit, e.g. "2900 rpm".
        #[arg(long)]
        speed: String,
        /// Impeller eyes: 2 for a double-suction impeller.
        #[arg(long, default_value_t = 1)]
        eyes: u32,
        /// Number of stages sharing the head.
        #[arg(long, default_value_t = 1)]
        stages: u32,
    },
    /// Convert a value between two units, e.g. `convert 150 psi bar`.
    Convert {
        #[arg(allow_negative_numbers = true)]
        value: f64,
        from: String,
        to: String,
    },
    /// Liquid properties of a fluid (CoolProp, or the built-in water model).
    Fluid {
        /// Fluid name, e.g. Water, Ethanol, n-Hexane.
        name: String,
        #[arg(long, default_value = "20 °C")]
        temperature: String,
        #[arg(long, default_value = "1 atm")]
        pressure: String,
        /// Units used for display.
        #[arg(long, value_enum, default_value_t = UnitsChoice::Si)]
        units: UnitsChoice,
    },
    /// Create a new project file.
    New(NewArgs),
    /// Compute the main dimensions and velocity triangles of the impellers of a pump
    /// project.
    Design(design::DesignArgs),
    /// Build the meridional contour, meridional flow and blades of the impellers, and
    /// optionally export points, an STL mesh and SVG drawings.
    Geometry(geometry::GeometryArgs),
    /// Show a project summary and its open issues.
    Info {
        file: PathBuf,
        /// Units used for display.
        #[arg(long, value_enum, default_value_t = UnitsChoice::Project)]
        units: UnitsChoice,
    },
}

#[derive(Args)]
#[command(group(ArgGroup::new("flow_spec").required(true).args(["flow", "mass_flow"])))]
#[command(group(
    ArgGroup::new("energy_spec")
        .required(true)
        .args(["head", "pressure_rise", "pressure_ratio", "pressure_ratio_ts", "specific_work"])
))]
struct NewArgs {
    /// Path of the project file to create (.tpj).
    file: PathBuf,
    /// Machine type: pump, fan, compressor, gas_turbine or hydro_turbine.
    #[arg(long)]
    machine: MachineType,
    /// Project name (default: file name).
    #[arg(long)]
    name: Option<String>,
    /// Volume flow with unit.
    #[arg(long)]
    flow: Option<String>,
    /// Mass flow with unit.
    #[arg(long)]
    mass_flow: Option<String>,
    /// Head with unit.
    #[arg(long)]
    head: Option<String>,
    /// Total pressure difference with unit.
    #[arg(long)]
    pressure_rise: Option<String>,
    /// Total-to-total pressure ratio.
    #[arg(long)]
    pressure_ratio: Option<f64>,
    /// Total-to-static pressure ratio (turbines).
    #[arg(long)]
    pressure_ratio_ts: Option<f64>,
    /// Specific work with unit.
    #[arg(long)]
    specific_work: Option<String>,
    /// Rotational speed with unit.
    #[arg(long)]
    speed: String,
    /// Working fluid (default: Water for pumps and hydro turbines, Air otherwise).
    #[arg(long)]
    fluid: Option<String>,
    /// Components from inlet to outlet: impeller, stator, vaneless-stator, volute.
    #[arg(long, value_delimiter = ',', default_value = "impeller")]
    components: Vec<String>,
    /// Overwrite an existing file.
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum UnitsChoice {
    /// The units saved in the project.
    Project,
    Si,
    Us,
}

/// Successful outcome of a command.
enum Outcome {
    Ok,
    Warnings,
}

type CliResult = Result<Outcome, String>;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Nq {
            flow,
            head,
            speed,
            eyes,
            stages,
        } => specific_speed(&flow, &head, &speed, eyes, stages),
        Command::Convert { value, from, to } => convert(value, &from, &to),
        Command::Fluid {
            name,
            temperature,
            pressure,
            units,
        } => fluid(&name, &temperature, &pressure, units),
        Command::New(args) => new_project(args),
        Command::Design(args) => design::run(args),
        Command::Geometry(args) => geometry::run(args),
        Command::Info { file, units } => info(&file, units),
    };
    match result {
        Ok(Outcome::Ok) => ExitCode::from(0),
        Ok(Outcome::Warnings) => ExitCode::from(1),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

fn parse(text: &str, kind: QuantityKind) -> Result<f64, String> {
    parse_quantity(text, kind).map_err(|e| e.to_string())
}

/// Formats an SI value in the chosen display unit with about four significant digits.
fn show(units: &DisplayUnits, kind: QuantityKind, si: f64) -> String {
    units.format(kind, si, decimals_for(units.to_display(kind, si)))
}

fn decimals_for(value: f64) -> usize {
    if value == 0.0 || !value.is_finite() {
        return 0;
    }
    (3 - value.abs().log10().floor() as i64).clamp(0, 6) as usize
}

fn specific_speed(flow: &str, head: &str, speed: &str, eyes: u32, stages: u32) -> CliResult {
    if eyes == 0 || stages == 0 {
        return Err("--eyes and --stages must be at least 1".into());
    }
    let q = parse(flow, QuantityKind::VolumeFlow)? / f64::from(eyes);
    let h = parse(head, QuantityKind::Head)? / f64::from(stages);
    let n = parse(speed, QuantityKind::RotationalSpeed)?;
    if q <= 0.0 || h <= 0.0 || n <= 0.0 {
        return Err("flow, head and speed must be positive".into());
    }
    let s = SpecificSpeed::from_head(q, h, n);
    println!("Specific speed (per stage, per impeller eye)");
    println!("  nq     {:>10.2}   rpm, m³/s, m", s.nq());
    println!("  ns US  {:>10.0}   rpm, gpm, ft", s.ns_us());
    println!(
        "  ωs     {:>10.4}   dimensionless (ISO 2548 type number K)",
        s.omega_s()
    );
    println!(
        "  σ      {:>10.4}   Cordier speed number",
        s.cordier_sigma()
    );
    Ok(Outcome::Ok)
}

fn convert(value: f64, from: &str, to: &str) -> CliResult {
    let result = tp_units::convert(value, from, to).map_err(|e| e.to_string())?;
    let decimals = decimals_for(result).max(2);
    println!("{value} {from} = {result:.decimals$} {to}");
    Ok(Outcome::Ok)
}

fn fluid(name: &str, temperature: &str, pressure: &str, units: UnitsChoice) -> CliResult {
    let t = parse(temperature, QuantityKind::Temperature)?;
    let p = parse(pressure, QuantityKind::Pressure)?;
    let units = match units {
        UnitsChoice::Us => DisplayUnits::us(),
        UnitsChoice::Project | UnitsChoice::Si => DisplayUnits::si(),
    };
    let liquid = tp_fluids::liquid_properties(name, p, t).map_err(|e| e.to_string())?;
    let source = match liquid.source {
        tp_fluids::PropertySource::CoolProp => "CoolProp",
        tp_fluids::PropertySource::BuiltinWater => "built-in water model",
    };
    println!(
        "{name} at {}, {} ({source})",
        show(&units, QuantityKind::Temperature, t),
        show(&units, QuantityKind::Pressure, p)
    );
    let rows = [
        ("Density", QuantityKind::Density, liquid.density),
        (
            "Dynamic viscosity",
            QuantityKind::DynamicViscosity,
            liquid.dynamic_viscosity,
        ),
        (
            "Kinematic viscosity",
            QuantityKind::KinematicViscosity,
            liquid.kinematic_viscosity(),
        ),
        (
            "Vapour pressure",
            QuantityKind::Pressure,
            liquid.vapor_pressure,
        ),
    ];
    for (label, kind, value) in rows {
        println!("  {label:<20} {}", show(&units, kind, value));
    }
    Ok(Outcome::Ok)
}

fn parse_component(name: &str) -> Result<ComponentKind, String> {
    match name.trim() {
        "impeller" | "runner" => Ok(ComponentKind::Impeller),
        "stator" => Ok(ComponentKind::Stator { vaned: true }),
        "vaneless-stator" => Ok(ComponentKind::Stator { vaned: false }),
        "volute" => Ok(ComponentKind::Volute),
        other => Err(format!(
            "unknown component {other:?}; expected impeller, stator, vaneless-stator or volute"
        )),
    }
}

fn new_project(args: NewArgs) -> CliResult {
    if args.file.exists() && !args.force {
        return Err(format!(
            "{} already exists; use --force to overwrite it",
            args.file.display()
        ));
    }
    let flow = match (&args.flow, &args.mass_flow) {
        (Some(v), _) => FlowRate::Volume(parse(v, QuantityKind::VolumeFlow)?),
        (None, Some(m)) => FlowRate::Mass(parse(m, QuantityKind::MassFlow)?),
        (None, None) => unreachable!("clap requires one flow argument"),
    };
    let energy = if let Some(h) = &args.head {
        EnergyTransfer::Head(parse(h, QuantityKind::Head)?)
    } else if let Some(p) = &args.pressure_rise {
        EnergyTransfer::TotalPressureDifference(parse(p, QuantityKind::Pressure)?)
    } else if let Some(r) = args.pressure_ratio {
        EnergyTransfer::TotalPressureRatio(r)
    } else if let Some(r) = args.pressure_ratio_ts {
        EnergyTransfer::TotalToStaticPressureRatio(r)
    } else if let Some(y) = &args.specific_work {
        EnergyTransfer::SpecificWork(parse(y, QuantityKind::SpecificEnergy)?)
    } else {
        unreachable!("clap requires one energy argument")
    };
    let speed = parse(&args.speed, QuantityKind::RotationalSpeed)?;

    let mut setup = GlobalSetup::new(
        args.machine,
        DesignPoint {
            flow,
            energy,
            speed,
        },
    );
    if let Some(fluid) = args.fluid {
        setup.fluid = fluid;
    }
    let name = args.name.unwrap_or_else(|| {
        args.file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into())
    });
    let mut project = Project::new(name, args.machine, setup).map_err(|e| e.to_string())?;
    for c in &args.components {
        project
            .add_component(parse_component(c)?, None)
            .map_err(|e| e.to_string())?;
    }
    project.save(&args.file).map_err(|e| e.to_string())?;
    println!("Created {}", args.file.display());
    print_summary(&project, &project.display_units)
}

fn info(file: &Path, units: UnitsChoice) -> CliResult {
    let project = Project::load(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let units = match units {
        UnitsChoice::Project => project.display_units.clone(),
        UnitsChoice::Si => DisplayUnits::si(),
        UnitsChoice::Us => DisplayUnits::us(),
    };
    print_summary(&project, &units)
}

fn print_summary(project: &Project, units: &DisplayUnits) -> CliResult {
    let setup = project.setup();
    let dp = &setup.design_point;
    println!("Project   {}", project.info.name);
    println!("Machine   {}", project.machine());
    println!("Fluid     {}", setup.fluid);
    println!();
    println!("Design point");
    match dp.flow {
        FlowRate::Volume(q) => println!(
            "  Flow            {}",
            show(units, QuantityKind::VolumeFlow, q)
        ),
        FlowRate::Mass(m) => println!(
            "  Mass flow       {}",
            show(units, QuantityKind::MassFlow, m)
        ),
    }
    let energy = match dp.energy {
        EnergyTransfer::Head(h) => show(units, QuantityKind::Head, h),
        EnergyTransfer::TotalPressureDifference(p) => show(units, QuantityKind::Pressure, p),
        EnergyTransfer::SpecificWork(y) => show(units, QuantityKind::SpecificEnergy, y),
        EnergyTransfer::TotalPressureRatio(r) | EnergyTransfer::TotalToStaticPressureRatio(r) => {
            format!("{r:.3}")
        }
    };
    println!("  {:<15} {energy}", capitalize(dp.energy.name()));
    println!(
        "  Speed           {}",
        show(units, QuantityKind::RotationalSpeed, dp.speed)
    );
    let specific_work = match dp.energy {
        EnergyTransfer::Head(h) => Some(tp_units::STANDARD_GRAVITY * h),
        EnergyTransfer::SpecificWork(y) => Some(y),
        _ => None,
    };
    if let (FlowRate::Volume(q), Some(y)) = (dp.flow, specific_work) {
        let s = SpecificSpeed::from_specific_work(q, y, dp.speed);
        println!("  nq              {:.2}  (ωs {:.4})", s.nq(), s.omega_s());
    }
    println!();

    println!("Components (inlet → outlet)");
    if project.components().is_empty() {
        println!("  (none)");
    }
    for c in project.components() {
        let progress: String = c
            .steps()
            .map(|(_, s)| match s {
                StepStatus::Done => '■',
                StepStatus::Stale => '▣',
                StepStatus::Pending => '□',
            })
            .collect();
        let next = c
            .next_step()
            .map_or("complete".to_owned(), |s| format!("next: {s}"));
        println!("  {} {:<18} {progress}  {next}", c.id(), c.name);
    }

    let issues = project.issues();
    if issues.is_empty() {
        return Ok(Outcome::Ok);
    }
    println!();
    println!("Issues");
    for issue in &issues {
        let tag = match issue.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        println!("  {tag}: {}", issue.message);
    }
    if issues.iter().any(|i| i.severity == Severity::Error) {
        Err("the project has errors".into())
    } else {
        Ok(Outcome::Warnings)
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
