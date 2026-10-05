//! `--json` is a shorthand for `--output json` on every verb.

use serde_json::{Value, json};

use crate::common::{Fixture, file, reported};

/// Drop the per-run fields so two runs of the same check compare equal.
fn stable(mut report: Value) -> Value {
    if let Some(results) = report["results"].as_array_mut() {
        for result in results {
            for key in ["started_at", "ended_at"] {
                result[key] = json!("<time>");
            }
            result["host"]["load1"] = json!("<load>");
            result["host"]["mem_available_mib"] = json!("<mem>");
        }
    }
    report
}

#[test]
fn json_and_output_json_print_the_same_on_every_verb() {
    let fixture = Fixture::new();
    fixture.budgets(
        "budgets.yaml",
        &file(&[
            reported("one", 1.0, "max", 2.0),
            reported("two", 3.0, "max", 2.0),
        ]),
    );

    let short = fixture.run(["check", "--json"]);
    let long = fixture.run(["check", "--output", "json"]);
    assert_eq!((short.status, long.status), (1, 1));
    assert_eq!(stable(short.check_report()), stable(long.check_report()));

    for verb in ["validate", "list", "schema"] {
        let short = fixture.run([verb, "--json"]);
        let long = fixture.run([verb, "--output", "json"]);
        assert_eq!(short.status, 0, "{verb}: {}", short.stderr);
        assert_eq!(short.stdout, long.stdout, "{verb}");
        assert_eq!(short.status, long.status, "{verb}");
        short.json();
    }
    fixture.run(["validate", "--json"]).list_report();
    fixture.run(["list", "--json"]).list_report();
    assert!(fixture.run(["schema", "--json"]).json()["roots"]["check-report"].is_object());

    let both = fixture.run(["list", "--json", "--output", "text"]);
    both.expect_status(2);
}
