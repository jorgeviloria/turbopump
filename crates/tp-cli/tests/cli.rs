//! End-to-end tests of the `turbopump` binary.

use std::path::PathBuf;
use std::process::{Command, Output};

fn turbopump(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_turbopump"))
        .args(args)
        // Deterministic results: the built-in water model, never a CoolProp that the
        // test environment happens to expose.
        .env("TURBOPUMP_COOLPROP", "none")
        .output()
        .expect("binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tp-cli-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn nq_of_reference_pump() {
    let out = turbopump(&[
        "nq", "--flow", "100 m3/h", "--head", "32 m", "--speed", "2900 rpm",
    ]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("35.92"), "{}", stdout(&out));
}

#[test]
fn nq_double_suction_halves_the_flow() {
    let single = turbopump(&[
        "nq", "--flow", "200 m3/h", "--head", "32 m", "--speed", "2900 rpm",
    ]);
    let double = turbopump(&[
        "nq", "--flow", "200 m3/h", "--head", "32 m", "--speed", "2900 rpm", "--eyes", "2",
    ]);
    assert!(stdout(&single).contains("50.80"), "{}", stdout(&single));
    assert!(stdout(&double).contains("35.92"), "{}", stdout(&double));
}

#[test]
fn convert_units() {
    let out = turbopump(&["convert", "150", "psi", "bar"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("10.34"), "{}", stdout(&out));

    let negative = turbopump(&["convert", "-40", "°F", "°C"]);
    assert!(
        stdout(&negative).contains("= -40.00 °C"),
        "{}",
        stdout(&negative)
    );

    let wrong = turbopump(&["convert", "1", "bar", "rpm"]);
    assert_eq!(wrong.status.code(), Some(2));
}

#[test]
fn new_then_info() {
    let dir = scratch_dir("new");
    let file = dir.join("pump.tpj");
    let file_arg = file.to_str().unwrap();

    let created = turbopump(&[
        "new",
        file_arg,
        "--machine",
        "pump",
        "--flow",
        "100 m3/h",
        "--head",
        "32 m",
        "--speed",
        "2900 rpm",
        "--components",
        "impeller,volute",
    ]);
    assert_eq!(created.status.code(), Some(0), "{}", stdout(&created));
    assert!(file.exists());

    let again = turbopump(&[
        "new",
        file_arg,
        "--machine",
        "pump",
        "--flow",
        "1 m3/h",
        "--head",
        "1 m",
        "--speed",
        "1 rpm",
    ]);
    assert_eq!(again.status.code(), Some(2), "refuses to overwrite");

    let us = turbopump(&["info", file_arg, "--units", "us"]);
    let text = stdout(&us);
    assert_eq!(us.status.code(), Some(0), "{text}");
    assert!(text.contains("440.3 gpm"), "{text}");
    assert!(text.contains("105.0 ft"), "{text}");
    assert!(text.contains("Volute 1"), "{text}");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn invalid_design_point_is_an_error() {
    let dir = scratch_dir("invalid");
    let file = dir.join("fan.tpj");
    let out = turbopump(&[
        "new",
        file.to_str().unwrap(),
        "--machine",
        "fan",
        "--flow",
        "2 m3/s",
        "--head",
        "10 m",
        "--speed",
        "1450 rpm",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot be specified by head"));
    assert!(!file.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn project_without_impeller_ends_with_warnings() {
    let dir = scratch_dir("warn");
    let file = dir.join("casing.tpj");
    let out = turbopump(&[
        "new",
        file.to_str().unwrap(),
        "--machine",
        "pump",
        "--flow",
        "100 m3/h",
        "--head",
        "32 m",
        "--speed",
        "2900 rpm",
        "--components",
        "volute",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("no impeller"));
    std::fs::remove_dir_all(&dir).unwrap();
}

fn reference_project(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch_dir(name);
    let file = dir.join("pump.tpj");
    let out = turbopump(&[
        "new",
        file.to_str().unwrap(),
        "--machine",
        "pump",
        "--flow",
        "100 m3/h",
        "--head",
        "32 m",
        "--speed",
        "2900 rpm",
        "--components",
        "impeller,volute",
    ]);
    assert_eq!(out.status.code(), Some(0));
    (dir, file)
}

#[test]
fn design_reference_pump() {
    let (dir, file) = reference_project("design");
    let file_arg = file.to_str().unwrap();
    let out = turbopump(&["design", file_arg]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("nq 35.92"), "{text}");
    assert!(text.contains("Outlet diameter d2   172.2 mm"), "{text}");
    assert!(text.contains("built-in water model"), "{text}");

    let info = stdout(&turbopump(&["info", file_arg]));
    assert!(info.contains("next: meridional contour"), "{info}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn design_overrides_are_saved() {
    let (dir, file) = reference_project("overrides");
    let file_arg = file.to_str().unwrap();
    let out = turbopump(&["design", file_arg, "--set", "d2=180", "--set", "z=6"]);
    let text = stdout(&out);
    assert!(text.contains("180.0 mm  (manual)"), "{text}");
    assert!(text.contains("Blades z             6  (manual)"), "{text}");

    // A second run keeps the overrides; `auto` releases one of them.
    let again = stdout(&turbopump(&["design", file_arg, "--set", "z=auto"]));
    assert!(again.contains("180.0 mm  (manual)"), "{again}");
    assert!(
        !again.contains("Blades z             6  (manual)"),
        "{again}"
    );

    let bad = turbopump(&["design", file_arg, "--set", "colour=blue"]);
    assert_eq!(bad.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn fluid_properties_without_coolprop() {
    let water = turbopump(&["fluid", "Water", "--temperature", "60 °C"]);
    assert_eq!(water.status.code(), Some(0));
    assert!(stdout(&water).contains("983.2 kg/m³"), "{}", stdout(&water));

    let ethanol = turbopump(&["fluid", "Ethanol"]);
    assert_eq!(ethanol.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&ethanol.stderr).contains("TURBOPUMP_COOLPROP"));
}

#[test]
fn fluid_properties_with_coolprop() {
    let out = Command::new(env!("CARGO_BIN_EXE_turbopump"))
        .args(["fluid", "Ethanol"])
        .env("TURBOPUMP_COOLPROP", coolprop_sys::COOLPROP_PATH)
        .output()
        .unwrap();
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("(CoolProp)") && text.contains("789"),
        "{text}"
    );
}

#[test]
fn geometry_needs_main_dimensions_first() {
    let (dir, file) = reference_project("geometry-order");
    let out = turbopump(&["geometry", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("main dimensions"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn geometry_exports_points_mesh_and_drawings() {
    let (dir, file) = reference_project("geometry-export");
    let file_arg = file.to_str().unwrap();
    assert_eq!(turbopump(&["design", file_arg]).status.code(), Some(0));
    let export = dir.join("out");
    let out = turbopump(&["geometry", file_arg, "--export", export.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("Blade sections (7 blades"), "{text}");

    let read = |name: &str| std::fs::read_to_string(export.join(name)).unwrap();
    assert!(read("impeller.txt").contains("[section span=1.0000]"));
    assert!(read("impeller.stl").starts_with("solid impeller"));
    assert!(read("meridional.svg").trim_end().ends_with("</svg>"));
    assert!(read("front.svg").contains("rotation clockwise"));

    let info = stdout(&turbopump(&["info", file_arg]));
    assert!(info.contains("next: CFD setup"), "{info}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn geometry_overrides() {
    let (dir, file) = reference_project("geometry-set");
    let file_arg = file.to_str().unwrap();
    turbopump(&["design", file_arg]);
    let out = turbopump(&[
        "geometry",
        file_arg,
        "--set",
        "spans=3",
        "--set",
        "le=round",
        "--set",
        "beta=smooth:0.2,0.3",
        "--set",
        "le-hub=0.3",
    ]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let rows = text
        .lines()
        .filter(|l| l.trim_start().starts_with("0."))
        .count()
        + text
            .lines()
            .filter(|l| l.trim_start().starts_with("1.00"))
            .count();
    assert_eq!(rows, 3, "{text}");

    let bad = turbopump(&["geometry", file_arg, "--set", "le=pointy"]);
    assert_eq!(bad.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn design_reports_verified_checks() {
    let (dir, file) = reference_project("checks");
    let file_arg = file.to_str().unwrap();
    let text = stdout(&turbopump(&["design", file_arg]));
    assert!(text.contains("η 0.814"), "{text}");
    assert!(
        text.contains("η ± 0.037 (single-stage pump correlation"),
        "{text}"
    );
    assert!(text.contains("Eye diameter d1      77.18 mm"), "{text}");
    assert!(text.contains("δ2 = β2B − β2 8.789 °"), "{text}");

    // A suction impeller gets a larger eye (f_d1 + 0.1).
    let suction = stdout(&turbopump(&["design", file_arg, "--set", "inlet=suction"]));
    assert!(
        suction.contains("Eye diameter d1      84.42 mm"),
        "{suction}"
    );

    let geometry = stdout(&turbopump(&["geometry", file_arg, "--set", "le=ellipse:2"]));
    assert!(geometry.contains("Blade loading  ξ_eff"), "{geometry}");
    assert!(
        geometry.contains("ellipse 2:1, c_p,min at shockless entry 0.89"),
        "{geometry}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn volute_designs_and_exports() {
    let (dir, file) = reference_project("volute");
    let file_arg = file.to_str().unwrap();
    let out_dir = dir.join("volute");
    let out = turbopump(&["volute", file_arg, "--export", out_dir.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("min 193.3 mm, Table 10.2"), "{text}");
    assert!(text.contains("conservation of angular momentum"), "{text}");
    for name in ["volute.txt", "volute.stl"] {
        assert!(std::fs::metadata(out_dir.join(name)).unwrap().len() > 10_000);
    }
    let info = stdout(&turbopump(&["info", file_arg]));
    assert!(info.contains("next: CFD setup"), "{info}");

    let manual = stdout(&turbopump(&[
        "volute",
        file_arg,
        "--set",
        "shape=circle",
        "--set",
        "law=velocity",
        "--dry-run",
    ]));
    assert!(manual.contains("circular; constant velocity"), "{manual}");
    // A double volute with an elliptic rib nose, exported with its rib.
    let double_dir = dir.join("double");
    let double = stdout(&turbopump(&[
        "volute",
        file_arg,
        "--set",
        "type=double",
        "--set",
        "rib_nose=2",
        "--dry-run",
        "--export",
        double_dir.to_str().unwrap(),
    ]));
    assert!(double.contains("elliptic 2.00:1 nose and tail"), "{double}");
    let points = std::fs::read_to_string(double_dir.join("volute.txt")).unwrap();
    assert!(points.contains("[rib outline thickness="), "{points}");
    let bad = turbopump(&["volute", file_arg, "--set", "shape=hexagon"]);
    assert_eq!(bad.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn diffuser_designs_vaned_and_vaneless_stators() {
    let dir = scratch_dir("diffuser");
    let file = dir.join("multistage.tpj");
    let file_arg = file.to_str().unwrap();
    let out = turbopump(&[
        "new",
        file_arg,
        "--machine",
        "pump",
        "--flow",
        "100 m3/h",
        "--head",
        "64 m",
        "--speed",
        "2900 rpm",
        "--components",
        "impeller,stator,impeller,vaneless-stator",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let out = turbopump(&["diffuser", file_arg]);
    let text = stdout(&out);
    assert!(text.contains("vaned diffuser"), "{text}");
    assert!(text.contains("vaneless diffuser"), "{text}");
    assert!(text.contains("Criteria   A"), "{text}");
    let manual = stdout(&turbopump(&[
        "diffuser",
        file_arg,
        "--component",
        "2",
        "--set",
        "z=11",
        "--dry-run",
    ]));
    assert!(manual.contains("Vanes      z 11"), "{manual}");
    let bad = turbopump(&["diffuser", file_arg, "--set", "colour=blue"]);
    assert_eq!(bad.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(feature = "cad")]
#[test]
fn cad_exports_impeller_and_flow_domain() {
    let (dir, file) = reference_project("cad");
    let file_arg = file.to_str().unwrap();

    let early = turbopump(&["cad", file_arg]);
    assert_eq!(early.status.code(), Some(2), "needs the geometry first");
    assert!(String::from_utf8_lossy(&early.stderr).contains("turbopump geometry"));

    assert_eq!(turbopump(&["design", file_arg]).status.code(), Some(0));
    assert_eq!(turbopump(&["geometry", file_arg]).status.code(), Some(0));
    let out_dir = dir.join("cad");
    let out = turbopump(&[
        "cad",
        file_arg,
        "--out",
        out_dir.to_str().unwrap(),
        "--format",
        "step,stl",
    ]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("Solids           valid"), "{text}");
    // OCCT's transfer statistics are silenced.
    assert!(!text.contains("Statistics on Transfer"), "{text}");
    for name in ["impeller.step", "fluid.step", "impeller.stl", "fluid.stl"] {
        let size = std::fs::metadata(out_dir.join(name)).unwrap().len();
        assert!(size > 100_000, "{name}: {size} bytes");
    }
    // The volute is reported but skipped until it is designed.
    assert!(text.contains("not designed yet"), "{text}");
    assert_eq!(turbopump(&["volute", file_arg]).status.code(), Some(0));
    let again = turbopump(&[
        "cad",
        file_arg,
        "--out",
        out_dir.to_str().unwrap(),
        "--format",
        "step",
    ]);
    assert_eq!(again.status.code(), Some(0), "{}", stdout(&again));
    let volute = std::fs::metadata(out_dir.join("volute.step"))
        .unwrap()
        .len();
    assert!(volute > 100_000, "volute.step: {volute} bytes");

    let step = std::fs::read_to_string(out_dir.join("impeller.step")).unwrap();
    assert!(step.starts_with("ISO-10303-21;"));
    assert!(
        step.contains("B_SPLINE_SURFACE"),
        "blades are B-spline surfaces"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn performance_predicts_the_characteristic() {
    let (dir, file) = reference_project("performance");
    let file_arg = file.to_str().unwrap();
    let out = turbopump(&["performance", file_arg]);
    let text = stdout(&out);
    // With 10.1 m available, NPSH₃ passes it near the end of the curve: a warning.
    assert_eq!(out.status.code(), Some(1), "{text}");
    // The seals leak less than the statistics assume, so the impeller gives a little
    // more head; the loss analysis puts η above the statistics.
    assert!(
        text.contains("Design     Q 100.0 m³/h   H 32.32 m"),
        "{text}"
    );
    assert!(
        text.contains("Losses     η 0.856 by losses (statistics 0.814"),
        "{text}"
    );
    // The hydraulic losses summed by Gülich Table 3.8 land near the statistical η_h.
    assert!(text.contains("η_h 0.909 (statistics 0.907)"), "{text}");
    assert!(text.contains("Shut-off   H"), "{text}");
    assert!(
        text.contains("Axial      ") && text.contains("at design toward the suction side"),
        "{text}"
    );
    assert!(text.contains("operating point Q 100.4 m³/h"), "{text}");
    // An oil shifts the water curve (Gülich Chap. 13.1).
    let oil = stdout(&turbopump(&[
        "performance",
        file_arg,
        "--set",
        "viscosity=200 cSt",
    ]));
    assert!(oil.contains("Liquid     ν 200.0 mm²/s"), "{oil}");
    assert!(oil.contains("f_η 0.823 by loss analysis"), "{oil}");
    assert!(text.contains("NPSH3      "), "{text}");
    assert!(text.contains("Volute     loses"), "{text}");
    assert!(text.contains("Radial     "), "{text}");
    assert!(text.contains("NPSH₃ exceeds the NPSH available"), "{text}");
    // CSV of the curve, with a manual collector and range.
    let csv = dir.join("curve.csv");
    let out = turbopump(&[
        "performance",
        file_arg,
        "--set",
        "collector=diffuser",
        "--set",
        "max=1.3",
        "--set",
        "points=11",
        "--csv",
        csv.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let rows = std::fs::read_to_string(&csv).unwrap();
    // Header, 11 points and the onset of recirculation.
    assert_eq!(rows.lines().count(), 13, "{rows}");
    assert!(rows.starts_with("flow_m3_s,head_m"));
    assert!(
        rows.lines()
            .next()
            .unwrap()
            .ends_with("npsh3_m,volute_loss_m,radial_force_n,axial_force_n")
    );
    let bad = turbopump(&["performance", file_arg, "--set", "onset=7"]);
    assert_eq!(bad.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn analysis_reports_potential_flow_and_blade_velocities() {
    let (dir, file) = reference_project("analysis");
    let file_arg = file.to_str().unwrap();
    let csv = dir.join("analysis");
    let out = turbopump(&["analysis", file_arg, "--csv", csv.to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("Potential flow   mesh"), "{text}");
    assert!(text.contains("Blade faces"), "{text}");
    let walls = std::fs::read_to_string(csv.join("potential-walls.csv")).unwrap();
    assert!(walls.starts_with("fraction,cm_hub_m_s"));
    assert!(walls.lines().count() > 50);
    let faces = std::fs::read_to_string(csv.join("blade-velocities.csv")).unwrap();
    assert!(faces.lines().count() > 100);
    std::fs::remove_dir_all(&dir).unwrap();
}
