//! The `onebudgetspec-sdk` wheel, installed into a fresh environment beside the
//! `onebudgetspec-cli` wheel it requires, checks, validates and lists a conformance case and
//! prints the schema through the binary that wheel installed; and a measurement written in
//! Python reports through it to that binary's check.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::common::{
    SDK_CASE, VERSION, artifact, assert_reports, assert_sdk_answers, conformance_case, exe,
    node_dir, succeed, venv_bin, write_reporting_budgets,
};

/// Each call over the case named by argv[1], with the selection its case.json gives.
const DRIVE: &str = r#"
import json, sys
from pathlib import Path
import onebudgetspec_sdk as sdk

case = Path(sys.argv[1])
args = json.loads((case / "case.json").read_text())["args"]
flags = {"--id": [], "--label": [], "--exclude-label": []}
words = iter(args[1:])
for word in words:
    if word in flags:
        flags[word].append(next(words))
selection = {
    "ids": flags["--id"],
    "labels": flags["--label"],
    "exclude_labels": flags["--exclude-label"],
}
print(json.dumps({
    "check": sdk.check(cwd=case, **selection).model_dump(mode="json"),
    "list": sdk.list_budgets(cwd=case, **selection).model_dump(mode="json"),
    "validate": sdk.validate(cwd=case).model_dump(mode="json"),
    "schema": sdk.schema(),
    "binary": str(sdk.resolve_binary()),
}))
"#;

/// The `Requires-Dist` lines of the wheel's own METADATA.
const REQUIRES: &str = r#"
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as wheel:
    name = next(n for n in wheel.namelist() if n.endswith(".dist-info/METADATA"))
    for line in wheel.read(name).decode().splitlines():
        if line.startswith("Requires-Dist: "):
            print(line.removeprefix("Requires-Dist: "))
"#;

/// A fresh environment under `dir` with `wheels` installed and their other requirements
/// resolved; returns its [`venv_bin`] directory.
fn venv_with_requirements(dir: &Path, wheels: &[&Path]) -> std::path::PathBuf {
    let venv = dir.join("venv");
    succeed("uv", &["venv", "--quiet", venv.to_str().unwrap()], dir);
    let python = venv_bin(&venv).join(exe("python"));
    let mut args = vec![
        "pip",
        "install",
        "--quiet",
        "--python",
        python.to_str().unwrap(),
    ];
    args.extend(wheels.iter().map(|wheel| wheel.to_str().unwrap()));
    succeed("uv", &args, dir);
    venv_bin(&venv)
}

#[test]
fn the_wheel_requires_the_cli_at_exactly_the_workspace_version() {
    let wheel = artifact("sdk-python");
    let name = wheel.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with(&format!("onebudgetspec_sdk-{VERSION}-")),
        "{name}"
    );
    let requires = succeed(
        "uv",
        &[
            "run",
            "--no-project",
            "--quiet",
            "python",
            "-c",
            REQUIRES,
            wheel.to_str().unwrap(),
        ],
        Path::new("."),
    );
    let cli: Vec<&str> = requires
        .lines()
        .filter(|line| line.starts_with("onebudgetspec-cli"))
        .collect();
    assert_eq!(cli, [format!("onebudgetspec-cli=={VERSION}")], "{requires}");
}

#[test]
fn the_installed_sdk_answers_a_conformance_case_through_the_wheel_s_binary() {
    let sdk = artifact("sdk-python");
    let cli = artifact("cli-wheel");
    let dir = tempfile::tempdir().unwrap();
    let bin = venv_with_requirements(dir.path(), &[&sdk, &cli]);
    let installed = bin.join(exe("onebudgetspec")).canonicalize().unwrap();
    let python = bin.join(exe("python"));

    let version = succeed(
        &python,
        &[
            "-c",
            "import onebudgetspec_sdk; print(onebudgetspec_sdk.__version__)",
        ],
        dir.path(),
    );
    assert_eq!(version.trim(), VERSION);
    let typed = succeed(
        &python,
        &[
            "-c",
            "import importlib.resources as r; print(r.files('onebudgetspec_sdk').joinpath('py.typed').is_file())",
        ],
        dir.path(),
    );
    assert_eq!(typed.trim(), "True", "the wheel ships no py.typed marker");

    let (case_dir, case) = conformance_case(dir.path(), SDK_CASE);
    // No ONEBUDGETSPEC_BIN, and on PATH only the environment's own programs, the node the
    // case measures with, and the system's directories: the SDK must find the binary the
    // onebudgetspec-cli wheel installed.
    let system: Vec<std::path::PathBuf> = if cfg!(windows) {
        let windows = std::env::var_os("SystemRoot").expect("Windows sets SystemRoot");
        vec![Path::new(&windows).join("System32")]
    } else {
        vec!["/usr/bin".into(), "/bin".into()]
    };
    let path = std::env::join_paths([bin.clone(), node_dir()].iter().chain(&system))
        .expect("the directories join into a PATH");
    let output = Command::new(&python)
        .args(["-c", DRIVE, case_dir.to_str().unwrap()])
        .env_remove("ONEBUDGETSPEC_BIN")
        .env("PATH", path)
        .current_dir(dir.path())
        .output()
        .expect("the environment's python runs");
    assert!(
        output.status.success(),
        "the SDK failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut answers: Value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    let resolved = Path::new(answers["binary"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    answers["binary"] = resolved.to_string_lossy().into_owned().into();
    assert_sdk_answers(&answers, &case_dir, &case, &installed);
}

/// One generic runner for every budget of a file: its figure is chosen by the budget's id.
const MEASURE: &str = r#"
import os
from onebudgetspec_sdk import report

figures = {"api-requests": (12.5, "p95 of the api"), "worker-requests": (30, None)}
value, detail = figures[os.environ["ONEBUDGETSPEC_BUDGET_ID"]]
assert report(value, detail) is True
"#;

#[test]
fn a_python_measurement_reports_through_the_installed_sdk_to_the_wheel_s_binary() {
    let sdk = artifact("sdk-python");
    let cli = artifact("cli-wheel");
    let dir = tempfile::tempdir().unwrap();
    let bin = venv_with_requirements(dir.path(), &[&sdk, &cli]);
    let python = bin.join(exe("python"));
    let budgets = dir.path().join("budgets");
    std::fs::create_dir_all(&budgets).unwrap();
    std::fs::write(budgets.join("measure.py"), MEASURE).unwrap();
    write_reporting_budgets(&budgets, &[python.to_str().unwrap(), "measure.py"]);
    assert_reports(&bin.join(exe("onebudgetspec")), &[], &budgets);
}
