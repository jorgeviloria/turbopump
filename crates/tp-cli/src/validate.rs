//! `turbopump validate`: comparison of a project's results with the reference values of
//! a targets file.

use std::path::PathBuf;

use clap::Args;
use tp_project::Project;
use tp_project::validation::{QUANTITIES, Report, Targets, Tolerance, Verdict, validate};
use tp_units::decimals_for;

use crate::{CliResult, Outcome};

#[derive(Args)]
pub(crate) struct ValidateArgs {
    /// Project file (.tpj). It is only read.
    #[arg(required_unless_present = "list")]
    project: Option<PathBuf>,
    /// Targets file (JSON) with the reference values and their tolerances.
    #[arg(required_unless_present = "list")]
    targets: Option<PathBuf>,
    /// List the quantities a targets file can refer to, and exit.
    #[arg(long)]
    list: bool,
}

pub(crate) fn run(args: ValidateArgs) -> CliResult {
    if args.list {
        list();
        return Ok(Outcome::Ok);
    }
    let (Some(project), Some(targets)) = (args.project, args.targets) else {
        return Err("give a project and a targets file".into());
    };
    let p = Project::load(&project).map_err(|e| format!("{}: {e}", project.display()))?;
    let text =
        std::fs::read_to_string(&targets).map_err(|e| format!("{}: {e}", targets.display()))?;
    let t = Targets::from_json(&text).map_err(|e| format!("{}: {e}", targets.display()))?;
    let report = validate(&p, &t);
    print_report(&t, &report);
    if report.passed() {
        Ok(Outcome::Ok)
    } else {
        let out = report.count(Verdict::Fail) + report.count(Verdict::Error);
        Err(format!("{out} target(s) not met"))
    }
}

fn list() {
    let width = QUANTITIES.iter().map(|q| q.id.len()).max().unwrap_or(0) + 3;
    println!("{:width$}{:16}description", "id", "kind");
    for q in QUANTITIES {
        let id = if q.per_span {
            format!("{}@span", q.id)
        } else {
            q.id.to_string()
        };
        println!("{id:width$}{:16}{}", q.kind.name(), q.description);
    }
    println!();
    println!("A span is hub, mid, shroud or a fraction 0–1 matching a blade section.");
}

/// A number with about four significant digits.
fn number(x: f64) -> String {
    format!("{x:.*}", decimals_for(x))
}

fn with_unit(text: String, unit: &str) -> String {
    if unit == "-" {
        text
    } else {
        format!("{text} {unit}")
    }
}

fn print_report(targets: &Targets, report: &Report) {
    let title = if targets.case.is_empty() {
        "Validation".to_string()
    } else {
        format!("Validation of {}", targets.case)
    };
    println!("{title}");
    if !targets.description.is_empty() {
        println!("{}", targets.description);
    }
    println!();

    let mut lines = vec![[
        "quantity".to_string(),
        "reference".into(),
        "computed".into(),
        "Δ %".into(),
        "tolerance".into(),
        "verdict".into(),
    ]];
    for row in &report.rows {
        let t = &row.target;
        let reference = match (t.value, t.band) {
            (_, Some([lo, hi])) => with_unit(format!("{}–{}", number(lo), number(hi)), row.unit),
            (Some(v), None) => with_unit(number(v), row.unit),
            (None, None) => String::new(),
        };
        let computed = row
            .computed
            .map(|x| with_unit(number(x), row.unit))
            .unwrap_or_else(|| "—".into());
        let difference = row
            .difference_percent
            .map(|d| format!("{d:+.1}"))
            .unwrap_or_default();
        let tolerance = match (t.band, t.tolerance) {
            (Some(_), _) => "band".to_string(),
            (None, Some(Tolerance::Relative(p))) => format!("±{p} %"),
            (None, Some(Tolerance::Absolute(a))) if row.unit == "%" => {
                format!("±{a} pt")
            }
            (None, Some(Tolerance::Absolute(a))) => with_unit(format!("±{a}"), row.unit),
            (None, None) => "—".into(),
        };
        let verdict = match row.verdict {
            Verdict::Pass => "ok",
            Verdict::Fail => "OUT",
            Verdict::Info => "info",
            Verdict::Error => "error",
        };
        lines.push([
            t.id.clone(),
            reference,
            computed,
            difference,
            tolerance,
            verdict.into(),
        ]);
    }
    let mut widths = [0; 6];
    for line in &lines {
        for (w, cell) in widths.iter_mut().zip(line) {
            *w = (*w).max(cell.chars().count());
        }
    }
    for line in &lines {
        let mut out = String::new();
        for (i, (cell, w)) in line.iter().zip(widths).enumerate() {
            let pad = w - cell.chars().count();
            // The quantity is left-aligned, the numbers right-aligned.
            if i == 0 || i == 5 {
                out.push_str(cell);
                out.push_str(&" ".repeat(pad));
            } else {
                out.push_str(&" ".repeat(pad));
                out.push_str(cell);
            }
            out.push_str("   ");
        }
        println!("{}", out.trim_end());
    }

    let errors: Vec<_> = report.rows.iter().filter(|r| r.error.is_some()).collect();
    if !errors.is_empty() {
        println!();
        for row in errors {
            println!(
                "  {}: {}",
                row.target.id,
                row.error.as_deref().unwrap_or_default()
            );
        }
    }
    let notes: Vec<_> = report
        .rows
        .iter()
        .filter(|r| !r.target.note.is_empty())
        .collect();
    if !notes.is_empty() {
        println!();
        println!("Notes");
        for row in notes {
            println!("  {}: {}", row.target.id, row.target.note);
        }
    }
    println!();
    println!(
        "{} within tolerance, {} out of tolerance, {} for information, {} not computed",
        report.count(Verdict::Pass),
        report.count(Verdict::Fail),
        report.count(Verdict::Info),
        report.count(Verdict::Error),
    );
    println!(
        "Compared with reference values: a preliminary design certifies no standard; \
         acceptance needs a pump test."
    );
}
