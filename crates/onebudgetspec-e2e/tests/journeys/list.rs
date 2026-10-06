//! `list` reports the selected budgets with every field, and runs no command.

use serde_json::{Value, json};

use crate::common::{Fixture, ids};

fn registered(fixture: &Fixture) {
    fixture.counted("condition.js", "ran.log", "condition", "console.log(1);");
    fixture.counted("measure.js", "ran.log", "budget", "");
    let budget = |id: &str, labels: Value| {
        json!({
            "id": id,
            "labels": labels,
            "measure": "elapsed",
            "command": ["node", "../measure.js", "--fast"],
            "unit": "seconds",
            "direction": "max",
            "threshold": 30,
        })
    };
    let mut detailed = budget("api-latency", json!(["api"]));
    detailed["description"] = json!("p95 of the smoke run");
    detailed["timeout_seconds"] = json!(120);
    detailed["measure"] = json!("reported");
    detailed["unit"] = json!("ms");
    detailed["direction"] = json!("min");
    let conditions = json!([{ "name": "probe", "command": ["node", "../condition.js"] }]);
    fixture.budgets(
        "api/budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": conditions,
            "budgets": [detailed, budget("api-errors", json!(["api", "flaky"]))],
        }),
    );
    fixture.budgets(
        "web/budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": conditions,
            "budgets": [budget("web-size", json!(["web"]))],
        }),
    );
}

#[test]
fn list_reports_every_field_of_the_selected_budgets() {
    let fixture = Fixture::new();
    registered(&fixture);
    let report = fixture
        .run(["list", "--json", "--recursive"])
        .expect_status(0)
        .list_report();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(
        ids(&report, "budgets"),
        ["api-latency", "api-errors", "web-size"]
    );
    assert_eq!(
        report["budgets"][0],
        json!({
            "id": "api-latency",
            "file": "api/budgets.yaml",
            "description": "p95 of the smoke run",
            "labels": ["api"],
            "measure": "reported",
            "command": ["node", "../measure.js", "--fast"],
            "unit": "ms",
            "direction": "min",
            "threshold": 30.0,
            "timeout_seconds": 120,
        })
    );
    assert!(report["budgets"][1]["description"].is_null());
    assert!(report["budgets"][1]["timeout_seconds"].is_null());
    assert!(fixture.log("ran.log").is_empty(), "list ran a command");
}

#[test]
fn list_selects_like_check() {
    let fixture = Fixture::new();
    registered(&fixture);
    let listed = |args: &[&str]| {
        let mut all = vec!["list", "--json", "--recursive"];
        all.extend_from_slice(args);
        ids(&fixture.run(all).expect_status(0).list_report(), "budgets")
    };
    assert_eq!(listed(&["--id", "web-size"]), ["web-size"]);
    assert_eq!(listed(&["--label", "api"]), ["api-latency", "api-errors"]);
    assert_eq!(
        listed(&["--label", "api", "--exclude-label", "flaky"]),
        ["api-latency"]
    );
    assert_eq!(listed(&["--exclude-label", "api"]), ["web-size"]);
    fixture
        .run(["list", "--recursive", "--id", "missing"])
        .expect_status(2);
    assert!(fixture.log("ran.log").is_empty(), "list ran a command");

    let text = fixture.run(["list", "--recursive", "--id", "api-latency"]);
    assert_eq!(
        text.expect_status(0).stdout,
        "api-latency (api/budgets.yaml): reported min 30 ms [api]\n"
    );
    let text = fixture.run(["list", "--recursive", "--label", "web"]);
    assert_eq!(
        text.expect_status(0).stdout,
        "web-size (web/budgets.yaml): elapsed max 30 seconds [web]\n"
    );
}
