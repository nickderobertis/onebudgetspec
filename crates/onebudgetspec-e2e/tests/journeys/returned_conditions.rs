//! Conditions a `reported` command returns: recorded beside the declared ones in that
//! result alone, and refused — never merged — when a name collides or is malformed.

use serde_json::{Value, json};

use crate::common::{Fixture, result, write_result};

/// A budget whose command records its own invocation in `budgets.log`, then writes
/// `result`.
fn budget(fixture: &Fixture, id: &str, result: &str) -> Value {
    let script = format!("{id}.js");
    fixture.counted(&script, "budgets.log", id, &write_result(result));
    json!({
        "id": id,
        "measure": "reported",
        "command": ["node", script],
        "unit": "seconds",
        "direction": "max",
        "threshold": 1800,
    })
}

fn declaring(fixture: &Fixture, budgets: &[Value]) {
    fixture.counted(
        "dispatches.js",
        "conditions.log",
        "dispatches",
        "console.log(\"3\");",
    );
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [{ "name": "dispatches", "command": ["node", "dispatches.js"] }],
            "budgets": budgets,
        }),
    );
}

#[test]
fn returned_conditions_sit_beside_declared_ones_in_their_own_result_only() {
    let fixture = Fixture::new();
    declaring(
        &fixture,
        &[
            budget(
                &fixture,
                "gate-time",
                r#"{"value": 1395, "conditions": {"gate_load": "2.4", "gate_runs": "6"}}"#,
            ),
            budget(&fixture, "plain", r#"{"value": 10}"#),
        ],
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    assert_eq!(
        result(&report, "gate-time")["host"]["conditions"],
        json!({ "dispatches": "3", "gate_load": "2.4", "gate_runs": "6" })
    );
    assert_eq!(
        result(&report, "plain")["host"]["conditions"],
        json!({ "dispatches": "3" })
    );
}

#[test]
fn a_colliding_or_malformed_returned_condition_makes_that_measurement_an_error() {
    let fixture = Fixture::new();
    let refused = [
        (
            "declared-name",
            r#"{"value": 1, "conditions": {"dispatches": "9"}}"#,
            "dispatches",
        ),
        (
            "load1",
            r#"{"value": 1, "conditions": {"load1": "0.1"}}"#,
            "load1",
        ),
        (
            "cpus",
            r#"{"value": 1, "conditions": {"cpus": "64"}}"#,
            "cpus",
        ),
        (
            "mem",
            r#"{"value": 1, "conditions": {"mem_available_mib": "1"}}"#,
            "mem_available_mib",
        ),
        (
            "not-an-object",
            r#"{"value": 1, "conditions": ["dispatches"]}"#,
            "conditions",
        ),
        (
            "malformed-name",
            r#"{"value": 1, "conditions": {"Gate-Load": "1"}}"#,
            "Gate-Load",
        ),
        (
            "not-a-string",
            r#"{"value": 1, "conditions": {"gate_runs": 6}}"#,
            "gate_runs",
        ),
    ];
    let mut budgets: Vec<Value> = refused
        .iter()
        .map(|(id, written, _)| budget(&fixture, id, written))
        .collect();
    budgets.push(budget(&fixture, "fine", r#"{"value": 1}"#));
    declaring(&fixture, &budgets);

    let run = fixture.run(["check", "--json"]);
    run.expect_status(3);
    let report = run.check_report();
    assert_eq!(fixture.log("conditions.log"), ["dispatches"]);
    let mut expected: Vec<&str> = refused.iter().map(|(id, _, _)| *id).collect();
    expected.push("fine");
    assert_eq!(
        fixture.log("budgets.log"),
        expected,
        "every budget measured once each"
    );
    for (id, _, named) in refused {
        let errored = result(&report, id);
        assert_eq!(errored["verdict"], "error", "{errored:#}");
        let error = errored["error"].as_str().unwrap();
        assert!(
            error.contains(named),
            "{id}: {error:?} does not name {named}"
        );
        assert!(errored["actual"].is_null());
        assert_eq!(
            errored["host"]["conditions"],
            json!({ "dispatches": "3" }),
            "an error records the declared conditions and none of the returned ones"
        );
    }
    assert_eq!(result(&report, "fine")["verdict"], "within");
}
