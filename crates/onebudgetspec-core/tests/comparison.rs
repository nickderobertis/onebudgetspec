//! Saved synthetic scalar observations exercise the public comparison interface.
use onebudgetspec_core::comparison::{
    BudgetRun, ComparisonInput, RollingHistory, render_markdown, validate_history,
};
use onebudgetspec_core::{Direction, Verdict};

fn input(n: usize) -> ComparisonInput {
    let current: BudgetRun =
        serde_json::from_str(include_str!("fixtures/budget-run.json")).unwrap();
    let baseline = (0..n)
        .map(|i| {
            let mut run = current.clone();
            run.head_sha = format!("base-{i:02}");
            run.run_id = format!("run-{i}");
            run.results.results[0].actual = Some(10.0 + f64::from(u32::try_from(i % 5).unwrap()));
            let result = &mut run.results.results[0];
            let (verdict, headroom, percent) = onebudgetspec_core::judge(
                result.direction,
                result.threshold,
                result.actual.unwrap(),
            );
            result.verdict = verdict;
            result.headroom = Some(headroom);
            result.headroom_percent = percent;
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
fn ordinary_verdicts_and_uniform_sample_arithmetic_survive() {
    let report = compare(&input(20));
    let row = &report.rows[0];
    assert_eq!(row.verdict, Some(Verdict::Over));
    assert_eq!(row.baseline_n, 20);
    assert_eq!(row.mean, Some(12.0));
    assert!((row.sd.unwrap() - (40.0_f64 / 19.0).sqrt()).abs() < 1e-12);
    assert_eq!(row.percent_change, Some(25.0));
    assert!(row.flag);
    assert!(row.sigma_change.unwrap() > 2.0);
    assert!(render_markdown(&report).contains("report-only"));
    assert_eq!(report.current.results.exit_code(), 1);
}

#[test]
fn both_directions_flag_only_adverse_changes_including_constant_baselines() {
    for direction in [Direction::Max, Direction::Min] {
        let mut data = input(5);
        data.current.catalog[0].direction = direction;
        data.current.results.results[0].direction = direction;
        for run in &mut data.baseline {
            run.catalog[0].direction = direction;
            run.results.results[0].direction = direction;
        }
        for constant in [false, true] {
            if constant {
                for run in &mut data.baseline {
                    run.results.results[0].actual = Some(12.0);
                }
            }
            for (value, adverse) in [
                (0.0, direction == Direction::Min),
                (24.0, direction == Direction::Max),
            ] {
                data.current.results.results[0].actual = Some(value);
                let row = &compare(&data).rows[0];
                assert_eq!(row.flag, adverse);
                assert!(row.mean.is_some());
                assert!(row.percent_change.is_some());
                assert_eq!(row.sigma_change.is_none(), constant);
            }
        }
    }
}

#[test]
fn missing_expired_and_incomplete_data_are_never_zero_samples() {
    for n in [0, 4, 5, 19, 20] {
        let report = compare(&input(n));
        assert_eq!(report.rows[0].baseline_n, u32::try_from(n).unwrap());
        assert_eq!(report.rows[0].mean.is_none(), n < 5);
        // A baseline still filling says how far it has got, and no reason invents a minimum.
        assert_eq!(
            render_markdown(&report).contains(&format!("baseline {n}/5")),
            n < 5
        );
        assert!(
            report.rows[0]
                .unavailable
                .as_deref()
                .is_none_or(|reason| !reason.contains("eligible samples"))
        );
    }
    let mut data = input(5);
    data.current.results.results.clear();
    let row = &compare(&data).rows[0];
    assert!(row.current.is_none());
    assert!(row.verdict.is_none());
    assert!(row.unavailable.as_ref().unwrap().contains("incomplete"));
    for run in &mut data.baseline {
        run.results.results[0].actual = Some(0.0);
    }
    data.current.results.results = input(0).current.results.results;
    data.current.results.results[0].actual = Some(0.0);
    let row = &compare(&data).rows[0];
    assert_eq!(row.sigma_change, Some(0.0));
    assert_eq!(row.percent_change, None);
    assert!(!row.flag);
}

#[test]
fn provenance_unique_commits_latest_attempt_and_scope_are_validated() {
    let mut data = input(5);
    let mut retry = data.baseline[0].clone();
    retry.attempt = 2;
    retry.results.results[0].actual = Some(20.0);
    data.baseline.push(retry);
    assert_eq!(compare(&data).rows[0].mean, Some(14.0));
    assert_eq!(compare(&data).rows[0].baseline_n, 5);
    for field in [
        "repository",
        "base_ref",
        "head_sha",
        "attempt",
        "completed_at",
        "scope",
    ] {
        let mut broken = input(5);
        match field {
            "repository" => broken.baseline[0].repository = "other/repo".into(),
            "base_ref" => broken.baseline[0].base_ref = "other".into(),
            "head_sha" => broken.baseline[0].head_sha = broken.current.head_sha.clone(),
            "attempt" => broken.baseline[0].attempt = 0,
            "completed_at" => broken.baseline[0].completed_at = "invalid".into(),
            _ => broken.baseline[0].catalog[0]
                .command
                .push("different".into()),
        }
        let report = compare(&broken);
        assert_eq!(report.rows[0].baseline_n, 4, "{field}");
        assert_eq!(report.rows[0].mean, None, "{field}");
        // A rejected run is a diagnostic; a run whose scope differs is a reason in the row.
        if field == "scope" {
            assert_eq!(
                report.rows[0].unavailable.as_deref(),
                Some("measurement/command scope mismatch (1 of 5 baseline samples)")
            );
        } else {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|d| d.starts_with(&format!("baseline {}: ", broken.baseline[0].head_sha))),
                "{field}: {:?}",
                report.diagnostics
            );
        }
    }
    let history = RollingHistory {
        schema_version: 1,
        repository: data.current.repository.clone(),
        base_ref: data.current.base_ref.clone(),
        runs: input(20).baseline,
    };
    let roundtrip: RollingHistory =
        serde_json::from_str(&serde_json::to_string(&history).unwrap()).unwrap();
    validate_history(&roundtrip).unwrap();
    let mut invalid = history;
    invalid.repository = "wrong".into();
    assert!(validate_history(&invalid).is_err());
}

#[test]
fn one_hundred_catalog_rows_and_twenty_commits_are_complete_and_failed_first() {
    let mut data = input(20);
    for run in std::iter::once(&mut data.current).chain(data.baseline.iter_mut()) {
        let budget = run.catalog[0].clone();
        let result = run.results.results[0].clone();
        run.catalog = (0..100)
            .map(|i| {
                let mut b = budget.clone();
                b.id = format!("budget-{i}");
                b
            })
            .collect();
        run.results.results = (0..100)
            .map(|i| {
                let mut r = result.clone();
                r.id = format!("budget-{i}");
                r
            })
            .collect();
    }
    data.current.results.results[0].verdict = Verdict::Within;
    data.current.results.results[0].actual = Some(10.0);
    data.current.results.results[99].verdict = Verdict::Error;
    data.current.results.results[99].actual = None;
    data.current.results.results[99].error = Some("cancelled".into());
    let report = compare(&data);
    assert_eq!(report.rows.len(), 100);
    assert_eq!(report.rows[0].id, "budget-99");
    assert_eq!(report.rows[1].verdict, Some(Verdict::Over));
    assert_eq!(report.rows.last().unwrap().verdict, Some(Verdict::Within));
    assert!(report.rows.iter().all(|r| r.baseline_n == 20));
    assert_eq!(report.current.results.exit_code(), 3);
    assert!(render_markdown(&report).contains("cancelled"));
}

#[test]
fn one_sample_sd_is_inclusive_and_zero_mean_has_no_percent() {
    for (direction, actual) in [(Direction::Max, 1.0), (Direction::Min, -1.0)] {
        let mut data = input(5);
        data.current.catalog[0].direction = direction;
        data.current.results.results[0].direction = direction;
        data.current.results.results[0].actual = Some(actual);
        for (run, value) in data.baseline.iter_mut().zip([-1.0, -1.0, 0.0, 1.0, 1.0]) {
            run.catalog[0].direction = direction;
            run.results.results[0].direction = direction;
            run.results.results[0].actual = Some(value);
        }
        let row = &compare(&data).rows[0];
        assert_eq!(row.mean, Some(0.0));
        assert_eq!(row.sd, Some(1.0));
        assert_eq!(row.sigma_change, Some(actual));
        assert_eq!(row.percent_change, None);
        assert!(row.flag);
    }
}

#[test]
fn new_empty_fields_roundtrip_without_changing_old_report_keys() {
    let mut value = serde_json::to_value(input(0).current).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("enforcement_exclude_labels");
    let run: BudgetRun = serde_json::from_value(value).unwrap();
    assert!(run.enforcement_exclude_labels.is_empty());
    let output = serde_json::to_value(run).unwrap();
    assert!(output.get("enforcement_exclude_labels").is_none());
    assert!(
        output["results"]["results"][0]
            .get("detail")
            .unwrap()
            .is_null()
    );
}

fn compare(input: &ComparisonInput) -> onebudgetspec_core::comparison::ComparisonReport {
    let mut input = input.clone();
    for run in std::iter::once(&mut input.current).chain(input.baseline.iter_mut()) {
        for result in &mut run.results.results {
            if let Some(actual) = result.actual {
                let (verdict, headroom, percent) =
                    onebudgetspec_core::judge(result.direction, result.threshold, actual);
                result.verdict = verdict;
                result.headroom = Some(headroom);
                result.headroom_percent = percent;
                result.error = None;
            } else {
                result.headroom = None;
                result.headroom_percent = None;
            }
        }
    }
    onebudgetspec_core::compare(&input)
}
