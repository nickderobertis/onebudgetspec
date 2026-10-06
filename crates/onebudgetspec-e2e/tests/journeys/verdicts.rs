//! Verdicts, the figures behind them, and the exit status they earn.

use serde_json::{Value, json};

use crate::common::{Fixture, exits, file, reported, result, results};

/// The headroom and percentage the contract defines, computed here independently.
fn expected(direction: &str, threshold: f64, actual: f64) -> (f64, Option<f64>) {
    let headroom = if direction == "max" {
        threshold - actual
    } else {
        actual - threshold
    };
    let percent = (threshold != 0.0).then(|| headroom / threshold * 100.0);
    (headroom, percent)
}

fn assert_figures(result: &Value, direction: &str, threshold: f64, actual: f64) {
    let (headroom, percent) = expected(direction, threshold, actual);
    assert_eq!(result["actual"].as_f64(), Some(actual), "{result:#}");
    assert_eq!(result["headroom"].as_f64(), Some(headroom), "{result:#}");
    match percent {
        Some(percent) => assert_eq!(result["headroom_percent"].as_f64(), Some(percent)),
        None => assert!(result["headroom_percent"].is_null(), "{result:#}"),
    }
}

#[test]
fn verdicts_and_computed_figures_under_both_directions() {
    let fixture = Fixture::new();
    let cases = [
        ("max-within", 1395.0, "max", 1800.0, "within"),
        ("max-over", 2000.0, "max", 1800.0, "over"),
        ("max-equal", 1800.0, "max", 1800.0, "within"),
        ("min-within", 120.0, "min", 100.0, "within"),
        ("min-over", 80.0, "min", 100.0, "over"),
        ("min-equal", 100.0, "min", 100.0, "within"),
        ("zero-threshold", 0.5, "max", 0.0, "over"),
    ];
    let mut budgets: Vec<Value> = cases
        .iter()
        .map(|(id, value, direction, threshold, _)| reported(id, *value, direction, *threshold))
        .collect();
    budgets[0]["labels"] = json!(["gate", "nightly"]);
    fixture.budgets("budgets.yaml", &file(&budgets));

    let run = fixture.run(["check", "--output", "json"]);
    run.expect_status(1);
    let report = run.check_report();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(results(&report).len(), cases.len());

    for (id, value, direction, threshold, verdict) in cases {
        let result = result(&report, id);
        assert_eq!(result["verdict"], verdict, "{result:#}");
        assert_eq!(result["id"], id);
        assert_eq!(result["unit"], "requests");
        assert_eq!(result["direction"], direction);
        assert_eq!(result["threshold"].as_f64(), Some(threshold));
        assert_eq!(result["file"], "budgets.yaml");
        assert!(result["error"].is_null(), "{result:#}");
        assert_figures(result, direction, threshold, value);
        let headroom = result["headroom"].as_f64().unwrap();
        assert_eq!(headroom < 0.0, verdict == "over", "{result:#}");
    }
    assert_eq!(
        result(&report, "max-within")["labels"],
        json!(["gate", "nightly"])
    );
    assert_eq!(result(&report, "max-over")["labels"], json!([]));
    assert!(result(&report, "zero-threshold")["headroom_percent"].is_null());
}

#[test]
fn every_budget_within_exits_zero() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            reported("one", 1.0, "max", 2.0),
            reported("two", 3.0, "min", 2.0),
        ]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    assert!(results(&report).iter().all(|r| r["verdict"] == "within"));
}

#[test]
fn within_and_over_exits_one_and_an_added_error_exits_three() {
    let fixture = Fixture::new();
    let within = reported("within", 1.0, "max", 2.0);
    let over = reported("over", 3.0, "max", 2.0);
    fixture.budgets("mixed.yaml", &file(&[within.clone(), over.clone()]));
    let broken = json!({
        "id": "broken",
        "measure": "reported",
        "command": exits(1),
        "unit": "requests",
        "direction": "max",
        "threshold": 2,
    });
    fixture.budgets("errored.yaml", &file(&[within, over, broken]));

    fixture
        .run(["check", "--json", "mixed.yaml"])
        .expect_status(1)
        .check_report();
    let report = fixture
        .run(["check", "--json", "errored.yaml"])
        .expect_status(3)
        .check_report();
    assert_eq!(result(&report, "broken")["verdict"], "error");
    assert_eq!(result(&report, "over")["verdict"], "over");
}
