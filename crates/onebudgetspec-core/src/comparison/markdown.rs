//! The budget report as Markdown: a summary line, the rows needing attention open, the rows
//! within budget and every reason and diagnostic collapsed beneath it.
//!
//! The layout is read on a phone as often as on a desktop, so the tables keep to three
//! columns at most and no row carries a pass or fail mark: the section a row sits in says
//! whether it passed.
use super::{ComparisonReport, ComparisonRow};
use crate::Verdict;
use std::fmt::Write;

/// Samples a baseline needs before it has a mean.
const MINIMUM_SAMPLES: u32 = 5;

/// The one section of the report a row belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    /// Over its limit, an enforced error, or flagged.
    Attention,
    /// Within its limit and not flagged.
    Within,
    /// No verdict, or an error on a report-only budget.
    NotMeasured,
}

/// One row with everything the layout derives from the saved report.
struct Line<'a> {
    row: &'a ComparisonRow,
    report_only: bool,
    bucket: Bucket,
    display: String,
}

/// Render every catalog row into exactly one of the report's three sections.
#[must_use]
pub fn render_markdown(report: &ComparisonReport) -> String {
    let lines = lines(report);
    let count = |bucket| lines.iter().filter(|line| line.bucket == bucket).count();
    let (attention, within, unmeasured) = (
        count(Bucket::Attention),
        count(Bucket::Within),
        count(Bucket::NotMeasured),
    );
    let mut out = String::from("## Budget report\n\n");
    if attention > 0 {
        let _ = write!(out, "**⚠️ {attention} need attention**");
    } else {
        out.push_str("**✅ Nothing needs attention**");
    }
    let _ = write!(out, " · {within} within budget");
    if unmeasured > 0 {
        let _ = write!(out, " · {unmeasured} not measured");
    }
    let current = &report.current;
    let _ = write!(
        out,
        " · head {} · {}\n\n",
        cell(super::short_sha(&current.head_sha)),
        run_link(&current.run_id, &current.run_url)
    );

    let _ = write!(
        out,
        "<details open><summary><b>Needs attention ({attention})</b></summary>\n\n"
    );
    section(
        &mut out,
        &lines,
        Bucket::Attention,
        "Nothing failed or was flagged.",
    );
    let _ = write!(
        out,
        "<details><summary><b>Within budget ({within})</b></summary>\n\n"
    );
    section(
        &mut out,
        &lines,
        Bucket::Within,
        "No budget is within its limit.",
    );
    details(&mut out, report, &lines, unmeasured);
    out
}

fn lines(report: &ComparisonReport) -> Vec<Line<'_>> {
    let prefix = format!(
        "{}-",
        report
            .current
            .repository
            .rsplit('/')
            .next()
            .unwrap_or_default()
    );
    report
        .rows
        .iter()
        .map(|row| {
            let report_only = report
                .current
                .catalog
                .iter()
                .find(|b| b.file == row.file && b.id == row.id)
                .is_some_and(|b| {
                    b.labels
                        .iter()
                        .any(|l| report.current.enforcement_exclude_labels.contains(l))
                });
            let bucket = match row.verdict {
                Some(Verdict::Over) => Bucket::Attention,
                Some(Verdict::Error) if !report_only => Bucket::Attention,
                _ if row.flag => Bucket::Attention,
                Some(Verdict::Within) => Bucket::Within,
                _ => Bucket::NotMeasured,
            };
            let short = row
                .id
                .strip_prefix(&prefix)
                .filter(|rest| !rest.is_empty())
                .unwrap_or(&row.id);
            let shared = report
                .rows
                .iter()
                .any(|other| other.id == row.id && other.file != row.file);
            let display = if shared {
                format!("{} ({})", cell(short), cell(&row.file))
            } else {
                cell(short)
            };
            Line {
                row,
                report_only,
                bucket,
                display,
            }
        })
        .collect()
}

/// One section's table, or the sentence saying it is empty, then the section's close.
fn section(out: &mut String, lines: &[Line<'_>], bucket: Bucket, empty: &str) {
    let rows: Vec<_> = lines.iter().filter(|line| line.bucket == bucket).collect();
    if rows.is_empty() {
        let _ = writeln!(out, "{empty}");
    } else if bucket == Bucket::Attention {
        out.push_str("| Budget | Now · vs base | Reason |\n| --- | --- | --- |\n");
        for line in rows {
            let _ = writeln!(
                out,
                "| **{}** | {} | {} |",
                line.display,
                now_vs_base(line.row),
                reason(line)
            );
        }
    } else {
        out.push_str("| Budget | Now · vs base |\n| --- | --- |\n");
        for line in rows {
            let _ = writeln!(out, "| **{}** | {} |", line.display, now_vs_base(line.row));
        }
    }
    out.push_str("\n</details>\n\n");
}

fn details(out: &mut String, report: &ComparisonReport, lines: &[Line<'_>], unmeasured: usize) {
    let current = &report.current;
    let _ = write!(
        out,
        "<details><summary>Details</summary>\n\nRepository: {} · head: {} · run: {} · attempt: {}\n",
        cell(&current.repository),
        cell(&current.head_sha),
        cell(&current.run_id),
        current.attempt
    );
    if unmeasured > 0 {
        let _ = write!(out, "\n**Not measured on this run ({unmeasured})**\n\n");
        for line in lines.iter().filter(|l| l.bucket == Bucket::NotMeasured) {
            let _ = writeln!(
                out,
                "- {}: no result this run{} · baseline {}/{MINIMUM_SAMPLES}",
                line.display,
                if line.report_only {
                    " (report-only)"
                } else {
                    ""
                },
                line.row.baseline_n
            );
        }
    }
    // Rows sharing one byte-identical reason share its bullet, so no reason repeats.
    let mut missing: Vec<(Vec<&str>, &str)> = Vec::new();
    for line in lines {
        let Some(reason) = line.row.unavailable.as_deref() else {
            continue;
        };
        match missing.iter_mut().find(|(_, seen)| *seen == reason) {
            Some((ids, _)) => ids.push(&line.display),
            None => missing.push((vec![&line.display], reason)),
        }
    }
    if !missing.is_empty() {
        out.push_str("\n**Why some figures are missing**\n\n");
        for (ids, reason) in missing {
            let _ = writeln!(out, "- {}: {}", ids.join(", "), cell(reason));
        }
    }
    let notes: Vec<_> = lines
        .iter()
        .filter_map(|line| {
            let note = current
                .result_notes
                .iter()
                .find(|n| n.file == line.row.file && n.id == line.row.id)?;
            // A note saying only what the row's not-measured line says is not repeated.
            let repeats = line.bucket == Bucket::NotMeasured
                && note
                    .note
                    .trim()
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case("no result this run");
            (!repeats).then_some((line, &note.note))
        })
        .collect();
    if !notes.is_empty() {
        out.push_str("\n**Notes on this run**\n\n");
        for (line, note) in notes {
            let _ = writeln!(out, "- {}: {}", line.display, cell(note));
        }
    }
    if !report.diagnostics.is_empty() {
        out.push('\n');
        for diagnostic in &report.diagnostics {
            let _ = writeln!(out, "- {}", cell(diagnostic));
        }
    }
    out.push_str("\n</details>\n");
}

/// The current figure and unit, then the change against the baseline, or how far the
/// baseline is from having a mean.
fn now_vs_base(row: &ComparisonRow) -> String {
    let now = row.current.map_or_else(
        || "—".to_string(),
        |value| format!("{} {}", figure(value), cell(&row.unit)),
    );
    let base = if let (Some(percent), Some(sigma)) = (row.percent_change, row.sigma_change) {
        Some(format!("{}% ({} SD)", signed(percent), signed(sigma)))
    } else if let Some(mean) = row.mean {
        Some(format!("base {} {}", figure(mean), cell(&row.unit)))
    } else {
        (row.baseline_n < MINIMUM_SAMPLES)
            .then(|| format!("baseline {}/{MINIMUM_SAMPLES}", row.baseline_n))
    };
    match base {
        Some(base) => format!("{now}<br>{base}"),
        None => now,
    }
}

/// The one plain phrase saying why a row needs attention.
fn reason(line: &Line<'_>) -> String {
    let row = line.row;
    let flag = if row.sd == Some(0.0) {
        "Flagged (was constant)".to_string()
    } else {
        format!(
            "Flagged {} SD",
            signed(row.sigma_change.unwrap_or_default())
        )
    };
    let phrase = match row.verdict {
        Some(Verdict::Over) if row.flag => "Over limit · flagged".to_string(),
        Some(Verdict::Over) => "Over limit".to_string(),
        Some(Verdict::Error) => "Error".to_string(),
        _ => flag,
    };
    if line.report_only {
        format!("{phrase} (report-only)")
    } else {
        phrase
    }
}

/// `[run <id>](<url>)` when the URL is a plain web address, else `run <id>`.
fn run_link(run_id: &str, url: &str) -> String {
    let label = cell(run_id).replace('[', "\\[").replace(']', "\\]");
    let linkable = (url.starts_with("https://") || url.starts_with("http://"))
        && !url
            .chars()
            .any(|c| c.is_whitespace() || "<>\"`".contains(c));
    if linkable {
        format!(
            "[run {label}]({})",
            url.replace('(', "%28").replace(')', "%29")
        )
    } else {
        format!("run {label}")
    }
}

/// A figure: whole with thousands separators from 100, one decimal from 10, else two.
fn figure(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 100.0 {
        let digits = format!("{magnitude:.0}");
        let mut grouped = String::with_capacity(digits.len() + digits.len() / 3 + 1);
        if value.is_sign_negative() {
            grouped.push('-');
        }
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        grouped
    } else if magnitude >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

/// A signed change to one decimal, with no negative zero.
fn signed(value: f64) -> String {
    let rounded = format!("{value:+.1}");
    if rounded == "-0.0" {
        "+0.0".into()
    } else {
        rounded
    }
}

/// Text safe inside a table cell or a list item: no HTML, no column break, one line.
fn cell(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace(['\r', '\n'], " ")
}
