//! Declared conditions: each file's condition commands run once per check, and every
//! result measured from that file — and no other — carries their values.

use serde_json::{Value, json};

use crate::common::{Fixture, ids, js, node, reported, result, results, write_result};

fn with_condition(fixture: &Fixture, dir: &str, name: &str, value: &str, ids: &[&str]) -> Value {
    let log = format!("{dir}-{name}.log");
    fixture.counted(
        &format!("{dir}/{name}.js"),
        &log,
        name,
        &format!("process.stdout.write({});", js(&format!("  {value}\n\n"))),
    );
    let budgets: Vec<Value> = ids.iter().map(|id| reported(id, 1.0, "max", 2.0)).collect();
    json!({
        "schema_version": 1,
        "conditions": [{ "name": name, "command": ["node", format!("{name}.js")] }],
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
                { "name": "broken", "command": node("console.log(\"partial\"); process.exitCode = 1;", &[]) },
                { "name": "missing", "command": ["./no-such-program"] },
                { "name": "empty_program", "command": [""] },
                { "name": "nul_argument", "command": ["node", "a\u{0}b"] },
                { "name": "fine", "command": node("console.log(\"ok\");", &[]) },
            ],
            "budgets": [reported("only", 1.0, "max", 2.0)],
        }),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(0).check_report();
    assert_eq!(
        result(&report, "only")["host"]["conditions"],
        json!({
            "broken": "unknown",
            "missing": "unknown",
            "empty_program": "unknown",
            "nul_argument": "unknown",
            "fine": "ok",
        })
    );
    assert!(run.stderr.contains("broken"), "{}", run.stderr);
}

#[test]
fn condition_output_that_is_not_utf8_is_recorded_with_replacement_characters() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [{
                "name": "raw",
                "command": node("process.stdout.write(Buffer.from(\"6f6bff6f6b\", \"hex\"));", &[]),
            }],
            "budgets": [reported("only", 1.0, "max", 2.0)],
        }),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    assert_eq!(
        result(&report, "only")["host"]["conditions"]["raw"],
        "ok\u{fffd}ok"
    );
}

/// One shared log shows the order things ran in: a file's conditions in declaration order,
/// just before its first measured budget, and a file whose budgets are all left out by the
/// selection runs no condition at all.
#[test]
fn conditions_run_in_order_before_their_file_is_measured_and_not_for_an_unselected_file() {
    let fixture = Fixture::new();
    for (dir, conditions, budgets) in [
        (
            "first",
            &["alpha", "beta"][..],
            &["first-one", "first-two"][..],
        ),
        ("second", &["gamma"][..], &["second-one"][..]),
    ] {
        for name in conditions {
            fixture.counted(
                &format!("{dir}/{name}.js"),
                "order.log",
                name,
                "console.log(\"1\");",
            );
        }
        for id in budgets {
            fixture.counted(
                &format!("{dir}/{id}.js"),
                "order.log",
                id,
                &write_result(r#"{"value": 1}"#),
            );
        }
        let conditions: Vec<Value> = conditions
            .iter()
            .map(|name| json!({ "name": name, "command": ["node", format!("{name}.js")] }))
            .collect();
        let budgets: Vec<Value> = budgets
            .iter()
            .map(|id| {
                json!({
                    "id": id,
                    "labels": [dir],
                    "measure": "reported",
                    "command": ["node", format!("{id}.js")],
                    "unit": "runs",
                    "direction": "max",
                    "threshold": 1,
                })
            })
            .collect();
        fixture.budgets(
            &format!("{dir}/budgets.yaml"),
            &json!({ "schema_version": 1, "conditions": conditions, "budgets": budgets }),
        );
    }

    fixture
        .run(["check", "--json", "--recursive"])
        .expect_status(0)
        .check_report();
    assert_eq!(
        fixture.log("order.log"),
        [
            "alpha",
            "beta",
            "first-one",
            "first-two",
            "gamma",
            "second-one"
        ]
    );

    std::fs::remove_file(fixture.path().join("order.log")).unwrap();
    let report = fixture
        .run(["check", "--json", "--recursive", "--exclude-label", "first"])
        .expect_status(0)
        .check_report();
    assert_eq!(ids(&report, "results"), ["second-one"]);
    assert_eq!(fixture.log("order.log"), ["gamma", "second-one"]);
}
