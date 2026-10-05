//! Finding budgets files, reading them, and holding them to the contract.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::error::Error;
use crate::model::{
    Budget, BudgetsFile, CONDITION_NAME_PATTERN, Condition, FILE_NAME, ID_PATTERN, LABEL_PATTERN,
    Measure, RESERVED_CONDITION_NAMES, SCHEMA_VERSION, UNIT_PATTERN,
};

static ID: LazyLock<Regex> = LazyLock::new(|| compile(ID_PATTERN));
static LABEL: LazyLock<Regex> = LazyLock::new(|| compile(LABEL_PATTERN));
static UNIT: LazyLock<Regex> = LazyLock::new(|| compile(UNIT_PATTERN));
static CONDITION_NAME: LazyLock<Regex> = LazyLock::new(|| compile(CONDITION_NAME_PATTERN));

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("the contract's patterns are valid regular expressions")
}

/// Whether `name` matches the condition-name pattern. The reserved host names are refused
/// separately, by each caller.
pub(crate) fn matches_condition_name_pattern(name: &str) -> bool {
    CONDITION_NAME.is_match(name)
}

/// One budgets file, read and validated.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedFile {
    /// Where the file is, as the caller gave it or discovery found it.
    pub path: PathBuf,
    /// The path as discovered or given; what results report as `file`.
    pub display: String,
    /// The directory holding the file, which every command it names runs from.
    pub dir: PathBuf,
    /// What the file declares.
    pub contents: BudgetsFile,
}

/// Every budgets file one invocation names or discovers, validated together: each file
/// on its own, and every id unique across all of them.
#[derive(Debug, Clone, PartialEq)]
pub struct Budgets {
    files: Vec<LoadedFile>,
}

impl Budgets {
    /// The files, in discovery order.
    #[must_use]
    pub fn files(&self) -> &[LoadedFile] {
        &self.files
    }

    /// How many budgets the files register between them.
    #[must_use]
    pub fn budget_count(&self) -> usize {
        self.files
            .iter()
            .map(|file| file.contents.budgets.len())
            .sum()
    }
}

/// A budgets file found by [`discover`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// Where the file is.
    pub path: PathBuf,
    /// The path as discovered or given.
    pub display: String,
}

/// Find the budgets files `paths` name.
///
/// With no paths, `./budgets.yaml`. A path naming a file is that file. A path naming a
/// directory is searched for files named `budgets.yaml` when `recursive` is set, honouring
/// `.gitignore`, in path order, and refused otherwise. A file named twice is read once.
///
/// # Errors
///
/// [`Error::Invocation`] when a path does not exist or names a directory without
/// `recursive`; [`Error::Invalid`] when a directory cannot be searched.
pub fn discover(paths: &[PathBuf], recursive: bool) -> Result<Vec<Discovered>, Error> {
    let defaulted;
    let paths = if paths.is_empty() {
        defaulted = [PathBuf::from(if recursive { "." } else { FILE_NAME })];
        &defaulted[..]
    } else {
        paths
    };

    let mut found = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        let metadata = std::fs::metadata(path).map_err(|_| {
            Error::invocation(
                format!("{}: no such file or directory", path.display()),
                "name an existing budgets.yaml, or a directory together with --recursive",
            )
        })?;
        if metadata.is_dir() {
            if !recursive {
                return Err(Error::invocation(
                    format!("{} is a directory", path.display()),
                    "pass --recursive to search it for budgets.yaml files, or name a file",
                ));
            }
            match search(path) {
                Ok(files) => found.extend(files),
                Err(problem) => problems.push(problem),
            }
        } else {
            found.push(Discovered {
                path: path.clone(),
                display: path.to_string_lossy().into_owned(),
            });
        }
    }
    if !problems.is_empty() {
        return Err(Error::Invalid { problems });
    }

    let mut seen = HashSet::new();
    found
        .retain(|file| seen.insert(std::fs::canonicalize(&file.path).unwrap_or(file.path.clone())));
    Ok(found)
}

fn search(root: &Path) -> Result<Vec<Discovered>, String> {
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .git_global(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    let mut found = Vec::new();
    for entry in walker {
        let entry = entry.map_err(|error| format!("{}: cannot search: {error}", root.display()))?;
        let is_file = entry.file_type().is_some_and(|kind| kind.is_file());
        if is_file && entry.file_name() == FILE_NAME {
            found.push(entry.into_path());
        }
    }
    found.sort();
    Ok(found
        .into_iter()
        .map(|path| {
            let shown = if root == Path::new(".") {
                path.strip_prefix(root)
                    .map_or(path.clone(), Path::to_path_buf)
            } else {
                path.clone()
            };
            Discovered {
                display: shown.to_string_lossy().into_owned(),
                path,
            }
        })
        .collect())
}

/// Discover the files `paths` name, read each, and validate them all.
///
/// # Errors
///
/// [`Error::Invocation`] as for [`discover`]; [`Error::Invalid`] naming the file and the
/// key of every problem found, across every file.
pub fn load(paths: &[PathBuf], recursive: bool) -> Result<Budgets, Error> {
    load_discovered(discover(paths, recursive)?)
}

/// Read and validate files already found. A file given twice, by any spelling of its
/// path, is read once.
///
/// # Errors
///
/// [`Error::Invalid`] naming the file and the key of every problem found, including an id
/// two files share, whatever the files are called.
pub fn load_discovered(mut discovered: Vec<Discovered>) -> Result<Budgets, Error> {
    let mut seen = HashSet::new();
    discovered
        .retain(|file| seen.insert(std::fs::canonicalize(&file.path).unwrap_or(file.path.clone())));
    let mut problems = Vec::new();
    let mut files = Vec::new();
    for found in discovered {
        match read_file(&found) {
            Ok(file) => files.push(file),
            Err(mut file_problems) => problems.append(&mut file_problems),
        }
    }

    // An id is unique across every file one invocation reads, so that `--id` names
    // exactly one budget whichever files a discovery finds.
    // Owners are files by position, not by name: two files may share a displayed path.
    let mut owners: BTreeMap<&str, usize> = BTreeMap::new();
    for (position, file) in files.iter().enumerate() {
        for (index, budget) in file.contents.budgets.iter().enumerate() {
            if let Some(&owner) = owners.get(budget.id.as_str()) {
                if owner != position {
                    problems.push(format!(
                        "{}: budgets[{index}].id: \"{}\" is already registered by {}; ids are unique across every file one discovery finds",
                        file.display, budget.id, files[owner].display
                    ));
                }
            } else {
                owners.insert(&budget.id, position);
            }
        }
    }

    if problems.is_empty() {
        Ok(Budgets { files })
    } else {
        Err(Error::Invalid { problems })
    }
}

fn read_file(found: &Discovered) -> Result<LoadedFile, Vec<String>> {
    let shown = &found.display;
    let text = std::fs::read_to_string(&found.path)
        .map_err(|error| vec![format!("{shown}: cannot read: {error}")])?;
    let contents = parse(shown, &text).map_err(|problem| vec![problem])?;
    let problems = validate(shown, &contents);
    if !problems.is_empty() {
        return Err(problems);
    }
    let dir = match found.path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    // Absolute, so a command's program and working directory resolve the same way however
    // the platform orders `chdir` and program lookup.
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    Ok(LoadedFile {
        path: found.path.clone(),
        display: shown.clone(),
        dir,
        contents,
    })
}

/// Parse a budgets file's text, naming the file and the key on failure.
fn parse(shown: &str, text: &str) -> Result<BudgetsFile, String> {
    let deserializer = serde_norway::Deserializer::from_str(text);
    serde_path_to_error::deserialize(deserializer).map_err(|error| {
        let path = error.path().to_string();
        let inner = error.into_inner();
        if path == "." {
            format!("{shown}: {inner}")
        } else {
            format!("{shown}: {path}: {inner}")
        }
    })
}

/// Every rule the shape alone cannot state, each problem naming the file and the key.
fn validate(shown: &str, file: &BudgetsFile) -> Vec<String> {
    let mut problems = Vec::new();
    let mut problem =
        |key: String, message: String| problems.push(format!("{shown}: {key}: {message}"));

    if file.schema_version != SCHEMA_VERSION {
        problem(
            "schema_version".into(),
            format!(
                "unsupported version {}; this onebudgetspec reads version {SCHEMA_VERSION}",
                file.schema_version
            ),
        );
    }

    let mut condition_names: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, condition) in file.conditions.iter().enumerate() {
        validate_condition(index, condition, &mut condition_names, &mut problem);
    }

    let mut ids: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, budget) in file.budgets.iter().enumerate() {
        validate_budget(index, budget, &mut ids, &mut problem);
    }
    problems
}

fn validate_condition<'a>(
    index: usize,
    condition: &'a Condition,
    names: &mut BTreeMap<&'a str, usize>,
    problem: &mut impl FnMut(String, String),
) {
    let key = format!("conditions[{index}]");
    let name = condition.name.as_str();
    if !CONDITION_NAME.is_match(name) {
        problem(
            format!("{key}.name"),
            format!("\"{name}\" does not match {CONDITION_NAME_PATTERN}"),
        );
    } else if RESERVED_CONDITION_NAMES.contains(&name) {
        problem(
            format!("{key}.name"),
            format!(
                "\"{name}\" is a host value every result records; name the condition something else"
            ),
        );
    }
    if let Some(first) = names.get(name) {
        problem(
            format!("{key}.name"),
            format!("\"{name}\" repeats conditions[{first}].name"),
        );
    } else {
        names.insert(name, index);
    }
    // llmlint: ignore[boundary_inputs_validated] the contract's file rule is a non-empty argv, which its schema states; an empty or NUL-holding element cannot be spawned, and the measurement then reports an error naming the program, which is where the contract puts a command that cannot run.
    if condition.command.is_empty() {
        problem(
            format!("{key}.command"),
            "must name at least the program to run".into(),
        );
    }
}

fn validate_budget<'a>(
    index: usize,
    budget: &'a Budget,
    ids: &mut BTreeMap<&'a str, usize>,
    problem: &mut impl FnMut(String, String),
) {
    let key = format!("budgets[{index}]");
    let id = budget.id.as_str();
    if !ID.is_match(id) {
        problem(
            format!("{key}.id"),
            format!("\"{id}\" does not match {ID_PATTERN}"),
        );
    }
    if let Some(first) = ids.get(id) {
        problem(
            format!("{key}.id"),
            format!("\"{id}\" repeats budgets[{first}].id"),
        );
    } else {
        ids.insert(id, index);
    }

    let mut labels = HashSet::new();
    for (label_index, label) in budget.labels.iter().enumerate() {
        let label_key = format!("{key}.labels[{label_index}]");
        if !LABEL.is_match(label) {
            problem(
                label_key,
                format!("\"{label}\" does not match {LABEL_PATTERN}"),
            );
        } else if !labels.insert(label.as_str()) {
            problem(
                label_key,
                format!("\"{label}\" is repeated within this budget"),
            );
        }
    }

    // llmlint: ignore[boundary_inputs_validated] the contract's file rule is a non-empty argv, which its schema states; an empty or NUL-holding element cannot be spawned, and the measurement then reports an error naming the program, which is where the contract puts a command that cannot run.
    if budget.command.is_empty() {
        problem(
            format!("{key}.command"),
            "must name at least the program to run".into(),
        );
    }
    if !UNIT.is_match(&budget.unit) {
        problem(
            format!("{key}.unit"),
            format!("\"{}\" does not match {UNIT_PATTERN}", budget.unit),
        );
    } else if budget.measure == Measure::Elapsed && budget.unit != "seconds" {
        problem(
            format!("{key}.unit"),
            format!(
                "an elapsed budget measures wall-clock seconds, so its unit must be \"seconds\", not \"{}\"",
                budget.unit
            ),
        );
    }
    if !budget.threshold.is_finite() || budget.threshold < 0.0 {
        problem(
            format!("{key}.threshold"),
            format!(
                "must be a finite, non-negative number, not {}",
                budget.threshold
            ),
        );
    }
    if budget.timeout_seconds == Some(0) {
        problem(
            format!("{key}.timeout_seconds"),
            "must be a positive integer, not 0".into(),
        );
    }
}
