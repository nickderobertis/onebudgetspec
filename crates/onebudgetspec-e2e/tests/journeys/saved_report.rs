//! Saved ordinary observations, reproduced offline through the real CLI and reporter.
use crate::common::{Fixture, run_in};
use onebudgetspec_core::comparison::{BudgetRun, ComparisonInput, compare};
use serde_json::{Value, json};

fn saved() -> ComparisonInput {
    let mut current: BudgetRun = serde_json::from_str(include_str!(
        "../../../onebudgetspec-core/tests/fixtures/budget-run.json"
    ))
    .unwrap();
    current.enforcement_exclude_labels.clear();
    let baseline = (0..20)
        .map(|i| {
            let mut run = current.clone();
            run.head_sha = format!("base-{i}");
            run.results.results[0].actual = Some(10.0);
            run.results.results[0].verdict = onebudgetspec_core::Verdict::Within;
            run.results.results[0].headroom = Some(2.0);
            run.results.results[0].headroom_percent = Some(2.0 / 12.0 * 100.0);
            run
        })
        .collect();
    ComparisonInput {
        schema_version: 1,
        current,
        baseline,
    }
}

#[test]
fn offline_json_equals_the_sdk_and_does_not_execute_commands() {
    let fixture = Fixture::new();
    let input = saved();
    fixture.write("input.json", &serde_json::to_string(&input).unwrap());
    let run = fixture.run(["report", "--input", "input.json", "--output", "json"]);
    let run = run.expect_status(1);
    let report: Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(report, serde_json::to_value(compare(&input)).unwrap());
    assert_eq!(report["rows"][0]["verdict"], "over");
    assert_eq!(report["rows"][0]["baseline_n"], 20);
    assert_eq!(report["rows"][0]["flag"], true);
    let markdown = fixture.run(["report", "--input", "input.json", "--output", "markdown"]);
    assert!(
        markdown
            .expect_status(1)
            .stdout
            .contains("constant-baseline-change")
    );
    assert!(!fixture.path().join("measure.js").exists());
    let mut invalid = serde_json::to_value(input).unwrap();
    invalid["current"]["catalog"][0]["file"] = json!("../budgets.yaml");
    fixture.write("invalid.json", &invalid.to_string());
    fixture
        .run(["report", "--input", "invalid.json"])
        .expect_status(2);
}

#[test]
fn subprocess_delivery_preserves_real_failures_and_updates_one_standing_history() {
    let fixture = Fixture::new();
    let input = saved();
    fixture.write("input.json", &serde_json::to_string(&input).unwrap());
    let reporter = env!("CARGO_BIN_EXE_onebudgetspec-e2e-publish");
    for key in ["ok", "warning", "malformed", "exit", "denied"] {
        let run = run_in(
            fixture.path(),
            [
                "report",
                "--input",
                "input.json",
                "--json",
                "--reporter",
                reporter,
                "--destination",
                "https://example.test/comment",
                "--key",
                key,
            ],
            &[],
        );
        let run = run.expect_status(1);
        let report: Value = serde_json::from_str(&run.stdout).unwrap();
        assert_eq!(report["rows"][0]["verdict"], "over");
        if key != "ok" {
            assert!(report["diagnostics"].as_array().unwrap().iter().any(|d| {
                d.as_str().unwrap().contains(match key {
                    "exit" => "exited",
                    "denied" => "credential",
                    "warning" => "delivery accepted",
                    _ => "malformed",
                })
            }));
        }
        if key == "exit" {
            assert!(report["diagnostics"].as_array().unwrap().iter().any(|d| {
                d.as_str()
                    .unwrap()
                    .contains("posting server refused request")
            }));
        }
    }
    let request: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.path().join("publish-request.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(request["head_sha"], input.current.head_sha);
    assert_eq!(request["attempt"], input.current.attempt);
    assert_eq!(request["flags"][0]["constant_baseline_change"], true);
    assert_numeric_delivery(&fixture, input.clone(), reporter);
    let mut improved = input;
    improved.current.attempt = 2;
    improved.current.results.results[0].actual = Some(1.0);
    improved.current.results.results[0].verdict = onebudgetspec_core::Verdict::Within;
    improved.current.results.results[0].headroom = Some(11.0);
    improved.current.results.results[0].headroom_percent = Some(11.0 / 12.0 * 100.0);
    fixture.write("input.json", &serde_json::to_string(&improved).unwrap());
    fixture
        .run(["report", "--input", "input.json", "--reporter", reporter])
        .expect_status(0);
    let body = std::fs::read_to_string(fixture.path().join("standing.md")).unwrap();
    assert!(body.contains("current (attempt 2): 0 adverse flag(s)"));
    assert!(!body.contains("current (attempt 1)"));
}

fn assert_numeric_delivery(fixture: &Fixture, mut input: ComparisonInput, reporter: &str) {
    for (index, run) in input.baseline.iter_mut().enumerate() {
        let result = &mut run.results.results[0];
        let actual = 10.0 + f64::from(u32::try_from(index % 5).unwrap());
        let (verdict, headroom, percent) =
            onebudgetspec_core::judge(result.direction, result.threshold, actual);
        result.actual = Some(actual);
        result.verdict = verdict;
        result.headroom = Some(headroom);
        result.headroom_percent = percent;
    }
    fixture.write("numeric.json", &serde_json::to_string(&input).unwrap());
    let run = fixture.run([
        "report",
        "--input",
        "numeric.json",
        "--json",
        "--reporter",
        reporter,
    ]);
    let report: Value = serde_json::from_str(&run.expect_status(1).stdout).unwrap();
    let request: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.path().join("publish-request.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(request["flags"][0]["constant_baseline_change"], false);
    assert_eq!(
        request["flags"][0]["sigma_change"],
        report["rows"][0]["sigma_change"]
    );
    assert!(
        request["flags"][0]["sigma_change"]
            .as_f64()
            .is_some_and(|sigma| sigma >= 1.0)
    );
}

#[test]
fn saved_enforcement_and_malformed_baselines_preserve_offline_outcomes() {
    let fixture = Fixture::new();
    let mut input = saved();
    input.current.enforcement_exclude_labels = vec!["report".into()];
    fixture.write("input.json", &serde_json::to_string(&input).unwrap());
    let run = fixture.run(["report", "--input", "input.json", "--json"]);
    let report: Value = serde_json::from_str(&run.expect_status(0).stdout).unwrap();
    assert_eq!(report["rows"][0]["verdict"], "over");
    assert_eq!(report["rows"][0]["flag"], true);
    fixture
        .run(["report", "--input", "input.json"])
        .expect_status(0);
    input.current.enforcement_exclude_labels.clear();
    let mut json = serde_json::to_value(input).unwrap();
    json["baseline"][0] = json!({"invalid": true});
    fixture.write("input.json", &json.to_string());
    let run = fixture.run([
        "report",
        "--input",
        "input.json",
        "--json",
        "--reporter",
        "missing-reporter",
    ]);
    let report: Value = serde_json::from_str(&run.expect_status(1).stdout).unwrap();
    assert_eq!(report["rows"][0]["baseline_n"], 19);
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap().contains("invalid saved run"))
    );
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap().contains("cannot start reporter"))
    );
    json["current"]["results"]["results"][0]["verdict"] = json!("error");
    json["current"]["results"]["results"][0]["actual"] = Value::Null;
    json["current"]["results"]["results"][0]["error"] = json!("cancelled");
    json["current"]["results"]["results"][0]["headroom"] = Value::Null;
    json["current"]["results"]["results"][0]["headroom_percent"] = Value::Null;
    fixture.write("input.json", &json.to_string());
    fixture
        .run([
            "report",
            "--input",
            "input.json",
            "--json",
            "--reporter",
            "missing-reporter",
        ])
        .expect_status(3);
}

#[test]
fn standing_comments_bound_history_and_link_oversized_current_tables() {
    let fixture = Fixture::new();
    let reporter = env!("CARGO_BIN_EXE_onebudgetspec-e2e-publish");
    let mut input = saved();
    for head in 0..25 {
        input.current.head_sha = format!("current-{head}");
        fixture.write("input.json", &serde_json::to_string(&input).unwrap());
        fixture
            .run(["report", "--input", "input.json", "--reporter", reporter])
            .expect_status(1);
    }
    let body = std::fs::read_to_string(fixture.path().join("standing.md")).unwrap();
    assert!(body.len() <= 4096);
    assert!(body.contains("oldest history head(s) omitted"));
    assert!(body.contains("current-24"));
    let budget = input.current.catalog[0].clone();
    let result = input.current.results.results[0].clone();
    input.current.catalog = (0..100)
        .map(|i| {
            let mut b = budget.clone();
            b.id = format!("budget-{i}");
            b
        })
        .collect();
    input.current.results.results = (0..100)
        .map(|i| {
            let mut r = result.clone();
            r.id = format!("budget-{i}");
            r
        })
        .collect();
    fixture.write("input.json", &serde_json::to_string(&input).unwrap());
    fixture
        .run(["report", "--input", "input.json", "--reporter", reporter])
        .expect_status(1);
    let body = std::fs::read_to_string(fixture.path().join("standing.md")).unwrap();
    assert!(body.len() <= 4096);
    assert!(body.contains("Complete summary/artifact: https://example.test/run/current"));
    assert!(body.contains("may expire"));
}

#[test]
fn markdown_is_refused_for_ordinary_verbs_and_bad_reporter_requests_exit_cleanly() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let fixture = Fixture::new();
    for verb in ["check", "validate", "list", "schema"] {
        let run = fixture.run([verb, "--output", "markdown"]);
        assert!(
            run.expect_status(2)
                .stderr
                .contains("supported only by report")
        );
    }
    for input in [
        "not JSON",
        "{}",
        r#"{"schema_version":2,"destination":"","key":"","head_sha":"head","run_id":"run","run_url":"url","attempt":1,"markdown":"body","flags":[]}"#,
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_onebudgetspec-e2e-publish"))
            .current_dir(fixture.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(!error.contains("panicked"), "{error}");
        assert!(!fixture.path().join("publish-request.json").exists());
    }
}

/// The bytes of a document the current release saved, with no `result_notes` field, and an
/// empty baseline, so its one row carries report-only and unavailable text.
fn released_document() -> String {
    let run = include_str!("../../../onebudgetspec-core/tests/fixtures/budget-run.json");
    assert!(!run.contains("result_notes"));
    format!(r#"{{"schema_version": 1, "current": {run}, "baseline": []}}"#)
}

/// What `report --output markdown` printed for [`released_document`] before notes existed.
const RELEASED_MARKDOWN: &str = "## Budget report

Repository: owner/repo · head: current · run: run-current · attempt: 1

| Budget | Verdict | Current | Mean | SD | Change % | Sigma | Samples | Status |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| budgets.yaml / latency (ms) | over | 15.000 | unavailable | unavailable | unavailable | unavailable | 0 | report-only; baseline unavailable: 0 eligible samples; minimum 5; additional usable baseline observations were not supplied |

Baseline provenance (saved observations; artifact links may expire):
";

#[test]
fn a_document_saved_without_notes_renders_as_before_and_stays_without_them() {
    let fixture = Fixture::new();
    fixture.write("input.json", &released_document());
    let run = fixture.run(["report", "--input", "input.json", "--output", "markdown"]);
    assert_eq!(run.expect_status(0).stdout, RELEASED_MARKDOWN);
    let json = fixture.run(["report", "--input", "input.json", "--json"]);
    let report = json.expect_status(0).json();
    crate::common::validate("comparison-report", &report);
    assert!(report["current"].get("result_notes").is_none());
}

#[test]
fn null_notes_read_as_no_notes() {
    let fixture = Fixture::new();
    let mut value: Value = serde_json::from_str(&released_document()).unwrap();
    value["current"]["result_notes"] = Value::Null;
    fixture.write("input.json", &value.to_string());
    let run = fixture.run(["report", "--input", "input.json", "--output", "markdown"]);
    assert_eq!(run.expect_status(0).stdout, RELEASED_MARKDOWN);
}

#[test]
fn a_rows_note_ends_its_status_after_every_other_text() {
    let fixture = Fixture::new();
    let mut input = saved();
    input.current.enforcement_exclude_labels = vec!["report".into()];
    let mut other = input.current.catalog[0].clone();
    other.id = "unnoted".into();
    input.current.catalog.push(other);
    let mut value = serde_json::to_value(&input).unwrap();
    value["current"]["result_notes"] = json!([{
        "file": "budgets.yaml",
        "id": "latency",
        "note": "measured at abc123 | <cached>",
    }]);
    // Fewer than 20 samples, so the row is provisional as well as report-only and flagged.
    value["baseline"].as_array_mut().unwrap().truncate(6);
    fixture.write("input.json", &value.to_string());
    let run = fixture.run(["report", "--input", "input.json", "--output", "markdown"]);
    let markdown = &run.expect_status(0).stdout;
    let row = |id: &str| {
        markdown
            .lines()
            .find(|line| line.starts_with(&format!("| budgets.yaml / {id} ")))
            .unwrap_or_else(|| panic!("no row for {id} in {markdown}"))
    };
    assert!(
        row("latency").ends_with(
            "| report-only; constant-baseline-change; provisional; sigma change unavailable: constant baseline changed; measured at abc123 &#124; &lt;cached&gt; |"
        ),
        "{markdown}"
    );
    assert!(!row("unnoted").contains("abc123"), "{markdown}");

    let json = fixture.run(["report", "--input", "input.json", "--json"]);
    let report = json.expect_status(0).json();
    crate::common::validate("comparison-report", &report);
    assert_eq!(
        report["current"]["result_notes"],
        value["current"]["result_notes"]
    );
}

#[test]
fn a_note_without_a_row_a_second_note_or_a_blank_note_is_refused() {
    let fixture = Fixture::new();
    let note = |id: &str, note: &str| json!({"file": "budgets.yaml", "id": id, "note": note});
    for notes in [
        json!([note("unregistered", "text")]),
        json!([note("latency", "one"), note("latency", "two")]),
        json!([note("latency", "  ")]),
    ] {
        let mut value = serde_json::to_value(saved()).unwrap();
        value["current"]["result_notes"] = notes;
        fixture.write("input.json", &value.to_string());
        let run = fixture.run(["report", "--input", "input.json"]);
        assert!(run.expect_status(2).stderr.contains("invalid result note"));
    }
}
