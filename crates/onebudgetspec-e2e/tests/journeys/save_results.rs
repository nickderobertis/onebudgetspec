//! `check --save-results` leaves a report on disk and changes nothing else.

use std::fs;

use serde_json::{Value, json};

use crate::common::{Fixture, exits, file, reported, validate};

const SAVED: &str = "nested/deeper/results.json";

/// A report with its per-run fields blanked, so two runs of one check compare equal.
fn stable(mut report: Value) -> Value {
    for result in report["results"]
        .as_array_mut()
        .expect("a report's results")
    {
        for key in ["started_at", "ended_at"] {
            result[key] = json!("<time>");
        }
        result["host"]["load1"] = json!("<load>");
        result["host"]["mem_available_mib"] = json!("<mem>");
    }
    report
}

/// Text output without each result line's host sample, which differs from run to run.
fn stable_text(text: &str) -> Vec<&str> {
    text.lines()
        .map(|line| {
            line.split_once("; host: ")
                .map_or(line, |(result, _)| result)
        })
        .collect()
}

/// A fixture whose only budget is `budget`, with every result's text path the same each run.
fn single(budget: &Value) -> Fixture {
    let fixture = Fixture::new();
    fixture.budgets("budgets.yaml", &file(std::slice::from_ref(budget)));
    fixture
}

fn assert_saved_like_output(budget: &Value, status: i32, verdict: &str) {
    let fixture = single(budget);
    let saved_path = fixture.path().join(SAVED);

    let plain_json = fixture.run(["check", "--json"]);
    plain_json.expect_status(status);
    let json_run = fixture.run(["check", "--json", "--save-results", SAVED]);
    json_run.expect_status(status);
    let saved = fs::read_to_string(&saved_path).unwrap();
    assert_eq!(saved, json_run.stdout, "the file is the printed document");
    assert_eq!(
        stable(json_run.check_report()),
        stable(plain_json.check_report()),
        "the flag leaves JSON stdout as it was"
    );
    let document: Value = serde_json::from_str(&saved).unwrap();
    validate("check-report", &document);
    assert_eq!(document["results"][0]["verdict"], verdict);
    assert_eq!(document["results"][0]["id"], budget["id"]);
    for key in [
        "id",
        "file",
        "labels",
        "unit",
        "direction",
        "threshold",
        "verdict",
        "actual",
        "headroom",
        "headroom_percent",
        "detail",
        "error",
        "started_at",
        "ended_at",
        "host",
    ] {
        assert!(document["results"][0].get(key).is_some(), "{key} is saved");
    }

    fs::remove_file(&saved_path).unwrap();
    let plain_text = fixture.run(["check"]);
    plain_text.expect_status(status);
    let text_run = fixture.run(["check", "--save-results", SAVED]);
    text_run.expect_status(status);
    assert_eq!(
        stable_text(&text_run.stdout),
        stable_text(&plain_text.stdout),
        "the flag leaves text stdout as it was"
    );
    let document: Value = serde_json::from_str(&fs::read_to_string(&saved_path).unwrap()).unwrap();
    assert_eq!(
        stable(document),
        stable(plain_json.json()),
        "text mode saves the same document"
    );
}

#[test]
fn within_is_saved_with_stdout_and_status_unchanged() {
    assert_saved_like_output(&reported("fast", 1.0, "max", 2.0), 0, "within");
}

#[test]
fn over_is_saved_with_stdout_and_status_unchanged() {
    assert_saved_like_output(&reported("slow", 3.0, "max", 2.0), 1, "over");
}

#[test]
fn a_measurement_error_is_saved_with_stdout_and_status_unchanged() {
    let budget = json!({
        "id": "broken",
        "measure": "reported",
        "command": exits(5),
        "unit": "requests",
        "direction": "max",
        "threshold": 2.0,
    });
    assert_saved_like_output(&budget, 3, "error");
}

#[test]
fn refused_invocations_write_nothing_and_run_nothing() {
    let fixture = Fixture::new();
    fixture.counted("measure.js", "ran.log", "ran", "");
    fixture.write("invalid.yaml", "schema_version: 1\nbudgets: nope\n");
    fixture
        .run(["check", "invalid.yaml", "--save-results", SAVED])
        .expect_status(2);
    assert!(!fixture.path().join("nested").exists());

    fixture.budgets(
        "budgets.yaml",
        &file(&[json!({
            "id": "counted",
            "measure": "elapsed",
            "command": ["node", "measure.js"],
            "unit": "seconds",
            "direction": "max",
            "threshold": 100.0,
        })]),
    );
    let refusals: [&[&str]; 4] = [
        &["check", "--id", "unregistered", "--save-results", SAVED],
        &["check", "--output", "markdown", "--save-results", SAVED],
        &["check", "--save-results", ""],
        &["check", "--save-results", "."],
    ];
    for args in refusals {
        let run = fixture.run(args);
        run.expect_status(2);
        assert!(!run.stderr.is_empty(), "{args:?} names its reason");
    }
    assert!(!fixture.path().join("nested").exists());
    assert_eq!(fixture.log("ran.log"), Vec::<String>::new());
    fixture.run(["check", "--save-results"]).expect_status(2);
}

#[test]
fn a_saved_file_is_replaced_whole_and_leaves_nothing_beside_it() {
    let fixture = single(&reported("fast", 1.0, "max", 2.0));
    let path = fixture.write("results.json", &"stale ".repeat(10_000));
    let run = fixture.run(["check", "--json", "--save-results", "results.json"]);
    run.expect_status(0);
    assert_eq!(fs::read_to_string(&path).unwrap(), run.stdout);
    let mut names: Vec<_> = fs::read_dir(fixture.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["budgets.yaml", "results.json"]);
}

#[test]
fn an_unwritable_file_exits_3_naming_the_path_and_still_prints_the_report() {
    let fixture = single(&reported("fast", 1.0, "max", 2.0));
    fixture.write("blocker", "a file where a directory is needed");
    let run = fixture.run(["check", "--json", "--save-results", "blocker/results.json"]);
    run.expect_status(3);
    assert!(
        run.stderr.contains("cannot save the results to") && run.stderr.contains("blocker"),
        "{}",
        run.stderr
    );
    assert_eq!(run.check_report()["results"][0]["verdict"], "within");
    assert_eq!(
        fs::read_to_string(fixture.path().join("blocker")).unwrap(),
        "a file where a directory is needed"
    );
}

// Unix only: Windows has no mode bits, and a temporary file's owner-only mode is what this catches.
#[cfg(unix)]
#[test]
fn a_saved_file_takes_the_ordinary_mode_rather_than_a_temporary_files() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = single(&reported("fast", 1.0, "max", 2.0));
    let ordinary = fixture.write("ordinary", "");
    fixture
        .run(["check", "--save-results", "results.json"])
        .expect_status(0);
    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode();
    assert_eq!(mode(&fixture.path().join("results.json")), mode(&ordinary));
}
