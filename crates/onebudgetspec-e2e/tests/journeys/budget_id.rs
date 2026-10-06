//! `ONEBUDGETSPEC_BUDGET_ID` names the budget whose command is running: every budget's
//! command, `elapsed` and `reported` alike, is given its own id, and no condition's command
//! is given one, so one generic runner can serve every budget of a file.

use std::fs;

use serde_json::json;

use crate::common::{Fixture, file, js, node, result, run_in};

/// JavaScript evaluating to the id the command was given, or `unset`.
const ID: &str = "(process.env.ONEBUDGETSPEC_BUDGET_ID ?? \"unset\")";

#[test]
fn every_budget_command_is_given_its_id_and_no_condition_command_is() {
    let fixture = Fixture::new();
    let recorded = fixture.path().join("elapsed-id.txt");
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [{ "name": "seen_id", "command": node(&format!("console.log({ID});"), &[]) }],
            "budgets": [
                {
                    "id": "timed",
                    "measure": "elapsed",
                    "command": node(
                        &format!("require(\"fs\").writeFileSync({}, {ID});", js(&recorded.to_string_lossy())),
                        &[],
                    ),
                    "unit": "seconds",
                    "direction": "max",
                    "threshold": 60,
                },
                {
                    "id": "counted",
                    "measure": "reported",
                    "command": node(
                        "require(\"fs\").writeFileSync(process.env.ONEBUDGETSPEC_RESULT, \
                         JSON.stringify({ value: 1, detail: process.env.ONEBUDGETSPEC_BUDGET_ID ?? \"unset\" }));",
                        &[],
                    ),
                    "unit": "requests",
                    "direction": "max",
                    "threshold": 10,
                },
            ],
        }),
    );
    // A check run by another check's budget inherits that budget's id; it reaches none of
    // this check's conditions and is replaced in each of its budgets.
    let run = run_in(
        fixture.path(),
        ["check", "--json"],
        &[("ONEBUDGETSPEC_BUDGET_ID", "outer")],
    );
    let report = run.expect_status(0).check_report();
    assert_eq!(fs::read_to_string(&recorded).unwrap(), "timed");
    let counted = result(&report, "counted");
    assert_eq!(counted["detail"], "counted");
    for id in ["timed", "counted"] {
        assert_eq!(
            result(&report, id)["host"]["conditions"]["seen_id"],
            "unset",
            "a condition's command was given a budget id"
        );
    }
}

#[test]
fn one_generic_runner_serves_two_budgets_with_a_figure_for_each() {
    let fixture = Fixture::new();
    fixture.script(
        "measure.js",
        r#"const figures = { "api-requests": 12, "worker-requests": 30 };
const id = process.env.ONEBUDGETSPEC_BUDGET_ID;
if (!(id in figures)) throw new Error(`no measurement for ${id}`);
require("fs").writeFileSync(process.env.ONEBUDGETSPEC_RESULT, JSON.stringify({ value: figures[id], detail: id }));"#,
    );
    let budget = |id: &str| {
        json!({
            "id": id,
            "measure": "reported",
            "command": ["node", "measure.js"],
            "unit": "requests",
            "direction": "max",
            "threshold": 20,
        })
    };
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget("api-requests"), budget("worker-requests")]),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(1).check_report();
    let api = result(&report, "api-requests");
    assert_eq!(api["actual"].as_f64(), Some(12.0));
    assert_eq!(api["verdict"], "within");
    assert_eq!(api["detail"], "api-requests");
    let worker = result(&report, "worker-requests");
    assert_eq!(worker["actual"].as_f64(), Some(30.0));
    assert_eq!(worker["verdict"], "over");
    assert_eq!(worker["detail"], "worker-requests");
}
