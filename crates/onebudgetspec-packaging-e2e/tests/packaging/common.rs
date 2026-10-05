//! What the packaging journeys share: building an artifact the way the release does, the
//! tools that install it, and one real budgets case to drive an installed entry point over.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// The workspace version every distribution releases at.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists")
}

/// Where the artifacts of this test run are built, shared by every journey in it.
fn dist() -> &'static Path {
    static DIST: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIST.get_or_init(|| tempfile::tempdir().expect("a temporary directory"))
        .path()
}

/// Build `artifact` with `scripts/build-dist.sh` (once per run) and return its path.
pub fn artifact(artifact: &str) -> PathBuf {
    static BUILT: Mutex<Vec<(String, PathBuf)>> = Mutex::new(Vec::new());
    let mut built = BUILT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, path)) = built.iter().find(|(name, _)| name == artifact) {
        return path.clone();
    }
    let out = dist().join(artifact);
    let output = Command::new("bash")
        .arg(root().join("scripts/build-dist.sh"))
        .arg(artifact)
        .arg(&out)
        .output()
        .expect("bash runs scripts/build-dist.sh");
    assert!(
        output.status.success(),
        "scripts/build-dist.sh {artifact} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    assert!(
        path.is_file(),
        "build-dist printed {}, which is not a file",
        path.display()
    );
    built.push((artifact.to_owned(), path.clone()));
    path
}

/// Run `program` with `args` from `cwd`, asserting it succeeds; returns its stdout.
pub fn succeed(program: impl AsRef<std::ffi::OsStr>, args: &[&str], cwd: &Path) -> String {
    let output = run(program.as_ref(), args, cwd);
    assert!(
        output.status.success(),
        "{} {args:?} failed:\nstdout:\n{}\nstderr:\n{}",
        program.as_ref().to_string_lossy(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}

pub fn run(program: impl AsRef<std::ffi::OsStr>, args: &[&str], cwd: &Path) -> Output {
    let program = program.as_ref();
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "cannot run {}: {error}; install it (the packaging journeys need uv, node and npm)",
                program.to_string_lossy()
            )
        })
}

/// A fresh virtual environment under `dir`, with `wheel` installed from the local file.
/// Returns the environment's `bin` directory.
pub fn venv_with(dir: &Path, wheel: &Path) -> PathBuf {
    let venv = dir.join("venv");
    succeed("uv", &["venv", "--quiet", venv.to_str().unwrap()], dir);
    let python = venv.join("bin/python");
    succeed(
        "uv",
        &[
            "pip",
            "install",
            "--quiet",
            "--offline",
            "--no-deps",
            "--python",
            python.to_str().unwrap(),
            wheel.to_str().unwrap(),
        ],
        dir,
    );
    venv.join("bin")
}

/// A fresh npm project under `dir` with `tarballs` installed from the local files and no
/// registry contacted. Returns the project directory.
pub fn npm_project_with(dir: &Path, tarballs: &[&Path]) -> PathBuf {
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"name": "consumer", "private": true}"#,
    )
    .unwrap();
    let mut args = vec![
        "install",
        "--offline",
        "--ignore-scripts",
        "--no-audit",
        "--no-fund",
        "--silent",
    ];
    let paths: Vec<String> = tarballs
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    args.extend(paths.iter().map(String::as_str));
    succeed("npm", &args, &project);
    project
}

/// The cargo-built binary, which an installed entry point must behave exactly like.
pub fn direct_binary() -> PathBuf {
    let binary = root().join("target/debug/onebudgetspec");
    assert!(
        binary.is_file(),
        "{} is missing; build it with `cargo build -p onebudgetspec` (Nx's test target depends on that build)",
        binary.display()
    );
    binary
}

/// Lay out one real case under `dir`: a budget within and a budget over, so its check
/// exits 1 and reports both.
pub fn case(dir: &Path) -> PathBuf {
    let case = dir.join("case");
    fs::create_dir_all(&case).unwrap();
    fs::write(
        case.join("budgets.yaml"),
        r#"schema_version: 1
budgets:
  - id: fast
    measure: reported
    command: ["sh", "-c", "echo '{\"value\": 2}' > \"$ONEBUDGETSPEC_RESULT\""]
    unit: ms
    direction: max
    threshold: 5
  - id: slow
    measure: reported
    command: ["sh", "-c", "echo '{\"value\": 9, \"detail\": \"too slow\"}' > \"$ONEBUDGETSPEC_RESULT\""]
    unit: ms
    direction: max
    threshold: 5
"#,
    )
    .unwrap();
    case
}

/// `check --json` over `case` with `program`: its exit status and its report with the
/// timestamps and host values (which differ between any two runs) normalized away.
pub fn check(program: &Path, case: &Path) -> (i32, Value) {
    let output = run(program, &["check", "--json"], case);
    let mut report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{} printed no report ({error}):\n{}",
            program.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    for result in report["results"].as_array_mut().expect("a results array") {
        result["started_at"] = Value::Null;
        result["ended_at"] = Value::Null;
        result["host"] = Value::Null;
    }
    (output.status.code().expect("an exit status"), report)
}

/// Assert `program` reports the release version and checks `case` exactly as the
/// cargo-built binary does.
pub fn behaves_like_the_binary(program: &Path, dir: &Path) {
    let version = succeed(program, &["--version"], dir);
    assert_eq!(version.trim(), format!("onebudgetspec {VERSION}"));
    let case = case(dir);
    let installed = check(program, &case);
    let direct = check(&direct_binary(), &case);
    assert_eq!(installed.0, 1, "one budget is over, so the check exits 1");
    assert_eq!(
        installed, direct,
        "the installed entry point and the binary disagree"
    );
    let verdicts: Vec<&str> = installed.1["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["verdict"].as_str().unwrap())
        .collect();
    assert_eq!(verdicts, ["within", "over"]);
}
