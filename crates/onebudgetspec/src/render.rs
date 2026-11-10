//! Rendering what the library returns: JSON for a program, one line per item for a person.

use std::fmt::Write as _;
use std::io::{self, Write};

use onebudgetspec_core::{
    CheckReport, CheckResult, Host, ListReport, SubjectsReport, UNKNOWN, Verdict,
};
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

/// Per subject, `subject <id>:` and then its results as [`check_text`] prints them,
/// indented by two spaces; then `subject <id>: unanswered — <reason>` for each subject
/// that could not be checked.
pub fn subjects_text(out: &mut impl Write, answer: &SubjectsReport) -> io::Result<()> {
    for subject in &answer.subjects {
        writeln!(out, "subject {}:", subject.id)?;
        let mut lines = Vec::new();
        check_text(&mut lines, &subject.report)?;
        for line in String::from_utf8_lossy(&lines).lines() {
            writeln!(out, "  {line}")?;
        }
    }
    for unanswered in &answer.unanswered {
        writeln!(
            out,
            "subject {}: unanswered — {}",
            unanswered.id, unanswered.reason
        )?;
    }
    Ok(())
}

/// One section per checked subject, each a table of its results, then one listing every
/// subject that could not be checked, with its reason.
pub fn subjects_markdown(out: &mut impl Write, answer: &SubjectsReport) -> io::Result<()> {
    writeln!(out, "# Budgets by subject")?;
    for subject in &answer.subjects {
        writeln!(out, "\n## Subject `{}`\n", code(&subject.id))?;
        if subject.report.results.is_empty() {
            writeln!(out, "No results.")?;
            continue;
        }
        writeln!(
            out,
            "| Budget | Verdict | Actual | Budget limit | Headroom |"
        )?;
        writeln!(out, "| --- | --- | --- | --- | --- |")?;
        for result in &subject.report.results {
            let unit = &result.unit;
            let with_unit = |value: Option<f64>| {
                value.map_or_else(|| "—".to_owned(), |value| format!("{value} {unit}"))
            };
            let verdict = match (&result.verdict, &result.error) {
                (Verdict::Error, Some(error)) => format!("error: {}", cell(error)),
                (verdict, _) => verdict.as_str().to_owned(),
            };
            writeln!(
                out,
                "| `{}` | {verdict} | {} | {} {unit} | {} |",
                code(&result.id),
                with_unit(result.actual),
                result.threshold,
                with_unit(result.headroom),
            )?;
        }
    }
    if !answer.unanswered.is_empty() {
        writeln!(out, "\n## Unanswered\n")?;
        for unanswered in &answer.unanswered {
            writeln!(
                out,
                "- `{}`: {}",
                code(&unanswered.id),
                cell(&unanswered.reason)
            )?;
        }
    }
    Ok(())
}

/// `text` safe inside a table cell or a list item: one line, with `|` escaped.
fn cell(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

/// `text` safe inside an inline code span.
fn code(text: &str) -> String {
    cell(text).replace('`', "'")
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
