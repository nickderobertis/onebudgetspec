//! Each selected budget is measured exactly once, one at a time, in file order; a budget
//! the selection leaves out is never run.

use serde_json::{Value, json};

use crate::common::{Fixture, file, ids};

/// A budget whose command records `start <id>` and `end <id>` around a short sleep.
fn marked(fixture: &Fixture, id: &str) -> Value {
    let script = format!("{id}.sh");
    let log = fixture.path().join("marks.log");
    fixture.script(
        &script,
        &format!(
            "echo start {id} >> '{log}'\nsleep 0.2\necho end {id} >> '{log}'\nprintf '{{\"value\": 1}}' > \"$ONEBUDGETSPEC_RESULT\"",
            log = log.display()
        ),
    );
    json!({
        "id": id,
        "labels": [if id == "skipped" { "manual" } else { "auto" }],
        "measure": "reported",
        "command": [format!("./{script}")],
        "unit": "runs",
        "direction": "max",
        "threshold": 5,
    })
}

#[test]
fn budgets_run_once_each_in_file_order_one_at_a_time() {
    let fixture = Fixture::new();
    let budgets =
        ["third-alphabetically", "first", "skipped", "second"].map(|id| marked(&fixture, id));
    fixture.budgets("budgets.yaml", &file(&budgets));

    let report = fixture
        .run(["check", "--json", "--exclude-label", "manual"])
        .expect_status(0)
        .check_report();
    assert_eq!(
        ids(&report, "results"),
        ["third-alphabetically", "first", "second"]
    );
    assert_eq!(
        fixture.log("marks.log"),
        [
            "start third-alphabetically",
            "end third-alphabetically",
            "start first",
            "end first",
            "start second",
            "end second",
        ],
        "each selected command runs once, and none overlaps another"
    );
}
