//! The argument grammar.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Register measurable budgets in budgets.yaml files and gate on them.
///
/// Exit status: 0 when every selected budget is within; 1 when at least one is over and
/// none errored; 3 when at least one measurement errored; 2 when the invocation or a
/// budgets file is invalid, with nothing run.
#[derive(Debug, Parser)]
#[command(name = "onebudgetspec", version, about, long_about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// How to print the result.
    #[arg(long, global = true, value_enum, default_value_t = Output::Text)]
    output: Output,

    /// Shorthand for `--output json`.
    #[arg(long, global = true, conflicts_with = "output")]
    json: bool,
}

impl Cli {
    /// The output format the invocation asked for.
    pub fn output(&self) -> Output {
        if self.json { Output::Json } else { self.output }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Output {
    /// One line per result, for a person.
    Text,
    /// One JSON document on stdout, for a program.
    Json,
    /// Markdown: a saved comparison's report, or `check --subjects`'s one section per subject.
    Markdown,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Measure every selected budget once and report actual, budget and headroom.
    Check(CheckArgs),
    /// Check the files' shape and id uniqueness; runs no command.
    Validate(Files),
    /// Report the selected budgets; runs no command.
    List(SelectedFiles),
    /// Print the JSON Schema bundle: budgets-file, check-report and list-report.
    Schema,
    /// Reproduce a comparison from saved observations.
    Report(ReportArgs),
}

#[derive(Debug, Args)]
pub struct Files {
    /// Budgets files, or directories with --recursive. Defaults to ./budgets.yaml.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Search directories for files named budgets.yaml, honouring .gitignore.
    #[arg(long)]
    pub recursive: bool,
}

#[derive(Debug, Args)]
pub struct SelectedFiles {
    #[command(flatten)]
    pub files: Files,

    #[command(flatten)]
    pub select: SelectArgs,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[command(flatten)]
    pub selected: SelectedFiles,

    /// Also write the JSON report `--output json` prints to this file, whatever the verdict.
    /// Missing parent directories are created.
    #[arg(long, value_name = "FILE", conflicts_with = "subjects")]
    pub save_results: Option<PathBuf>,

    /// Check the selection once per subject this JSON document names (`-` reads it from
    /// stdin), each in its own directory and environment, and report each apart. Each
    /// subject names its own budgets files, so no PATH is given.
    #[arg(long, value_name = "FILE|-", conflicts_with = "paths")]
    pub subjects: Option<PathBuf>,

    /// With --subjects, keep only the results whose verdict is `over`.
    #[arg(long)]
    pub over_only: bool,
}

// llmlint: ignore-block[invalid_states_unrepresentable] the contract gives these their meaning at selection, not parsing: an id matching no budget is refused by Budgets::select with status 2, and a label matching none leaves an empty selection that exits 0, which a validating type here would turn into a refusal.
#[derive(Debug, Args)]
pub struct SelectArgs {
    /// Keep only this budget (repeatable). An id no file registers is an error.
    #[arg(long = "id", value_name = "ID")]
    pub ids: Vec<String>,

    /// Keep budgets carrying at least one of these labels (repeatable).
    #[arg(long = "label", value_name = "LABEL")]
    pub labels: Vec<String>,

    /// Drop budgets carrying any of these labels (repeatable).
    #[arg(long = "exclude-label", value_name = "LABEL")]
    pub exclude_labels: Vec<String>,
}
// llmlint: ignore-end[invalid_states_unrepresentable]

/// Saved comparison and optional external delivery.
#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Saved `ComparisonInput` JSON.
    #[arg(long)]
    pub input: PathBuf,
    /// External JSON reporter executable.
    #[arg(long)]
    pub reporter: Option<PathBuf>,
    /// Reporter destination.
    #[arg(long, default_value = "")]
    pub destination: String,
    /// Standing report key.
    #[arg(long, default_value = "")]
    pub key: String,
}
