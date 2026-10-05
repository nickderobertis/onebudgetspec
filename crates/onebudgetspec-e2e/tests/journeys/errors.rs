//! Measurements that fail are reported as errors, never as values.

use serde_json::{Value, json};

use crate::common::{Fixture, file, result, results};

fn budget(id: &str, command: &Value) -> Value {
    json!({
        "id": id,
        "measure": "reported",
        "command": command,
        "unit": "requests",
        "direction": "max",
        "threshold": 10,
    })
}

fn writes(text: &str) -> Value {
    json!([
        "sh",
        "-c",
        format!("printf '%s' '{text}' > \"$ONEBUDGETSPEC_RESULT\"")
    ])
}

#[test]
fn every_kind_of_failed_measurement_is_an_error() {
    let fixture = Fixture::new();
    let mut slow = budget("times-out", &json!(["sleep", "30"]));
    slow["timeout_seconds"] = json!(1);
    let cases = [
        (budget("fails", &json!(["sh", "-c", "exit 2"])), "status 2"),
        (slow, "timed out"),
        (budget("empty", &json!(["true"])), "empty"),
        (budget("not-json", &writes("value: 3")), "not valid JSON"),
        (
            budget("not-an-object", &writes("[3]")),
            "an array rather than a JSON object",
        ),
        (
            budget("bad-detail", &writes(r#"{"value": 3, "detail": 7}"#)),
            "\"detail\" is a number rather than a string",
        ),
        (
            budget(
                "removed-result",
                &json!(["sh", "-c", "rm \"$ONEBUDGETSPEC_RESULT\""]),
            ),
            "cannot read the result file",
        ),
        (
            budget("no-value", &writes(r#"{"detail": "x"}"#)),
            "no \"value\"",
        ),
        (
            budget("string-value", &writes(r#"{"value": "3"}"#)),
            "rather than a number",
        ),
        (
            budget("null-value", &writes(r#"{"value": null}"#)),
            "rather than a number",
        ),
        (
            budget("boolean-value", &writes(r#"{"value": true}"#)),
            "a boolean rather than a number",
        ),
        (
            budget("object-value", &writes(r#"{"value": {"p95": 3}}"#)),
            "an object rather than a number",
        ),
        (
            budget("unknown-key", &writes(r#"{"value": 3, "unit": "ms"}"#)),
            "unknown key \"unit\"",
        ),
        (
            budget("killed", &json!(["sh", "-c", "kill -9 $$"])),
            "terminated by signal 9",
        ),
        (
            budget("missing-program", &json!(["./no-such-program"])),
            "cannot run ./no-such-program",
        ),
        (budget("empty-program", &json!([""])), "cannot run"),
        (
            budget("nul-argument", &json!(["echo", "a\u{0}b"])),
            "cannot run echo",
        ),
        // Over budget AND failing: the error wins.
        (
            budget(
                "over-then-fails",
                &json!([
                    "sh",
                    "-c",
                    "printf '{\"value\": 99}' > \"$ONEBUDGETSPEC_RESULT\"; exit 1"
                ]),
            ),
            "status 1",
        ),
    ];
    let budgets: Vec<Value> = cases.iter().map(|(budget, _)| budget.clone()).collect();
    fixture.budgets("budgets.yaml", &file(&budgets));

    let run = fixture.run(["check", "--json"]);
    run.expect_status(3);
    let report = run.check_report();
    assert_eq!(results(&report).len(), cases.len());
    for (budget, reason) in &cases {
        let id = budget["id"].as_str().unwrap();
        let errored = result(&report, id);
        assert_eq!(errored["verdict"], "error", "{errored:#}");
        let error = errored["error"].as_str().unwrap_or_default();
        assert!(!error.is_empty(), "{errored:#}");
        assert!(
            error.contains(reason),
            "{id}: {error:?} does not say {reason:?}"
        );
        assert!(errored["actual"].is_null(), "{errored:#}");
        assert!(errored["headroom"].is_null(), "{errored:#}");
        assert!(errored["headroom_percent"].is_null(), "{errored:#}");
    }
}

#[test]
fn a_timeout_ends_the_command_promptly() {
    let fixture = Fixture::new();
    let mut slow = budget("times-out", &json!(["sh", "-c", "sleep 30; sleep 30"]));
    slow["timeout_seconds"] = json!(1);
    fixture.budgets("budgets.yaml", &file(&[slow]));
    let started = std::time::Instant::now();
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert!(
        started.elapsed().as_secs() < 20,
        "the timeout did not end the command"
    );
    let errored = result(&report, "times-out");
    let started_at = chrono::DateTime::parse_from_rfc3339(errored["started_at"].as_str().unwrap());
    let ended_at = chrono::DateTime::parse_from_rfc3339(errored["ended_at"].as_str().unwrap());
    let took = ended_at.unwrap() - started_at.unwrap();
    assert!(took.num_milliseconds() >= 1000, "{errored:#}");
}

#[test]
fn a_result_file_that_cannot_be_created_is_an_error() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget("needs-a-file", &writes(r#"{"value": 1}"#))]),
    );
    let missing = fixture.path().join("no-such-temporary-directory");
    let run = crate::common::run_in(
        fixture.path(),
        ["check", "--json"],
        &[("TMPDIR", missing.to_str().unwrap())],
    );
    let report = run.expect_status(3).check_report();
    let errored = result(&report, "needs-a-file");
    assert_eq!(errored["verdict"], "error");
    assert!(
        errored["error"]
            .as_str()
            .unwrap()
            .contains("cannot create the result file"),
        "{errored:#}"
    );
    assert!(errored["actual"].is_null());
}
