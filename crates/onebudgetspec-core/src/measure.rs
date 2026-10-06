//! Measuring the selected budgets: each once, in file order, one at a time.
//!
//! Every command runs from the directory holding its budgets file, with the environment
//! inherited, no shell, and its stdout and stderr copied to this process's stderr so they
//! never mix with a report on stdout. A command that fails, or whose result cannot be
//! read, has the tail of its stderr kept in the reason recorded for it.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
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
    ///
    /// On Windows this process's standard handles are first made uninheritable, so that a
    /// process a command leaves running cannot hold this process's own output open; a child
    /// this process starts later is still handed them, as `std::process` and the like
    /// hand a child its streams, explicitly.
    #[must_use]
    pub fn check(&self) -> CheckReport {
        #[cfg(windows)]
        keep_standard_handles();
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

/// Make this process's standard handles uninheritable. Windows hands a new process every
/// inheritable handle of its parent's, not only the streams it is given, and a command
/// passes them on in turn to whatever it starts: a process left running would otherwise
/// hold this process's stdout and stderr open after it exits, and whoever reads them to
/// their end would wait for it.
#[cfg(windows)]
fn keep_standard_handles() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    for handle in [
        io::stdin().as_raw_handle(),
        io::stdout().as_raw_handle(),
        io::stderr().as_raw_handle(),
    ] {
        // SAFETY: the handle is this process's own standard handle, or null when it has
        // none, which the call refuses without touching anything. A refusal leaves the
        // handle as it was: inheritable only if it already was.
        unsafe {
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
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
    let mut child = command(dir, argv)
        .spawn()
        .map_err(|error| format!("cannot run {}: {error}", argv[0]))?;
    // The value is the command's stdout, read here rather than passed through.
    let value_pipe = child.stdout.take();
    let tee = StderrTee::start(&mut child, &Tree::Alone, &argv[0])?;
    let mut value = Vec::new();
    if let Some(mut pipe) = value_pipe {
        // llmlint: ignore[changed_behavior_has_e2e] reading a pipe this process holds fails only when the OS fails the read, which no journey can bring about; the command is reaped and the OS's reason recorded.
        if let Err(error) = pipe.read_to_end(&mut value) {
            Tree::Alone.kill(&mut child);
            return Err(format!("cannot read the stdout of {}: {error}", argv[0]));
        }
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for {}: {error}", argv[0]))?;
    let stderr = tee.finish();
    // Like every command's output, the value reaches this process's stderr too.
    let _ = io::stderr().write_all(&value);
    if !status.success() {
        return Err(with_stderr(describe_exit(&argv[0], status), &stderr));
    }
    Ok(String::from_utf8_lossy(&value).trim().to_owned())
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
///
/// The inputs are the contract's: a threshold [`load`](crate::load) has held finite and
/// non-negative, and a finite measured value. Other values give their IEEE 754 results.
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
    let finished = run(&mut command, &argv[0], timeout)?;
    if !finished.status.success() {
        return Err(finished.failure(&argv[0], None));
    }
    Ok(Measured {
        value: finished.elapsed.as_secs_f64(),
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
    let finished = run(&mut command, &argv[0], timeout)?;
    if !finished.status.success() {
        return Err(finished.failure(&argv[0], None));
    }
    let text = std::fs::read_to_string(result_file.path()).map_err(|error| {
        finished.failure(
            &argv[0],
            Some(format!(
                "cannot read the result file {RESULT_ENV} names: {error}"
            )),
        )
    })?;
    parse_result(&text, declared).map_err(|reason| finished.failure(&argv[0], Some(reason)))
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
///
/// Its stdout and stderr are pipes of this process's, never this process's own streams:
/// a process the command leaves running then holds only those pipes, which are not waited
/// for past [`STDERR_DRAIN`], rather than holding this process's output open after it
/// exits for whoever reads it to its end.
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// A command that ran to completion.
struct Finished {
    status: ExitStatus,
    elapsed: Duration,
    /// The bounded tail of its stderr, as [`StderrTee::finish`] returns it.
    stderr: String,
}

impl Finished {
    /// Why the measurement failed: `reason` when what it produced could not be read, then
    /// how the command exited and the tail of its stderr.
    fn failure(&self, program: &str, reason: Option<String>) -> String {
        let exit = describe_exit(program, self.status);
        let reason = reason.map_or_else(|| exit.clone(), |reason| format!("{reason}; {exit}"));
        with_stderr(reason, &self.stderr)
    }
}

/// Run `command` to completion, killing it once `timeout` passes, with its output passed
/// through a [`StderrTee`].
fn run(
    command: &mut Command,
    program: &str,
    timeout: Option<Duration>,
) -> Result<Finished, String> {
    // A budget with a timeout runs its command in a tree of its own, so the timeout ends
    // everything the command started rather than only the command itself.
    let processes = if timeout.is_some() {
        Tree::prepare(command).map_err(|error| format!("cannot run {program}: {error}"))?
    } else {
        Tree::Alone
    };
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    if let Err(error) = processes.adopt(&child) {
        // llmlint: ignore[changed_behavior_has_e2e] adopting fails only when Windows refuses to place a process it just created in a job, which no journey can bring about; the command is ended and the OS's reason recorded.
        processes.kill(&mut child);
        return Err(format!("cannot run {program} under its timeout: {error}"));
    }
    let tee = StderrTee::start(&mut child, &processes, program)?;
    let Some(timeout) = timeout else {
        let status = child
            .wait()
            .map_err(|error| format!("cannot wait for {program}: {error}"))?;
        let elapsed = started.elapsed();
        return Ok(Finished {
            status,
            elapsed,
            stderr: tee.finish(),
        });
    };

    let mut pause = Duration::from_millis(1);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("cannot wait for {program}: {error}"))?
        {
            let elapsed = started.elapsed();
            return Ok(Finished {
                status,
                elapsed,
                stderr: tee.finish(),
            });
        }
        let waited = started.elapsed();
        if waited >= timeout {
            processes.kill(&mut child);
            let reason = format!(
                "{program} timed out after {} seconds and was killed",
                timeout.as_secs()
            );
            return Err(with_stderr(reason, &tee.finish()));
        }
        std::thread::sleep(pause.min(timeout.saturating_sub(waited)));
        pause = (pause * 2).min(Duration::from_millis(50));
    }
}

/// Where a command runs, and so what ending it ends.
enum Tree {
    /// On its own: ending it ends the command alone.
    Alone,
    /// In a process group of its own, which ending it signals as a whole.
    #[cfg(unix)]
    Group,
    /// In a Job Object of its own, which ending it terminates as a whole: every process
    /// the command starts joins its job.
    #[cfg(windows)]
    Job(job::Job),
}

impl Tree {
    /// Set `command` up to run in a tree of its own.
    #[cfg(unix)]
    #[allow(clippy::unnecessary_wraps)] // Windows' preparation can fail; this one cannot.
    fn prepare(command: &mut Command) -> io::Result<Self> {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        Ok(Self::Group)
    }

    /// Set `command` up to run in a tree of its own: it starts suspended, so it starts
    /// nothing before [`Tree::adopt`] has placed it in the job.
    #[cfg(windows)]
    fn prepare(command: &mut Command) -> io::Result<Self> {
        use std::os::windows::process::CommandExt;
        let job = job::Job::new()?;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
        Ok(Self::Job(job))
    }

    /// Place `child`, just spawned from the prepared command, in this tree and let it run.
    #[allow(clippy::unused_self, clippy::unnecessary_wraps)] // Only Windows has a step here.
    fn adopt(&self, child: &Child) -> io::Result<()> {
        #[cfg(windows)]
        if let Self::Job(job) = self {
            return job.adopt(child);
        }
        let _ = child;
        Ok(())
    }

    /// End `child` and everything in its tree, and reap it.
    fn kill(&self, child: &mut Child) {
        match self {
            Self::Alone => {}
            #[cfg(unix)]
            Self::Group => {
                if let Ok(group) = i32::try_from(child.id()) {
                    // SAFETY: kill(2) with a negative pid signals the process group the
                    // child leads, which `prepare` created for it; it touches no memory of
                    // this process.
                    unsafe {
                        libc::kill(-group, libc::SIGKILL);
                    }
                }
            }
            #[cfg(windows)]
            Self::Job(job) => job.terminate(),
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// The Job Object a budget with a timeout runs its command in on Windows, where there are
/// no process groups to signal.
#[cfg(windows)]
mod job {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    /// The exit code every process in a terminated job ends with.
    const TERMINATED: u32 = 1;

    /// An unnamed job, closed when dropped. Closing it ends nothing: like a process group,
    /// what a command left running when it finished keeps running.
    pub(super) struct Job(HANDLE);

    impl Job {
        pub(super) fn new() -> io::Result<Self> {
            // SAFETY: both arguments may be null: default security, no name. The handle
            // returned is owned by the `Job` and closed once, on drop.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(handle))
        }

        /// Place `child`, spawned suspended, in this job, then resume it: anything it
        /// starts from then on is in the job too.
        pub(super) fn adopt(&self, child: &Child) -> io::Result<()> {
            // SAFETY: both handles are open for the call: the job's by `self`, the
            // process's by `child`.
            if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            resume(child.id())
        }

        pub(super) fn terminate(&self) {
            // SAFETY: the job's handle is open for the call. A failure leaves the command
            // to `Child::kill`, which follows.
            unsafe {
                TerminateJobObject(self.0, TERMINATED);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned by this `Job`, and closed only here.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// Resume every thread of process `pid`: the one a suspended process starts with.
    fn resume(pid: u32) -> io::Result<()> {
        // SAFETY: a snapshot of every thread on the host; the handle is closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry = THREADENTRY32 {
            dwSize: u32::try_from(size_of::<THREADENTRY32>()).unwrap_or(u32::MAX),
            cntUsage: 0,
            th32ThreadID: 0,
            th32OwnerProcessID: 0,
            tpBasePri: 0,
            tpDeltaPri: 0,
            dwFlags: 0,
        };
        let mut resumed = 0;
        let mut failure = None;
        // SAFETY: `entry` is a THREADENTRY32 whose dwSize is its own size, as both calls
        // require, and the snapshot handle is open.
        let mut more = unsafe { Thread32First(snapshot, &raw mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: OpenThread takes a thread id and returns an owned handle or null;
                // a non-null one is resumed and then closed.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() || unsafe { ResumeThread(thread) } == u32::MAX {
                    failure = Some(io::Error::last_os_error());
                } else {
                    resumed += 1;
                }
                if !thread.is_null() {
                    // SAFETY: the thread handle was opened above and is closed once.
                    unsafe {
                        CloseHandle(thread);
                    }
                }
            }
            // SAFETY: as for Thread32First.
            more = unsafe { Thread32Next(snapshot, &raw mut entry) } != 0;
        }
        // SAFETY: the snapshot handle was opened above and is closed once.
        unsafe {
            CloseHandle(snapshot);
        }
        match (resumed, failure) {
            (0, Some(error)) => Err(error),
            (0, None) => Err(io::Error::other(format!(
                "process {pid} has no thread to resume"
            ))),
            _ => Ok(()),
        }
    }
}

/// How many characters of a command's stderr a failure's reason keeps, from its end.
const STDERR_TAIL_CHARS: usize = 1000;

/// The raw bytes kept to make that tail: enough for every character to take four.
const STDERR_TAIL_BYTES: usize = STDERR_TAIL_CHARS * 4;

/// How long a command's stdout and stderr are still read once the command has exited. A
/// pipe closes as soon as nothing holds it open; a process the command left running may
/// hold it for ever, and is not waited for.
const STDERR_DRAIN: Duration = Duration::from_millis(250);

/// A command's stderr, written through to this process's stderr as it arrives, with its
/// tail kept for the reason a failure records; and its stdout, when piped and not taken
/// first, written through the same way.
struct StderrTee {
    tail: Arc<Mutex<StderrTail>>,
    closed: mpsc::Receiver<()>,
    /// How many pipes are being read: each sends on `closed` once it closes.
    reading: usize,
}

impl StderrTee {
    /// Start reading `child`'s piped stdout and stderr. When no thread can be started to
    /// read one, `child` is killed, since nothing would drain the pipe, and the reason is
    /// returned.
    fn start(child: &mut Child, tree: &Tree, program: &str) -> Result<Self, String> {
        let tail = Arc::new(Mutex::new(StderrTail::default()));
        let (sender, closed) = mpsc::channel();
        let mut reading = 0;
        let started = [
            child
                .stdout
                .take()
                .map(|pipe| ("stdout", read_through(pipe, None, &sender))),
            child
                .stderr
                .take()
                .map(|pipe| ("stderr", read_through(pipe, Some(&tail), &sender))),
        ];
        for (stream, reader) in started.into_iter().flatten() {
            // llmlint: ignore[changed_behavior_has_e2e] a thread fails to start only when the OS is out of threads or memory, which no journey can bring about without destabilising the run around it; this path kills the command and records the OS's reason.
            if let Err(error) = reader {
                tree.kill(child);
                return Err(format!("cannot read the {stream} of {program}: {error}"));
            }
            reading += 1;
        }
        Ok(Self {
            tail,
            closed,
            reading,
        })
    }

    /// The tail of what the command wrote to stderr, once it has exited: decoded lossily,
    /// each line trimmed, blank lines dropped and the rest joined by ` | `, with any other
    /// control character made a space. Past [`STDERR_TAIL_CHARS`] characters only the last
    /// that many are kept, after a leading `…`. Empty when it wrote nothing but whitespace.
    fn finish(self) -> String {
        let deadline = Instant::now() + STDERR_DRAIN;
        for _ in 0..self.reading {
            let left = deadline.saturating_duration_since(Instant::now());
            if self.closed.recv_timeout(left).is_err() {
                break;
            }
        }
        self.tail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .text()
    }
}

/// Copy `pipe` to this process's stderr, and into `tail` when given, on a thread of its own
/// that sends on `closed` once the pipe closes.
fn read_through(
    mut pipe: impl Read + Send + 'static,
    tail: Option<&Arc<Mutex<StderrTail>>>,
    closed: &mpsc::Sender<()>,
) -> io::Result<()> {
    let mut through = StderrThrough(tail.map(Arc::clone));
    let closed = closed.clone();
    std::thread::Builder::new()
        .spawn(move || {
            // A pipe that cannot be read further ends the reading like its close does.
            let _ = io::copy(&mut pipe, &mut through);
            let _ = closed.send(());
        })
        .map(drop)
}

/// Where a [`StderrTee`] copies a pipe to: this process's stderr, and for the command's
/// stderr the tail.
struct StderrThrough(Option<Arc<Mutex<StderrTail>>>);

impl Write for StderrThrough {
    fn write(&mut self, chunk: &[u8]) -> io::Result<usize> {
        // This process's stderr failing must not stop the tail being kept.
        let _ = io::stderr().write_all(chunk);
        if let Some(tail) = &self.0 {
            tail.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(chunk);
        }
        Ok(chunk.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct StderrTail {
    bytes: VecDeque<u8>,
    /// Whether earlier bytes were dropped to keep `bytes` within [`STDERR_TAIL_BYTES`].
    dropped: bool,
}

impl StderrTail {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend(chunk);
        let excess = self.bytes.len().saturating_sub(STDERR_TAIL_BYTES);
        if excess > 0 {
            self.bytes.drain(..excess);
            self.dropped = true;
        }
    }

    fn text(&self) -> String {
        let mut bytes: Vec<u8> = self.bytes.iter().copied().collect();
        if self.dropped {
            // The cut may have split a character; its continuation bytes are not text.
            let split = bytes
                .iter()
                .take_while(|byte| (0x80..0xC0).contains(*byte))
                .count();
            bytes.drain(..split);
        }
        let decoded = String::from_utf8_lossy(&bytes);
        let joined: String = decoded
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect();
        let count = joined.chars().count();
        if count == 0 || (count <= STDERR_TAIL_CHARS && !self.dropped) {
            return joined;
        }
        let kept: String = joined
            .chars()
            .skip(count.saturating_sub(STDERR_TAIL_CHARS))
            .collect();
        format!("…{}", kept.trim_start())
    }
}

/// `reason`, followed by the tail of the command's stderr when it wrote any.
fn with_stderr(reason: String, stderr: &str) -> String {
    if stderr.is_empty() {
        reason
    } else {
        format!("{reason}; its stderr: {stderr}")
    }
}

fn describe_exit(program: &str, status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("{program} exited with status {}", exit_code(code));
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
    // Windows gives every process that has exited an exit code.
    #[cfg(not(unix))]
    format!("{program} exited without an exit code")
}

/// An exit code as its platform writes it. Only Windows has negative ones: a code with the
/// top bit set is an NTSTATUS, such as `0xC0000005` for an access violation, and reads in
/// hexadecimal. Every other code reads in decimal.
fn exit_code(code: i32) -> String {
    if code < 0 {
        format!("0x{:08X}", code.cast_unsigned())
    } else {
        code.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{exit_code, judge, parse_result};
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
    fn a_windows_failure_code_reads_in_hexadecimal_and_any_other_in_decimal() {
        assert_eq!(exit_code(4), "4");
        assert_eq!(exit_code(255), "255");
        assert_eq!(exit_code(-1_073_741_819), "0xC0000005");
        assert_eq!(exit_code(-1_073_740_791), "0xC0000409");
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
