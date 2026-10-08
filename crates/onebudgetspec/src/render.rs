//! Rendering what the library returns: JSON for a program, one line per item for a person.

use std::fmt::Write as _;
use std::io::{self, Write};

use onebudgetspec_core::{CheckReport, CheckResult, Host, ListReport, UNKNOWN, Verdict};
use serde::Serialize;

/// One JSON document, then a newline.
pub fn json(out: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    writeln!(out, "{text}")
}

/// One line per result:
///
/// `budget <id>: actual <actual> <unit>, budget <threshold> <unit>, headroom <headroom>
/// <unit> (<percent>%) — <within|over>; host: ...`, or `budget <id>: error — <reason>;
/// host: ...`.
///
/// Each line of a result's non-empty `detail` follows its result line, indented by two
/// spaces, whatever the verdict.
pub fn check_text(out: &mut impl Write, report: &CheckReport) -> io::Result<()> {
    for result in &report.results {
        writeln!(out, "{}", result_line(result))?;
        for line in result.detail.as_deref().unwrap_or_default().lines() {
            writeln!(out, "  {line}")?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An errored result, as the report spells it, carrying `detail`. The engine attaches
    /// no detail to an error result, so no real run can produce one.
    fn errored(detail: Option<&str>) -> CheckReport {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "results": [{
                "id": "broken",
                "file": "budgets.yaml",
                "labels": [],
                "unit": "requests",
                "direction": "max",
                "threshold": 1.0,
                "verdict": "error",
                "actual": null,
                "headroom": null,
                "headroom_percent": null,
                "detail": detail,
                "error": "node exited with status 5",
                "started_at": "1970-01-01T00:00:00Z",
                "ended_at": "1970-01-01T00:00:00Z",
                "host": { "load1": 0.5, "cpus": 2, "mem_available_mib": null, "conditions": {} },
            }],
        }))
        .expect("a valid check report")
    }

    fn text(report: &CheckReport) -> String {
        let mut out = Vec::new();
        check_text(&mut out, report).unwrap();
        String::from_utf8(out).unwrap()
    }

    const LINE: &str = "budget broken: error — node exited with status 5; host: load=0.5/2 mem_available=unknownMiB\n";

    #[test]
    fn an_error_results_detail_follows_its_line_indented() {
        assert_eq!(
            text(&errored(Some("retried twice\nthen gave up"))),
            format!("{LINE}  retried twice\n  then gave up\n")
        );
    }

    #[test]
    fn an_error_result_without_detail_is_its_line_alone() {
        assert_eq!(text(&errored(None)), LINE);
        assert_eq!(text(&errored(Some(""))), LINE);
    }
}
