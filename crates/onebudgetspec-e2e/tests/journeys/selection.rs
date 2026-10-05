//! Selecting budgets by id, label and excluded label, all filters applying together.

use serde_json::{Value, json};

use crate::common::{Fixture, file, ids};

/// Five budgets whose commands each record their own invocation in `runs.log`.
fn labelled(fixture: &Fixture) {
    let budgets: Vec<Value> = [
        ("api-latency", vec!["api", "fast"]),
        ("api-errors", vec!["api"]),
        ("web-size", vec!["web", "fast"]),
        ("web-paint", vec!["web", "flaky"]),
        ("unlabelled", vec![]),
    ]
    .into_iter()
    .map(|(id, labels)| {
        fixture.counted(
            &format!("{id}.sh"),
            "runs.log",
            id,
            "printf '{\"value\": 1}' > \"$ONEBUDGETSPEC_RESULT\"",
        );
        json!({
            "id": id,
            "labels": labels,
            "measure": "reported",
            "command": [format!("./{id}.sh")],
            "unit": "ms",
            "direction": "max",
            "threshold": 5,
        })
    })
    .collect();
    fixture.budgets("budgets.yaml", &file(&budgets));
}

fn checked(fixture: &Fixture, filters: &[&str]) -> Vec<String> {
    let mut args = vec!["check", "--json"];
    args.extend_from_slice(filters);
    ids(
        &fixture.run(args).expect_status(0).check_report(),
        "results",
    )
}

#[test]
fn repeated_id_keeps_exactly_the_named_budgets_and_runs_only_them() {
    let fixture = Fixture::new();
    labelled(&fixture);
    assert_eq!(
        checked(&fixture, &["--id", "web-size", "--id", "api-errors"]),
        ["api-errors", "web-size"]
    );
    assert_eq!(fixture.log("runs.log"), ["api-errors", "web-size"]);
}

#[test]
fn an_unknown_id_is_refused_and_nothing_runs() {
    let fixture = Fixture::new();
    labelled(&fixture);
    let run = fixture.run(["check", "--json", "--id", "api-latency", "--id", "nope"]);
    run.expect_status(2);
    assert!(run.stdout.is_empty(), "{}", run.stdout);
    assert!(run.stderr.contains("nope"), "{}", run.stderr);
    assert!(
        fixture.log("runs.log").is_empty(),
        "a refused invocation ran a command"
    );
}

#[test]
fn labels_keep_either_and_exclusions_win() {
    let fixture = Fixture::new();
    labelled(&fixture);
    assert_eq!(
        checked(&fixture, &["--label", "api", "--label", "web"]),
        ["api-latency", "api-errors", "web-size", "web-paint"]
    );
    assert_eq!(
        checked(&fixture, &["--label", "web", "--exclude-label", "flaky"]),
        ["web-size"]
    );
    assert_eq!(
        checked(
            &fixture,
            &["--exclude-label", "fast", "--exclude-label", "flaky"]
        ),
        ["api-errors", "unlabelled"]
    );
}

#[test]
fn every_filter_applies_together_with_id() {
    let fixture = Fixture::new();
    labelled(&fixture);
    assert_eq!(
        checked(
            &fixture,
            &[
                "--id",
                "api-latency",
                "--id",
                "api-errors",
                "--id",
                "web-size",
                "--label",
                "fast",
                "--exclude-label",
                "web",
            ]
        ),
        ["api-latency"]
    );
}

#[test]
fn a_selection_that_leaves_nothing_reports_no_results_and_exits_zero() {
    let fixture = Fixture::new();
    labelled(&fixture);
    let report = fixture
        .run(["check", "--json", "--label", "no-such-label"])
        .expect_status(0)
        .check_report();
    assert_eq!(report, json!({ "schema_version": 1, "results": [] }));
    assert!(fixture.log("runs.log").is_empty());
}
