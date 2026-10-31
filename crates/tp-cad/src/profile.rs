//! Wall-clock time of the stages of a CAD build and of each kind of kernel operation,
//! printed to stderr when the environment variable `TP_CAD_PROFILE` is set, to find
//! where a build spends its time.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("TP_CAD_PROFILE").is_some())
}

/// Calls and seconds of each kind of kernel operation since the last report.
fn totals() -> &'static Mutex<BTreeMap<&'static str, (usize, f64)>> {
    static TOTALS: OnceLock<Mutex<BTreeMap<&'static str, (usize, f64)>>> = OnceLock::new();
    TOTALS.get_or_init(Default::default)
}

/// Runs `f`, reporting how long it took under `name` when profiling.
pub(crate) fn stage<T>(name: &str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let result = f();
    eprintln!("[cad] {name}: {:.2} s", start.elapsed().as_secs_f64());
    result
}

/// Runs the kernel operation `f`, adding its time to the total of `kind` when profiling.
pub(crate) fn op<T>(kind: &'static str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let result = f();
    let seconds = start.elapsed().as_secs_f64();
    if let Ok(mut totals) = totals().lock() {
        let entry = totals.entry(kind).or_default();
        entry.0 += 1;
        entry.1 += seconds;
    }
    result
}

/// Prints and clears the totals of the kernel operations, under `title`.
pub(crate) fn report(title: &str) {
    if !enabled() {
        return;
    }
    let Ok(mut totals) = totals().lock() else {
        return;
    };
    let mut rows: Vec<_> = totals.iter().map(|(k, v)| (*k, *v)).collect();
    rows.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
    eprintln!("[cad] operations of {title}:");
    for (kind, (calls, seconds)) in rows {
        eprintln!("[cad]   {kind:<20} {calls:>5} calls {seconds:>8.2} s");
    }
    totals.clear();
}

/// Prints `message` when profiling, after the name of the thread (a test's name, in a
/// test run), so that notes of builds running side by side can be told apart.
pub(crate) fn note(message: impl FnOnce() -> String) {
    if enabled() {
        let thread = std::thread::current();
        eprintln!("[cad] {}: {}", thread.name().unwrap_or("main"), message());
    }
}
