//! The text result line, and that the JSON report for the same run carries the same
//! figures.

use regex::Regex;
use serde_json::{Value, json};

use crate::common::{Fixture, exits, file, node, reported, reports, result};

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
                { "name": "dispatches", "command": node("console.log(\"3\");", &[]) },
                { "name": "flaky", "command": exits(1) },
            ],
            "budgets": [
                reported("gate-time", 1395.0, "max", 1800.0),
                reported("bundle-size", 900.0, "max", 800.0),
                reported("free-tier", 0.25, "min", 0.0),
                {
                    "id": "broken",
                    "measure": "reported",
                    "command": exits(5),
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

    // Linux and macOS have a load average and Windows has none; Linux and Windows report
    // available memory and macOS does not.
    let load = if cfg!(unix) { "[0-9.]+" } else { "unknown" };
    let mem = if cfg!(any(target_os = "linux", windows)) {
        "[0-9]+"
    } else {
        "unknown"
    };
    let host =
        format!("host: load={load}/[0-9]+ mem_available={mem}MiB dispatches=3 flaky=unknown");
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

fn detailed(id: &str, value: f64, threshold: f64, detail: &str) -> Value {
    json!({
        "id": id,
        "measure": "reported",
        "command": reports(&json!({ "value": value, "detail": detail }).to_string()),
        "unit": "requests",
        "direction": "max",
        "threshold": threshold,
    })
}

#[test]
fn a_detail_follows_its_result_line_indented_by_two_spaces() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            detailed(
                "grew",
                900.0,
                800.0,
                "vendor chunk: 610 requests\napp chunk: 290 requests\n\n  nested: 4",
            ),
            detailed("steady", 4.0, 10.0, "4 of 10 calls hit the cache"),
            reported("plain", 4.0, "max", 10.0),
            detailed("empty", 4.0, 10.0, ""),
            {
                let mut broken = reported("broken", 1.0, "max", 10.0);
                broken["command"] = exits(5);
                broken
            },
        ]),
    );
    let text = fixture.run(["check"]);
    text.expect_status(3);
    let report = fixture
        .run(["check", "--json"])
        .expect_status(3)
        .check_report();
    assert_eq!(result(&report, "broken")["verdict"], "error");
    assert!(result(&report, "plain")["detail"].is_null());
    assert_eq!(result(&report, "empty")["detail"], "");

    let lines: Vec<&str> = text.stdout.lines().collect();
    let expected_after = |at: usize, id: &str| {
        assert!(
            lines[at].starts_with(&format!("budget {id}: ")),
            "line {at} is not {id}'s result line:\n{}",
            text.stdout
        );
    };
    assert_eq!(lines.len(), 10, "{}", text.stdout);
    expected_after(0, "grew");
    assert!(lines[0].contains(" — over; host: "), "{}", lines[0]);
    assert_eq!(
        lines[1..5],
        [
            "  vendor chunk: 610 requests",
            "  app chunk: 290 requests",
            "  ",
            "    nested: 4",
        ]
    );
    expected_after(5, "steady");
    assert!(lines[5].contains(" — within; host: "), "{}", lines[5]);
    assert_eq!(lines[6], "  4 of 10 calls hit the cache");
    // No detail and an empty one add nothing: the next line is the next result's.
    expected_after(7, "plain");
    expected_after(8, "empty");
    expected_after(9, "broken");
    assert!(lines[9].contains(": error — "), "{}", lines[9]);
    assert!(text.stdout.ends_with('\n'), "{}", text.stdout);
}
