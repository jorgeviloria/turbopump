//! End-to-end tests of the `turbopump` binary.

use std::path::PathBuf;
use std::process::{Command, Output};

fn turbopump(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_turbopump"))
        .args(args)
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
