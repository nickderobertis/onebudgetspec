//! Which files `check` reads: `./budgets.yaml` by default, every named file otherwise.

use crate::common::{Fixture, file, ids, reported, result, write_result};

#[test]
fn check_with_no_path_reads_budgets_yaml_from_the_working_directory() {
    let fixture = Fixture::new();
    fixture.budgets("budgets.yaml", &file(&[reported("here", 1.0, "max", 2.0)]));
    fixture.budgets(
        "nested/budgets.yaml",
        &file(&[reported("not-here", 1.0, "max", 2.0)]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    assert_eq!(ids(&report, "results"), ["here"]);
    assert_eq!(result(&report, "here")["file"], "budgets.yaml");
}

#[test]
fn two_named_files_are_both_checked_each_result_naming_its_own() {
    let fixture = Fixture::new();
    fixture.budgets(
        "api/budgets.yaml",
        &file(&[reported("api", 1.0, "max", 2.0)]),
    );
    fixture.budgets(
        "web/limits.yaml",
        &file(&[reported("web", 1.0, "max", 2.0)]),
    );
    let report = fixture
        .run(["check", "--json", "web/limits.yaml", "api/budgets.yaml"])
        .expect_status(0)
        .check_report();
    assert_eq!(ids(&report, "results"), ["web", "api"]);
    assert_eq!(result(&report, "web")["file"], "web/limits.yaml");
    assert_eq!(result(&report, "api")["file"], "api/budgets.yaml");
}

#[test]
fn a_missing_file_is_refused_with_status_two() {
    let fixture = Fixture::new();
    let run = fixture.run(["check", "--json", "absent.yaml"]);
    run.expect_status(2);
    assert!(run.stdout.is_empty(), "{}", run.stdout);
    assert!(run.stderr.contains("absent.yaml"), "{}", run.stderr);

    let run = fixture.run(["check"]);
    run.expect_status(2);
    assert!(run.stderr.contains("budgets.yaml"), "{}", run.stderr);
}

#[test]
fn a_directory_without_recursive_is_refused_with_status_two() {
    let fixture = Fixture::new();
    fixture.budgets(
        "api/budgets.yaml",
        &file(&[reported("api", 1.0, "max", 2.0)]),
    );
    let run = fixture.run(["check", "api"]);
    run.expect_status(2);
    assert!(run.stderr.contains("--recursive"), "{}", run.stderr);
}

#[test]
fn a_file_named_twice_or_by_two_spellings_is_measured_once() {
    let fixture = Fixture::new();
    fixture.counted(
        "measure.js",
        "ran.log",
        "measured",
        &write_result(r#"{"value": 1}"#),
    );
    fixture.budgets(
        "budgets.yaml",
        &file(&[serde_json::json!({
            "id": "once",
            "measure": "reported",
            "command": ["node", "measure.js"],
            "unit": "runs",
            "direction": "max",
            "threshold": 2,
        })]),
    );
    let report = fixture
        .run([
            "check",
            "--json",
            "budgets.yaml",
            "./budgets.yaml",
            "budgets.yaml",
        ])
        .expect_status(0)
        .check_report();
    assert_eq!(ids(&report, "results"), ["once"]);
    assert_eq!(fixture.log("ran.log"), ["measured"]);
}
