//! The library's public API, called the way a Rust consumer calls it: real files in a
//! temporary directory and real commands, with nothing doubled.

use std::fs;
use std::path::{Path, PathBuf};

use onebudgetspec_core::{
    Direction, Error, Measure, Selection, Verdict, discover, exit, load, schema_bundle,
};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn script(path: &Path, body: &str) {
    write(path, &format!("#!/bin/sh\n{body}\n"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

const TWO_BUDGETS: &str = r#"schema_version: 1
conditions:
  - name: runners
    command: ["sh", "-c", "echo '  4  '"]
budgets:
  - id: gate-time
    description: How long the gate takes.
    labels: [gate, slow]
    measure: reported
    command: ["./report.sh", "1395"]
    unit: seconds
    direction: max
    threshold: 1800
    timeout_seconds: 60
  - id: startup
    measure: elapsed
    command: ["sleep", "0.1"]
    unit: seconds
    direction: max
    threshold: 30
"#;

fn two_budgets(dir: &Path) -> PathBuf {
    let file = dir.join("budgets.yaml");
    write(&file, TWO_BUDGETS);
    script(
        &dir.join("report.sh"),
        r#"printf '{"value": %s, "detail": "from the api test"}' "$1" > "$ONEBUDGETSPEC_RESULT""#,
    );
    file
}

#[test]
fn loads_validates_and_lists_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = two_budgets(dir.path());

    let budgets = load(&[file.clone()], false).unwrap();
    assert_eq!(budgets.files().len(), 1);
    assert_eq!(budgets.budget_count(), 2);

    let report = budgets.all().list_report();
    assert_eq!(report.schema_version, 1);
    let ids: Vec<_> = report.budgets.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["gate-time", "startup"]);
    let gate = &report.budgets[0];
    assert_eq!(gate.file, file.to_string_lossy());
    assert_eq!(
        gate.description.as_deref(),
        Some("How long the gate takes.")
    );
    assert_eq!(gate.labels, ["gate", "slow"]);
    assert_eq!(gate.measure, Measure::Reported);
    assert_eq!(gate.command, ["./report.sh", "1395"]);
    assert_eq!(gate.unit, "seconds");
    assert_eq!(gate.direction, Direction::Max);
    assert!((gate.threshold - 1800.0).abs() < f64::EPSILON);
    assert_eq!(gate.timeout_seconds, Some(60));
    assert_eq!(report.budgets[1].description, None);
    assert_eq!(report.budgets[1].timeout_seconds, None);
}

#[test]
fn refuses_an_invalid_file_naming_the_file_and_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("budgets.yaml");
    write(
        &file,
        "schema_version: 1\nbudgets:\n  - id: Bad\n    measure: elapsed\n    command: [true]\n    unit: ms\n    direction: max\n    threshold: -1\n",
    );
    let Err(Error::Invalid { problems }) = load(&[file], false) else {
        panic!("an invalid file must be refused");
    };
    let joined = problems.join("\n");
    assert!(joined.contains("budgets[0].id"), "{joined}");
    assert!(joined.contains("budgets[0].unit"), "{joined}");
    assert!(joined.contains("budgets[0].threshold"), "{joined}");
    assert!(
        problems.iter().all(|p| p.contains("budgets.yaml")),
        "{joined}"
    );
}

#[test]
fn discovers_nested_files_in_path_order_honouring_gitignore() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let entry = |id: &str| {
        format!(
            "schema_version: 1\nbudgets:\n  - id: {id}\n    measure: elapsed\n    command: [\"true\"]\n    unit: seconds\n    direction: max\n    threshold: 5\n"
        )
    };
    write(&root.join("budgets.yaml"), &entry("root"));
    write(&root.join("b/budgets.yaml"), &entry("b"));
    write(&root.join("a/deep/budgets.yaml"), &entry("deep"));
    write(&root.join("ignored/budgets.yaml"), &entry("ignored"));
    write(&root.join(".gitignore"), "ignored/\n");

    let found = discover(&[root.to_path_buf()], true).unwrap();
    let shown: Vec<_> = found
        .iter()
        .map(|file| file.path.strip_prefix(root).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        shown,
        [
            PathBuf::from("a/deep/budgets.yaml"),
            PathBuf::from("b/budgets.yaml"),
            PathBuf::from("budgets.yaml"),
        ]
    );

    let Err(Error::Invocation { .. }) = discover(&[root.to_path_buf()], false) else {
        panic!("a directory without --recursive must be refused");
    };
    let Err(Error::Invocation { .. }) = discover(&[root.join("missing.yaml")], false) else {
        panic!("a missing file must be refused");
    };
}

#[test]
fn selects_by_id_label_and_excluded_label() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("budgets.yaml");
    let budget = |id: &str, labels: &str| {
        format!(
            "  - id: {id}\n    labels: {labels}\n    measure: elapsed\n    command: [\"true\"]\n    unit: seconds\n    direction: max\n    threshold: 5\n"
        )
    };
    write(
        &file,
        &format!(
            "schema_version: 1\nbudgets:\n{}{}{}{}",
            budget("one", "[fast]"),
            budget("two", "[slow]"),
            budget("three", "[fast, flaky]"),
            budget("four", "[]")
        ),
    );
    let budgets = load(&[file], false).unwrap();
    let ids = |selection: Selection| -> Vec<String> {
        budgets
            .select(&selection)
            .unwrap()
            .list_report()
            .budgets
            .into_iter()
            .map(|b| b.id)
            .collect()
    };

    assert_eq!(
        ids(Selection {
            ids: vec!["two".into(), "four".into()],
            ..Selection::default()
        }),
        ["two", "four"]
    );
    assert_eq!(
        ids(Selection {
            labels: vec!["fast".into(), "slow".into()],
            ..Selection::default()
        }),
        ["one", "two", "three"]
    );
    assert_eq!(
        ids(Selection {
            labels: vec!["fast".into()],
            exclude_labels: vec!["flaky".into()],
            ..Selection::default()
        }),
        ["one"]
    );
    assert!(
        ids(Selection {
            ids: vec!["three".into()],
            exclude_labels: vec!["flaky".into()],
            ..Selection::default()
        })
        .is_empty()
    );
    let Err(error @ Error::Invocation { .. }) = budgets.select(&Selection {
        ids: vec!["nope".into()],
        ..Selection::default()
    }) else {
        panic!("an unknown id must be refused");
    };
    assert_eq!(error.exit_code(), exit::INVALID);
}

#[test]
fn measures_a_reported_and_an_elapsed_budget() {
    let dir = tempfile::tempdir().unwrap();
    let file = two_budgets(dir.path());
    let budgets = load(&[file.clone()], false).unwrap();
    let report = budgets.all().check();

    assert_eq!(report.schema_version, 1);
    assert_eq!(report.exit_code(), exit::WITHIN);
    let [gate, startup] = &report.results[..] else {
        panic!("two results expected, got {report:?}");
    };

    assert_eq!(gate.id, "gate-time");
    assert_eq!(gate.file, file.to_string_lossy());
    assert_eq!(gate.labels, ["gate", "slow"]);
    assert_eq!(gate.verdict, Verdict::Within);
    assert_eq!(gate.actual, Some(1395.0));
    assert_eq!(gate.headroom, Some(405.0));
    assert_eq!(gate.headroom_percent, Some(405.0 / 1800.0 * 100.0));
    assert_eq!(gate.detail.as_deref(), Some("from the api test"));
    assert_eq!(gate.error, None);
    assert_eq!(
        gate.host.conditions.get("runners").map(String::as_str),
        Some("4")
    );
    assert!(gate.ended_at >= gate.started_at);
    assert!(gate.host.cpus >= 1);

    assert_eq!(startup.verdict, Verdict::Within);
    let actual = startup.actual.unwrap();
    assert!(actual >= 0.1, "slept 0.1s but measured {actual}");
    assert_eq!(startup.headroom, Some(30.0 - actual));
    assert_eq!(startup.detail, None);
    assert_eq!(startup.host.conditions, gate.host.conditions);
}

#[test]
fn an_over_budget_and_an_error_set_the_exit_status() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("budgets.yaml");
    write(
        &file,
        r#"schema_version: 1
budgets:
  - id: slow
    measure: elapsed
    command: ["sleep", "0.05"]
    unit: seconds
    direction: max
    threshold: 0
  - id: broken
    measure: reported
    command: ["sh", "-c", "exit 4"]
    unit: count
    direction: min
    threshold: 1
"#,
    );
    let budgets = load(&[file], false).unwrap();
    let only = |id: &str| Selection {
        ids: vec![id.into()],
        ..Selection::default()
    };

    let over = budgets.select(&only("slow")).unwrap().check();
    assert_eq!(over.results[0].verdict, Verdict::Over);
    assert_eq!(over.results[0].headroom_percent, None);
    assert_eq!(over.exit_code(), exit::OVER);

    let all = budgets.all().check();
    let broken = &all.results[1];
    assert_eq!(broken.verdict, Verdict::Error);
    assert!(broken.error.as_deref().unwrap().contains("status 4"));
    assert_eq!((broken.actual, broken.headroom), (None, None));
    assert_eq!(all.exit_code(), exit::ERROR);
}

#[test]
fn the_schema_bundle_has_the_three_roots() {
    let bundle = schema_bundle();
    let roots = bundle["roots"].as_object().unwrap();
    let names: Vec<_> = roots.keys().map(String::as_str).collect();
    assert_eq!(names, ["budgets-file", "check-report", "list-report"]);
}
