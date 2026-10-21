//! `turbopump volute`: volute casings of a pump project, after the impeller.

use std::path::{Path, PathBuf};

use clap::Args;
use tp_design::Param;
use tp_design::volute::{
    AreaLaw, CutwaterShape, DischargeShape, SectionShape, StraightWall, VoluteGeometry,
    VoluteInput, VoluteKind,
};
use tp_project::{ComponentId, ComponentKind, Project, VOLUTE_STEPS};
use tp_units::{DisplayUnits, QuantityKind, parse_quantity_or};

use crate::design::err;
use crate::{CliResult, Outcome, show};

#[derive(Args)]
pub(crate) struct VoluteArgs {
    /// Project file (.tpj). Inputs and step status are saved back into it.
    file: PathBuf,
    /// Only this component id (default: every volute).
    #[arg(long)]
    component: Option<u32>,
    /// Fix an input as KEY=VALUE, or KEY=auto. Keys: flow-factor (Q_Le/Q), dz
    /// (cutwater diameter), b3 (inlet width), e3 (cutwater thickness), shape
    /// (rectangle, circle or trapezoid:DEGREES), law (auto, momentum or velocity),
    /// sections, correction (on or off), length and outlet (discharge diffuser),
    /// incidence (cutwater), wall, type (single or double), rib_wrap (wrap of the inner
    /// volute of a double volute), rib (rib thickness), channel_ratio (outlet over
    /// inlet area of the outer channel), discharge (tangential, tangential:inner,
    /// tangential:centered, tangential:outer, radial or radial:DEGREES from the line
    /// through the pump axis), bend (bend radius of a radial discharge), cutwater
    /// (tangent, sharp or rounded), nose (radius of a rounded cutwater).
    #[arg(long = "set", value_name = "KEY=VALUE")]
    settings: Vec<String>,
    /// Write volute.txt (points) and volute.stl (mesh) into this folder.
    #[arg(long, value_name = "DIR")]
    export: Option<PathBuf>,
    /// Compute and print without saving the project.
    #[arg(long)]
    dry_run: bool,
}

pub(crate) fn run(args: VoluteArgs) -> CliResult {
    let mut project =
        Project::load(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let units = project.display_units.clone();
    let volutes: Vec<(ComponentId, String)> = project
        .components()
        .iter()
        .filter(|c| c.kind() == ComponentKind::Volute)
        .filter(|c| args.component.is_none_or(|n| c.id().0 == n))
        .map(|c| (c.id(), c.name.clone()))
        .collect();
    if volutes.is_empty() {
        return Err(match args.component {
            Some(n) => format!("no volute with id #{n}"),
            None => "the project has no volute".into(),
        });
    }

    let mut warned = false;
    for (id, name) in &volutes {
        let mut input = project.volute_input(*id).map_err(err)?;
        for setting in &args.settings {
            apply(&mut input, &units, setting)?;
        }
        project.set_volute(*id, input).map_err(err)?;
        for step in VOLUTE_STEPS {
            project.design_step(*id, step).map_err(err)?;
        }
        let volute = project.compute_volute(*id).map_err(err)?;
        print_report(name, *id, &volute, &units);
        warned |= !volute.warnings.is_empty();
        if let Some(dir) = &args.export {
            let dir = if volutes.len() > 1 {
                dir.join(format!("volute-{}", id.0))
            } else {
                dir.clone()
            };
            export(&dir, &volute)?;
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

fn export(dir: &Path, g: &VoluteGeometry) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let files = [
        ("volute.txt", tp_io::points::volute_txt(g)),
        (
            "volute.stl",
            tp_io::stl::ascii("volute", &tp_io::mesh::volute_triangles(g)),
        ),
    ];
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("Wrote {}", path.display());
    }
    Ok(())
}

fn parse_shape(value: &str) -> Result<SectionShape, String> {
    match value {
        "rectangle" => Ok(SectionShape::Rectangular),
        "circle" => Ok(SectionShape::Circular),
        "trapezoid" => Ok(SectionShape::default()),
        _ => value
            .strip_prefix("trapezoid:")
            .and_then(|d| d.trim().parse::<f64>().ok())
            .map(|degrees| SectionShape::Trapezoidal {
                flank_angle: degrees.to_radians(),
            })
            .ok_or_else(|| {
                format!("shape must be rectangle, circle or trapezoid:DEGREES, got {value:?}")
            }),
    }
}

fn parse_discharge(value: &str) -> Result<DischargeShape, String> {
    let wall = |w| DischargeShape::Tangential { straight_wall: w };
    match value {
        "tangential" | "tangential:inner" => Ok(wall(StraightWall::Inner)),
        "tangential:centered" => Ok(wall(StraightWall::Centered)),
        "tangential:outer" => Ok(wall(StraightWall::Outer)),
        "radial" => Ok(DischargeShape::Radial { axis_angle: 0.0 }),
        _ => value
            .strip_prefix("radial:")
            .and_then(|d| d.trim().parse::<f64>().ok())
            .map(|degrees| DischargeShape::Radial {
                axis_angle: degrees.to_radians(),
            })
            .ok_or_else(|| {
                format!(
                    "discharge must be tangential[:inner|centered|outer] or \
                     radial[:DEGREES], got {value:?}"
                )
            }),
    }
}

/// Applies one `KEY=VALUE` override to the volute inputs.
fn apply(input: &mut VoluteInput, units: &DisplayUnits, setting: &str) -> Result<(), String> {
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
    use QuantityKind::{Angle, Dimensionless, Length};
    match key {
        "flow-factor" => input.setup.flow_factor = quantity(Dimensionless)?,
        "dz" => input.setup.cutwater_diameter = quantity(Length)?,
        "b3" => input.setup.inlet_width = quantity(Length)?,
        "e3" => input.setup.cutwater_thickness = quantity(Length)?,
        "shape" => input.cross_section.shape = parse_shape(value)?,
        "law" => {
            input.spiral.law = match value {
                "auto" => AreaLaw::Auto,
                "momentum" => AreaLaw::AngularMomentum,
                "velocity" => AreaLaw::ConstantVelocity,
                _ => {
                    return Err(format!(
                        "law must be auto, momentum or velocity, got {value:?}"
                    ));
                }
            }
        }
        "sections" => {
            input.spiral.sections = if auto {
                Param::Auto
            } else {
                Param::Manual(
                    value
                        .parse()
                        .map_err(|_| format!("sections must be a whole number, got {value:?}"))?,
                )
            }
        }
        "correction" => {
            input.spiral.cutwater_correction = match value {
                "on" => true,
                "off" => false,
                _ => return Err(format!("correction must be on or off, got {value:?}")),
            }
        }
        "length" => input.diffuser.length = quantity(Length)?,
        "outlet" => input.diffuser.outlet_diameter = quantity(Length)?,
        "incidence" => input.cutwater.incidence = quantity(Angle)?,
        "wall" => input.wall.thickness = quantity(Length)?,
        "type" => {
            input.setup.kind = match value {
                "single" => VoluteKind::Single,
                "double" => VoluteKind::Double,
                _ => return Err(format!("type must be single or double, got {value:?}")),
            }
        }
        "rib_wrap" => input.setup.rib_wrap = quantity(Angle)?,
        "rib" => input.setup.rib_thickness = quantity(Length)?,
        "channel_ratio" => input.setup.channel_area_ratio = quantity(Dimensionless)?,
        "discharge" => input.diffuser.shape = parse_discharge(value)?,
        "bend" => input.diffuser.bend_radius = quantity(Length)?,
        "cutwater" => {
            input.cutwater.shape = match value {
                "tangent" => CutwaterShape::Tangent,
                "sharp" => CutwaterShape::Sharp,
                "rounded" => CutwaterShape::Rounded {
                    radius: match input.cutwater.shape {
                        CutwaterShape::Rounded { radius } => radius,
                        _ => Param::Auto,
                    },
                },
                _ => {
                    return Err(format!(
                        "cutwater must be tangent, sharp or rounded, got {value:?}"
                    ));
                }
            }
        }
        "nose" => {
            input.cutwater.shape = CutwaterShape::Rounded {
                radius: quantity(Length)?,
            }
        }
        _ => {
            return Err(format!(
                "unknown key {key:?}; valid keys: flow-factor, dz, b3, e3, shape, law, \
                 sections, correction, length, outlet, incidence, wall, type, rib_wrap, rib, \
                 channel_ratio, discharge, bend, cutwater, nose"
            ));
        }
    }
    Ok(())
}

fn print_report(name: &str, id: ComponentId, g: &VoluteGeometry, units: &DisplayUnits) {
    use QuantityKind::*;
    let s = |kind, value| show(units, kind, value);
    let d = &g.design;
    let b = &g.basis;
    println!("{name} ({id}): volute");
    println!();
    let shape = match d.shape {
        SectionShape::Rectangular => "rectangular".to_owned(),
        SectionShape::Circular => "circular".to_owned(),
        SectionShape::Trapezoidal { flank_angle } => {
            format!("trapezoidal, flanks at {}", s(Angle, flank_angle))
        }
    };
    let law = match d.law {
        AreaLaw::ConstantVelocity => "constant velocity (Gülich eqs. 7.26–7.27)",
        _ => "conservation of angular momentum (Gülich eqs. 7.24–7.25)",
    };
    println!(
        "Inlet      Q_Le {}   c2u {}   d2 {}",
        s(VolumeFlow, d.flow),
        s(Velocity, b.cu),
        s(Length, b.impeller_diameter)
    );
    println!(
        "Cutwater   dz {} (min {}, Table 10.2)   e3 {}   b3 {}",
        s(Length, d.cutwater_diameter),
        s(Length, d.min_cutwater_diameter),
        s(Length, d.cutwater_thickness),
        s(Length, d.inlet_width)
    );
    println!(
        "           flow angle α3' {}   cutwater angle α3B {}",
        s(Angle, d.cutwater_flow_angle),
        s(Angle, d.cutwater_angle)
    );
    let c = &g.cutwater;
    match d.cutwater_shape {
        CutwaterShape::Tangent => {
            println!("           tangent cutwater: the spiral starts where it clears the diffuser")
        }
        shape => println!(
            "           {} cutwater: tip at {} ({} from the axis), wedge {}{}   nearest to the axis {}",
            if matches!(shape, CutwaterShape::Sharp) {
                "sharp"
            } else {
                "rounded"
            },
            s(Angle, c.angle),
            s(Length, c.tip.truncate().length()),
            s(Angle, c.wedge),
            if c.radius > 0.0 {
                format!(", nose radius {}", s(Length, c.radius))
            } else {
                String::new()
            },
            s(Length, c.nose_distance)
        ),
    }
    println!("Sections   {shape}; {law}");
    println!(
        "Throat     area {}   height {}   outer radius {}   correction Δa3 {}",
        s(Area, d.throat.area),
        s(Length, d.throat.height),
        s(Length, d.throat.outer_radius),
        s(Length, d.throat_correction)
    );
    if let Some(rib) = &d.rib {
        println!(
            "Double     inner volute wraps {} from the rib nose at {}   rib {}",
            s(Angle, rib.wrap),
            s(Angle, rib.start),
            s(Length, rib.thickness)
        );
        println!(
            "           first throat {} at the rib nose   outer channel {} → {}",
            s(Area, rib.outer_throat.area),
            s(Area, rib.channel_inlet_area),
            s(Area, rib.channel_outlet_area)
        );
    }
    println!(
        "           c3q {}   c3q/c2 {:.2} (not much below 0.8, Table 10.2)   B/H {:.2} (2 to 3)",
        s(Velocity, d.throat_velocity),
        d.deceleration,
        d.throat_aspect
    );
    let dif = &d.diffuser;
    let discharge = match dif.shape {
        DischargeShape::Tangential { straight_wall } => format!(
            "tangential, {} wall straight",
            match straight_wall {
                StraightWall::Inner => "inner",
                StraightWall::Centered => "no",
                StraightWall::Outer => "outer",
            }
        ),
        DischargeShape::Radial { axis_angle } => format!(
            "radial, bend R_N {} (Gülich eq. 7.29) turning {}, axis {} from the line through the pump axis",
            s(Length, dif.bend_radius),
            s(Angle, g.nozzle_bend),
            s(Angle, axis_angle)
        ),
    };
    println!("Diffuser   {discharge}");
    println!(
        "           length {} (centerline {})   outlet {}   area ratio {:.2}   c_d {}",
        s(Length, dif.length),
        s(Length, g.nozzle_length),
        s(Length, dif.outlet_diameter),
        dif.area_ratio,
        s(Velocity, dif.outlet_velocity)
    );
    println!(
        "           opening {} (allowed {}, Gülich eq. 1.44)   c_p,id {:.2}",
        s(Angle, dif.opening_angle),
        s(Angle, dif.allowable_angle),
        dif.ideal_pressure_recovery
    );
    println!(
        "Spiral     {} sections from {} to {}   wall {}",
        g.sections.len(),
        s(Angle, g.start_angle),
        s(Angle, d.rib.map_or(std::f64::consts::TAU, |r| r.start)),
        s(Length, d.wall_thickness)
    );
    if d.rib.is_some() {
        println!(
            "           inner volute {} sections, outer channel {} sections",
            g.inner_sections.len(),
            g.outer_channel.len()
        );
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
