//! The rendered report's per-row rules, over saved comparison reports: figures, display
//! ids, the Reason phrase and where report-only, notes and reasons land.
use onebudgetspec_core::comparison::{
    BudgetRun, ComparisonInput, ComparisonReport, ComparisonRow, ResultNote, compare,
    render_markdown,
};
use onebudgetspec_core::{Direction, Verdict};

/// A saved report whose catalog and rows are exactly `rows`, each report-only when marked.
fn report(rows: Vec<(ComparisonRow, bool)>) -> ComparisonReport {
    let current: BudgetRun =
        serde_json::from_str(include_str!("fixtures/budget-run.json")).unwrap();
    let mut report = compare(&ComparisonInput {
        schema_version: 1,
        current,
        baseline: Vec::new(),
    });
    let template = report.current.catalog[0].clone();
    report.current.catalog = rows
        .iter()
        .map(|(row, report_only)| {
            let mut budget = template.clone();
            budget.id.clone_from(&row.id);
            budget.file.clone_from(&row.file);
            budget.labels = if *report_only {
                vec!["report".into()]
            } else {
                Vec::new()
            };
            budget
        })
        .collect();
    report.current.results.results.clear();
    report.diagnostics.clear();
    report.rows = rows.into_iter().map(|(row, _)| row).collect();
    report
}

/// A measured row within its limit with a full baseline and no change.
fn row(id: &str) -> ComparisonRow {
    ComparisonRow {
        file: "budgets.yaml".into(),
        id: id.into(),
        unit: "ms".into(),
        direction: Direction::Max,
        threshold: 1_000_000.0,
        current: Some(10.0),
        verdict: Some(Verdict::Within),
        baseline_n: 20,
        mean: Some(10.0),
        sd: Some(1.0),
        percent_change: Some(0.0),
        sigma_change: Some(0.0),
        flag: false,
        unavailable: None,
    }
}

/// The Markdown table line whose Budget cell is `**<display>**`.
fn table_line<'a>(markdown: &'a str, display: &str) -> &'a str {
    let start = format!("| **{display}** |");
    markdown
        .lines()
        .find(|line| line.starts_with(&start))
        .unwrap_or_else(|| panic!("no table row for {display} in\n{markdown}"))
}

#[test]
fn figures_are_rounded_by_magnitude_and_changes_are_signed() {
    let mut big = row("big");
    big.current = Some(1_234_567.4);
    big.percent_change = Some(16.573);
    big.sigma_change = Some(0.802);
    let mut middle = row("middle");
    middle.current = Some(12.34);
    middle.unit = "requests/min".into();
    middle.percent_change = None;
    middle.mean = Some(-1_500.0);
    let mut small = row("small");
    small.current = Some(0.5);
    small.percent_change = Some(-3.04);
    small.sigma_change = Some(-0.04);
    let mut filling = row("filling");
    filling.baseline_n = 3;
    (
        filling.mean,
        filling.sd,
        filling.percent_change,
        filling.sigma_change,
    ) = (None, None, None, None);
    let mut missing = filling.clone();
    missing.id = "missing".into();
    (missing.current, missing.verdict) = (None, None);
    let markdown = render_markdown(&report(vec![
        (big, false),
        (middle, false),
        (small, false),
        (filling, false),
        (missing, false),
    ]));
    let cells = |id: &str| table_line(&markdown, id).to_string();
    assert_eq!(
        cells("big"),
        "| **big** | 1,234,567 ms<br>+16.6% (+0.8 SD) |"
    );
    // A mean without both changes reads as the base figure, in the budget's own unit.
    assert_eq!(
        cells("middle"),
        "| **middle** | 12.3 requests/min<br>base -1,500 requests/min |"
    );
    assert_eq!(cells("small"), "| **small** | 0.50 ms<br>-3.0% (+0.0 SD) |");
    assert_eq!(
        cells("filling"),
        "| **filling** | 10.0 ms<br>baseline 3/5 |"
    );
    assert!(markdown.contains("- missing: no result this run · baseline 3/5\n"));
    assert!(!markdown.contains("| **missing**"));
}

#[test]
fn display_ids_drop_only_an_exact_repository_name_prefix() {
    let markdown = render_markdown(&report(
        [
            "repo-latency",
            "repository-size",
            "other-repo-time",
            "repo-",
        ]
        .into_iter()
        .map(|id| (row(id), false))
        .collect(),
    ));
    for display in ["latency", "repository-size", "other-repo-time", "repo-"] {
        table_line(&markdown, display);
    }
    assert!(!markdown.contains("**repo-latency**"));
    // The same id in two files names each row's file.
    let mut twin = row("repo-latency");
    twin.file = "nested/budgets.yaml".into();
    let markdown = render_markdown(&report(vec![(row("repo-latency"), false), (twin, false)]));
    table_line(&markdown, "latency (budgets.yaml)");
    table_line(&markdown, "latency (nested/budgets.yaml)");
}

#[test]
fn each_row_needing_attention_carries_one_plain_reason() {
    let mut over_flagged = row("over-flagged");
    (over_flagged.verdict, over_flagged.flag) = (Some(Verdict::Over), true);
    let mut constant = row("constant");
    (constant.flag, constant.sd, constant.sigma_change) = (true, Some(0.0), None);
    let mut falling = row("falling");
    falling.direction = Direction::Min;
    (falling.flag, falling.sigma_change) = (true, Some(-2.04));
    let mut over_only = row("over-only");
    over_only.verdict = Some(Verdict::Over);
    let mut flagged_only = falling.clone();
    flagged_only.id = "flagged-only".into();
    let mut error = row("error");
    (error.verdict, error.current) = (Some(Verdict::Error), None);
    error.unavailable = Some("node exited with status 1".into());
    let mut error_only = error.clone();
    error_only.id = "error-only".into();
    let markdown = render_markdown(&report(vec![
        (error, false),
        (over_flagged, false),
        (over_only, true),
        (constant, false),
        (falling, false),
        (flagged_only, true),
        (error_only, true),
        (row("within-only"), true),
    ]));
    for (display, reason) in [
        ("error", "Error"),
        ("over-flagged", "Over limit · flagged"),
        ("over-only", "Over limit (report-only)"),
        ("constant", "Flagged (was constant)"),
        ("falling", "Flagged -2.0 SD"),
        ("flagged-only", "Flagged -2.0 SD (report-only)"),
    ] {
        assert!(
            table_line(&markdown, display).ends_with(&format!(" | {reason} |")),
            "{display}: {markdown}"
        );
    }
    assert!(markdown.starts_with("## Budget report\n\n**⚠️ 6 need attention** · 1 within budget · 1 not measured · head current · [run run-current](https://example.test/run/current)\n"));
    // A report-only error is not measured; a report-only row within its limit has no marker.
    assert!(markdown.contains("- error-only: no result this run (report-only) · baseline 20/5\n"));
    assert_eq!(
        table_line(&markdown, "within-only"),
        "| **within-only** | 10.0 ms<br>+0.0% (+0.0 SD) |"
    );
    // Unavailable text is a Details bullet, never a table cell.
    // Rows sharing one reason share its bullet, in report order.
    assert!(markdown.contains("\n- error, error-only: node exited with status 1\n"));
    assert_eq!(markdown.matches("node exited with status 1").count(), 1);
    assert!(!table_line(&markdown, "error").contains("status 1"));
    for line in markdown.lines().filter(|line| line.starts_with('|')) {
        assert!(!line.contains(['✅', '❌', '⚠']), "{line}");
    }
}

#[test]
fn notes_are_listed_in_details_unless_they_repeat_the_not_measured_line() {
    let mut missing = row("missing");
    (missing.current, missing.verdict) = (None, None);
    let mut saved = report(vec![
        (row("noted"), false),
        (missing, false),
        (row("plain"), false),
    ]);
    let note = |id: &str, note: &str| ResultNote {
        file: "budgets.yaml".into(),
        id: id.into(),
        note: note.into(),
    };
    saved.current.result_notes = vec![
        note("noted", "measured at abc123 | <cached>"),
        note("missing", "No result this run."),
    ];
    saved.diagnostics = vec!["baseline: 20 base commits, aaaaaaa … bbbbbbb".into()];
    let markdown = render_markdown(&saved);
    let details = markdown
        .split_once("<details><summary>Details</summary>")
        .unwrap()
        .1;
    let notes = details.split_once("**Notes on this run**\n\n").unwrap().1;
    assert!(notes.starts_with(
        "- noted: measured at abc123 &#124; &lt;cached&gt;\n\n- baseline: 20 base commits"
    ));
    assert_eq!(markdown.matches("abc123").count(), 1);
    assert!(!notes.contains("missing"));
    // Without notes there is no Notes block at all.
    saved.current.result_notes.clear();
    assert!(!render_markdown(&saved).contains("Notes on this run"));
}

#[test]
fn an_empty_report_says_so_in_every_section() {
    let markdown = render_markdown(&report(Vec::new()));
    assert!(markdown.contains("**✅ Nothing needs attention** · 0 within budget · head"));
    assert!(markdown.contains("Nothing failed or was flagged.\n"));
    assert!(markdown.contains("No budget is within its limit.\n"));
    assert!(!markdown.contains("not measured"));
}
