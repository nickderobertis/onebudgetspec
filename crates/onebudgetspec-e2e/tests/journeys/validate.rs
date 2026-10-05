//! `validate` accepts a well-formed file and runs no command.

use serde_json::json;

use crate::common::Fixture;

#[test]
fn validate_accepts_a_well_formed_file_without_running_anything() {
    let fixture = Fixture::new();
    fixture.counted("condition.sh", "ran.log", "condition", "echo 1");
    fixture.counted("budget.sh", "ran.log", "budget", "true");
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [{ "name": "probe", "command": ["./condition.sh"] }],
            "budgets": [
                {
                    "id": "startup",
                    "description": "Cold start of the service.",
                    "labels": ["service"],
                    "measure": "elapsed",
                    "command": ["./budget.sh"],
                    "unit": "seconds",
                    "direction": "max",
                    "threshold": 2.5,
                    "timeout_seconds": 30,
                },
                {
                    "id": "throughput",
                    "measure": "reported",
                    "command": ["./budget.sh"],
                    "unit": "requests/s",
                    "direction": "min",
                    "threshold": 0,
                },
            ],
        }),
    );
    let run = fixture.run(["validate"]);
    run.expect_status(0);
    assert_eq!(run.stdout, "valid: 2 budget(s) in 1 file(s)\n");
    assert!(fixture.log("ran.log").is_empty(), "validate ran a command");

    let report = fixture
        .run(["validate", "--json"])
        .expect_status(0)
        .list_report();
    assert_eq!(report["budgets"].as_array().unwrap().len(), 2);
    assert!(fixture.log("ran.log").is_empty(), "validate ran a command");
}
