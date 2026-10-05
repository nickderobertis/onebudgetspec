//! How a command is run: from the directory holding its budgets file, with the caller's
//! environment, a fresh empty result file, and its arguments passed with no shell.

use std::fs;

use serde_json::json;

use crate::common::{Fixture, file, result, run_in};

#[test]
fn a_relative_command_runs_from_the_directory_holding_its_file() {
    let fixture = Fixture::new();
    let project = fixture.path().join("services/api");
    let recorded = fixture.path().join("cwd.txt");
    fixture.script(
        "services/api/scripts/where.sh",
        &format!(
            "pwd -P > '{}'\nprintf '{{\"value\": 1}}' > \"$ONEBUDGETSPEC_RESULT\"",
            recorded.display()
        ),
    );
    fixture.budgets(
        "services/api/budgets.yaml",
        &file(&[json!({
            "id": "where",
            "measure": "reported",
            "command": ["scripts/where.sh"],
            "unit": "runs",
            "direction": "max",
            "threshold": 1,
        })]),
    );
    let elsewhere = fixture.path().join("somewhere/else");
    fs::create_dir_all(&elsewhere).unwrap();

    let file_arg = project.join("budgets.yaml");
    let run = run_in(
        &elsewhere,
        ["check".as_ref(), "--json".as_ref(), file_arg.as_os_str()],
        &[],
    );
    run.expect_status(0).check_report();
    let cwd = fs::read_to_string(recorded).unwrap();
    assert_eq!(
        cwd.trim(),
        fs::canonicalize(&project).unwrap().to_string_lossy(),
        "the command ran from the wrong directory"
    );
}

#[test]
fn the_environment_is_inherited_and_the_result_file_starts_empty() {
    let fixture = Fixture::new();
    fixture.script(
        "probe.sh",
        r#"if [ -f "$ONEBUDGETSPEC_RESULT" ] && [ ! -s "$ONEBUDGETSPEC_RESULT" ]; then fresh=yes; else fresh=no; fi
printf '{"value": %s, "detail": "fresh=%s"}' "$JOURNEY_WORKERS" "$fresh" > "$ONEBUDGETSPEC_RESULT""#,
    );
    fixture.budgets(
        "budgets.yaml",
        &file(&[json!({
            "id": "probe",
            "measure": "reported",
            "command": ["./probe.sh"],
            "unit": "workers",
            "direction": "max",
            "threshold": 100,
        })]),
    );
    let run = run_in(
        fixture.path(),
        ["check", "--json"],
        &[("JOURNEY_WORKERS", "42")],
    );
    let report = run.expect_status(0).check_report();
    let probe = result(&report, "probe");
    assert_eq!(probe["actual"].as_f64(), Some(42.0));
    assert_eq!(probe["detail"], "fresh=yes");
}

#[test]
fn arguments_reach_the_command_byte_for_byte_with_no_shell() {
    let fixture = Fixture::new();
    let tricky = ["two words", "$HOME", "a;b", "*", "'quoted'", "$(echo no)"];
    let recorded = fixture.path().join("args.txt");
    fixture.script(
        "args.sh",
        &format!(
            "for arg in \"$@\"; do printf '%s\\n' \"$arg\"; done > '{}'\nprintf '{{\"value\": 1}}' > \"$ONEBUDGETSPEC_RESULT\"",
            recorded.display()
        ),
    );
    let mut command = vec![json!("./args.sh")];
    command.extend(tricky.iter().map(|arg| json!(arg)));
    fixture.budgets(
        "budgets.yaml",
        &file(&[json!({
            "id": "args",
            "measure": "reported",
            "command": command,
            "unit": "runs",
            "direction": "max",
            "threshold": 1,
        })]),
    );
    fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    let received = fs::read_to_string(recorded).unwrap();
    assert_eq!(received.lines().collect::<Vec<_>>(), tricky);
}

/// Commands never read the caller's stdin: a check piped data still gives every command
/// an immediate end of input, so none can hang on or consume it.
#[test]
fn commands_see_end_of_input_whatever_the_caller_pipes_in() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let fixture = Fixture::new();
    let counts =
        "n=$(wc -c | tr -d ' '); printf '{\"value\": %s}' \"$n\" > \"$ONEBUDGETSPEC_RESULT\"";
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [{ "name": "stdin_bytes", "command": ["sh", "-c", "wc -c | tr -d ' '"] }],
            "budgets": [{
                "id": "stdin-bytes",
                "measure": "reported",
                "command": ["sh", "-c", counts],
                "unit": "bytes",
                "direction": "max",
                "threshold": 0,
            }],
        }),
    );
    let mut child = Command::new(crate::common::binary())
        .args(["check", "--json"])
        .current_dir(fixture.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("onebudgetspec runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"caller data the commands must not see\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    crate::common::validate("check-report", &report);
    let measured = result(&report, "stdin-bytes");
    assert_eq!(measured["actual"], 0.0);
    assert_eq!(measured["host"]["conditions"]["stdin_bytes"], "0");
}
