//! A command that fails, or whose result cannot be read, has the tail of its stderr in its
//! reason; a command that succeeds is reported as if it had written nothing there.

use serde_json::{Value, json};

use crate::common::{Fixture, exits, file, js, node, reported, result, write_result};

/// The argv of a command that writes what the JavaScript expression `stderr` gives to its
/// stderr, then exits with `status`.
fn failing(stderr: &str, status: i32) -> Value {
    node(
        &format!("process.stderr.write({stderr}); process.exitCode = {status};"),
        &[],
    )
}

/// The argv of a command that writes `stderr` to its stderr, then runs `then`.
fn noting(stderr: &str, then: &str) -> Value {
    node(
        &format!("process.stderr.write({}); {then}", js(stderr)),
        &[],
    )
}

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
        "explain.js",
        "process.stderr.write(\"telemetry is absent\\n\\n   run the collector first  \\n\");\n\
         process.exitCode = 1;",
    );
}

const EXPLAIN: [&str; 2] = ["node", "explain.js"];

const EXPLAINED: &str =
    "node exited with status 1; its stderr: telemetry is absent | run the collector first";

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
            budget("reported", "reported", &json!(EXPLAIN)),
            budget("elapsed", "elapsed", &json!(EXPLAIN)),
            budget("silent", "elapsed", &exits(4)),
        ]),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(3).check_report();
    assert_eq!(error(&report, "reported"), EXPLAINED);
    assert_eq!(error(&report, "elapsed"), EXPLAINED);
    // A command that wrote nothing to stderr is reported as before.
    assert_eq!(error(&report, "silent"), "node exited with status 4");
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
        &file(&[budget("explained", "reported", &json!(EXPLAIN))]),
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
        "verbose.js",
        "console.error(\"FIRST-LINE\");\n\
         for (let i = 0; i < 500; i++) console.error(`progress line ${i} of the noise`);\n\
         console.error(\"LAST-LINE\");\n\
         process.exitCode = 1;",
    );
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget("verbose", "elapsed", &json!(["node", "verbose.js"]))]),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(3).check_report();
    let error = error(&report, "verbose");
    let (exit, tail) = error
        .split_once("; its stderr: ")
        .unwrap_or_else(|| panic!("{error}"));
    assert_eq!(exit, "node exited with status 1");
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
            &failing("Buffer.from(\"62616420ff20627974650a\", \"hex\")", 2),
        )]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert_eq!(
        error(&report, "raw"),
        "node exited with status 2; its stderr: bad \u{FFFD} byte"
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
                &noting("could not reach the database\n", &write_result("nope")),
            ),
            budget("empty", "reported", &noting("no samples today\n", "")),
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
        not_json.ends_with("; node exited with status 0; its stderr: could not reach the database"),
        "{not_json}"
    );
    let empty = error(&report, "empty");
    assert!(
        empty.starts_with("the command left its result file empty"),
        "{empty}"
    );
    assert!(
        empty.ends_with("; node exited with status 0; its stderr: no samples today"),
        "{empty}"
    );
}

#[test]
fn a_timed_out_command_keeps_what_it_wrote_to_stderr() {
    let fixture = Fixture::new();
    let mut slow = budget(
        "slow",
        "elapsed",
        &noting("waiting on the lock\n", "setTimeout(() => {}, 30000);"),
    );
    // Long enough for Node to start and write before the timeout, on a slow runner too.
    slow["timeout_seconds"] = json!(3);
    fixture.budgets("budgets.yaml", &file(&[slow]));
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert_eq!(
        error(&report, "slow"),
        "node timed out after 3 seconds and was killed; its stderr: waiting on the lock"
    );
}

#[test]
fn a_succeeding_command_is_reported_as_if_it_wrote_nothing_to_stderr() {
    let fixture = Fixture::new();
    let quiet = reported("quiet", 3.0, "max", 10.0);
    let mut noisy = quiet.clone();
    noisy["id"] = json!("noisy");
    noisy["command"] = noting("warning: cache cold\n", &write_result(r#"{"value": 3}"#));
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
                "command": noting("fatal: not a git repository\n", "process.exitCode = 128;"),
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
            "onebudgetspec: condition commit in budgets.yaml: node exited with status 128; \
             its stderr: fatal: not a git repository; recorded as unknown"
        ),
        "{}",
        run.stderr
    );
}

#[test]
fn a_removed_result_file_names_the_exit_status_and_the_stderr() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget(
            "removed",
            "reported",
            &noting(
                "cleaned up too eagerly\n",
                "require(\"fs\").unlinkSync(process.env.ONEBUDGETSPEC_RESULT);",
            ),
        )]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    let removed = error(&report, "removed");
    assert!(
        removed.starts_with("cannot read the result file ONEBUDGETSPEC_RESULT names: "),
        "{removed}"
    );
    assert!(
        removed.ends_with("; node exited with status 0; its stderr: cleaned up too eagerly"),
        "{removed}"
    );
}

// Unix only: a process ended by a signal exists only there.
#[cfg(unix)]
#[test]
fn a_command_killed_by_a_signal_keeps_its_stderr() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget(
            "killed",
            "elapsed",
            &node(
                "process.stderr.write(\"out of memory\\n\", () => process.kill(process.pid, \"SIGKILL\"));",
                &[],
            ),
        )]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert_eq!(
        error(&report, "killed"),
        "node was terminated by signal 9; its stderr: out of memory"
    );
}

#[test]
fn a_process_left_holding_stderr_does_not_hold_up_the_check() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget(
            "orphaned",
            "elapsed",
            // The sleeper keeps only the command's stderr open: the command's stdout is the
            // binary's stderr, which this test reads to the end. It is detached on Windows,
            // where Node otherwise ends it as soon as the command exits.
            &noting(
                "left a sleeper behind\n",
                "require(\"child_process\").spawn(process.execPath, \
                 [\"-e\", \"setTimeout(() => {}, 30000);\"], \
                 { stdio: [\"ignore\", \"ignore\", \"inherit\"], \
                 detached: process.platform === \"win32\" }).unref(); \
                 process.exitCode = 1;",
            ),
        )]),
    );
    let started = std::time::Instant::now();
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert!(
        started.elapsed().as_secs() < 20,
        "the check waited for the process its command left running"
    );
    assert_eq!(
        error(&report, "orphaned"),
        "node exited with status 1; its stderr: left a sleeper behind"
    );
}

#[test]
fn control_characters_become_spaces_and_whitespace_alone_adds_nothing() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            budget("controls", "elapsed", &failing(&js("a\tb\u{1b}c\r\n"), 1)),
            budget("blank", "elapsed", &failing(&js("  \n\n\t\n"), 1)),
            budget(
                "blank-and-long",
                "elapsed",
                &failing("\" \".repeat(5000) + \"\\n\"", 1),
            ),
        ]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert_eq!(
        error(&report, "controls"),
        "node exited with status 1; its stderr: a b c"
    );
    assert_eq!(error(&report, "blank"), "node exited with status 1");
    assert_eq!(
        error(&report, "blank-and-long"),
        "node exited with status 1"
    );
}

#[test]
fn the_bound_counts_characters_whatever_their_bytes() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            // 1000 characters exactly is kept whole; one more is cut to the last 1000.
            budget("at-the-bound", "elapsed", &failing("\"0\".repeat(1000)", 1)),
            budget(
                "past-the-bound",
                "elapsed",
                &failing("\"1\" + \"0\".repeat(1000)", 1),
            ),
            // 4001 bytes: the 4000 kept start inside `€`, whose remaining bytes are dropped
            // rather than decoded as replacement characters; the blank lines after it are
            // dropped too, so nothing else pushes them out of the tail.
            budget(
                "split",
                "elapsed",
                &failing("\"€\" + \"\\n\".repeat(3995) + \"END\"", 1),
            ),
            // Three bytes a character, past the 4000 bytes kept, so the cut splits one.
            budget("multibyte", "elapsed", &failing("\"€\".repeat(3001)", 1)),
        ]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    let tail = |id: &str| {
        let error = error(&report, id);
        error
            .split_once("; its stderr: ")
            .unwrap_or_else(|| panic!("{error}"))
            .1
            .to_owned()
    };
    assert_eq!(tail("at-the-bound"), "0".repeat(1000));
    assert_eq!(tail("past-the-bound"), format!("…{}", "0".repeat(1000)));
    assert_eq!(tail("split"), "…END");
    assert_eq!(tail("multibyte"), format!("…{}", "€".repeat(1000)));
}

#[test]
fn a_refused_result_shape_or_returned_condition_names_the_exit_status_and_the_stderr() {
    let fixture = Fixture::new();
    let writes = |id: &str, result: &str| {
        budget(
            id,
            "reported",
            &noting("schema drifted; update the script\n", &write_result(result)),
        )
    };
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            writes("unknown-key", r#"{"value": 3, "unit": "ms"}"#),
            writes("string-value", r#"{"value": "3"}"#),
            writes("collides", r#"{"value": 3, "conditions": {"load1": "9"}}"#),
        ]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    for (id, reason) in [
        ("unknown-key", "the result file has an unknown key \"unit\""),
        (
            "string-value",
            "the result's \"value\" is a string rather than a number",
        ),
        ("collides", "the result returns a condition named \"load1\""),
    ] {
        let error = error(&report, id);
        assert!(error.starts_with(reason), "{id}: {error}");
        assert!(
            error.ends_with(
                "; node exited with status 0; its stderr: schema drifted; update the script"
            ),
            "{id}: {error}"
        );
    }
}

/// The binary's own stderr failing does not stop a command's stderr being read, so the
/// command still finishes and its reason still reaches the report.
// Linux only: /dev/full, a device that refuses every write, exists only there.
#[cfg(target_os = "linux")]
#[test]
fn an_unwritable_stderr_still_leaves_the_reason_in_the_report() {
    let fixture = Fixture::new();
    explaining(&fixture);
    fixture.budgets(
        "budgets.yaml",
        &file(&[budget("explained", "reported", &json!(EXPLAIN))]),
    );
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .expect("/dev/full opens for writing");
    let output = crate::common::spawn(
        std::process::Command::new(crate::common::binary())
            .args(["check", "--json"])
            .current_dir(fixture.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(full),
    )
    .wait_with_output()
    .expect("onebudgetspec's output is read");
    assert_eq!(output.status.code(), Some(3));
    let report: Value = serde_json::from_slice(&output.stdout).expect("stdout is one report");
    crate::common::validate("check-report", &report);
    assert_eq!(error(&report, "explained"), EXPLAINED);
}

/// A budget with a timeout is waited on another way; one that finishes in time is
/// reported exactly as one without a timeout.
#[test]
fn a_command_finishing_within_its_timeout_is_reported_the_same_way() {
    let fixture = Fixture::new();
    explaining(&fixture);
    let mut failing = budget("failing", "reported", &json!(EXPLAIN));
    failing["timeout_seconds"] = json!(30);
    let mut succeeding = budget(
        "succeeding",
        "reported",
        &noting("warning: cache cold\n", &write_result(r#"{"value": 3}"#)),
    );
    succeeding["timeout_seconds"] = json!(30);
    fixture.budgets("budgets.yaml", &file(&[failing, succeeding]));
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert_eq!(error(&report, "failing"), EXPLAINED);
    let succeeding = result(&report, "succeeding");
    assert_eq!(succeeding["verdict"], "within", "{succeeding:#}");
    assert_eq!(succeeding["actual"], 3.0);
    assert!(succeeding["error"].is_null(), "{succeeding:#}");
    assert!(!report.to_string().contains("cache cold"), "{report:#}");
}
