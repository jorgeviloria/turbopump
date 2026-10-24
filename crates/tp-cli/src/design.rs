//! `turbopump design`: main dimensions of the impellers of a pump project.

use std::path::PathBuf;

use clap::Args;
use tp_design::pump::{
    CentrifugalPumpDesign, CentrifugalPumpInput, ImpellerType, InletDesign, InletSection, Suction,
    correlations,
};
use tp_design::{MainDimensionsInput, Param};
use tp_project::{ComponentId, ComponentKind, Project, ProjectError};
use tp_units::{DisplayUnits, QuantityKind, parse_quantity_or};

use crate::{CliResult, Outcome, show};

const KEYS: &str = "type, eps, d2i, suction, inlet, eta-h, eta-v, eta, psi, d2, b2, d1, d1-factor, z, \
                    head-rise, beta2, e, shaft-stress, d-shaft, d-hub, incidence, nss";

#[derive(Args)]
pub(crate) struct DesignArgs {
    /// Project file (.tpj). Inputs and step status are saved back into it.
    file: PathBuf,
    /// Only design this component id (default: every impeller).
    #[arg(long)]
    component: Option<u32>,
    /// Fix an input as KEY=VALUE, or KEY=auto to return to the automatic value.
    /// Keys: type (auto|radial|semi-axial), eps (outlet inclination of a semi-axial
    /// impeller), d2i (its outlet hub diameter), suction (single|double), inlet (normal|suction impeller), eta-h, eta-v, eta,
    /// psi, d2, b2, d1, d1-factor, z, head-rise (fraction, e.g. 0.2: picks z and β2B),
    /// beta2 (outlet blade angle: d2, or b2 when d2 is fixed, then follows from the head),
    /// e, shaft-stress, d-shaft, d-hub, incidence, nss.
    /// Bare numbers use the project
    /// display units, e.g. `--set d2=180` for 180 mm.
    #[arg(long = "set", value_name = "KEY=VALUE")]
    settings: Vec<String>,
    /// Compute and print without saving the project.
    #[arg(long)]
    dry_run: bool,
}

pub(crate) fn err(e: ProjectError) -> String {
    e.to_string()
}

pub(crate) fn run(args: DesignArgs) -> CliResult {
    let mut project =
        Project::load(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let units = project.display_units.clone();
    let ids: Vec<ComponentId> = project
        .components()
        .iter()
        .filter(|c| c.kind() == ComponentKind::Impeller)
        .filter(|c| args.component.is_none_or(|n| c.id().0 == n))
        .map(|c| c.id())
        .collect();
    if ids.is_empty() {
        return Err(match args.component {
            Some(n) => format!("no impeller with id #{n}"),
            None => "the project has no impeller".into(),
        });
    }

    let mut warned = false;
    for id in ids {
        let mut input = project.pump_impeller_input(id).map_err(err)?;
        for setting in &args.settings {
            apply(&mut input, &units, setting)?;
        }
        project
            .set_main_dimensions(id, MainDimensionsInput::CentrifugalPump(input.clone()))
            .map_err(err)?;
        let result = project.design_main_dimensions(id).map_err(err)?;
        print_report(&project, id, &input, &result, &units).map_err(err)?;
        warned |= !result.warnings.is_empty();
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

/// Applies one `KEY=VALUE` override to the inputs.
fn apply(
    input: &mut CentrifugalPumpInput,
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
    use QuantityKind::*;
    match key {
        "type" => {
            input.impeller_type = match value {
                "auto" => ImpellerType::Auto,
                "radial" => ImpellerType::Radial,
                "semi-axial" | "mixed-flow" => ImpellerType::SemiAxial,
                _ => {
                    return Err(format!(
                        "type must be auto, radial or semi-axial, got {value:?}"
                    ));
                }
            }
        }
        "eps" => input.outlet_inclination = quantity(Angle)?,
        "d2i" => input.outlet_hub_diameter = quantity(Length)?,
        "inlet" => {
            input.inlet_design = match value {
                "normal" => InletDesign::Normal,
                "suction" => InletDesign::Suction,
                _ => return Err(format!("inlet must be normal or suction, got {value:?}")),
            }
        }
        "suction" => {
            input.suction = match value {
                "single" => Suction::Single,
                "double" => Suction::Double,
                _ => return Err(format!("suction must be single or double, got {value:?}")),
            }
        }
        "eta-h" => input.hydraulic_efficiency = quantity(Dimensionless)?,
        "eta-v" => input.volumetric_efficiency = quantity(Dimensionless)?,
        "eta" => input.overall_efficiency = quantity(Dimensionless)?,
        "psi" => input.pressure_coefficient = quantity(Dimensionless)?,
        "d2" => input.outlet_diameter = quantity(Length)?,
        "b2" => input.outlet_width = quantity(Length)?,
        "d1" => input.inlet_diameter = quantity(Length)?,
        "d1-factor" => input.inlet_diameter_factor = quantity(Dimensionless)?,
        "e" => input.blade_thickness = quantity(Length)?,
        "shaft-stress" => input.shaft_shear_stress = quantity(Pressure)?,
        "d-shaft" => input.shaft_diameter = quantity(Length)?,
        "d-hub" => input.hub_diameter = quantity(Length)?,
        "incidence" => input.incidence = quantity(Angle)?,
        "nss" => input.suction_specific_speed = quantity(Dimensionless)?,
        "head-rise" => input.head_rise = quantity(Dimensionless)?,
        "beta2" => input.outlet_blade_angle = quantity(Angle)?,
        "z" => {
            input.blade_count = if auto {
                Param::Auto
            } else {
                Param::Manual(
                    value
                        .parse()
                        .map_err(|_| format!("z must be a whole number, got {value:?}"))?,
                )
            }
        }
        _ => return Err(format!("unknown key {key:?}; valid keys: {KEYS}")),
    }
    Ok(())
}

pub(crate) fn tag<T>(param: &Param<T>) -> &'static str {
    match param {
        Param::Auto => "",
        Param::Manual(_) => "  (manual)",
    }
}

fn print_report(
    project: &Project,
    id: ComponentId,
    input: &CentrifugalPumpInput,
    d: &CentrifugalPumpDesign,
    units: &DisplayUnits,
) -> Result<(), ProjectError> {
    use QuantityKind::*;
    let s = |kind, value| show(units, kind, value);
    let name = project.component(id).map_or("", |c| c.name.as_str());
    let duty = project.pump_duty(id)?;
    let liquid = project.liquid()?;
    let source = match liquid.source {
        tp_fluids::PropertySource::CoolProp => "CoolProp",
        tp_fluids::PropertySource::BuiltinWater => "built-in water model",
    };

    println!("{name} ({id}): main dimensions");
    println!();
    println!("Fluid  {} ({source})", project.setup().fluid);
    println!(
        "  ρ {}   ν {}   p_v {}",
        s(Density, liquid.density),
        s(KinematicViscosity, liquid.kinematic_viscosity()),
        s(Pressure, liquid.vapor_pressure)
    );
    println!("Stage");
    println!(
        "  Q {}   H {}   n {}   nq {:.2}",
        s(VolumeFlow, duty.flow),
        s(Head, duty.head),
        s(RotationalSpeed, duty.speed),
        d.nq
    );
    println!(
        "  Q_La {}   H_th {}   P {}   T {}",
        s(VolumeFlow, d.impeller_flow),
        s(Head, d.theoretical_head),
        s(Power, d.stage_power),
        s(Torque, d.shaft_torque)
    );
    let e = &d.efficiencies;
    println!(
        "Efficiencies  η_h {:.3}{}   η_v {:.3}{}   η_m {:.3}   η {:.3}{}",
        e.hydraulic,
        tag(&input.hydraulic_efficiency),
        e.volumetric,
        tag(&input.volumetric_efficiency),
        e.mechanical,
        e.overall,
        tag(&input.overall_efficiency)
    );
    if input.overall_efficiency.is_auto() {
        println!(
            "              η ± {:.3} ({} pump correlation, Gülich eq. 3.9.10)",
            d.efficiency_tolerance,
            d.layout.name()
        );
    }
    println!();

    println!("Dimensions");
    let rows = [
        (
            "Shaft diameter",
            s(Length, d.shaft_diameter),
            tag(&input.shaft_diameter),
        ),
        (
            "Hub diameter",
            s(Length, d.hub_diameter),
            tag(&input.hub_diameter),
        ),
        (
            "Eye diameter d1",
            s(Length, d.inlet_diameter),
            tag(&input.inlet_diameter),
        ),
        (
            "Outlet diameter d2",
            s(Length, d.outlet_diameter),
            tag(&input.outlet_diameter),
        ),
        (
            "Outlet width b2",
            s(Length, d.outlet_width),
            tag(&input.outlet_width),
        ),
        (
            "Blades z",
            d.blade_count.to_string(),
            tag(&input.blade_count),
        ),
        (
            "Head rise to H0",
            format!("{:.0} %", d.head_rise * 100.0),
            tag(&input.head_rise),
        ),
        (
            "Blade thickness",
            s(Length, d.blade_thickness),
            tag(&input.blade_thickness),
        ),
        (
            "Pressure coeff. ψ",
            format!("{:.4}", d.pressure_coefficient),
            tag(&input.pressure_coefficient),
        ),
    ];
    for (label, value, manual) in rows {
        println!("  {label:<20} {value}{manual}");
    }
    if d.semi_axial {
        println!(
            "  Semi-axial outlet    d2i {}{}   d2m {}   ε_s {}{}",
            s(Length, d.outlet_hub_diameter),
            tag(&input.outlet_hub_diameter),
            s(Length, d.outlet_mean_diameter),
            s(Angle, d.outlet_inclination),
            tag(&input.outlet_inclination)
        );
    }
    println!();

    println!(
        "Inlet    {:>11} {:>11} {:>11} {:>11} {:>9} {:>9}",
        "d", "u", "cm", "w", "β", "β_B"
    );
    let inlet_row = |label: &str, sec: &InletSection| {
        let t = &sec.triangle;
        println!(
            "  {label:<6} {:>11} {:>11} {:>11} {:>11} {:>9} {:>9}",
            s(Length, sec.diameter),
            s(Velocity, t.u),
            s(Velocity, t.cm),
            s(Velocity, t.w()),
            s(Angle, t.beta()),
            s(Angle, sec.blade_angle)
        );
    };
    inlet_row("hub", &d.inlet_hub);
    inlet_row("mean", &d.inlet_mean);
    inlet_row("shroud", &d.inlet_shroud);

    let o = &d.outlet;
    let t = &o.triangle;
    println!(
        "Outlet   u2 {}   cm2 {}   cu2 {}   c2 {}   w2 {}",
        s(Velocity, t.u),
        s(Velocity, t.cm),
        s(Velocity, t.cu),
        s(Velocity, t.c()),
        s(Velocity, t.w())
    );
    println!(
        "         β2 {}   β2B {}   α2 {}   slip γ {:.3}",
        s(Angle, t.beta()),
        s(Angle, o.blade_angle),
        s(Angle, t.alpha()),
        o.slip_factor
    );
    if !input.outlet_blade_angle.is_auto() {
        println!("         β2B is manual: the head sets d2 (or b2 when d2 is manual)");
    }
    let (q1, q2) = (d.at_pump_flow(&d.inlet_mean.triangle), d.at_pump_flow(t));
    println!(
        "With Q   cm1 {}   β1 {} (mean)   cm2 {}   w2 {}   β2 {}   (pump flow Q = η_v·Q_La)",
        s(Velocity, q1.cm),
        s(Angle, q1.beta()),
        s(Velocity, q2.cm),
        s(Velocity, q2.w()),
        s(Angle, q2.beta())
    );
    println!(
        "Checks   δ2 = β2B − β2 {} (≤ {}°)   w2/w1 {:.2} (≥ {})",
        s(Angle, d.deviation_angle),
        correlations::MAX_DEVIATION_ANGLE,
        d.de_haller,
        correlations::DE_HALLER_LIMIT
    );
    let (lo, hi) = correlations::inlet_area_ratio_band(d.nq);
    println!(
        "         A_v/A_e {:.2} (Lobanoff & Ross {lo:.2}–{hi:.2} for large eyes)",
        d.inlet_area_ratio
    );
    println!();
    println!(
        "Cavitation  NPSH3 ≈ {} (nss {:.0}{})   NPSH available {}",
        s(Head, d.npsh_required),
        input
            .suction_specific_speed
            .resolve(|| correlations::SUCTION_SPECIFIC_SPEED),
        tag(&input.suction_specific_speed),
        s(Head, d.npsh_available)
    );
    let band = &d.npsh_band;
    println!(
        "            NPSH3 band {} to {} by the published methods (estimates; NPSH3 is a test result)",
        s(Head, band.low),
        s(Head, band.high)
    );
    for e in &band.estimates {
        let value = if (e.high - e.low).abs() < 1e-9 {
            s(Head, e.low)
        } else {
            format!("{} to {}", s(Head, e.low), s(Head, e.high))
        };
        println!("              {:<28} {value}", e.method.name());
    }

    if !d.warnings.is_empty() {
        println!();
        println!("Warnings");
        for w in &d.warnings {
            println!("  warning: {w}");
        }
    }
    println!();
    Ok(())
}
