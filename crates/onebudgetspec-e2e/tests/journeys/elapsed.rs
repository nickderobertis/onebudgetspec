//! `elapsed` measurement: the library times the command's wall clock once.

use serde_json::json;

use crate::common::{Fixture, exits, file, node, result};

#[test]
fn elapsed_times_a_sleeping_command() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[json!({
            "id": "nap",
            "measure": "elapsed",
            "command": node("setTimeout(() => {}, 300);", &[]),
            "unit": "seconds",
            "direction": "max",
            "threshold": 60,
        })]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    let nap = result(&report, "nap");
    assert_eq!(nap["verdict"], "within");
    let actual = nap["actual"].as_f64().unwrap();
    assert!(actual >= 0.3, "slept 0.3s but measured {actual}");
    assert!(actual < 30.0, "a 0.3s sleep measured {actual}");
    assert_eq!(nap["headroom"].as_f64(), Some(60.0 - actual));
    assert!(nap["detail"].is_null());
}

#[test]
fn an_elapsed_command_exiting_non_zero_is_an_error_not_a_measurement() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[json!({
            "id": "fails-fast",
            "measure": "elapsed",
            "command": exits(7),
            "unit": "seconds",
            "direction": "max",
            "threshold": 60,
        })]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    let failed = result(&report, "fails-fast");
    assert_eq!(failed["verdict"], "error");
    assert!(failed["actual"].is_null());
    assert!(
        failed["error"].as_str().unwrap().contains("status 7"),
        "{failed:#}"
    );
}
