//! The host conditions and timestamps recorded beside every result.

use chrono::DateTime;
use serde_json::json;

use crate::common::{Fixture, reported, results};

#[test]
fn every_result_records_host_values_of_the_contract_types_and_ordered_times() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [
                { "name": "region", "command": ["echo", "eu-west"] },
                { "name": "flaky", "command": ["false"] },
            ],
            "budgets": [
                reported("first", 1.0, "max", 2.0),
                {
                    "id": "second",
                    "measure": "elapsed",
                    "command": ["sleep", "0.05"],
                    "unit": "seconds",
                    "direction": "max",
                    "threshold": 10,
                },
            ],
        }),
    );
    let report = fixture
        .run(["check", "--json", "budgets.yaml"])
        .expect_status(0)
        .check_report();

    for result in results(&report) {
        let host = &result["host"];
        assert!(
            host["load1"].is_null() || host["load1"].as_f64().is_some_and(|l| l >= 0.0),
            "{host:#}"
        );
        assert!(
            host["cpus"].as_u64().is_some_and(|cpus| cpus >= 1),
            "{host:#}"
        );
        assert!(
            host["mem_available_mib"].is_null() || host["mem_available_mib"].is_u64(),
            "{host:#}"
        );
        assert_eq!(
            host["conditions"],
            json!({ "region": "eu-west", "flaky": "unknown" })
        );
        assert_eq!(result["file"], "budgets.yaml");

        let started = DateTime::parse_from_rfc3339(result["started_at"].as_str().unwrap())
            .expect("started_at is RFC 3339");
        let ended = DateTime::parse_from_rfc3339(result["ended_at"].as_str().unwrap())
            .expect("ended_at is RFC 3339");
        assert!(ended >= started, "{result:#}");
    }
    let second = &results(&report)[1];
    let started = DateTime::parse_from_rfc3339(second["started_at"].as_str().unwrap()).unwrap();
    let ended = DateTime::parse_from_rfc3339(second["ended_at"].as_str().unwrap()).unwrap();
    assert!((ended - started).num_milliseconds() >= 50, "{second:#}");
    #[cfg(target_os = "linux")]
    {
        let host = &second["host"];
        assert!(
            host["load1"].is_f64(),
            "Linux reads the load average: {host:#}"
        );
        assert!(
            host["mem_available_mib"].is_u64(),
            "Linux reads MemAvailable: {host:#}"
        );
    }
}
