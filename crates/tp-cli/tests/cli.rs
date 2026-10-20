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
