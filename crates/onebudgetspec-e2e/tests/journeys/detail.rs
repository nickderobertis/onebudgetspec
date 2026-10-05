//! The `detail` a `reported` command writes is its result's `detail`.

use serde_json::json;

use crate::common::{Fixture, file, reports, result};

#[test]
fn detail_is_carried_when_written_and_null_when_not() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            json!({
                "id": "with-detail",
                "measure": "reported",
                "command": reports(r#"{"value": 4, "detail": "4 of 10 calls hit the cache"}"#),
                "unit": "calls",
                "direction": "max",
                "threshold": 10,
            }),
            json!({
                "id": "without-detail",
                "measure": "reported",
                "command": reports(r#"{"value": 4}"#),
                "unit": "calls",
                "direction": "max",
                "threshold": 10,
            }),
        ]),
    );
    let report = fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    assert_eq!(
        result(&report, "with-detail")["detail"],
        "4 of 10 calls hit the cache"
    );
    assert!(result(&report, "without-detail")["detail"].is_null());
}
