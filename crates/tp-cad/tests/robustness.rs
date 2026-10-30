//! Robustness matrix of the volute CAD: one design per run, chosen by `TP_CASE`, so a
//! driver can time each out on its own. `TP_CASE=list` prints the cases. Each run
//! prints one `ROW` line of semicolon-separated fields.

use std::time::Instant;

use tp_design::Param;
use tp_design::outlet::ImpellerOutlet;
use tp_design::volute::*;

fn basis() -> ImpellerOutlet {
    ImpellerOutlet {
        flow: 100.0 / 3600.0,
        impeller_diameter: 0.172,
        impeller_width: 0.0174,
        cu: 13.27,
        cm: 3.03,
        nq: 35.9,
        stage_head: 32.0,
        density: 998.2,
        midplane: 0.0378,
        rotation_sign: 1.0,
        impeller_blades: 7,
        sides: None,
    }
}

type Edit = Box<dyn Fn(&mut VoluteInput)>;
type Choice = (&'static str, fn(&mut VoluteInput));

fn cases() -> Vec<(String, Edit)> {
    let sections: [Choice; 3] = [
        ("trapezoid", |_| {}),
        ("round", |v| {
            v.cross_section = CrossSectionInput::single(SectionShape::round())
        }),
        ("rectangular", |v| {
            v.cross_section = CrossSectionInput::single(SectionShape::rectangular())
        }),
    ];
    let diffusers: [Choice; 3] = [
        ("radial", |_| {}),
        ("tangential", |v| {
            v.diffuser.shape = DischargeShape::Tangential {
                straight_wall: StraightWall::Inner,
            }
        }),
        ("eccentric", |v| {
            v.diffuser.shape = DischargeShape::Tangential {
                straight_wall: StraightWall::Centered,
            };
            v.diffuser.eccentricity = Param::Manual(-0.5);
        }),
    ];
    let shapes: [(&str, CutwaterShape); 3] = [
        ("sharp", CutwaterShape::Sharp),
        (
            "fillet",
            CutwaterShape::Fillet {
                radius: Param::Manual(0.0015),
            },
        ),
        (
            "simple",
            CutwaterShape::Simple {
                cut: Param::Auto,
                handle: Param::Auto,
                measure: CutMeasure::Along,
            },
        ),
    ];
    let mut out: Vec<(String, Edit)> = Vec::new();
    for (sn, se) in sections {
        for (dn, de) in diffusers {
            for (cn, shape) in shapes {
                for base in [0.0, 0.5, 1.0] {
                    out.push((
                        format!("{sn} {dn} {cn} base{base}"),
                        Box::new(move |v: &mut VoluteInput| {
                            se(v);
                            de(v);
                            v.cutwater.shape = shape;
                            v.diffuser.base_form = Param::Manual(base);
                        }),
                    ));
                }
            }
        }
    }
    let extras: [Choice; 6] = [
        ("wrap330", |v| {
            v.spiral.wrap = Param::Manual(330f64.to_radians())
        }),
        ("compensation", |v| v.spiral.compensation.enabled = true),
        ("double", |v| v.setup.kind = VoluteKind::Double),
        ("stepanoff arcs", |v| {
            v.spiral.law = AreaLaw::Stepanoff;
            v.spiral.arcs.enabled = true;
        }),
        ("oblique15", |v| {
            v.cutwater.obliquity = Param::Manual(15f64.to_radians())
        }),
        ("tangent", |v| v.cutwater.shape = CutwaterShape::Tangent),
    ];
    for (en, ee) in extras {
        for (cn, shape) in [shapes[0], shapes[1]] {
            if en == "tangent" && cn == "fillet" {
                continue;
            }
            out.push((
                format!("trapezoid radial {en} {cn}"),
                Box::new(move |v: &mut VoluteInput| {
                    v.cutwater.shape = shape;
                    ee(v);
                }),
            ));
        }
    }
    out
}

fn short(e: impl ToString) -> String {
    e.to_string()
        .replace([';', '\n'], ",")
        .chars()
        .take(110)
        .collect()
}

/// The number of problems OCCT's checker reports.
fn defects(shape: &tp_cad::Shape) -> (usize, usize) {
    (0, shape.check_report().lines().count())
}

#[test]
#[ignore = "robustness matrix: run through the driver"]
fn matrix() {
    let all = cases();
    let which = std::env::var("TP_CASE").unwrap_or_default();
    if which == "list" {
        for (i, (name, _)) in all.iter().enumerate() {
            println!("CASE {i};{name}");
        }
        return;
    }
    let i: usize = which.parse().expect("TP_CASE=<index>|list");
    let (name, edit) = &all[i];
    let mut input = VoluteInput::default();
    edit(&mut input);
    let mut row = vec![i.to_string(), name.clone()];
    let g = match VoluteGeometry::build(&basis(), &input) {
        Ok(g) => g,
        Err(e) => {
            row.push(format!("design error: {}", short(e)));
            println!("ROW {}", row.join(";"));
            return;
        }
    };
    row.push("design ok".into());
    row.push(format!("start {:.1}", g.start_angle.to_degrees()));
    row.push(format!("tip {:.1}", g.cutwater.angle.to_degrees()));
    row.push(format!("wedge {:.1}", g.cutwater.wedge.to_degrees()));
    row.push(format!(
        "misalign {:.1}",
        g.wall_misalignment().to_degrees()
    ));
    row.push(format!("warnings {}", g.warnings.len()));
    let expected = tp_cad::expected_fluid_volume(&g) * 1e9;

    let t = Instant::now();
    let boolean = tp_cad::build_volute(&g);
    let tb = t.elapsed().as_secs_f64();
    let vb = match &boolean {
        Ok(c) => {
            let (bare, issues) = defects(&c.fluid);
            row.push(format!(
                "bool valid {} solids {} {tb:.1}s bare {bare} issues {issues} warn {}",
                c.fluid.is_valid(),
                c.fluid.solid_count(),
                c.warnings.len()
            ));
            row.push(format!(
                "bool/expected {:+.1}%",
                (c.fluid.volume() / expected - 1.0) * 100.0
            ));
            Some(c.fluid.volume())
        }
        Err(e) => {
            row.push(format!("bool FAIL {tb:.1}s {}", short(e)));
            None
        }
    };

    let t = Instant::now();
    let flow = tp_cad::build_volute_flow(&g);
    let tf = t.elapsed().as_secs_f64();
    match &flow {
        Ok((c, how)) => {
            let (bare, issues) = defects(&c.fluid);
            let dv = vb.map_or(f64::NAN, |v| (c.fluid.volume() / v - 1.0) * 100.0);
            row.push(format!(
                "flow/expected {:+.1}%",
                (c.fluid.volume() / expected - 1.0) * 100.0
            ));
            row.push(format!(
                "flow {how:?} valid {} solids {} {tf:.1}s dvol {dv:+.3}% bare {bare} issues {issues}",
                c.fluid.is_valid(),
                c.fluid.solid_count()
            ));
        }
        Err(e) => row.push(format!("flow FAIL {tf:.1}s {}", short(e))),
    }

    if let Ok((c, _)) = &flow {
        let mut bare = g.clone();
        bare.design.flange = None;
        let t = Instant::now();
        let casing = tp_cad::build_volute_casing(&bare, c);
        let tc = t.elapsed().as_secs_f64();
        row.push(match casing {
            Ok(k) => format!(
                "casing valid {} solids {} {tc:.1}s warn {}",
                k.solid.is_valid(),
                k.solid.solid_count(),
                k.warnings.len()
            ),
            Err(e) => format!("casing FAIL {tc:.1}s {}", short(e)),
        });
    }
    println!("ROW {}", row.join(";"));
}
