//! What every journey shares: a temporary directory holding a real `budgets.yaml` and
//! real commands, the built binary run over it as a subprocess, and the schema every
//! report it prints must validate against. Nothing is doubled.
//!
//! Measuring commands run Node.js, as `node` from `PATH`, or a built program, never a shell
//! or a Unix utility, so the same journey runs on Linux, macOS and Windows.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use serde_json::Value;

/// Held while a file is written and while a process is spawned, so the two never overlap.
/// A process forked while another test thread has a program open for writing inherits that
/// descriptor until it execs, and running the program meanwhile fails with "text file busy".
fn fork_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Spawn `command`, with whatever streams it was given, under the fork lock.
pub fn spawn(command: &mut Command) -> Child {
    let _forking = fork_lock();
    command.spawn().expect("onebudgetspec runs")
}

/// Run `command` to completion with its output captured, spawned under the fork lock.
pub fn output(command: &mut Command) -> Output {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn(command)
        .wait_with_output()
        .expect("onebudgetspec's output is read")
}

/// The built `onebudgetspec` binary: `ONEBUDGETSPEC_BIN` when set, else the one cargo
/// built beside this test executable.
pub fn binary() -> PathBuf {
    if let Some(path) = std::env::var_os("ONEBUDGETSPEC_BIN") {
        // Resolved now, because the journeys run it from other working directories.
        return fs::canonicalize(&path).unwrap_or_else(|error| {
            panic!(
                "ONEBUDGETSPEC_BIN={} is not a file ({error})",
                path.display()
            )
        });
    }
    let exe = std::env::current_exe().expect("the test executable has a path");
    let mut dir = exe.parent().expect("the test executable has a directory");
    if dir.ends_with("deps") {
        dir = dir.parent().expect("deps has a parent");
    }
    let binary = dir.join(format!("onebudgetspec{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary.is_file(),
        "{} is missing; build it first with `cargo build -p onebudgetspec` (Nx's onebudgetspec-e2e:test depends on that build)",
        binary.display()
    );
    binary
}

/// One run of the binary.
pub struct Run {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    fn from(output: Output) -> Self {
        Self {
            status: output
                .status
                .code()
                .expect("onebudgetspec exits with a status"),
            stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            // Commands' own output reaches stderr byte for byte, and it may not be UTF-8.
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Stdout as exactly one JSON document.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|error| {
            panic!(
                "stdout is not one JSON document ({error}):\n{}\nstderr:\n{}",
                self.stdout, self.stderr
            )
        })
    }

    /// Stdout as a check report, validated against the `check-report` root.
    pub fn check_report(&self) -> Value {
        let report = self.json();
        validate("check-report", &report);
        report
    }

    /// Stdout as a list report, validated against the `list-report` root.
    pub fn list_report(&self) -> Value {
        let report = self.json();
        validate("list-report", &report);
        report
    }

    /// Assert the exit status, showing both streams when it differs.
    pub fn expect_status(&self, expected: i32) -> &Self {
        assert_eq!(
            self.status, expected,
            "exit status\nstdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self
    }
}

/// The schema bundle the binary emits, read once.
pub fn schema_bundle() -> &'static Value {
    static BUNDLE: OnceLock<Value> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let output = output(Command::new(binary()).arg("schema"));
        assert!(output.status.success(), "onebudgetspec schema failed");
        serde_json::from_slice(&output.stdout).expect("the schema bundle is JSON")
    })
}

/// Assert `instance` validates against the bundle's `root`.
pub fn validate(root: &str, instance: &Value) {
    let schema = &schema_bundle()["roots"][root];
    assert!(schema.is_object(), "the schema bundle has no root {root}");
    let validator = jsonschema::validator_for(schema).expect("the root is a valid JSON Schema");
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "{root} does not validate:\n{}\n{instance:#}",
        errors.join("\n")
    );
}

/// A temporary directory to lay real files out in and run the binary over.
pub struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("a temporary directory"),
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Write `text` to `relative`, creating its directories, under the fork lock.
    pub fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.path().join(relative);
        let _writing = fork_lock();
        fs::create_dir_all(path.parent().expect("a file has a directory")).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    /// Copy the file at `from` to `relative`, creating its directories, under the fork
    /// lock, since the copy may be run as a program.
    pub fn copy(&self, relative: &str, from: &Path) -> PathBuf {
        let path = self.path().join(relative);
        let _writing = fork_lock();
        fs::create_dir_all(path.parent().expect("a file has a directory")).unwrap();
        fs::copy(from, &path).unwrap();
        path
    }

    /// Write a budgets file from a JSON value, rendered as block YAML.
    pub fn budgets(&self, relative: &str, contents: &Value) -> PathBuf {
        let yaml = serde_norway::to_string(contents).expect("the value renders as YAML");
        self.write(relative, &yaml)
    }

    /// Write a Node.js script, run by a command such as `["node", "<relative>"]`.
    pub fn script(&self, relative: &str, body: &str) -> PathBuf {
        self.write(relative, &format!("\"use strict\";\n{body}\n"))
    }

    /// A Node.js script that records each invocation by appending `label` to `log`, then
    /// runs `body`. Returns its path.
    pub fn counted(&self, relative: &str, log: &str, label: &str, body: &str) -> PathBuf {
        let log = self.path().join(log);
        self.script(
            relative,
            &format!(
                "require(\"fs\").appendFileSync({}, {});\n{body}",
                js(&log.to_string_lossy()),
                js(&format!("{label}\n"))
            ),
        )
    }

    /// The lines a log written by [`Fixture::counted`] holds; empty when nothing ran.
    pub fn log(&self, relative: &str) -> Vec<String> {
        fs::read_to_string(self.path().join(relative))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Run the binary from this directory.
    pub fn run<I, S>(&self, args: I) -> Run
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run_in(self.path(), args, &[])
    }
}

/// Run the binary from `cwd`, with extra environment variables.
pub fn run_in<I, S>(cwd: &Path, args: I, env: &[(&str, &str)]) -> Run
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(binary());
    command.args(args).current_dir(cwd);
    for (name, value) in env {
        command.env(name, value);
    }
    Run::from(output(&mut command))
}

/// `text` as a JavaScript string literal.
pub fn js(text: &str) -> String {
    serde_json::to_string(text).expect("a string renders as JSON")
}

/// The argv running `script` with Node.js, `args` following as `process.argv[1..]`.
pub fn node(script: &str, args: &[&str]) -> Value {
    let mut command = vec![Value::from("node"), Value::from("-e"), Value::from(script)];
    command.extend(args.iter().map(|arg| Value::from(*arg)));
    Value::Array(command)
}

/// JavaScript that writes `result` (a JSON text) to the file `ONEBUDGETSPEC_RESULT` names.
pub fn write_result(result: &str) -> String {
    format!(
        "require(\"fs\").writeFileSync(process.env.ONEBUDGETSPEC_RESULT, {});",
        js(result)
    )
}

/// The argv of a `reported` command that writes `result` (a JSON text) and exits 0.
pub fn reports(result: &str) -> Value {
    node(
        "require(\"fs\").writeFileSync(process.env.ONEBUDGETSPEC_RESULT, process.argv[1]);",
        &[result],
    )
}

/// The argv of a command that writes nothing and exits with `status`.
pub fn exits(status: i32) -> Value {
    node(&format!("process.exitCode = {status};"), &[])
}

/// A `reported` budget writing `value` under `direction` against `threshold`.
pub fn reported(id: &str, value: f64, direction: &str, threshold: f64) -> Value {
    serde_json::json!({
        "id": id,
        "measure": "reported",
        "command": reports(&format!("{{\"value\": {value}}}")),
        "unit": "requests",
        "direction": direction,
        "threshold": threshold,
    })
}

/// A budgets file holding `budgets`.
pub fn file(budgets: &[Value]) -> Value {
    serde_json::json!({ "schema_version": 1, "budgets": budgets })
}

/// The results of a check report.
pub fn results(report: &Value) -> &Vec<Value> {
    report["results"]
        .as_array()
        .expect("a check report has results")
}

/// The one result with `id`.
pub fn result<'a>(report: &'a Value, id: &str) -> &'a Value {
    results(report)
        .iter()
        .find(|result| result["id"] == id)
        .unwrap_or_else(|| panic!("no result for {id} in {report:#}"))
}

/// The ids of a report's results, in order.
pub fn ids(report: &Value, key: &str) -> Vec<String> {
    report[key]
        .as_array()
        .expect("a report holds a list")
        .iter()
        .map(|item| item["id"].as_str().expect("an id").to_owned())
        .collect()
}
