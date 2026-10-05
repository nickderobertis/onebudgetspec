//! `--recursive` discovery: nested `budgets.yaml` files in path order, `.gitignore`
//! honoured, and ids unique across every file found.

use serde_json::json;

use crate::common::{Fixture, file, ids, reported, result};

fn nested(fixture: &Fixture) {
    fixture.budgets("budgets.yaml", &file(&[reported("root", 1.0, "max", 2.0)]));
    fixture.budgets(
        "zeta/budgets.yaml",
        &file(&[reported("zeta", 1.0, "max", 2.0)]),
    );
    fixture.budgets(
        "alpha/deep/budgets.yaml",
        &file(&[reported("alpha-deep", 1.0, "max", 2.0)]),
    );
    fixture.budgets(
        "alpha/budgets.yaml",
        &file(&[reported("alpha", 1.0, "max", 2.0)]),
    );
    fixture.budgets(
        "build/budgets.yaml",
        &file(&[reported("generated", 1.0, "max", 2.0)]),
    );
    fixture.budgets(
        "alpha/other.yaml",
        &file(&[reported("wrong-name", 1.0, "max", 2.0)]),
    );
    fixture.write(".gitignore", "build/\n");
}

#[test]
fn recursive_check_finds_nested_files_in_path_order_and_skips_gitignored_ones() {
    let fixture = Fixture::new();
    nested(&fixture);
    let report = fixture
        .run(["check", "--json", "--recursive"])
        .expect_status(0)
        .check_report();
    assert_eq!(
        ids(&report, "results"),
        ["alpha", "alpha-deep", "root", "zeta"]
    );
    assert_eq!(
        result(&report, "alpha-deep")["file"],
        "alpha/deep/budgets.yaml"
    );
    assert_eq!(result(&report, "root")["file"], "budgets.yaml");

    let named = fixture
        .run(["check", "--json", "--recursive", "alpha"])
        .expect_status(0)
        .check_report();
    assert_eq!(ids(&named, "results"), ["alpha", "alpha-deep"]);
    assert_eq!(result(&named, "alpha")["file"], "alpha/budgets.yaml");
}

#[test]
fn an_id_repeated_across_two_files_is_refused() {
    let fixture = Fixture::new();
    fixture.counted(
        "ran.sh",
        "ran.log",
        "ran",
        "printf '{\"value\": 1}' > \"$ONEBUDGETSPEC_RESULT\"",
    );
    let counted = json!({
        "id": "shared",
        "measure": "reported",
        "command": ["../ran.sh"],
        "unit": "runs",
        "direction": "max",
        "threshold": 2,
    });
    fixture.budgets("one/budgets.yaml", &file(std::slice::from_ref(&counted)));
    fixture.budgets("two/budgets.yaml", &file(&[counted]));
    let run = fixture.run(["check", "--json", "--recursive"]);
    run.expect_status(2);
    assert!(run.stderr.contains("two/budgets.yaml"), "{}", run.stderr);
    assert!(run.stderr.contains("shared"), "{}", run.stderr);
    assert!(fixture.log("ran.log").is_empty());
}

#[test]
fn validate_recursive_refuses_an_invalid_nested_file() {
    let fixture = Fixture::new();
    nested(&fixture);
    fixture.write(
        "alpha/deep/budgets.yaml",
        "schema_version: 1\nbudgets:\n  - id: deep\n    measure: sometimes\n    command: [true]\n    unit: s\n    direction: max\n    threshold: 1\n",
    );
    let run = fixture.run(["validate", "--recursive"]);
    run.expect_status(2);
    assert!(
        run.stderr.contains("alpha/deep/budgets.yaml"),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("measure"), "{}", run.stderr);

    fixture.write(
        "alpha/deep/budgets.yaml",
        "schema_version: 1\nbudgets: []\n",
    );
    fixture.run(["validate", "--recursive"]).expect_status(0);
}
