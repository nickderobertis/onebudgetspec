//! The Rust SDK's `report`, called by a measurement written in Rust: under `onebudgetspec
//! check` its value and detail are the result's `actual` and `detail`; outside a check it
//! writes nothing; a non-finite value is refused and a failed write surfaced, neither
//! writing anything.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use crate::common::{Fixture, file, output, result};

/// The program that reports its arguments through `onebudgetspec_core::report`.
const REPORTER: &str = env!("CARGO_BIN_EXE_onebudgetspec-e2e-report");

/// Run the reporter directly with `args`, `ONEBUDGETSPEC_RESULT` set to `result` or, when
/// `None`, removed. Returns its exit status, stdout and stderr.
fn report(args: &[&str], result: Option<&Path>) -> (i32, String, String) {
    let mut command = Command::new(REPORTER);
    command.args(args);
    match result {
        Some(path) => command.env("ONEBUDGETSPEC_RESULT", path),
        None => command.env_remove("ONEBUDGETSPEC_RESULT"),
    };
    let finished = output(&mut command);
    (
        finished
            .status
            .code()
            .expect("the reporter exits with a status"),
        String::from_utf8(finished.stdout).unwrap(),
        String::from_utf8(finished.stderr).unwrap(),
    )
}

#[test]
fn a_check_reports_what_a_rust_measurement_reported() {
    let fixture = Fixture::new();
    let budget = |id: &str, args: &[&str]| {
        let mut command = vec![json!(REPORTER)];
        command.extend(args.iter().map(|arg| json!(arg)));
        json!({
            "id": id,
            "measure": "reported",
            "command": command,
            "unit": "ms",
            "direction": "max",
            "threshold": 1500,
        })
    };
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            budget("p95", &["1395.5", "p95 of 200 requests"]),
            budget("bare", &["1600"]),
        ]),
    );
    let run = fixture.run(["check", "--json"]);
    let report = run.expect_status(1).check_report();
    let p95 = result(&report, "p95");
    assert_eq!(p95["actual"].as_f64(), Some(1395.5));
    assert_eq!(p95["detail"], "p95 of 200 requests");
    assert_eq!(p95["verdict"], "within");
    let bare = result(&report, "bare");
    assert_eq!(bare["actual"].as_f64(), Some(1600.0));
    assert!(bare["detail"].is_null(), "{bare:#}");
    assert_eq!(bare["verdict"], "over");
}

#[test]
fn with_the_variable_set_the_result_file_is_replaced_by_the_value_and_detail() {
    let fixture = Fixture::new();
    let path = fixture.write("result.json", "what an earlier write left behind");
    let (status, stdout, stderr) = report(&["2.5", "two and a half"], Some(&path));
    assert_eq!((status, stdout.trim()), (0, "true"), "{stderr}");
    let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written, json!({"value": 2.5, "detail": "two and a half"}));

    let (status, stdout, stderr) = report(&["3"], Some(&path));
    assert_eq!((status, stdout.trim()), (0, "true"), "{stderr}");
    let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written, json!({"value": 3.0}));
}

#[test]
fn outside_a_check_nothing_is_written_and_report_returns_false() {
    let fixture = Fixture::new();
    for result in [None, Some(Path::new(""))] {
        let (status, stdout, stderr) = report(&["7", "detail"], result);
        assert_eq!((status, stdout.trim()), (0, "false"), "{stderr}");
    }
    let entries = fs::read_dir(fixture.path()).unwrap().count();
    assert_eq!(entries, 0, "something was written");
}

#[test]
fn a_non_finite_value_is_refused_without_writing() {
    let fixture = Fixture::new();
    let path = fixture.write("result.json", "untouched");
    for value in ["NaN", "inf", "-inf"] {
        let (status, stdout, stderr) = report(&[value], Some(&path));
        assert_eq!(status, 2, "{value}: {stdout}{stderr}");
        assert!(stderr.contains("finite"), "{value}: {stderr}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "untouched");
    }
    // Refused outside a check too, so a measurement fails the same way wherever it runs.
    let (status, _, stderr) = report(&["NaN"], None);
    assert_eq!(status, 2, "{stderr}");
}

#[test]
fn a_failed_write_is_an_error() {
    let fixture = Fixture::new();
    let path = fixture.path().join("no-such-directory/result.json");
    let (status, stdout, stderr) = report(&["1"], Some(&path));
    assert_eq!(status, 1, "{stdout}{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(!stderr.trim().is_empty());
    assert!(!path.exists());
}
