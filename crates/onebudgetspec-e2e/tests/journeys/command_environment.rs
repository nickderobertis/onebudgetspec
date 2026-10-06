//! How a command is run: from the directory holding its budgets file, with the caller's
//! environment, a fresh empty result file, and its arguments passed with no shell.

use std::fs;

use serde_json::json;

use crate::common::{Fixture, binary, file, js, node, result, run_in, write_result};

#[test]
fn a_relative_command_runs_from_the_directory_holding_its_file() {
    let fixture = Fixture::new();
    let project = fixture.path().join("services/api");
    let recorded = fixture.path().join("cwd.txt");
    fixture.script(
        "services/api/scripts/where.js",
        &format!(
            "const fs = require(\"fs\");\nfs.writeFileSync({}, process.cwd());\n{}",
            js(&recorded.to_string_lossy()),
            write_result(r#"{"value": 1}"#)
        ),
    );
    // A program named by a relative path is found from the file's directory too: here a
    // copy of this binary, validating the file beside it.
    let tool = format!("tools/onebudgetspec{}", std::env::consts::EXE_SUFFIX);
    fixture.copy(&format!("services/api/{tool}"), &binary());
    fixture.budgets(
        "services/api/budgets.yaml",
        &file(&[
            json!({
                "id": "where",
                "measure": "reported",
                "command": ["node", "scripts/where.js"],
                "unit": "runs",
                "direction": "max",
                "threshold": 1,
            }),
            json!({
                "id": "tool",
                "measure": "elapsed",
                "command": [tool, "validate"],
                "unit": "seconds",
                "direction": "max",
                "threshold": 60,
            }),
        ]),
    );
    let elsewhere = fixture.path().join("somewhere/else");
    fs::create_dir_all(&elsewhere).unwrap();

    let file_arg = project.join("budgets.yaml");
    let run = run_in(
        &elsewhere,
        ["check".as_ref(), "--json".as_ref(), file_arg.as_os_str()],
        &[],
    );
    let report = run.expect_status(0).check_report();
    assert_eq!(result(&report, "tool")["verdict"], "within");
    let cwd = fs::read_to_string(recorded).unwrap();
    assert_eq!(
        fs::canonicalize(cwd.trim()).unwrap(),
        fs::canonicalize(&project).unwrap(),
        "the command ran from the wrong directory"
    );
}

#[test]
fn the_environment_is_inherited_and_the_result_file_starts_empty() {
    let fixture = Fixture::new();
    fixture.script(
        "probe.js",
        r#"const fs = require("fs");
const path = process.env.ONEBUDGETSPEC_RESULT;
const fresh = fs.existsSync(path) && fs.statSync(path).isFile() && fs.statSync(path).size === 0 ? "yes" : "no";
fs.writeFileSync(path, `{"value": ${process.env.JOURNEY_WORKERS}, "detail": "fresh=${fresh}"}`);"#,
    );
    fixture.budgets(
        "budgets.yaml",
        &file(&[json!({
            "id": "probe",
            "measure": "reported",
            "command": ["node", "probe.js"],
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
    let tricky = [
        "two words",
        "$HOME",
        "%PATH%",
        "a;b",
        "a&b",
        "*",
        "'quoted'",
        "\"double\"",
        "$(echo no)",
    ];
    let recorded = fixture.path().join("args.txt");
    fixture.script(
        "args.js",
        &format!(
            "const fs = require(\"fs\");\n\
             fs.writeFileSync({}, process.argv.slice(2).map((arg) => arg + \"\\n\").join(\"\"));\n{}",
            js(&recorded.to_string_lossy()),
            write_result(r#"{"value": 1}"#)
        ),
    );
    let mut command = vec![json!("node"), json!("args.js")];
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
    let counting = |then: &str| {
        node(
            &format!(
                "let n = 0; process.stdin.on(\"data\", (chunk) => {{ n += chunk.length; }}); \
                 process.stdin.on(\"end\", () => {{ {then} }});"
            ),
            &[],
        )
    };
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [{ "name": "stdin_bytes", "command": counting("console.log(String(n));") }],
            "budgets": [{
                "id": "stdin-bytes",
                "measure": "reported",
                "command": counting(
                    "require(\"fs\").writeFileSync(process.env.ONEBUDGETSPEC_RESULT, `{\"value\": ${n}}`);"
                ),
                "unit": "bytes",
                "direction": "max",
                "threshold": 0,
            }],
        }),
    );
    let mut child = crate::common::spawn(
        Command::new(crate::common::binary())
            .args(["check", "--json"])
            .current_dir(fixture.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
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
