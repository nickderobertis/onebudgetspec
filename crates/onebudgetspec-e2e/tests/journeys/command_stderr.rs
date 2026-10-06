//! A command that fails, or whose result cannot be read, has the tail of its stderr in its
//! reason; a command that succeeds is reported as if it had written nothing there.

use serde_json::{Value, json};

use crate::common::{Fixture, file, reported, result};

fn budget(id: &str, measure: &str, command: &Value) -> Value {
    json!({
        "id": id,
        "measure": measure,
        "command": command,
        "unit": "seconds",
        "direction": "max",
        "threshold": 60,
    })
}

/// A measurement script that explains on stderr why it cannot measure, the way a script
/// with no telemetry to read does.
fn explaining(fixture: &Fixture) {
    fixture.script(
        "explain.sh",
        "printf 'telemetry is absent\\n\\n   run the collector first  \\n' >&2\nexit 1",
    );
}

const EXPLAINED: &str =
    "./explain.sh exited with status 1; its stderr: telemetry is absent | run the collector first";

fn error(report: &Value, id: &str) -> String {
    let errored = result(report, id);
    assert_eq!(errored["verdict"], "error", "{errored:#}");
    errored["error"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn a_failing_command_names_its_exit_status_and_its_stderr() {
    let fixture = Fixture::new();
    explaining(&fixture);
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            budget("reported", "reported", &json!(["./explain.sh"])),
            budget("elapsed", "elapsed", &json!(["./explain.sh"])),
            budget("silent", "elapsed", &json!(["sh", "-c", "exit 4"])),
        ]),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(3).check_report();
    assert_eq!(error(&report, "reported"), EXPLAINED);
    assert_eq!(error(&report, "elapsed"), EXPLAINED);
    // A command that wrote nothing to stderr is reported as before.
    assert_eq!(error(&report, "silent"), "sh exited with status 4");
    // Its stderr still reaches the binary's stderr as written.
    assert!(
        run.stderr.contains("   run the collector first  \n"),
        "{}",
        run.stderr
    );
}

#[test]
fn the_text_line_carries_the_same_reason() {
    let fixture = Fixture::new();
    explaining(&fixture);
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget("explained", "reported", &json!(["./explain.sh"]))]),
    );
    let run = fixture.run(["check"]);
    run.expect_status(3);
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{}", run.stdout);
    assert!(
        lines[0].starts_with(&format!("budget explained: error — {EXPLAINED}; host: ")),
        "{}",
        lines[0]
    );
}

#[test]
fn only_the_bounded_tail_of_a_long_stderr_is_kept() {
    let fixture = Fixture::new();
    fixture.script(
        "verbose.sh",
        "echo FIRST-LINE >&2\n\
         i=0\n\
         while [ $i -lt 500 ]; do echo \"progress line $i of the noise\" >&2; i=$((i+1)); done\n\
         echo LAST-LINE >&2\n\
         exit 1",
    );
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget("verbose", "elapsed", &json!(["./verbose.sh"]))]),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(3).check_report();
    let error = error(&report, "verbose");
    let (exit, tail) = error
        .split_once("; its stderr: ")
        .unwrap_or_else(|| panic!("{error}"));
    assert_eq!(exit, "./verbose.sh exited with status 1");
    assert!(tail.starts_with('…'), "{tail}");
    assert!(
        tail.ends_with("progress line 499 of the noise | LAST-LINE"),
        "{tail}"
    );
    assert!(!tail.contains("FIRST-LINE"), "{tail}");
    // The bound README states: at most the last 1000 characters, after the leading `…`.
    let kept = tail.chars().count() - 1;
    assert!(
        (990..=1000).contains(&kept),
        "{kept} characters kept: {tail}"
    );
    // Only the report is bounded; the binary's stderr still has every line.
    assert!(run.stderr.contains("FIRST-LINE"), "{}", run.stderr);
}

#[test]
fn stderr_that_is_not_utf8_is_kept_with_replacement_characters() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget(
            "raw",
            "elapsed",
            &json!(["sh", "-c", "printf 'bad \\377 byte\\n' >&2; exit 2"]),
        )]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert_eq!(
        error(&report, "raw"),
        "sh exited with status 2; its stderr: bad \u{FFFD} byte"
    );
}

#[test]
fn an_unreadable_result_names_the_exit_status_and_the_stderr() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            budget(
                "not-json",
                "reported",
                &json!([
                    "sh",
                    "-c",
                    "echo 'could not reach the database' >&2; printf nope > \"$ONEBUDGETSPEC_RESULT\""
                ]),
            ),
            budget(
                "empty",
                "reported",
                &json!(["sh", "-c", "echo 'no samples today' >&2"]),
            ),
        ]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    let not_json = error(&report, "not-json");
    assert!(
        not_json.starts_with("the result file is not valid JSON"),
        "{not_json}"
    );
    assert!(
        not_json.ends_with("; sh exited with status 0; its stderr: could not reach the database"),
        "{not_json}"
    );
    let empty = error(&report, "empty");
    assert!(
        empty.starts_with("the command left its result file empty"),
        "{empty}"
    );
    assert!(
        empty.ends_with("; sh exited with status 0; its stderr: no samples today"),
        "{empty}"
    );
}

#[test]
fn a_timed_out_command_keeps_what_it_wrote_to_stderr() {
    let fixture = Fixture::new();
    let mut slow = budget(
        "slow",
        "elapsed",
        &json!(["sh", "-c", "echo 'waiting on the lock' >&2; sleep 30"]),
    );
    slow["timeout_seconds"] = json!(1);
    fixture.budgets("budgets.yaml", &file(&[slow]));
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert_eq!(
        error(&report, "slow"),
        "sh timed out after 1 seconds and was killed; its stderr: waiting on the lock"
    );
}

#[test]
fn a_succeeding_command_is_reported_as_if_it_wrote_nothing_to_stderr() {
    let fixture = Fixture::new();
    let quiet = reported("quiet", 3.0, "max", 10.0);
    let mut noisy = quiet.clone();
    noisy["id"] = json!("noisy");
    noisy["command"] = json!([
        "sh",
        "-c",
        "echo 'warning: cache cold' >&2; printf '{\"value\": 3}' > \"$ONEBUDGETSPEC_RESULT\""
    ]);
    fixture.budgets("budgets.yaml", &file(&[quiet, noisy]));
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(0).check_report();
    let comparable = |id: &str| {
        let mut result = result(&report, id).clone();
        for varies in ["id", "started_at", "ended_at", "host"] {
            result.as_object_mut().unwrap().remove(varies);
        }
        result
    };
    assert_eq!(comparable("noisy"), comparable("quiet"));
    assert!(result(&report, "noisy")["error"].is_null());
    assert!(!report.to_string().contains("cache cold"), "{report:#}");
    assert!(run.stderr.contains("warning: cache cold"), "{}", run.stderr);
}

#[test]
fn a_failing_condition_states_its_stderr_on_the_diagnostic_line_and_stays_unknown() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [{
                "name": "commit",
                "command": ["sh", "-c", "echo 'fatal: not a git repository' >&2; exit 128"],
            }],
            "budgets": [reported("only", 1.0, "max", 2.0)],
        }),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(0).check_report();
    let only = result(&report, "only");
    assert_eq!(only["verdict"], "within");
    assert!(only["error"].is_null());
    assert_eq!(only["host"]["conditions"], json!({ "commit": "unknown" }));
    assert!(
        run.stderr.contains(
            "onebudgetspec: condition commit in budgets.yaml: sh exited with status 128; \
             its stderr: fatal: not a git repository; recorded as unknown"
        ),
        "{}",
        run.stderr
    );
}
