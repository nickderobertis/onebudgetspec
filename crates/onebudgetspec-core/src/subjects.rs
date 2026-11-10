//! Checking one selection once per subject: each subject a directory and an environment of
//! its own, its budgets files read from that directory, and its verdicts reported apart.
//!
//! A subject is whatever a caller measures separately — a change, a commit, a deployment.
//! The subjects document names them; [`check_subjects`] answers with one
//! [`CheckReport`] per subject that could be checked and one [`Unanswered`] entry, with its
//! reason, per subject that could not.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Error;
use crate::load::{Discovered, discover, load_discovered};
use crate::model::{FILE_NAME, SCHEMA_VERSION};
use crate::report::{CheckReport, Verdict, exit};
use crate::select::Selection;

// llmlint: ignore-block[invalid_states_unrepresentable] These structs are the serde shape of an untrusted document and the source of the contract's schema. `Subjects::from_json` holds every field to its rules and names each problem in one refusal; validating newtypes would refuse at deserialization, one problem at a time, and change the emitted schema.
/// The subjects `check --subjects` checks, each once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "subjects")]
pub struct Subjects {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// The subjects, checked and reported in this order.
    pub subjects: Vec<Subject>,
}

/// One subject: where its budgets files are, and the environment its commands run with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    /// The caller's key for the subject, non-empty and unique in the document.
    #[schemars(length(min = 1))]
    pub id: String,
    /// The directory the subject's budgets files are read from and its commands run in; a
    /// relative one is read from the working directory.
    #[schemars(length(min = 1))]
    pub directory: String,
    /// Variables added to the inherited environment of every command the subject runs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// The budgets files, relative to `directory`; `["budgets.yaml"]` when omitted. With
    /// `--recursive`, a directory is searched for files named `budgets.yaml`.
    #[serde(default = "default_files")]
    #[schemars(length(min = 1))]
    pub files: Vec<String>,
    /// Any JSON, echoed back beside the subject's answer; null when omitted.
    #[serde(default)]
    pub context: Value,
}

/// What `check --subjects` answers with: each subject's check report, and every subject
/// that could not be checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "subjects-report")]
pub struct SubjectsReport {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// One entry per checked subject, in document order.
    pub subjects: Vec<SubjectReport>,
    /// One entry per subject that could not be checked, in document order.
    pub unanswered: Vec<Unanswered>,
}

/// One checked subject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubjectReport {
    /// The subject's id.
    pub id: String,
    /// The subject's context, echoed.
    pub context: Value,
    /// The check report `check` prints, over the subject's selected budgets.
    pub report: CheckReport,
}

/// A subject that could not be checked, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Unanswered {
    /// The subject's id.
    pub id: String,
    /// The subject's context, echoed.
    pub context: Value,
    /// Why it could not be checked: its directory or a budgets file could not be read or
    /// is invalid, or the selection keeps none of its budgets.
    pub reason: String,
}
// llmlint: ignore-end[invalid_states_unrepresentable]

fn default_files() -> Vec<String> {
    vec![FILE_NAME.to_owned()]
}

impl Subjects {
    /// Read a subjects document from its JSON text, holding it to its rules.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming every problem: text that is not the document's shape, a
    /// `schema_version` other than `1`, an empty or repeated id, an empty directory or
    /// file list, or an environment variable name that is empty or holds `=` or NUL.
    pub fn from_json(text: &[u8]) -> Result<Self, Error> {
        let mut deserializer = serde_json::Deserializer::from_slice(text);
        let subjects: Self =
            serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
                let path = error.path().to_string();
                let at = if path == "." {
                    String::new()
                } else {
                    format!("{path}: ")
                };
                Error::Invalid {
                    problems: vec![format!("subjects: {at}{}", error.inner())],
                }
            })?;
        // Text after the document is no part of it: refuse it rather than ignore it.
        deserializer.end().map_err(|error| Error::Invalid {
            problems: vec![format!("subjects: {error}")],
        })?;
        let problems = subjects.problems();
        if problems.is_empty() {
            Ok(subjects)
        } else {
            Err(Error::Invalid { problems })
        }
    }

    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema_version != SCHEMA_VERSION {
            problems.push(format!(
                "subjects: schema_version: {} is not supported; use {SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        let mut seen = HashSet::new();
        for (index, subject) in self.subjects.iter().enumerate() {
            let at = format!("subjects: subjects[{index}]");
            if subject.id.is_empty() {
                problems.push(format!("{at}.id: must not be empty"));
            } else if !seen.insert(subject.id.as_str()) {
                problems.push(format!(
                    "{at}.id: \"{}\" is already a subject's id; ids are unique in the document",
                    subject.id
                ));
            }
            if subject.directory.is_empty() {
                problems.push(format!("{at}.directory: must not be empty"));
            }
            if subject.files.is_empty() {
                problems.push(format!(
                    "{at}.files: must name at least one file; omit it for {FILE_NAME}"
                ));
            }
            for (position, file) in subject.files.iter().enumerate() {
                if file.is_empty() {
                    problems.push(format!("{at}.files[{position}]: must not be empty"));
                }
            }
            for name in subject.env.keys() {
                if name.is_empty() || name.contains(['=', '\0']) {
                    problems.push(format!(
                        "{at}.env: {name:?} is not an environment variable name"
                    ));
                }
            }
        }
        problems
    }
}

/// Check `selection` once per subject, in document order: read each subject's budgets
/// files from its directory, apply the selection, and measure what it keeps exactly as
/// [`crate::SelectedBudgets::check`] does, except that every command, conditions included,
/// runs from the subject's directory — whichever file, however nested, names it — with the
/// subject's `env` added to the inherited environment. A program named by a relative path
/// is found from that directory too.
///
/// A subject whose directory or a budgets file cannot be read or is invalid, or whose
/// selection keeps nothing (an id it does not register included), is not checked: it is
/// [`Unanswered`], with the reason, and every other subject is still checked.
///
/// `recursive` lets a subject's `files` name directories, searched as `--recursive` does.
#[must_use]
pub fn check_subjects(
    subjects: &Subjects,
    selection: &Selection,
    recursive: bool,
) -> SubjectsReport {
    let mut report = SubjectsReport {
        schema_version: SCHEMA_VERSION,
        subjects: Vec::new(),
        unanswered: Vec::new(),
    };
    for subject in &subjects.subjects {
        match check_subject(subject, selection, recursive) {
            Ok(checked) => report.subjects.push(SubjectReport {
                id: subject.id.clone(),
                context: subject.context.clone(),
                report: checked,
            }),
            Err(reason) => report.unanswered.push(Unanswered {
                id: subject.id.clone(),
                context: subject.context.clone(),
                reason,
            }),
        }
    }
    report
}

fn check_subject(
    subject: &Subject,
    selection: &Selection,
    recursive: bool,
) -> Result<CheckReport, String> {
    let directory = Path::new(&subject.directory);
    std::fs::read_dir(directory)
        .map_err(|error| format!("directory {} cannot be read: {error}", directory.display()))?;
    let mut found = Vec::new();
    for file in &subject.files {
        let path = directory.join(file);
        let discovered = discover(std::slice::from_ref(&path), recursive)
            .map_err(|error| reason(&error, &subject.directory))?;
        found.extend(discovered.into_iter().map(|entry| Discovered {
            display: shown(directory, &entry.path),
            path: entry.path,
        }));
    }
    let budgets = load_discovered(found).map_err(|error| reason(&error, &subject.directory))?;
    let selected = budgets
        .select(selection)
        .map_err(|error| reason(&error, &subject.directory))?;
    if selected.is_empty() {
        return Err(format!(
            "the selection is empty: it keeps none of the {} budget(s) in {}",
            budgets.budget_count(),
            subject.files.join(", ")
        ));
    }
    // Every command runs from the subject's directory, whichever file names it; absolute,
    // as a budgets file's own directory is, so a command's program and working directory
    // resolve alike on every platform.
    let working = std::path::absolute(directory).unwrap_or_else(|_| directory.to_path_buf());
    Ok(selected.check_in(Some(&working), &subject.env))
}

/// A refusal as an unanswered subject's reason: its message, without the next step a
/// person at a command line would take. A file's problems name it relative to the
/// subject's directory, so that directory is named too.
fn reason(error: &Error, directory: &str) -> String {
    match error {
        Error::Invocation { message, .. } => message.clone(),
        Error::Invalid { problems } => {
            format!("{} (in directory {directory})", problems.join("; "))
        }
    }
}

/// A file's path as results report it: relative to the subject's directory, with `/`
/// between components on every platform.
fn shown(directory: &Path, path: &Path) -> String {
    let relative: PathBuf = path.strip_prefix(directory).unwrap_or(path).to_path_buf();
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

impl SubjectsReport {
    /// The exit status this answer earns: `3` when any subject's report holds an error,
    /// else `1` when any holds an over-budget result, else `0`. An unanswered subject
    /// holds no result, so it moves none of them.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        let codes = || {
            self.subjects
                .iter()
                .map(|subject| subject.report.exit_code())
        };
        if codes().any(|code| code == exit::ERROR) {
            exit::ERROR
        } else if codes().any(|code| code == exit::OVER) {
            exit::OVER
        } else {
            exit::WITHIN
        }
    }

    /// Keep only the results whose verdict is `over`. Every subject stays, with an empty
    /// report when none of its results is over; the unanswered subjects are unchanged.
    pub fn retain_over(&mut self) {
        for subject in &mut self.subjects {
            subject
                .report
                .results
                .retain(|result| result.verdict == Verdict::Over);
        }
    }
}
