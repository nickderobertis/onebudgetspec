//! Each selected budget is measured exactly once, one at a time, in file order; a budget
//! the selection leaves out is never run.

use serde_json::{Value, json};

use crate::common::{Fixture, file, ids, js, write_result};

/// A budget whose command records `start <id>` and `end <id>` around a short wait.
fn marked(fixture: &Fixture, id: &str) -> Value {
    let script = format!("{id}.js");
    let log = js(&fixture.path().join("marks.log").to_string_lossy());
    let (start, end) = (js(&format!("start {id}\n")), js(&format!("end {id}\n")));
    fixture.script(
        &script,
        &format!(
            "const fs = require(\"fs\");\n\
             fs.appendFileSync({log}, {start});\n\
             setTimeout(() => {{\n\
             fs.appendFileSync({log}, {end});\n\
             {}\n\
             }}, 200);",
            write_result(r#"{"value": 1}"#)
        ),
    );
    json!({
        "id": id,
        "labels": [if id == "skipped" { "manual" } else { "auto" }],
        "measure": "reported",
        "command": ["node", script],
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
