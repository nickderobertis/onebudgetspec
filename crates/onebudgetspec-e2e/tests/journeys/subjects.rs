//! `check --subjects`: one selection checked once per subject, each subject's budgets files
//! read from its own directory and its commands run there with its own environment, and the
//! verdicts reported per subject, with every subject that could not be checked named apart.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::common::{Fixture, Run, file, node, validate};

/// Reports the minutes the subject's environment gives in `SUBJECT_MINUTES`.
const DELIVERY: &str = "require(\"fs\").writeFileSync(process.env.ONEBUDGETSPEC_RESULT, \
    JSON.stringify({ value: Number(process.env.SUBJECT_MINUTES) }));";
/// Leaves `gate-runs.ran` where it runs, then reports the number in `./measure.txt`.
const GATE_RUNS: &str = "const fs = require(\"fs\"); fs.writeFileSync(\"gate-runs.ran\", \"\"); \
    fs.writeFileSync(process.env.ONEBUDGETSPEC_RESULT, \
    JSON.stringify({ value: Number(fs.readFileSync(\"./measure.txt\", \"utf8\")) }));";

fn delivery_time() -> Value {
    json!({
        "id": "delivery-time",
        "labels": ["delivery"],
        "measure": "reported",
        "command": node(DELIVERY, &[]),
        "unit": "minutes",
        "direction": "max",
        "threshold": 10,
    })
}

fn gate_runs() -> Value {
    json!({
        "id": "gate-runs",
        "labels": ["gates"],
        "measure": "reported",
        "command": node(GATE_RUNS, &[]),
        "unit": "runs",
        "direction": "max",
        "threshold": 5,
    })
}

/// Two subjects sharing one budgets file's shape, each with its own `measure.txt`; a third
/// whose directory holds no budgets file; and a fourth holding only a `gates` budget.
fn subjects(fixture: &Fixture) {
    for (subject, runs) in [("change-a", "2"), ("change-b", "7")] {
        fixture.budgets(
            &format!("{subject}/budgets.yaml"),
            &file(&[delivery_time(), gate_runs()]),
        );
        fixture.write(&format!("{subject}/measure.txt"), runs);
    }
    fixture.write("change-c/README.md", "no budgets here\n");
    fixture.budgets("change-d/budgets.yaml", &file(&[gate_runs()]));
    fixture.write("change-d/measure.txt", "1");
}

/// The subjects document over the fixture: `change-a`'s environment puts it over the
/// delivery budget, `change-b`'s keeps it within.
fn document(base: &str, with_d: bool) -> Value {
    let at = |name: &str| {
        if base.is_empty() {
            name.to_owned()
        } else {
            format!("{base}/{name}")
        }
    };
    let mut subjects = vec![
        json!({
            "id": "change-a",
            "directory": at("change-a"),
            "env": { "SUBJECT_MINUTES": "30" },
            "context": { "node": 1, "title": "first" },
        }),
        json!({
            "id": "change-b",
            "directory": at("change-b"),
            "env": { "SUBJECT_MINUTES": "4" },
            "files": ["budgets.yaml"],
            "context": ["second", 2],
        }),
        json!({
            "id": "change-c",
            "directory": at("change-c"),
            "env": { "SUBJECT_MINUTES": "1" },
            "context": "third",
        }),
    ];
    if with_d {
        subjects.push(json!({
            "id": "change-d",
            "directory": at("change-d"),
            "context": null,
        }));
    }
    json!({ "schema_version": 1, "subjects": subjects })
}

/// Run the binary from `cwd` with `document` on stdin as `--subjects -`.
fn run_with_stdin(cwd: &Path, args: &[&str], document: &Value) -> Run {
    let mut child = crate::common::spawn(
        Command::new(crate::common::binary())
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(document.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    Run {
        status: output.status.code().expect("an exit status"),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The subject with `id` in a subjects report's `key` list.
fn subject<'a>(answer: &'a Value, key: &str, id: &str) -> &'a Value {
    answer[key]
        .as_array()
        .expect("a list")
        .iter()
        .find(|subject| subject["id"] == id)
        .unwrap_or_else(|| panic!("no {key} entry {id} in {answer:#}"))
}

/// `(budget id, verdict, actual)` for every result in a subject's report.
fn verdicts(subject: &Value) -> Vec<(String, String, f64)> {
    subject["report"]["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|result| {
            (
                result["id"].as_str().unwrap().to_owned(),
                result["verdict"].as_str().unwrap().to_owned(),
                result["actual"].as_f64().unwrap(),
            )
        })
        .collect()
}

fn ids(answer: &Value, key: &str) -> Vec<String> {
    crate::common::ids(answer, key)
}

#[test]
fn each_subject_is_checked_in_its_own_directory_and_environment() {
    let fixture = Fixture::new();
    subjects(&fixture);
    fixture.write("subjects.json", &document("", false).to_string());

    let run = fixture.run(["check", "--subjects", "subjects.json", "--json"]);
    let answer = run.expect_status(1).subjects_report();
    assert_eq!(answer["schema_version"], 1);
    assert_eq!(ids(&answer, "subjects"), ["change-a", "change-b"]);

    let a = subject(&answer, "subjects", "change-a");
    assert_eq!(a["context"], json!({ "node": 1, "title": "first" }));
    validate("check-report", &a["report"]);
    assert_eq!(
        verdicts(a),
        [
            ("delivery-time".into(), "over".into(), 30.0),
            ("gate-runs".into(), "within".into(), 2.0),
        ]
    );
    assert_eq!(a["report"]["results"][0]["file"], "budgets.yaml");

    let b = subject(&answer, "subjects", "change-b");
    assert_eq!(b["context"], json!(["second", 2]));
    validate("check-report", &b["report"]);
    assert_eq!(
        verdicts(b),
        [
            ("delivery-time".into(), "within".into(), 4.0),
            ("gate-runs".into(), "over".into(), 7.0),
        ]
    );

    assert_eq!(ids(&answer, "unanswered"), ["change-c"]);
    let c = subject(&answer, "unanswered", "change-c");
    assert_eq!(c["context"], "third");
    let reason = c["reason"].as_str().unwrap();
    assert!(
        reason.contains("budgets.yaml") && reason.contains("no such file"),
        "{reason}"
    );
    // The environment a subject supplies reaches only that subject's commands.
    assert!(!fixture.path().join("gate-runs.ran").exists());
}

#[test]
fn over_only_keeps_only_the_over_results_and_the_exit_status() {
    let fixture = Fixture::new();
    subjects(&fixture);
    fixture.write("subjects.json", &document("", false).to_string());

    let run = fixture.run([
        "check",
        "--subjects",
        "subjects.json",
        "--json",
        "--over-only",
    ]);
    let answer = run.expect_status(1).subjects_report();
    assert_eq!(
        verdicts(subject(&answer, "subjects", "change-a")),
        [("delivery-time".into(), "over".into(), 30.0)]
    );
    assert_eq!(
        verdicts(subject(&answer, "subjects", "change-b")),
        [("gate-runs".into(), "over".into(), 7.0)]
    );
    assert_eq!(ids(&answer, "unanswered"), ["change-c"]);
}

#[test]
fn markdown_renders_one_section_per_subject() {
    let fixture = Fixture::new();
    subjects(&fixture);
    let run = run_with_stdin(
        fixture.path(),
        &["check", "--subjects", "-", "--output", "markdown"],
        &document("", false),
    );
    run.expect_status(1);
    let markdown = &run.stdout;
    let sections: Vec<&str> = markdown
        .lines()
        .filter(|line| line.starts_with("## "))
        .collect();
    assert_eq!(
        sections,
        [
            "## Subject `change-a`",
            "## Subject `change-b`",
            "## Unanswered"
        ],
        "{markdown}"
    );
    let a = &markdown[markdown.find("## Subject `change-a`").unwrap()
        ..markdown.find("## Subject `change-b`").unwrap()];
    assert!(
        a.contains("| `delivery-time` | over | 30 minutes | 10 minutes | -20 minutes |"),
        "{a}"
    );
    assert!(
        a.contains("| `gate-runs` | within | 2 runs | 5 runs | 3 runs |"),
        "{a}"
    );
    assert!(markdown.contains("- `change-c`: "), "{markdown}");
}

#[test]
fn text_names_each_subject_and_each_unanswered_one() {
    let fixture = Fixture::new();
    subjects(&fixture);
    fixture.write("subjects.json", &document("", false).to_string());
    let run = fixture.run(["check", "--subjects", "subjects.json"]);
    run.expect_status(1);
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(lines[0], "subject change-a:");
    assert!(
        lines[1].starts_with("  budget delivery-time: actual 30 minutes"),
        "{}",
        run.stdout
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("subject change-c: unanswered — ")),
        "{}",
        run.stdout
    );
}

#[test]
fn a_label_selects_one_budget_per_subject_and_an_empty_selection_is_unanswered() {
    let fixture = Fixture::new();
    subjects(&fixture);
    fixture.write("subjects.json", &document("", true).to_string());

    let run = fixture.run([
        "check",
        "--subjects",
        "subjects.json",
        "--label",
        "delivery",
        "--json",
    ]);
    let answer = run.expect_status(1).subjects_report();
    assert_eq!(
        verdicts(subject(&answer, "subjects", "change-a")),
        [("delivery-time".into(), "over".into(), 30.0)]
    );
    assert_eq!(
        verdicts(subject(&answer, "subjects", "change-b")),
        [("delivery-time".into(), "within".into(), 4.0)]
    );
    for unselected in ["change-a", "change-b", "change-d"] {
        assert!(
            !fixture
                .path()
                .join(unselected)
                .join("gate-runs.ran")
                .exists(),
            "{unselected}'s unselected gate-runs command ran"
        );
    }
    assert_eq!(ids(&answer, "unanswered"), ["change-c", "change-d"]);
    let d = subject(&answer, "unanswered", "change-d");
    assert_eq!(d["context"], Value::Null);
    let reason = d["reason"].as_str().unwrap();
    assert!(reason.contains("selection is empty"), "{reason}");
}

#[test]
fn every_result_within_exits_0_and_an_error_exits_3() {
    let fixture = Fixture::new();
    subjects(&fixture);
    let within = json!({ "schema_version": 1, "subjects": [{
        "id": "calm", "directory": "change-a", "env": { "SUBJECT_MINUTES": "1" },
    }] });
    run_with_stdin(
        fixture.path(),
        &["check", "--subjects", "-", "--json"],
        &within,
    )
    .expect_status(0);
    // No SUBJECT_MINUTES: the delivery command reports NaN, which is an error.
    let errored = json!({ "schema_version": 1, "subjects": [
        { "id": "bare", "directory": "change-a" },
        { "id": "over", "directory": "change-b", "env": { "SUBJECT_MINUTES": "99" } },
    ] });
    let run = run_with_stdin(
        fixture.path(),
        &["check", "--subjects", "-", "--json"],
        &errored,
    );
    let answer = run.expect_status(3).subjects_report();
    let bare = subject(&answer, "subjects", "bare");
    assert_eq!(bare["report"]["results"][0]["verdict"], "error");
}

#[test]
fn an_invalid_document_is_refused_with_2_running_nothing() {
    let fixture = Fixture::new();
    subjects(&fixture);
    for (document, expected) in [
        ("not json", "subjects:"),
        (r#"{"schema_version": 2, "subjects": []}"#, "schema_version"),
        (
            r#"{"schema_version": 1, "subjects": [{"id": "x", "directory": "change-a"}, {"id": "x", "directory": "change-b"}]}"#,
            "subjects[1].id",
        ),
        (
            r#"{"schema_version": 1, "subjects": [{"id": "x", "directory": "change-a", "colour": 1}]}"#,
            "colour",
        ),
    ] {
        fixture.write("subjects.json", document);
        let run = fixture.run(["check", "--subjects", "subjects.json", "--json"]);
        run.expect_status(2);
        assert!(run.stdout.is_empty(), "{}", run.stdout);
        assert!(run.stderr.contains(expected), "{expected}: {}", run.stderr);
    }
    assert!(!fixture.path().join("change-a/gate-runs.ran").exists());
    let run = fixture.run(["check", "--subjects", "missing.json"]);
    run.expect_status(2);
    assert!(run.stderr.contains("missing.json"), "{}", run.stderr);
    // Each subject names its own files, so a PATH beside --subjects is refused.
    fixture
        .run([
            "check",
            "--subjects",
            "subjects.json",
            "change-a/budgets.yaml",
        ])
        .expect_status(2);
    // --over-only means something only with --subjects.
    fixture
        .run(["check", "--over-only", "change-a/budgets.yaml"])
        .expect_status(2);
}

/// Strip what differs between two runs of the same measurement: the times and the host.
fn steady(report: &Value) -> Value {
    let mut report = report.clone();
    for result in report["results"].as_array_mut().unwrap() {
        let result = result.as_object_mut().unwrap();
        result.remove("started_at");
        result.remove("ended_at");
        result.remove("host");
    }
    report
}

#[test]
fn a_subjects_report_is_the_check_report_check_prints_for_it() {
    let fixture = Fixture::new();
    subjects(&fixture);
    fixture.write("subjects.json", &document("", false).to_string());
    let answer = fixture
        .run(["check", "--subjects", "subjects.json", "--json"])
        .subjects_report();

    // A check with no --subjects, in the subject's directory and environment.
    let plain = crate::common::run_in(
        &fixture.path().join("change-a"),
        ["check", "--json"],
        &[("SUBJECT_MINUTES", "30")],
    );
    let plain = plain.expect_status(1).check_report();
    assert_eq!(
        steady(&subject(&answer, "subjects", "change-a")["report"]),
        steady(&plain)
    );
}

#[test]
fn the_library_answers_as_the_binary_does() {
    let fixture = Fixture::new();
    subjects(&fixture);
    let base = fixture.path().to_string_lossy().replace('\\', "/");
    let document = document(&base, true);
    let selection = onebudgetspec_core::Selection {
        labels: vec!["delivery".into()],
        ..onebudgetspec_core::Selection::default()
    };
    let parsed = onebudgetspec_core::Subjects::from_json(document.to_string().as_bytes())
        .expect("a valid subjects document");
    let library = onebudgetspec_core::check_subjects(&parsed, &selection, false);
    assert_eq!(library.exit_code(), 1);

    let binary = run_with_stdin(
        fixture.path(),
        &["check", "--subjects", "-", "--label", "delivery", "--json"],
        &document,
    );
    let binary = binary.expect_status(1).subjects_report();
    let library = serde_json::to_value(&library).unwrap();
    for (from_library, from_binary) in library["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .zip(binary["subjects"].as_array().unwrap())
    {
        assert_eq!(from_library["id"], from_binary["id"]);
        assert_eq!(from_library["context"], from_binary["context"]);
        assert_eq!(
            steady(&from_library["report"]),
            steady(&from_binary["report"])
        );
    }
    assert_eq!(ids(&library, "subjects"), ids(&binary, "subjects"));
    assert_eq!(library["unanswered"], binary["unanswered"]);
}
