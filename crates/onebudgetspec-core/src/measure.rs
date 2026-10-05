//! Measuring the selected budgets: each once, in file order, one at a time.
//!
//! Every command runs from the directory holding its budgets file, with the environment
//! inherited, no shell, and its stdout and stderr sent to this process's stderr so they
//! never mix with a report on stdout.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::Value;

use crate::host;
use crate::load::{LoadedFile, matches_condition_name_pattern};
use crate::model::{
    CONDITION_NAME_PATTERN, Direction, Measure, RESERVED_CONDITION_NAMES, RESULT_ENV,
    SCHEMA_VERSION,
};
use crate::report::{CheckReport, CheckResult, Host, Verdict};
use crate::select::{Selected, SelectedBudgets};

/// What a condition command that could not be read records.
pub const UNKNOWN: &str = "unknown";

impl SelectedBudgets<'_> {
    /// Measure every selected budget exactly once, in file order, one at a time, and
    /// report each.
    ///
    /// Each file's condition commands run once, just before the first of its budgets is
    /// measured, and their values are recorded beside every result from that file and no
    /// other.
    #[must_use]
    pub fn check(&self) -> CheckReport {
        let mut declared: Vec<(&Path, BTreeMap<String, String>)> = Vec::new();
        let mut results = Vec::with_capacity(self.entries.len());
        for selected in &self.entries {
            let known = declared
                .iter()
                .position(|(path, _)| *path == selected.file.path.as_path());
            let index = known.unwrap_or_else(|| {
                declared.push((&selected.file.path, run_conditions(selected.file)));
                declared.len() - 1
            });
            results.push(measure(*selected, &declared[index].1));
        }
        CheckReport {
            schema_version: SCHEMA_VERSION,
            results,
        }
    }
}

/// Run every condition a file declares, once each, in declaration order.
fn run_conditions(file: &LoadedFile) -> BTreeMap<String, String> {
    file.contents
        .conditions
        .iter()
        .map(|condition| {
            let value = match run_condition(&file.dir, &condition.command) {
                Ok(value) => value,
                Err(reason) => {
                    eprintln!(
                        "onebudgetspec: condition {} in {}: {reason}; recorded as {UNKNOWN}",
                        condition.name, file.display
                    );
                    UNKNOWN.to_owned()
                }
            };
            (condition.name.clone(), value)
        })
        .collect()
}

fn run_condition(dir: &Path, argv: &[String]) -> Result<String, String> {
    let mut command = command(dir, argv);
    command.stdout(Stdio::piped()).stderr(stderr());
    let output = command
        .output()
        .map_err(|error| format!("cannot run {}: {error}", argv[0]))?;
    // The value is the command's stdout, and like every command's output it reaches this
    // process's stderr too.
    let _ = io::stderr().write_all(&output.stdout);
    if !output.status.success() {
        return Err(describe_exit(&argv[0], output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// What a successful measurement found.
struct Measured {
    value: f64,
    detail: Option<String>,
    // llmlint: ignore[invalid_states_unrepresentable] private to this module and built only by returned_conditions, which has just held every name to the pattern and the reserved names; it becomes the report's plain string map.
    returned: BTreeMap<String, String>,
}

fn measure(selected: Selected<'_>, declared: &BTreeMap<String, String>) -> CheckResult {
    let Selected { file, budget } = selected;
    let started_at = Utc::now();
    let sample = host::sample();
    let timeout = budget.timeout_seconds.map(Duration::from_secs);
    let outcome = match budget.measure {
        Measure::Elapsed => measure_elapsed(&file.dir, &budget.command, timeout),
        Measure::Reported => measure_reported(&file.dir, &budget.command, timeout, declared),
    };
    let ended_at = Utc::now().max(started_at);

    let mut conditions = declared.clone();
    let (verdict, actual, headroom, headroom_percent, detail, error) = match outcome {
        Ok(measured) => {
            conditions.extend(measured.returned);
            let (verdict, headroom, percent) =
                judge(budget.direction, budget.threshold, measured.value);
            (
                verdict,
                Some(measured.value),
                Some(headroom),
                percent,
                measured.detail,
                None,
            )
        }
        Err(reason) => (Verdict::Error, None, None, None, None, Some(reason)),
    };

    CheckResult {
        id: budget.id.clone(),
        file: file.display.clone(),
        labels: budget.labels.clone(),
        unit: budget.unit.clone(),
        direction: budget.direction,
        threshold: budget.threshold,
        verdict,
        actual,
        headroom,
        headroom_percent,
        detail,
        error,
        started_at,
        ended_at,
        host: Host {
            load1: sample.load1,
            cpus: sample.cpus,
            mem_available_mib: sample.mem_available_mib,
            conditions,
        },
    }
}

/// The verdict, headroom and headroom percentage of `actual` against a budget.
///
/// Headroom is `threshold - actual` under `max` and `actual - threshold` under `min`, so
/// it is negative exactly when the budget is over; an actual equal to the threshold is
/// within. The percentage is `headroom / threshold * 100`, and `None` at a threshold of 0.
#[must_use]
pub fn judge(direction: Direction, threshold: f64, actual: f64) -> (Verdict, f64, Option<f64>) {
    let headroom = match direction {
        Direction::Max => threshold - actual,
        Direction::Min => actual - threshold,
    };
    let verdict = if headroom >= 0.0 {
        Verdict::Within
    } else {
        Verdict::Over
    };
    let percent = (threshold != 0.0).then(|| headroom / threshold * 100.0);
    (verdict, headroom, percent)
}

fn measure_elapsed(
    dir: &Path,
    argv: &[String],
    timeout: Option<Duration>,
) -> Result<Measured, String> {
    let mut command = command(dir, argv);
    let (status, elapsed) = run(&mut command, &argv[0], timeout)?;
    if !status.success() {
        return Err(describe_exit(&argv[0], status));
    }
    Ok(Measured {
        value: elapsed.as_secs_f64(),
        detail: None,
        returned: BTreeMap::new(),
    })
}

fn measure_reported(
    dir: &Path,
    argv: &[String],
    timeout: Option<Duration>,
    declared: &BTreeMap<String, String>,
) -> Result<Measured, String> {
    let result_file = tempfile::Builder::new()
        .prefix("onebudgetspec-result-")
        .suffix(".json")
        .tempfile()
        .map_err(|error| format!("cannot create the result file: {error}"))?;
    let mut command = command(dir, argv);
    command.env(RESULT_ENV, result_file.path());
    let (status, _) = run(&mut command, &argv[0], timeout)?;
    if !status.success() {
        return Err(describe_exit(&argv[0], status));
    }
    let text = std::fs::read_to_string(result_file.path())
        .map_err(|error| format!("cannot read the result file {RESULT_ENV} names: {error}"))?;
    parse_result(&text, declared)
}

/// Read what a `reported` command wrote to its result file.
fn parse_result(text: &str, declared: &BTreeMap<String, String>) -> Result<Measured, String> {
    const EXPECTED: &str =
        "write one JSON object such as {\"value\": 12.5} to the file ONEBUDGETSPEC_RESULT names";
    if text.trim().is_empty() {
        return Err(format!(
            "the command left its result file empty; {EXPECTED}"
        ));
    }
    let document: Value = serde_json::from_str(text)
        .map_err(|error| format!("the result file is not valid JSON ({error}); {EXPECTED}"))?;
    let Value::Object(fields) = document else {
        return Err(format!(
            "the result file holds {} rather than a JSON object; {EXPECTED}",
            kind(&document)
        ));
    };
    if let Some(unknown) = fields
        .keys()
        .find(|key| !matches!(key.as_str(), "value" | "detail" | "conditions"))
    {
        return Err(format!(
            "the result file has an unknown key \"{unknown}\"; it may hold only value, detail and conditions"
        ));
    }

    let value = match fields.get("value") {
        None => return Err(format!("the result file has no \"value\"; {EXPECTED}")),
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("the result's \"value\" {number} is not a finite number"))?,
        Some(other) => {
            return Err(format!(
                "the result's \"value\" is {} rather than a number",
                kind(other)
            ));
        }
    };

    let detail = match fields.get("detail") {
        None | Some(Value::Null) => None,
        Some(Value::String(detail)) => Some(detail.clone()),
        Some(other) => {
            return Err(format!(
                "the result's \"detail\" is {} rather than a string",
                kind(other)
            ));
        }
    };

    let returned = match fields.get("conditions") {
        None => BTreeMap::new(),
        Some(Value::Object(conditions)) => returned_conditions(conditions, declared)?,
        Some(other) => {
            return Err(format!(
                "the result's \"conditions\" is {} rather than an object of names to strings",
                kind(other)
            ));
        }
    };

    Ok(Measured {
        value,
        detail,
        returned,
    })
}

fn returned_conditions(
    conditions: &serde_json::Map<String, Value>,
    declared: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut returned = BTreeMap::new();
    for (name, value) in conditions {
        if !matches_condition_name_pattern(name) {
            return Err(format!(
                "the result returns a condition named \"{name}\", which does not match {CONDITION_NAME_PATTERN}"
            ));
        }
        // Refused rather than merged: a returned value must never silently replace one
        // this library sampled or the file declared.
        if RESERVED_CONDITION_NAMES.contains(&name.as_str()) {
            return Err(format!(
                "the result returns a condition named \"{name}\", which collides with the host value every result records"
            ));
        }
        if declared.contains_key(name) {
            return Err(format!(
                "the result returns a condition named \"{name}\", which collides with a condition the budgets file declares"
            ));
        }
        let Value::String(value) = value else {
            return Err(format!(
                "the result's condition \"{name}\" is {} rather than a string",
                kind(value)
            ));
        };
        returned.insert(name.clone(), value.clone());
    }
    Ok(returned)
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// The command for `argv`, run from `dir` with no shell. A program named by a relative
/// path with more than one component is resolved against `dir`, the directory holding the
/// budgets file, so it means the same thing wherever `onebudgetspec` is invoked from.
fn command(dir: &Path, argv: &[String]) -> Command {
    let program = Path::new(&argv[0]);
    let program = if program.is_relative() && program.components().count() > 1 {
        dir.join(program)
    } else {
        program.to_path_buf()
    };
    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(stderr())
        .stderr(stderr());
    command
}

fn stderr() -> Stdio {
    Stdio::from(io::stderr())
}

/// Run `command` to completion, killing it once `timeout` passes. Returns its exit status
/// and its wall clock.
fn run(
    command: &mut Command,
    program: &str,
    timeout: Option<Duration>,
) -> Result<(ExitStatus, Duration), String> {
    // A budget with a timeout runs in a process group of its own, so the timeout ends
    // everything the command started rather than only the command itself.
    #[cfg(unix)]
    if timeout.is_some() {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    let Some(timeout) = timeout else {
        let status = child
            .wait()
            .map_err(|error| format!("cannot wait for {program}: {error}"))?;
        return Ok((status, started.elapsed()));
    };

    let mut pause = Duration::from_millis(1);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("cannot wait for {program}: {error}"))?
        {
            return Ok((status, started.elapsed()));
        }
        let waited = started.elapsed();
        if waited >= timeout {
            kill(&mut child);
            return Err(format!(
                "{program} timed out after {} seconds and was killed",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(pause.min(timeout.saturating_sub(waited)));
        pause = (pause * 2).min(Duration::from_millis(50));
    }
}

fn kill(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(group) = i32::try_from(child.id()) {
        // SAFETY: kill(2) with a negative pid signals the process group the child leads,
        // which `run` created for it; it touches no memory of this process.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn describe_exit(program: &str, status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("{program} exited with status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        // A process that has exited without a status was ended by a signal.
        format!(
            "{program} was terminated by signal {}",
            status.signal().unwrap_or_default()
        )
    }
    #[cfg(not(unix))]
    format!("{program} exited abnormally")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{judge, parse_result};
    use crate::model::Direction;
    use crate::report::Verdict;

    #[test]
    fn equal_to_the_threshold_is_within_either_way() {
        assert_eq!(judge(Direction::Max, 10.0, 10.0).0, Verdict::Within);
        assert_eq!(judge(Direction::Min, 10.0, 10.0).0, Verdict::Within);
    }

    #[test]
    fn zero_threshold_has_no_percentage() {
        assert_eq!(judge(Direction::Max, 0.0, 1.0), (Verdict::Over, -1.0, None));
    }

    #[test]
    fn a_returned_null_detail_is_no_detail() {
        let measured = parse_result(r#"{"value": 1, "detail": null}"#, &BTreeMap::new()).unwrap();
        assert_eq!(measured.detail, None);
    }

    #[test]
    fn an_unknown_result_key_or_odd_detail_is_refused() {
        let declared = BTreeMap::new();
        let unknown = parse_result(r#"{"value": 1, "unit": "s"}"#, &declared)
            .err()
            .unwrap();
        assert!(unknown.contains("\"unit\""), "{unknown}");
        let detail = parse_result(r#"{"value": 1, "detail": 3}"#, &declared)
            .err()
            .unwrap();
        assert!(detail.contains("detail"), "{detail}");
        let array = parse_result("[1]", &declared).err().unwrap();
        assert!(array.contains("an array"), "{array}");
    }
}
