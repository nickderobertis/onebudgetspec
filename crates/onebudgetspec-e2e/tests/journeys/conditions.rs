//! Declared conditions: each file's condition commands run once per check, and every
//! result measured from that file — and no other — carries their values.

use serde_json::{Value, json};

use crate::common::{Fixture, ids, reported, result, results};

fn with_condition(fixture: &Fixture, dir: &str, name: &str, value: &str, ids: &[&str]) -> Value {
    let log = format!("{dir}-{name}.log");
    fixture.counted(
        &format!("{dir}/{name}.sh"),
        &log,
        name,
        &format!("printf '  {value}\\n\\n'"),
    );
    let budgets: Vec<Value> = ids.iter().map(|id| reported(id, 1.0, "max", 2.0)).collect();
    json!({
        "schema_version": 1,
        "conditions": [{ "name": name, "command": [format!("./{name}.sh")] }],
        "budgets": budgets,
    })
}

#[test]
fn a_condition_runs_once_per_check_and_every_result_carries_it_trimmed() {
    let fixture = Fixture::new();
    let contents = with_condition(&fixture, ".", "dispatches", "3", &["a", "b", "c"]);
    fixture.budgets("budgets.yaml", &contents);

    let report = fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    assert_eq!(fixture.log(".-dispatches.log"), ["dispatches"]);
    for result in results(&report) {
        assert_eq!(result["host"]["conditions"], json!({ "dispatches": "3" }));
    }
}

#[test]
fn each_discovered_file_runs_its_own_conditions_once_and_records_only_its_own() {
    let fixture = Fixture::new();
    let api = with_condition(
        &fixture,
        "api",
        "api_workers",
        "4",
        &["api-latency", "api-errors"],
    );
    let web = with_condition(
        &fixture,
        "web",
        "web_build",
        "17",
        &["web-size", "web-paint"],
    );
    fixture.budgets("api/budgets.yaml", &api);
    fixture.budgets("web/budgets.yaml", &web);

    let report = fixture
        .run(["check", "--json", "--recursive"])
        .expect_status(0)
        .check_report();
    assert_eq!(
        ids(&report, "results"),
        ["api-latency", "api-errors", "web-size", "web-paint"]
    );
    assert_eq!(fixture.log("api-api_workers.log"), ["api_workers"]);
    assert_eq!(fixture.log("web-web_build.log"), ["web_build"]);
    for id in ["api-latency", "api-errors"] {
        let result = result(&report, id);
        assert_eq!(result["host"]["conditions"], json!({ "api_workers": "4" }));
        assert_eq!(result["file"], "api/budgets.yaml");
    }
    for id in ["web-size", "web-paint"] {
        let result = result(&report, id);
        assert_eq!(result["host"]["conditions"], json!({ "web_build": "17" }));
        assert_eq!(result["file"], "web/budgets.yaml");
    }
}

#[test]
fn a_failing_condition_is_recorded_as_unknown() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [
                { "name": "broken", "command": ["sh", "-c", "echo partial; exit 1"] },
                { "name": "missing", "command": ["./no-such-program"] },
                { "name": "fine", "command": ["echo", "ok"] },
            ],
            "budgets": [reported("only", 1.0, "max", 2.0)],
        }),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(0).check_report();
    assert_eq!(
        result(&report, "only")["host"]["conditions"],
        json!({ "broken": "unknown", "missing": "unknown", "fine": "ok" })
    );
    assert!(run.stderr.contains("broken"), "{}", run.stderr);
}
