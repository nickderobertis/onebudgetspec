//! `check_subjects` called the way a Rust consumer calls it: a subjects document over real
//! directories and real `node` commands, with nothing doubled.

use std::fs;
use std::path::Path;

use onebudgetspec_core::{Error, Selection, Subjects, Verdict, check_subjects, exit};
use serde_json::{Value, json};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A budgets file whose one `reported` budget, `seen`, reports the number in the
/// environment variable `NUMBER`, and whose condition records the same variable.
fn number_budget(id: &str) -> String {
    format!(
        r#"schema_version: 1
conditions:
  - name: number
    command: ["node", "-e", "console.log(process.env.NUMBER)"]
budgets:
  - id: {id}
    measure: reported
    command: ["node", "-e", "require('fs').writeFileSync(process.env.ONEBUDGETSPEC_RESULT, JSON.stringify({{value: Number(process.env.NUMBER)}}))"]
    unit: things
    direction: max
    threshold: 10
"#
    )
}

fn parse(document: &Value) -> Subjects {
    Subjects::from_json(document.to_string().as_bytes()).expect("a valid subjects document")
}

fn problems(text: &str) -> Vec<String> {
    match Subjects::from_json(text.as_bytes()) {
        Err(Error::Invalid { problems }) => problems,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_document_is_held_to_its_rules_naming_every_problem() {
    let found = problems(
        r#"{"schema_version": 1, "subjects": [
            {"id": "a", "directory": "", "files": []},
            {"id": "b", "directory": "b", "files": [""], "env": {"": "x", "A=B": "y"}}
        ]}"#,
    );
    assert_eq!(
        found,
        [
            "subjects: subjects[0].directory: must not be empty",
            "subjects: subjects[0].files: must name at least one file; omit it for budgets.yaml",
            "subjects: subjects[1].files[0]: must not be empty",
            "subjects: subjects[1].env: \"\" is not an environment variable name",
            "subjects: subjects[1].env: \"A=B\" is not an environment variable name",
        ]
    );
    let shape = problems(r#"{"schema_version": 1, "subjects": [{"id": 3, "directory": "d"}]}"#);
    assert_eq!(shape.len(), 1);
    assert!(
        shape[0].starts_with("subjects: subjects[0].id: "),
        "{shape:?}"
    );
    let trailing = problems(r#"{"schema_version": 1, "subjects": []} trailing"#);
    assert_eq!(trailing.len(), 1);
    assert!(trailing[0].contains("trailing characters"), "{trailing:?}");
    let missing = problems(r#"{"schema_version": 1}"#);
    assert!(missing[0].contains("subjects"), "{missing:?}");
}

#[test]
fn each_subjects_environment_reaches_its_budgets_and_conditions_alone() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["low", "high"] {
        write(
            &dir.path().join(name).join("budgets.yaml"),
            &number_budget("seen"),
        );
    }
    let base = dir.path().to_string_lossy();
    let answer = check_subjects(
        &parse(&json!({ "schema_version": 1, "subjects": [
            { "id": "low", "directory": format!("{base}/low"), "env": { "NUMBER": "3" } },
            { "id": "high", "directory": format!("{base}/high"), "env": { "NUMBER": "12" } },
        ] })),
        &Selection::default(),
        false,
    );
    assert!(answer.unanswered.is_empty(), "{:?}", answer.unanswered);
    let low = &answer.subjects[0].report.results[0];
    assert_eq!((low.verdict, low.actual), (Verdict::Within, Some(3.0)));
    assert_eq!(low.host.conditions["number"], "3");
    let high = &answer.subjects[1].report.results[0];
    assert_eq!((high.verdict, high.actual), (Verdict::Over, Some(12.0)));
    assert_eq!(high.host.conditions["number"], "12");
    assert_eq!(answer.exit_code(), exit::OVER);
}

#[test]
fn recursive_files_are_searched_and_shown_from_the_subjects_directory() {
    let dir = tempfile::tempdir().unwrap();
    let subject = dir.path().join("checkout");
    write(
        &subject.join("services/api/budgets.yaml"),
        &number_budget("api"),
    );
    write(
        &subject.join("services/web/budgets.yaml"),
        &number_budget("web"),
    );
    let document = json!({ "schema_version": 1, "subjects": [{
        "id": "checkout",
        "directory": subject.to_string_lossy(),
        "env": { "NUMBER": "1" },
        "files": ["services"],
    }] });

    let answer = check_subjects(&parse(&document), &Selection::default(), true);
    let files: Vec<_> = answer.subjects[0]
        .report
        .results
        .iter()
        .map(|result| (result.id.as_str(), result.file.as_str()))
        .collect();
    assert_eq!(
        files,
        [
            ("api", "services/api/budgets.yaml"),
            ("web", "services/web/budgets.yaml")
        ]
    );
    assert_eq!(answer.exit_code(), exit::WITHIN);

    // Without --recursive a directory is no budgets file: the subject is unanswered.
    let flat = check_subjects(&parse(&document), &Selection::default(), false);
    assert!(flat.subjects.is_empty());
    assert!(
        flat.unanswered[0].reason.contains("is a directory"),
        "{}",
        flat.unanswered[0].reason
    );
}

#[test]
fn what_cannot_be_checked_is_unanswered_with_its_reason_and_the_rest_is_checked() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().into_owned();
    write(
        &dir.path().join("good/budgets.yaml"),
        &number_budget("seen"),
    );
    write(
        &dir.path().join("invalid/budgets.yaml"),
        "schema_version: 1\nbudgets: [{id: Bad}]\n",
    );
    let document = json!({ "schema_version": 1, "subjects": [
        { "id": "absent", "directory": format!("{base}/absent"), "context": 1 },
        { "id": "invalid", "directory": format!("{base}/invalid") },
        { "id": "good", "directory": format!("{base}/good"), "env": { "NUMBER": "x" } },
    ] });

    let answer = check_subjects(&parse(&document), &Selection::default(), false);
    let reasons: Vec<_> = answer
        .unanswered
        .iter()
        .map(|entry| (entry.id.as_str(), entry.reason.as_str()))
        .collect();
    assert_eq!(reasons[0].0, "absent");
    assert!(
        reasons[0]
            .1
            .starts_with(&format!("directory {base}/absent cannot be read: ")),
        "{}",
        reasons[0].1
    );
    assert_eq!(answer.unanswered[0].context, json!(1));
    assert_eq!(reasons[1].0, "invalid");
    assert!(
        reasons[1].1.starts_with("budgets.yaml: ")
            && reasons[1]
                .1
                .ends_with(&format!("(in directory {base}/invalid)")),
        "{}",
        reasons[1].1
    );
    // NUMBER is not a number, so the one checked subject's result is an error.
    assert_eq!(answer.subjects[0].report.results[0].verdict, Verdict::Error);
    assert_eq!(answer.exit_code(), exit::ERROR);

    // An id the subject does not register leaves it unanswered rather than refusing all.
    let unknown = check_subjects(
        &parse(&document),
        &Selection {
            ids: vec!["seen-elsewhere".into()],
            ..Selection::default()
        },
        false,
    );
    assert!(unknown.subjects.is_empty());
    assert!(
        unknown.unanswered[2]
            .reason
            .contains("no budget has the id seen-elsewhere"),
        "{:?}",
        unknown.unanswered
    );
}

#[test]
fn retain_over_keeps_every_subject_and_only_its_over_results() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("budgets.yaml"), &number_budget("seen"));
    let directory = dir.path().to_string_lossy();
    let mut answer = check_subjects(
        &parse(&json!({ "schema_version": 1, "subjects": [
            { "id": "within", "directory": directory, "env": { "NUMBER": "1" } },
        ] })),
        &Selection::default(),
        false,
    );
    assert_eq!(answer.exit_code(), exit::WITHIN);
    answer.retain_over();
    assert_eq!(answer.subjects.len(), 1);
    assert!(answer.subjects[0].report.results.is_empty());
}
