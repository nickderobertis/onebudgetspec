//! Rendering what the library returns: JSON for a program, one line per item for a person.

use std::fmt::Write as _;
use std::io::{self, Write};

use onebudgetspec_core::{CheckReport, CheckResult, Host, ListReport, UNKNOWN, Verdict};
use serde::Serialize;

/// One JSON document, then a newline.
pub fn json(out: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).expect("reports serialise to JSON");
    writeln!(out, "{text}")
}

/// One line per result:
///
/// `budget <id>: actual <actual> <unit>, budget <threshold> <unit>, headroom <headroom>
/// <unit> (<percent>%) — <within|over>; host: ...`, or `budget <id>: error — <reason>;
/// host: ...`.
pub fn check_text(out: &mut impl Write, report: &CheckReport) -> io::Result<()> {
    for result in &report.results {
        writeln!(out, "{}", result_line(result))?;
    }
    Ok(())
}

fn result_line(result: &CheckResult) -> String {
    let id = &result.id;
    let unit = &result.unit;
    let measured = match (result.verdict, result.actual, result.headroom) {
        (Verdict::Within | Verdict::Over, Some(actual), Some(headroom)) => format!(
            "actual {actual} {unit}, budget {} {unit}, headroom {headroom} {unit} ({}%) — {}",
            result.threshold,
            number(result.headroom_percent),
            result.verdict.as_str()
        ),
        _ => format!(
            "error — {}",
            result.error.as_deref().unwrap_or("no value was measured")
        ),
    };
    format!("budget {id}: {measured}; host: {}", host(&result.host))
}

fn host(host: &Host) -> String {
    let mut line = format!(
        "load={}/{} mem_available={}MiB",
        number(host.load1),
        host.cpus,
        host.mem_available_mib
            .map_or_else(|| UNKNOWN.to_owned(), |mib| mib.to_string())
    );
    for (name, value) in &host.conditions {
        let _ = write!(line, " {name}={value}");
    }
    line
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| UNKNOWN.to_owned(), |value| value.to_string())
}

/// One line per budget: `<id> (<file>): <measure> <direction> <threshold> <unit>`, with
/// its labels when it has any.
pub fn list_text(out: &mut impl Write, report: &ListReport) -> io::Result<()> {
    for budget in &report.budgets {
        let mut line = format!(
            "{} ({}): {} {} {} {}",
            budget.id,
            budget.file,
            budget.measure.as_str(),
            budget.direction.as_str(),
            budget.threshold,
            budget.unit
        );
        if !budget.labels.is_empty() {
            let _ = write!(line, " [{}]", budget.labels.join(", "));
        }
        writeln!(out, "{line}")?;
    }
    Ok(())
}
