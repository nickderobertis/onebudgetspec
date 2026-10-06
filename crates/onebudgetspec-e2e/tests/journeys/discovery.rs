//! `--recursive` discovery: nested `budgets.yaml` files in path order, `.gitignore`
//! honoured, and ids unique across every file found.

use serde_json::json;

use crate::common::{Fixture, file, ids, reported, result, write_result};

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

    // A root given with a trailing separator, `/` or the platform's own, reads the same.
    for root in [
        "alpha/".to_owned(),
        format!("alpha{}", std::path::MAIN_SEPARATOR),
    ] {
        let trailing = fixture
            .run(["check", "--json", "--recursive", root.as_str()])
            .expect_status(0)
            .check_report();
        assert_eq!(result(&trailing, "alpha")["file"], "alpha/budgets.yaml");
        assert_eq!(
            result(&trailing, "alpha-deep")["file"],
            "alpha/deep/budgets.yaml"
        );
    }
}

#[test]
fn an_id_repeated_across_two_files_is_refused() {
    let fixture = Fixture::new();
    fixture.counted("ran.js", "ran.log", "ran", &write_result(r#"{"value": 1}"#));
    let counted = json!({
        "id": "shared",
        "measure": "reported",
        "command": ["node", "../ran.js"],
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

// Unix only: it locks the directory with POSIX permission bits.
#[cfg(unix)]
#[test]
fn a_directory_recursive_discovery_cannot_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    nested(&fixture);
    let locked = fixture.path().join("zeta");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read_dir(&locked).is_ok();
    let run = fixture.run(["check", "--recursive"]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        // Running as root: permissions lock nothing, so there is no refusal to see.
        return;
    }
    run.expect_status(2);
    assert!(run.stderr.contains("cannot search"), "{}", run.stderr);
}

/// Discovery reads `.gitignore` alone: a budgets file in a hidden directory is and
/// neither an `.ignore` file nor the user's global git excludes can hide one, while the
/// repository's own `.git` directory is never searched.
#[test]
fn only_gitignore_hides_a_budgets_file_and_git_internals_are_never_searched() {
    let fixture = Fixture::new();
    fixture.budgets(
        ".config/budgets.yaml",
        &file(&[reported("hidden-dir", 1.0, "max", 2.0)]),
    );
    fixture.budgets(
        "listed-in-ignore/budgets.yaml",
        &file(&[reported("dot-ignore", 1.0, "max", 2.0)]),
    );
    fixture.write(".ignore", "listed-in-ignore/\n");
    fixture.budgets(
        "globally-excluded/budgets.yaml",
        &file(&[reported("global-exclude", 1.0, "max", 2.0)]),
    );
    let home = fixture.path().join("home");
    fixture.write("home/.config/git/ignore", "globally-excluded/\n");
    fixture.write(
        "home/.gitconfig",
        "[core]\n\texcludesFile = ~/.config/git/ignore\n",
    );
    fixture.budgets(
        ".git/budgets.yaml",
        &file(&[reported("inside-git", 1.0, "max", 2.0)]),
    );

    let home = home.to_str().unwrap();
    let report = crate::common::run_in(
        fixture.path(),
        ["check", "--json", "--recursive"],
        &[
            ("HOME", home),
            ("XDG_CONFIG_HOME", &format!("{home}/.config")),
        ],
    )
    .expect_status(0)
    .check_report();
    assert_eq!(
        ids(&report, "results"),
        ["hidden-dir", "global-exclude", "dot-ignore"]
    );
}
