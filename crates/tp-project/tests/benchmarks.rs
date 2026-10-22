//! The validation cases of `tests/benchmarks/`, run as `turbopump validate` runs them.

use std::path::{Path, PathBuf};

use tp_project::Project;
use tp_project::validation::{Report, Targets, Verdict, validate};

fn benchmarks() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/benchmarks")
}

fn run(case: &Path) -> Report {
    let project = Project::load(case.join("caso.tpj")).unwrap();
    let text = std::fs::read_to_string(case.join("targets.json")).unwrap();
    let targets = Targets::from_json(&text).unwrap();
    validate(&project, &targets)
}

fn verdicts(report: &Report) -> Vec<(&str, Verdict)> {
    report
        .rows
        .iter()
        .map(|r| (r.target.id.as_str(), r.verdict))
        .collect()
}

/// The design example of Japikse, Marscher and Furst (1997), Chap. 7.5. Every verdict is
/// pinned: a change here changes the comparison, so update the table in the case README.
#[test]
fn japikse_design_example() {
    use Verdict::*;
    let report = run(&benchmarks().join("japikse-7.5"));
    assert_eq!(
        verdicts(&report),
        [
            ("nq", Pass),
            ("cu2", Fail),
            ("cm2", Pass),
            ("alpha2", Fail),
            ("beta2", Pass),
            ("beta2B", Pass),
            ("slip", Info),
            ("cm1", Info),
            ("w1_shroud", Info),
            ("beta1_shroud", Info),
            ("eta", Fail),
            ("power", Fail),
            ("perf.volute_swirl", Pass),
            ("perf.volute_loss_coefficient", Pass),
            ("npsh3", Info),
        ]
    );
}

/// Private reference cases live in `tests/benchmarks/privado/`, excluded from git; each
/// is run when present. They only have to compute: their verdicts are results.
#[test]
fn private_cases_when_present() {
    let Ok(entries) = std::fs::read_dir(benchmarks().join("privado")) else {
        eprintln!("no private validation cases");
        return;
    };
    for entry in entries.flatten() {
        let case = entry.path();
        if !case.join("targets.json").exists() {
            continue;
        }
        let report = run(&case);
        for row in &report.rows {
            assert!(
                row.verdict != Verdict::Error,
                "{}: {}: {:?}",
                case.display(),
                row.target.id,
                row.error
            );
        }
        eprintln!(
            "{}: {} within tolerance, {} out",
            report.case,
            report.count(Verdict::Pass),
            report.count(Verdict::Fail)
        );
    }
}
