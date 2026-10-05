//! A command's own output goes to the binary's stderr, so stdout is only the report.

use serde_json::json;

use crate::common::{Fixture, results};

fn noisy(fixture: &Fixture) {
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [{
                "name": "noisy_condition",
                "command": ["sh", "-c", "echo CONDITION-OUT; echo CONDITION-ERR >&2"],
            }],
            "budgets": [{
                "id": "noisy",
                "measure": "reported",
                "command": ["sh", "-c", "echo BUDGET-OUT; echo BUDGET-ERR >&2; printf '{\"value\": 1}' > \"$ONEBUDGETSPEC_RESULT\""],
                "unit": "runs",
                "direction": "max",
                "threshold": 2,
            }],
        }),
    );
}

const MARKS: [&str; 4] = ["CONDITION-OUT", "CONDITION-ERR", "BUDGET-OUT", "BUDGET-ERR"];

#[test]
fn json_stdout_is_exactly_one_report_and_command_output_is_on_stderr() {
    let fixture = Fixture::new();
    noisy(&fixture);
    let run = fixture.run(["check", "--output", "json"]);
    let report = run.expect_status(0).check_report();
    assert_eq!(results(&report).len(), 1);
    assert_eq!(
        results(&report)[0]["host"]["conditions"]["noisy_condition"],
        "CONDITION-OUT"
    );
    // Stdout parsed as one report, so nothing else was printed there. The condition's
    // stdout is its recorded value; past that one field, no command output is in it.
    let mut rest = report.clone();
    rest["results"][0]["host"]["conditions"]
        .as_object_mut()
        .unwrap()
        .remove("noisy_condition");
    let rest = rest.to_string();
    for mark in MARKS {
        assert!(
            !rest.contains(mark),
            "{mark} leaked onto stdout:\n{}",
            run.stdout
        );
        assert!(run.stderr.contains(mark), "{mark} missing from stderr");
    }
}

#[test]
fn text_stdout_holds_none_of_the_command_output() {
    let fixture = Fixture::new();
    noisy(&fixture);
    let run = fixture.run(["check"]);
    run.expect_status(0);
    assert_eq!(run.stdout.lines().count(), 1, "{}", run.stdout);
    assert!(run.stdout.starts_with("budget noisy: "), "{}", run.stdout);
    // The condition's stdout is its recorded value, so it appears on the result line
    // as `noisy_condition=CONDITION-OUT` and nowhere else.
    assert!(
        run.stdout.contains(" noisy_condition=CONDITION-OUT"),
        "{}",
        run.stdout
    );
    let rest = run.stdout.replace(" noisy_condition=CONDITION-OUT", "");
    for mark in MARKS {
        assert!(
            !rest.contains(mark),
            "{mark} leaked onto stdout:\n{}",
            run.stdout
        );
        assert!(run.stderr.contains(mark), "{mark} missing from stderr");
    }
}

/// A report that cannot be written is an error: exit status 3 and the reason on stderr,
/// for every verb that writes one, even when every budget is within.
#[cfg(target_os = "linux")]
#[test]
fn an_unwritable_stdout_is_reported_on_stderr() {
    let fixture = Fixture::new();
    noisy(&fixture);
    for args in [
        &["check", "--json"][..],
        &["check"][..],
        &["validate"][..],
        &["list", "--json"][..],
        &["schema"][..],
    ] {
        let full = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .expect("/dev/full opens for writing");
        let output = std::process::Command::new(crate::common::binary())
            .args(args)
            .current_dir(fixture.path())
            .stdout(full)
            .output()
            .expect("onebudgetspec runs");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(3), "{args:?}: {stderr}");
        assert!(
            stderr.contains("onebudgetspec: cannot write the report: No space left on device"),
            "{args:?}: {stderr}"
        );
    }
}
