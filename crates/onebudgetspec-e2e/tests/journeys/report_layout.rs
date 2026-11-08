//! The budget report's layout over real saved comparison inputs, printed by the real CLI:
//! every row in exactly one of Needs attention, Within budget and Not measured, the right
//! sections open, and every reason and diagnostic once, inside Details.
//!
//! The fixtures are onepipeline-ui's saved inputs from run 42384341444 (pull request #676)
//! and run 42421538096 (#686), and a copy of the second with a flagged and an enforced
//! `error` row added; `tests/fixtures/README.md` says how each was made.
use crate::common::Fixture;
use serde_json::Value;
use std::collections::BTreeSet;

const PREFIX: &str = "onepipeline-ui-";

/// Which budget ids belong in each section, named by hand from each fixture's rows.
struct Expected {
    fixture: &'static str,
    attention: &'static [(&'static str, &'static str)],
    within: &'static [&'static str],
    not_measured: &'static [&'static str],
    exit: i32,
}

const PR_676: Expected = Expected {
    fixture: "onepipeline-ui-676.json",
    attention: &[],
    within: &[
        "onepipeline-ui-macos-jobs-per-run",
        "onepipeline-ui-macos-runs-per-change",
        "onepipeline-ui-budget-report-command-seconds",
        "onepipeline-ui-ci-check-time",
        "onepipeline-ui-ci-job-count",
    ],
    not_measured: &[
        "onepipeline-ui-cache-headroom",
        "onepipeline-ui-pool-maintenance-seconds",
    ],
    exit: 0,
};

const WITHIN_686: &[&str] = &[
    "onepipeline-ui-budget-report-command-seconds",
    "dag-ui-human-items-requests-per-minute",
    "dag-ui-human-items-update-lag-seconds",
    "onepipeline-ui-ci-job-count",
];
const NOT_MEASURED_686: &[&str] = &[
    "onepipeline-ui-macos-jobs-per-run",
    "onepipeline-ui-macos-runs-per-change",
    "onepipeline-ui-pool-maintenance-seconds",
];

const PR_686: Expected = Expected {
    fixture: "onepipeline-ui-686.json",
    attention: &[
        ("onepipeline-ui-ci-check-time", "Over limit (report-only)"),
        ("onepipeline-ui-cache-headroom", "Over limit (report-only)"),
    ],
    within: WITHIN_686,
    not_measured: NOT_MEASURED_686,
    exit: 0,
};

const PR_686_FLAGGED_ERROR: Expected = Expected {
    fixture: "onepipeline-ui-686-flagged-error.json",
    attention: &[
        ("onepipeline-ui-docs-build-seconds", "Error"),
        ("onepipeline-ui-ci-check-time", "Over limit (report-only)"),
        ("onepipeline-ui-cache-headroom", "Over limit (report-only)"),
        ("onepipeline-ui-budget-render-seconds", "Flagged +2.5 SD"),
    ],
    within: WITHIN_686,
    not_measured: NOT_MEASURED_686,
    exit: 3,
};

/// One `<details>` section: its opening tag, its summary text and what follows it.
struct Section<'a> {
    tag: &'a str,
    summary: &'a str,
    body: &'a str,
}

/// The printed body, split into its summary line and its three sections.
struct Body<'a> {
    summary: &'a str,
    attention: Section<'a>,
    within: Section<'a>,
    details: Section<'a>,
}

fn parse(markdown: &str) -> Body<'_> {
    let rest = markdown
        .strip_prefix("## Budget report\n\n")
        .expect("the report heading first");
    let (summary, rest) = rest.split_once("\n\n").unwrap();
    let mut sections = Vec::new();
    let mut rest = rest;
    while !rest.is_empty() {
        let (tag, after) = rest.split_once("<summary>").expect("a section summary");
        let (summary, after) = after.split_once("</summary>\n\n").unwrap();
        let (body, after) = after
            .split_once("\n</details>\n")
            .expect("a closed section");
        sections.push(Section { tag, summary, body });
        rest = after.strip_prefix('\n').unwrap_or(after);
    }
    let [attention, within, details]: [Section<'_>; 3] = sections
        .try_into()
        .unwrap_or_else(|_| panic!("three sections in\n{markdown}"));
    Body {
        summary,
        attention,
        within,
        details,
    }
}

/// The header and the rows of a section's table, each as its cells.
fn table(body: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut lines = body.lines().filter(|line| line.starts_with('|'));
    let cells = |line: &str| {
        line.trim_matches('|')
            .split(" | ")
            .map(|cell| cell.trim().to_string())
            .collect::<Vec<_>>()
    };
    let Some(header) = lines.next() else {
        return (Vec::new(), Vec::new());
    };
    let separator = lines.next().unwrap();
    assert!(separator.starts_with("| ---"), "{separator}");
    (cells(header), lines.map(cells).collect())
}

fn display(id: &str) -> &str {
    id.strip_prefix(PREFIX).unwrap_or(id)
}

fn bold_budgets(rows: &[Vec<String>]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row[0]
                .strip_prefix("**")
                .and_then(|cell| cell.strip_suffix("**"))
                .unwrap_or_else(|| panic!("an unbolded Budget cell {row:?}"))
                .to_string()
        })
        .collect()
}

/// The `- <display id>: …` bullets under the Not measured heading, by display id.
fn not_measured(details: &str, count: usize) -> Vec<&str> {
    let heading = format!("**Not measured on this run ({count})**\n\n");
    let Some((_, list)) = details.split_once(&heading) else {
        assert_eq!(count, 0, "no Not measured list in\n{details}");
        assert!(!details.contains("Not measured on this run"));
        return Vec::new();
    };
    list.split("\n\n")
        .next()
        .unwrap()
        .lines()
        .map(|line| {
            let bullet = line.strip_prefix("- ").unwrap();
            assert!(bullet.contains(": no result this run"), "{bullet}");
            bullet.split_once(": ").unwrap().0
        })
        .collect()
}

/// A scratch directory holding `name`, one of `tests/fixtures`, as `input.json`.
fn saved(name: &str) -> Fixture {
    let fixture = Fixture::new();
    let input = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap();
    fixture.write("input.json", &input);
    fixture
}

fn check(expected: &Expected) -> String {
    let fixture = saved(expected.fixture);
    let markdown = fixture
        .run(["report", "--input", "input.json", "--output", "markdown"])
        .expect_status(expected.exit)
        .stdout
        .clone();
    let report: Value = fixture
        .run(["report", "--input", "input.json", "--json"])
        .expect_status(expected.exit)
        .json();
    let body = parse(&markdown);

    // Needs attention opens; Within budget and Details are collapsed.
    assert_eq!(body.attention.tag, "<details open>");
    assert_eq!(body.within.tag, "<details>");
    assert_eq!(body.details.tag, "<details>");

    let (attention_header, attention_rows) = table(body.attention.body);
    let (within_header, within_rows) = table(body.within.body);
    let attention: Vec<_> = expected.attention.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        bold_budgets(&attention_rows),
        attention.iter().map(|id| display(id)).collect::<Vec<_>>()
    );
    for (row, (id, reason)) in attention_rows.iter().zip(expected.attention) {
        assert_eq!(row[2], *reason, "{id}");
    }
    assert_eq!(
        bold_budgets(&within_rows),
        expected
            .within
            .iter()
            .map(|id| display(id))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        not_measured(body.details.body, expected.not_measured.len()),
        expected
            .not_measured
            .iter()
            .map(|id| display(id))
            .collect::<Vec<_>>()
    );

    // Every row is in exactly one of the three, and the three hold nothing else.
    let rows: Vec<&str> = report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    let mut named: Vec<&str> = attention.clone();
    named.extend(expected.within);
    named.extend(expected.not_measured);
    assert_eq!(named.len(), rows.len());
    assert_eq!(
        named.iter().collect::<BTreeSet<_>>(),
        rows.iter().collect::<BTreeSet<_>>()
    );

    // The counts agree with the sets.
    let (n, k, u) = (
        attention.len(),
        expected.within.len(),
        expected.not_measured.len(),
    );
    assert_eq!(
        body.attention.summary,
        format!("<b>Needs attention ({n})</b>")
    );
    assert_eq!(body.within.summary, format!("<b>Within budget ({k})</b>"));
    assert_eq!(body.details.summary, "Details");
    let lead = if n > 0 {
        format!("**⚠️ {n} need attention**")
    } else {
        "**✅ Nothing needs attention**".into()
    };
    let unmeasured = if u > 0 {
        format!(" · {u} not measured")
    } else {
        String::new()
    };
    assert!(
        body.summary
            .starts_with(&format!("{lead} · {k} within budget{unmeasured} · head ")),
        "{}",
        body.summary
    );
    if n > 0 {
        assert_eq!(attention_header, ["Budget", "Now · vs base", "Reason"]);
    }
    assert_eq!(within_header, ["Budget", "Now · vs base"]);

    // No row carries a pass or fail mark, and no Reason a commit hash.
    let hash = regex::Regex::new(r"\b[0-9a-f]{7,40}\b").unwrap();
    for line in markdown.lines().filter(|line| line.starts_with('|')) {
        assert!(!line.contains(['✅', '❌', '⚠']), "{line}");
    }
    for row in &attention_rows {
        assert!(!hash.is_match(&row[2]), "{row:?}");
    }

    reasons_once_inside_details(&markdown, body.details.body, &report);
    markdown
}

/// Each distinct reason and each diagnostic once, inside Details, never in a table.
fn reasons_once_inside_details(markdown: &str, details: &str, report: &Value) {
    let mut texts: Vec<&str> = report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["unavailable"].as_str())
        .collect();
    texts.sort_unstable();
    texts.dedup();
    let diagnostics: Vec<&str> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap())
        .collect();
    assert!(!diagnostics.is_empty());
    // A reason ends exactly one bullet, after the ids of the rows sharing it.
    let bullets = |text: &str, within: &str| {
        within
            .lines()
            .filter(|line| {
                line.starts_with("- ")
                    && (line.ends_with(&format!(": {text}")) || *line == format!("- {text}"))
            })
            .count()
    };
    for text in texts.iter().chain(&diagnostics) {
        assert!(!text.contains(['&', '<', '>', '|', '\n']), "{text}");
        assert_eq!(bullets(text, markdown), 1, "{text}\n{markdown}");
        assert_eq!(bullets(text, details), 1, "{text}");
        for line in markdown.lines().filter(|line| line.starts_with('|')) {
            assert!(!line.contains(text), "{line}");
        }
    }
    for diagnostic in &diagnostics {
        assert_eq!(markdown.matches(diagnostic).count(), 1, "{diagnostic}");
        assert!(
            details.contains(&format!("\n- {diagnostic}\n")),
            "{diagnostic}"
        );
    }
    assert!(!markdown.contains("Diagnostic:"));
    assert!(!markdown.contains("baseline provenance:"));
    assert!(!markdown.contains("eligible samples"));
}

#[test]
fn pr_676_has_nothing_needing_attention_and_two_rows_not_measured() {
    let markdown = check(&PR_676);
    let body = parse(&markdown);
    assert!(body.summary.starts_with(
        "**✅ Nothing needs attention** · 5 within budget · 2 not measured · head 3f29a3c"
    ));
    assert_eq!(body.attention.body, "Nothing failed or was flagged.\n");
    // A reason shared by all five baseline samples is written once, with its count.
    assert!(markdown.contains(
        "- macos-jobs-per-run, macos-runs-per-change: its command reads .github/.telemetry/macos-policy.json, which is not available in this job (5 of 5 baseline samples)\n"
    ));
    assert_eq!(
        markdown.matches("(5 of 5 baseline samples)").count(),
        3,
        "{markdown}"
    );
    assert!(markdown.contains("\n- baseline: 5 base commits, 7d264e1 … ce3633f\n"));
}

#[test]
fn pr_686_lists_its_report_only_over_rows_and_shows_other_ids_in_full() {
    let markdown = check(&PR_686);
    assert!(markdown.contains("| **dag-ui-human-items-update-lag-seconds** |"));
    assert!(markdown.contains("| **ci-check-time** | 1,943 seconds<br>+19.2% (+0.6 SD) |"));
    assert!(!markdown.contains("**onepipeline-ui-"));
}

#[test]
fn a_flagged_row_and_an_enforced_error_need_attention_beside_the_over_rows() {
    let markdown = check(&PR_686_FLAGGED_ERROR);
    assert!(markdown.contains("| **docs-build-seconds** | —<br>baseline 0/5 | Error |"));
}

#[test]
fn verdicts_flags_statistics_and_exit_codes_equal_v0_2_0s() {
    let golden: Value =
        serde_json::from_str(include_str!("../fixtures/v0.2.0-outcomes.json")).unwrap();
    for expected in [&PR_676, &PR_686, &PR_686_FLAGGED_ERROR] {
        let fixture = saved(expected.fixture);
        let was = &golden[expected.fixture];
        let run = fixture.run(["report", "--input", "input.json", "--json"]);
        assert_eq!(i64::from(run.status), was["exit_code"].as_i64().unwrap());
        let report = run.json();
        let rows: Vec<Value> = report["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let mut kept = serde_json::Map::new();
                for key in [
                    "file",
                    "id",
                    "verdict",
                    "flag",
                    "baseline_n",
                    "mean",
                    "sd",
                    "percent_change",
                    "sigma_change",
                ] {
                    kept.insert(key.into(), row.get(key).cloned().unwrap_or(Value::Null));
                }
                Value::Object(kept)
            })
            .collect();
        assert_eq!(Value::Array(rows), was["rows"], "{}", expected.fixture);
    }
}
