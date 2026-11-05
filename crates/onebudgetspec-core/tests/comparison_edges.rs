//! Untrusted saved artifacts exercise the public parser, selection and rendering boundary.
use onebudgetspec_core::comparison::{
    BudgetRun, ComparisonInput, RollingHistory, compare, compare_json, render_markdown,
    validate_history, validate_run,
};
use onebudgetspec_core::{Direction, Verdict, judge};
use serde_json::{Value, json};

fn observation(actual: f64) -> BudgetRun {
    let mut run: BudgetRun =
        serde_json::from_str(include_str!("fixtures/budget-run.json")).unwrap();
    set_actual(&mut run, actual);
    run
}
fn set_actual(run: &mut BudgetRun, actual: f64) {
    let result = &mut run.results.results[0];
    let (verdict, headroom, percent) = judge(result.direction, result.threshold, actual);
    result.actual = Some(actual);
    result.verdict = verdict;
    result.headroom = Some(headroom);
    result.headroom_percent = percent;
    result.error = None;
}
fn saved(n: usize) -> ComparisonInput {
    ComparisonInput {
        schema_version: 1,
        current: observation(15.0),
        baseline: (0..n)
            .map(|i| {
                let mut run = observation(10.0);
                run.head_sha = format!("base-{i:02}");
                run
            })
            .collect(),
    }
}
fn encode(input: &ComparisonInput) -> Vec<u8> {
    serde_json::to_vec(input).unwrap()
}
fn errored(run: &mut BudgetRun) {
    let result = &mut run.results.results[0];
    result.actual = None;
    result.headroom = None;
    result.headroom_percent = None;
    result.verdict = Verdict::Error;
    result.error = Some("cancelled measurement".into());
}

#[test]
fn newest_twenty_heads_and_equal_attempt_ties_are_deterministic() {
    let mut input = saved(25);
    for (i, run) in input.baseline.iter_mut().enumerate() {
        run.completed_at = format!("2026-10-{:02}T00:00:00Z", i + 1);
        set_actual(run, f64::from(u32::try_from(i).unwrap()));
    }
    let mut attempt = input.baseline[24].clone();
    attempt.attempt = 2;
    attempt.completed_at = "2026-10-26T01:00:00+01:00".into();
    attempt.run_id = "a".into();
    set_actual(&mut attempt, 100.0);
    input.baseline.push(attempt.clone());
    attempt.run_id = "b".into();
    set_actual(&mut attempt, 200.0);
    input.baseline.push(attempt.clone());
    attempt.completed_at = "2026-10-25T00:00:00Z".into();
    attempt.run_id = "z".into();
    set_actual(&mut attempt, 999.0);
    input.baseline.push(attempt);
    let report = compare_json(&encode(&input)).unwrap();
    assert_eq!(report.rows[0].baseline_n, 20);
    assert!((report.rows[0].mean.unwrap() - 23.3).abs() < 1e-10);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("run b attempt 2"))
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.contains("head base-00 run"))
    );
}

#[test]
fn unavailable_and_rejected_samples_keep_their_precise_reasons() {
    let mut input = saved(5);
    input.current.results.results.clear();
    input
        .current
        .unavailable
        .push(onebudgetspec_core::comparison::UnavailableBudget {
            file: "budgets.yaml".into(),
            id: "latency".into(),
            reason: "command is not installed".into(),
        });
    input.baseline[0].catalog.clear();
    input.baseline[0].results.results.clear();
    input.baseline[1].results.results.clear();
    input.baseline[1].unavailable = input.current.unavailable.clone();
    input.baseline[2].results.results.clear();
    errored(&mut input.baseline[3]);
    input.baseline[4].catalog[0]
        .command
        .push("changed scope".into());
    let report = compare_json(&encode(&input)).unwrap();
    let row = &report.rows[0];
    assert_eq!(row.baseline_n, 0);
    assert_eq!(row.verdict, None);
    assert_eq!(row.current, None);
    for reason in [
        "command is not installed",
        "catalog entry absent",
        "measurement absent or incomplete",
        "cancelled measurement",
        "scope mismatch",
    ] {
        assert!(
            row.unavailable.as_ref().unwrap().contains(reason),
            "{reason}"
        );
    }
    for scope in ["unit", "direction", "measure", "timeout"] {
        let mut input = saved(5);
        let run = &mut input.baseline[0];
        match scope {
            "unit" => {
                run.catalog[0].unit = "bytes".into();
                run.results.results[0].unit = "bytes".into();
            }
            "direction" => {
                run.catalog[0].direction = Direction::Min;
                run.results.results[0].direction = Direction::Min;
                set_actual(run, 10.0);
            }
            "measure" => run.catalog[0].measure = onebudgetspec_core::Measure::Elapsed,
            _ => run.catalog[0].timeout_seconds = Some(7),
        }
        let row = &compare_json(&encode(&input)).unwrap().rows[0];
        assert_eq!(row.baseline_n, 4, "{scope}");
        assert!(row.unavailable.as_ref().unwrap().contains("scope mismatch"));
    }
    let mut changed_threshold = saved(5);
    for run in &mut changed_threshold.baseline {
        run.catalog[0].threshold = 5.0;
        run.results.results[0].threshold = 5.0;
        set_actual(run, 10.0);
        assert_eq!(run.results.results[0].verdict, Verdict::Over);
    }
    let row = &compare_json(&encode(&changed_threshold)).unwrap().rows[0];
    assert_eq!(row.baseline_n, 5);
    assert_eq!(row.mean, Some(10.0));
    assert!((row.threshold - 12.0).abs() < f64::EPSILON);
}

#[test]
fn malformed_current_artifacts_are_refused_and_bad_baselines_are_diagnostic() {
    let valid = serde_json::to_value(saved(5)).unwrap();
    for (pointer, invalid) in [
        ("/schema_version", json!(2)),
        ("/current/schema_version", json!(2)),
        ("/current/results/schema_version", json!(2)),
        ("/current/repository", json!("")),
        ("/current/base_ref", json!("")),
        ("/current/head_sha", json!("")),
        ("/current/run_id", json!("")),
        ("/current/run_url", json!("")),
        ("/current/attempt", json!(0)),
        ("/current/completed_at", json!("yesterday")),
        ("/current/enforcement_exclude_labels", json!(["BAD"])),
        ("/current/catalog/0/id", json!("BAD")),
        ("/current/catalog/0/file", json!("../bad")),
        ("/current/catalog/0/unit", json!("BAD")),
        ("/current/catalog/0/threshold", json!(-1)),
        ("/current/catalog/0/timeout_seconds", json!(0)),
        ("/current/catalog/0/command", json!([])),
        ("/current/catalog/0/command/0", json!("")),
        ("/current/results/results/0/id", json!("unlisted")),
        (
            "/current/results/results/0/ended_at",
            json!("2020-01-01T00:00:00Z"),
        ),
        ("/current/catalog/0/labels", json!(["BAD"])),
        ("/current/results/results/0/unit", json!("bytes")),
        ("/current/results/results/0/threshold", json!(1)),
        ("/current/results/results/0/labels", json!([])),
        ("/current/results/results/0/verdict", json!("within")),
        ("/current/results/results/0/headroom", json!(0)),
        ("/current/results/results/0/headroom_percent", Value::Null),
        ("/current/results/results/0/error", json!("contradictory")),
        (
            "/current/unavailable",
            json!([{"file":"budgets.yaml","id":"latency","reason":"already measured"}]),
        ),
    ] {
        let mut invalid_input = valid.clone();
        *invalid_input.pointer_mut(pointer).unwrap() = invalid;
        assert!(
            compare_json(&serde_json::to_vec(&invalid_input).unwrap()).is_err(),
            "{pointer}"
        );
    }
    for pointer in ["/current/catalog", "/current/results/results"] {
        let mut input = valid.clone();
        let array = input.pointer_mut(pointer).unwrap().as_array_mut().unwrap();
        array.push(array[0].clone());
        assert!(compare_json(&serde_json::to_vec(&input).unwrap()).is_err());
    }
    let mut mismatched = saved(5);
    mismatched.current.results.results[0].direction = Direction::Min;
    set_actual(&mut mismatched.current, 15.0);
    let mismatch = compare_json(&encode(&mismatched)).unwrap_err();
    assert!(mismatch.contains("mismatched result"));
    let mut invalid = valid;
    invalid["baseline"][0]["results"]["results"][0]["verdict"] = json!("over");
    let report = compare_json(&serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert_eq!(report.rows[0].baseline_n, 4);
    assert_eq!(report.current.results.exit_code(), 1);
    assert!(report.diagnostics.iter().any(|d| d.contains("contradicts")));
}

#[test]
fn typed_invalid_inputs_and_extreme_arithmetic_have_no_invented_statistics() {
    let mut input = saved(5);
    input.schema_version = 2;
    let report = compare(&input);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.contains("schema_version"))
    );
    assert_eq!(report.rows[0].mean, None);
    input.schema_version = 1;
    input.current.repository.clear();
    assert!(
        compare(&input)
            .diagnostics
            .iter()
            .any(|d| d.contains("current:"))
    );
    input = saved(5);
    input.baseline[0].results.results[0].actual = Some(f64::NAN);
    assert_eq!(compare(&input).rows[0].baseline_n, 4);
    for run in std::iter::once(&mut input.current).chain(input.baseline.iter_mut()) {
        run.catalog[0].threshold = 0.0;
        run.results.results[0].threshold = 0.0;
        set_actual(run, 0.0);
    }
    for (run, value) in
        input
            .baseline
            .iter_mut()
            .zip([-f64::MAX, -f64::MAX, -f64::MAX, f64::MAX, f64::MAX])
    {
        set_actual(run, value);
    }
    let report = compare_json(&encode(&input)).unwrap();
    assert_eq!(report.rows[0].sd, None);
    assert!(
        report.rows[0]
            .unavailable
            .as_ref()
            .unwrap()
            .contains("finite range")
    );
    for run in &mut input.baseline {
        set_actual(run, f64::MIN_POSITIVE);
    }
    set_actual(&mut input.current, 1.0);
    let report = compare_json(&encode(&input)).unwrap();
    assert_eq!(report.rows[0].percent_change, None);
    assert!(
        report.rows[0]
            .unavailable
            .as_ref()
            .unwrap()
            .contains("percent change exceeds")
    );
    for (run, value) in
        input
            .baseline
            .iter_mut()
            .zip([0.0, f64::MIN_POSITIVE, 0.0, f64::MIN_POSITIVE, 0.0])
    {
        set_actual(run, value);
    }
    set_actual(&mut input.current, f64::MAX);
    let report = compare_json(&encode(&input)).unwrap();
    assert_eq!(report.rows[0].sigma_change, None);
    assert!(
        report.rows[0]
            .unavailable
            .as_ref()
            .unwrap()
            .contains("sigma change exceeds")
    );
}

#[test]
fn rolling_history_rejects_invalid_headers_duplicates_and_nested_runs() {
    let input = saved(20);
    let valid = RollingHistory {
        schema_version: 1,
        repository: "owner/repo".into(),
        base_ref: "main".into(),
        runs: input.baseline,
    };
    validate_history(&valid).unwrap();
    for change in [
        "version",
        "excess",
        "duplicate",
        "ref",
        "repository",
        "nested",
    ] {
        let mut history = valid.clone();
        match change {
            "version" => history.schema_version = 2,
            "excess" => history.runs.push(history.runs[0].clone()),
            "duplicate" => history.runs[1] = history.runs[0].clone(),
            "ref" => history.base_ref = "other".into(),
            "repository" => history.repository.clear(),
            _ => history.runs[0].attempt = 0,
        }
        assert!(validate_history(&history).is_err(), "{change}");
    }
    let mut run = observation(1.0);
    run.results.results[0].host.load1 = Some(f64::INFINITY);
    assert!(validate_run(&run).is_err());
}

#[test]
fn markdown_escapes_saved_paths_error_reasons_and_provenance() {
    let mut input = saved(0);
    input.current.catalog[0].file = "a<>&|/budgets.yaml".into();
    input.current.results.results[0].file = input.current.catalog[0].file.clone();
    errored(&mut input.current);
    input.current.results.results[0].error = Some("line1\n<script>|x".into());
    input.current.head_sha = "head<>&".into();
    let markdown = render_markdown(&compare_json(&encode(&input)).unwrap());
    assert!(markdown.contains("a&lt;&gt;&amp;&#124;/budgets.yaml"));
    assert!(markdown.contains("line1 &lt;script&gt;&#124;x"));
    assert!(!markdown.contains("<script>"));
    assert!(markdown.contains("head&lt;&gt;&amp;"));
}
