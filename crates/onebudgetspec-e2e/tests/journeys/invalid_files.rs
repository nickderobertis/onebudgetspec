//! Invalid budgets files: `validate` and `check` each refuse them with status 2, naming the
//! file and the key, and run no command at all.

use serde_json::{Value, json};

use crate::common::Fixture;

/// A valid file whose condition and budget commands each record their invocation.
fn base(fixture: &Fixture) -> Value {
    fixture.counted("condition.sh", "ran.log", "condition", "echo 1");
    fixture.counted(
        "budget.sh",
        "ran.log",
        "budget",
        "printf '{\"value\": 1}' > \"$ONEBUDGETSPEC_RESULT\"",
    );
    json!({
        "schema_version": 1,
        "conditions": [{ "name": "probe", "command": ["./condition.sh"] }],
        "budgets": [{
            "id": "ok",
            "measure": "reported",
            "command": ["./budget.sh"],
            "unit": "ms",
            "direction": "max",
            "threshold": 5,
        }],
    })
}

type Defect = fn(&mut Value);

const DEFECTS: &[(&str, Defect, &str)] = &[
    (
        "unknown top-level key",
        |f| f["owner"] = json!("me"),
        "owner",
    ),
    (
        "unknown budget key",
        |f| f["budgets"][0]["workload"] = json!("x"),
        "workload",
    ),
    (
        "unknown condition key",
        |f| f["conditions"][0]["shell"] = json!(true),
        "shell",
    ),
    (
        "malformed id",
        |f| f["budgets"][0]["id"] = json!("Gate_Time"),
        "budgets[0].id",
    ),
    (
        "malformed label",
        |f| f["budgets"][0]["labels"] = json!(["Fast"]),
        "budgets[0].labels[0]",
    ),
    (
        "repeated label",
        |f| f["budgets"][0]["labels"] = json!(["fast", "fast"]),
        "budgets[0].labels[1]",
    ),
    (
        "malformed unit",
        |f| f["budgets"][0]["unit"] = json!("Seconds"),
        "budgets[0].unit",
    ),
    (
        "empty command",
        |f| f["budgets"][0]["command"] = json!([]),
        "budgets[0].command",
    ),
    (
        "negative threshold",
        |f| f["budgets"][0]["threshold"] = json!(-1),
        "budgets[0].threshold",
    ),
    (
        "zero timeout",
        |f| f["budgets"][0]["timeout_seconds"] = json!(0),
        "budgets[0].timeout_seconds",
    ),
    (
        "id repeated in one file",
        |f| {
            let copy = f["budgets"][0].clone();
            f["budgets"].as_array_mut().unwrap().push(copy);
        },
        "budgets[1].id",
    ),
    (
        "elapsed not in seconds",
        |f| f["budgets"][0]["measure"] = json!("elapsed"),
        "budgets[0].unit",
    ),
    (
        "unsupported schema_version",
        |f| f["schema_version"] = json!(2),
        "schema_version",
    ),
    (
        "empty condition command",
        |f| f["conditions"][0]["command"] = json!([]),
        "conditions[0].command",
    ),
    (
        "missing budgets",
        |f| {
            f.as_object_mut().unwrap().remove("budgets");
        },
        "budgets",
    ),
    (
        "malformed condition name",
        |f| f["conditions"][0]["name"] = json!("Probe"),
        "conditions[0].name",
    ),
    (
        "condition name repeated",
        |f| {
            let copy = f["conditions"][0].clone();
            f["conditions"].as_array_mut().unwrap().push(copy);
        },
        "conditions[1].name",
    ),
    (
        "condition named load1",
        |f| f["conditions"][0]["name"] = json!("load1"),
        "conditions[0].name",
    ),
    (
        "condition named cpus",
        |f| f["conditions"][0]["name"] = json!("cpus"),
        "conditions[0].name",
    ),
    (
        "condition named mem_available_mib",
        |f| f["conditions"][0]["name"] = json!("mem_available_mib"),
        "conditions[0].name",
    ),
];

fn assert_refused(fixture: &Fixture, case: &str, key: &str) {
    for verb in ["validate", "check"] {
        let run = fixture.run([verb]);
        assert_eq!(
            run.status, 2,
            "{case}: {verb} exited {}\n{}",
            run.status, run.stderr
        );
        assert!(
            run.stdout.is_empty(),
            "{case}: {verb} printed {}",
            run.stdout
        );
        assert!(
            run.stderr.contains("budgets.yaml"),
            "{case}: {verb} did not name the file:\n{}",
            run.stderr
        );
        assert!(
            run.stderr.contains(key),
            "{case}: {verb} did not name {key}:\n{}",
            run.stderr
        );
        assert!(
            fixture.log("ran.log").is_empty(),
            "{case}: {verb} ran a command"
        );
    }
}

#[test]
fn every_invalid_shape_is_refused_by_validate_and_check() {
    for (case, defect, key) in DEFECTS {
        let fixture = Fixture::new();
        let mut contents = base(&fixture);
        defect(&mut contents);
        fixture.budgets("budgets.yaml", &contents);
        assert_refused(&fixture, case, key);
    }
}

#[test]
fn a_non_finite_threshold_is_refused() {
    for infinite in [".inf", "-.inf", ".nan"] {
        let fixture = Fixture::new();
        let contents = base(&fixture);
        let yaml = serde_norway::to_string(&contents)
            .unwrap()
            .replace("threshold: 5", &format!("threshold: {infinite}"));
        assert!(yaml.contains(infinite));
        fixture.write("budgets.yaml", &yaml);
        assert_refused(&fixture, infinite, "budgets[0].threshold");
    }
}

#[test]
fn a_file_that_is_not_a_mapping_and_one_with_several_problems_are_refused() {
    let fixture = Fixture::new();
    base(&fixture);
    fixture.write("budgets.yaml", "- just\n- a list\n");
    assert_refused(&fixture, "a list", "budgets.yaml");

    let mut contents = base(&fixture);
    contents["budgets"][0]["id"] = json!("Bad");
    contents["budgets"][0]["unit"] = json!("Bad");
    fixture.budgets("budgets.yaml", &contents);
    let run = fixture.run(["validate"]);
    run.expect_status(2);
    assert!(run.stderr.contains("budgets[0].id"), "{}", run.stderr);
    assert!(run.stderr.contains("budgets[0].unit"), "{}", run.stderr);
    assert!(run.stderr.contains("problems above"), "{}", run.stderr);
}

#[test]
fn the_valid_base_is_accepted() {
    let fixture = Fixture::new();
    let contents = base(&fixture);
    fixture.budgets("budgets.yaml", &contents);
    fixture.run(["validate"]).expect_status(0);
    fixture
        .run(["check", "--json"])
        .expect_status(0)
        .check_report();
    assert_eq!(fixture.log("ran.log"), ["condition", "budget"]);
}

#[test]
fn a_budgets_file_that_is_not_utf8_is_refused() {
    let fixture = Fixture::new();
    let contents = base(&fixture);
    let path = fixture.budgets("budgets.yaml", &contents);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"# \xff\xfe\n");
    std::fs::write(&path, bytes).unwrap();
    assert_refused(&fixture, "not UTF-8", "cannot read");
}

#[cfg(unix)]
#[test]
fn a_budgets_file_that_cannot_be_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let contents = base(&fixture);
    let path = fixture.budgets("budgets.yaml", &contents);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        // Running as root: permissions lock nothing, so there is no refusal to see.
        return;
    }
    assert_refused(&fixture, "unreadable", "cannot read");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}
