//! Measurements that fail are reported as errors, never as values.

use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::common::{Fixture, exits, file, js, node, reports, result, results, write_result};

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

#[test]
fn every_kind_of_failed_measurement_is_an_error() {
    let fixture = Fixture::new();
    let mut slow = budget("times-out", &node("setTimeout(() => {}, 30000);", &[]));
    slow["timeout_seconds"] = json!(1);
    // Only the Unix-only signal case below is pushed onto the list.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut cases = vec![
        (budget("fails", &exits(2)), "status 2"),
        (slow, "timed out"),
        (budget("empty", &node("", &[])), "empty"),
        (budget("whitespace", &reports(" \n\t \r\n")), "empty"),
        (budget("not-json", &reports("value: 3")), "not valid JSON"),
        (
            budget("not-an-object", &reports("[3]")),
            "an array rather than a JSON object",
        ),
        (
            budget("bad-detail", &reports(r#"{"value": 3, "detail": 7}"#)),
            "\"detail\" is a number rather than a string",
        ),
        (
            budget(
                "removed-result",
                &node(
                    "require(\"fs\").unlinkSync(process.env.ONEBUDGETSPEC_RESULT);",
                    &[],
                ),
            ),
            "cannot read the result file",
        ),
        (
            budget("no-value", &reports(r#"{"detail": "x"}"#)),
            "no \"value\"",
        ),
        (
            budget("string-value", &reports(r#"{"value": "3"}"#)),
            "rather than a number",
        ),
        (
            budget("null-value", &reports(r#"{"value": null}"#)),
            "rather than a number",
        ),
        (
            budget("boolean-value", &reports(r#"{"value": true}"#)),
            "a boolean rather than a number",
        ),
        (
            budget("object-value", &reports(r#"{"value": {"p95": 3}}"#)),
            "an object rather than a number",
        ),
        (
            budget("unknown-key", &reports(r#"{"value": 3, "unit": "ms"}"#)),
            "unknown key \"unit\"",
        ),
        (
            budget("missing-program", &json!(["./no-such-program"])),
            "cannot run ./no-such-program",
        ),
        (budget("empty-program", &json!([""])), "cannot run"),
        (
            budget("nul-argument", &json!(["node", "a\u{0}b"])),
            "cannot run node",
        ),
        // Over budget AND failing: the error wins.
        (
            budget(
                "over-then-fails",
                &node(
                    &format!("{} process.exitCode = 1;", write_result(r#"{"value": 99}"#)),
                    &[],
                ),
            ),
            "status 1",
        ),
    ];
    // Unix only: a process ended by a signal exists only there.
    #[cfg(unix)]
    cases.push((
        budget(
            "killed",
            &node("process.kill(process.pid, \"SIGKILL\");", &[]),
        ),
        "terminated by signal 9",
    ));
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

/// The error of the one budget in `command`'s check, which must have failed.
fn failure(command: &Value) -> String {
    let fixture = Fixture::new();
    fixture.budgets("budgets.yaml", &file(&[budget("failing", command)]));
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    let errored = result(&report, "failing");
    assert_eq!(errored["verdict"], "error", "{errored:#}");
    errored["error"].as_str().unwrap().to_owned()
}

#[test]
fn a_failing_command_is_described_by_its_exit_status() {
    assert_eq!(failure(&exits(4)), "node exited with status 4");
    assert_eq!(failure(&exits(255)), "node exited with status 255");
}

/// Windows exit codes: one with its top bit set is an NTSTATUS, such as an access
/// violation, and reads as one.
#[cfg(windows)]
#[test]
fn a_windows_exit_code_is_described_in_decimal_or_as_an_ntstatus() {
    assert_eq!(
        failure(&node("process.exit(4);", &[])),
        "node exited with status 4"
    );
    assert_eq!(
        failure(&node("process.exit(-1073741819);", &[])),
        "node exited with status 0xC0000005"
    );
}

#[test]
fn a_timeout_ends_the_command_promptly() {
    let fixture = Fixture::new();
    let mut slow = budget("times-out", &node("setTimeout(() => {}, 60000);", &[]));
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
    assert!(
        errored["error"]
            .as_str()
            .unwrap()
            .starts_with("node timed out after 1 seconds and was killed"),
        "{errored:#}"
    );
    let started_at = chrono::DateTime::parse_from_rfc3339(errored["started_at"].as_str().unwrap());
    let ended_at = chrono::DateTime::parse_from_rfc3339(errored["ended_at"].as_str().unwrap());
    let took = ended_at.unwrap() - started_at.unwrap();
    assert!(took.num_milliseconds() >= 1000, "{errored:#}");
}

/// How long the child of a timed-out command waits before it would mark that it survived:
/// well past the timeout, however slowly Node starts.
const SURVIVAL_DELAY_MS: u64 = 8000;

/// The budget's command is a Node process that starts a second one and waits far past
/// the timeout. The child marks `started` at once and, had it survived, would mark
/// `survived` once its delay passed; the timeout must end it along with its parent.
#[test]
fn a_timeout_ends_every_process_the_command_started() {
    let fixture = Fixture::new();
    let started = fixture.path().join("started");
    let survived = fixture.path().join("survived");
    let child = format!(
        "const fs = require(\"fs\"); \
         fs.writeFileSync(process.argv[1], String(Date.now())); \
         setTimeout(() => fs.writeFileSync(process.argv[2], \"\"), {SURVIVAL_DELAY_MS});"
    );
    // On Unix the child stays in its parent's process group. On Windows it is detached,
    // because Node otherwise puts it in a job of its own that ends it with its parent, and
    // only the timeout's ending of the whole tree may end it here.
    let parent = format!(
        "require(\"child_process\").spawn(process.execPath, \
         [\"-e\", {}, process.argv[1], process.argv[2]], \
         {{ stdio: \"ignore\", detached: process.platform === \"win32\" }}); \
         setTimeout(() => {{}}, 120000);",
        js(&child)
    );
    let mut tree = budget(
        "tree",
        &node(
            &parent,
            &[&started.to_string_lossy(), &survived.to_string_lossy()],
        ),
    );
    tree["timeout_seconds"] = json!(4);
    fixture.budgets("budgets.yaml", &file(&[tree]));

    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    let errored = result(&report, "tree");
    assert!(
        errored["error"]
            .as_str()
            .unwrap()
            .starts_with("node timed out after 4 seconds and was killed"),
        "{errored:#}"
    );

    let marked = std::fs::read_to_string(&started)
        .expect("the child started before the timeout, so there was a process tree to end");
    let marked_ms: u64 = marked.parse().expect("the child marked when it started");
    // Wait until the child, had it survived, would have marked so, and two seconds more.
    let due = std::time::UNIX_EPOCH + Duration::from_millis(marked_ms + SURVIVAL_DELAY_MS + 2000);
    if let Ok(wait) = due.duration_since(SystemTime::now()) {
        std::thread::sleep(wait);
    }
    assert!(
        !survived.exists(),
        "the child the command started outlived the timeout"
    );
}

#[test]
fn a_result_file_that_cannot_be_created_is_an_error() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget("needs-a-file", &reports(r#"{"value": 1}"#))]),
    );
    let missing = fixture.path().join("no-such-temporary-directory");
    let missing = missing.to_str().unwrap();
    // Unix reads the temporary directory from TMPDIR, Windows from TMP and then TEMP.
    let run = crate::common::run_in(
        fixture.path(),
        ["check", "--json"],
        &[("TMPDIR", missing), ("TMP", missing), ("TEMP", missing)],
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
