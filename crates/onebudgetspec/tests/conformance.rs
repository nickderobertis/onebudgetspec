//! The conformance suite under `conformance/cases`, run against a command.
//!
//! The command under test is a parameter: `ONEBUDGETSPEC_CONFORMANCE_COMMAND` names it as a
//! JSON array of argv (a launcher and its arguments, say), and without it this crate's own
//! binary is run. `conformance/README.md` is the format every runner follows.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/cases")
}

/// The argv prefix of the command under test.
fn command_under_test() -> Vec<String> {
    match std::env::var("ONEBUDGETSPEC_CONFORMANCE_COMMAND") {
        Ok(text) => serde_json::from_str(&text)
            .expect("ONEBUDGETSPEC_CONFORMANCE_COMMAND is a JSON array of strings"),
        Err(_) => vec![env!("CARGO_BIN_EXE_onebudgetspec").to_owned()],
    }
}

fn run(command: &[String], cwd: &Path, args: &[String]) -> std::process::Output {
    Command::new(&command[0])
        .args(&command[1..])
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("cannot run {command:?}: {error}"))
}

fn check_report_validator(command: &[String]) -> jsonschema::Validator {
    let output = run(command, Path::new("."), &["schema".to_owned()]);
    assert!(output.status.success(), "{command:?} schema failed");
    let bundle: Value = serde_json::from_slice(&output.stdout).expect("the bundle is JSON");
    jsonschema::validator_for(&bundle["roots"]["check-report"])
        .expect("check-report is a valid JSON Schema")
}

fn assert_valid(validator: &jsonschema::Validator, report: &Value, what: &str) {
    let errors: Vec<String> = validator
        .iter_errors(report)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "{what} is not a valid check report:\n{}",
        errors.join("\n")
    );
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The normalization `conformance/README.md` defines.
fn normalize(report: &mut Value, case: &Value, name: &str) {
    let timed: Vec<&str> = case["timed"]
        .as_array()
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for result in report["results"].as_array_mut().into_iter().flatten() {
        let id = result["id"].as_str().unwrap_or_default().to_owned();
        result["started_at"] = json!("1970-01-01T00:00:00Z");
        result["ended_at"] = json!("1970-01-01T00:00:00Z");
        result["host"]["load1"] = json!(0.0);
        result["host"]["cpus"] = json!(1);
        result["host"]["mem_available_mib"] = json!(0);
        if let Some(error) = result["error"].as_str() {
            if let Some(expected) = case["error_contains"][&id].as_str() {
                assert!(
                    error.contains(expected),
                    "{name}: {id}'s error {error:?} does not contain {expected:?}"
                );
            }
            result["error"] = json!("<error>");
        }
        if timed.contains(&id.as_str()) {
            for key in ["actual", "headroom", "headroom_percent"] {
                if !result[key].is_null() {
                    result[key] = json!(0.0);
                }
            }
        }
    }
}

fn case_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(cases_dir())
        .expect("conformance/cases exists")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no conformance cases found");
    dirs
}

#[test]
fn every_case_passes() {
    let command = command_under_test();
    let validator = check_report_validator(&command);
    for dir in case_dirs() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let case: Value = serde_json::from_str(&fs::read_to_string(dir.join("case.json")).unwrap())
            .unwrap_or_else(|error| panic!("{name}/case.json: {error}"));
        let args: Vec<String> = serde_json::from_value(case["args"].clone()).unwrap();
        let scratch = tempfile::tempdir().unwrap();
        copy_dir(&dir, scratch.path());

        let output = run(&command, scratch.path(), &args);
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            case["exit"]
                .as_i64()
                .map(|code| i32::try_from(code).unwrap()),
            "{name}: exit status\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );

        let expected_path = dir.join("expected.json");
        if expected_path.is_file() {
            let mut report: Value = serde_json::from_str(&stdout)
                .unwrap_or_else(|error| panic!("{name}: stdout is not JSON ({error}):\n{stdout}"));
            assert_valid(&validator, &report, &format!("{name}'s report"));
            normalize(&mut report, &case, &name);
            let expected: Value =
                serde_json::from_str(&fs::read_to_string(&expected_path).unwrap()).unwrap();
            assert_eq!(
                report, expected,
                "{name}: the normalized report differs from expected.json\nactual:\n{report:#}"
            );
        } else {
            assert!(
                stdout.is_empty(),
                "{name}: expected no stdout, got:\n{stdout}"
            );
        }
    }
}

#[test]
fn every_expected_report_is_a_valid_check_report() {
    let validator = check_report_validator(&command_under_test());
    let mut checked = 0;
    for dir in case_dirs() {
        let path = dir.join("expected.json");
        if path.is_file() {
            let expected: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_valid(&validator, &expected, &path.display().to_string());
            checked += 1;
        }
    }
    assert!(checked > 0, "no expected report was checked");
}
