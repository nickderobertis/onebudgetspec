//! The text result line, and that the JSON report for the same run carries the same
//! figures.

use regex::Regex;
use serde_json::{Value, json};

use crate::common::{Fixture, reported, result};

fn figure(value: &Value) -> String {
    value
        .as_f64()
        .map_or_else(|| "unknown".to_owned(), |number| number.to_string())
}

fn mixed(fixture: &Fixture) {
    fixture.budgets(
        "budgets.yaml",
        &json!({
            "schema_version": 1,
            "conditions": [
                { "name": "dispatches", "command": ["echo", "3"] },
                { "name": "flaky", "command": ["false"] },
            ],
            "budgets": [
                reported("gate-time", 1395.0, "max", 1800.0),
                reported("bundle-size", 900.0, "max", 800.0),
                reported("free-tier", 0.25, "min", 0.0),
                {
                    "id": "broken",
                    "measure": "reported",
                    "command": ["sh", "-c", "exit 5"],
                    "unit": "requests",
                    "direction": "max",
                    "threshold": 1,
                },
            ],
        }),
    );
}

#[test]
fn the_text_line_matches_the_contract_for_within_over_and_error() {
    let fixture = Fixture::new();
    mixed(&fixture);
    let text = fixture.run(["check"]);
    text.expect_status(3);
    let json = fixture.run(["check", "--json"]);
    let report = json.expect_status(3).check_report();

    let host = r"host: load=(?:[0-9.]+|unknown)/[0-9]+ mem_available=(?:[0-9]+|unknown)MiB dispatches=3 flaky=unknown";
    let measured = Regex::new(&format!(
        r"^budget (?P<id>[a-z0-9-]+): actual (?P<actual>\S+) (?P<unit>\S+), budget (?P<threshold>\S+) (?P<unit2>\S+), headroom (?P<headroom>\S+) (?P<unit3>\S+) \((?P<percent>\S+)%\) — (?P<verdict>within|over); {host}$"
    ))
    .unwrap();
    let errored = Regex::new(&format!(
        r"^budget (?P<id>[a-z0-9-]+): error — (?P<reason>.+); {host}$"
    ))
    .unwrap();

    let lines: Vec<&str> = text.stdout.lines().collect();
    assert_eq!(lines.len(), 4, "{}", text.stdout);
    for line in &lines[..3] {
        let captures = measured
            .captures(line)
            .unwrap_or_else(|| panic!("not a result line: {line}"));
        let result = result(&report, &captures["id"]);
        assert_eq!(&captures["actual"], figure(&result["actual"]));
        assert_eq!(&captures["threshold"], figure(&result["threshold"]));
        assert_eq!(&captures["headroom"], figure(&result["headroom"]));
        assert_eq!(&captures["percent"], figure(&result["headroom_percent"]));
        assert_eq!(&captures["verdict"], result["verdict"].as_str().unwrap());
        for unit in ["unit", "unit2", "unit3"] {
            assert_eq!(&captures[unit], result["unit"].as_str().unwrap());
        }
    }
    let error = errored
        .captures(lines[3])
        .unwrap_or_else(|| panic!("not an error line: {}", lines[3]));
    assert_eq!(&error["id"], "broken");
    assert_eq!(
        &error["reason"],
        result(&report, "broken")["error"].as_str().unwrap()
    );

    assert_eq!(
        lines[0].split(';').next().unwrap(),
        "budget gate-time: actual 1395 requests, budget 1800 requests, headroom 405 requests (22.5%) — within"
    );
    assert!(lines[2].contains("(unknown%)"), "{}", lines[2]);
}
