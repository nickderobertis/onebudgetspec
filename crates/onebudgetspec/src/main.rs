//! The `onebudgetspec` command line.
//!
//! It parses arguments, calls `onebudgetspec-core` and renders what it returns. Loading,
//! discovery, selection, measurement and the reports are the library's; nothing here
//! decides any of them.

mod cli;
mod render;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use onebudgetspec_core::{CheckReport, Error, Selection, exit, load, schema_bundle};

use crate::cli::{Cli, Command, Output, SelectedFiles};

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            // clap answers --help and --version through this path too, with exit 0.
            return ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(2));
        }
    };
    let output = cli.output();
    let code = match run(&cli.command, output) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("onebudgetspec: {error}");
            error.exit_code()
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn run(command: &Command, output: Output) -> Result<i32, Error> {
    if output == Output::Markdown && !matches!(command, Command::Report(_)) {
        return Err(Error::Invocation {
            message: "--output markdown is supported only by report".into(),
            next: "use --output text or --output json for this command".into(),
        });
    }
    let stdout = &mut std::io::stdout().lock();
    match command {
        Command::Check(args) => {
            if let Some(path) = &args.save_results {
                refuse_unusable_results_file(path)?;
            }
            let SelectedFiles { files, select } = &args.selected;
            let budgets = load(&files.paths, files.recursive)?;
            let selected = budgets.select(&selection(select))?;
            let report = selected.check();
            let saved = args
                .save_results
                .as_deref()
                .is_none_or(|path| saved(&report, path));
            let written = match output {
                Output::Json => render::json(stdout, &report),
                Output::Text | Output::Markdown => render::check_text(stdout, &report),
            };
            let code = delivered(written, report.exit_code());
            Ok(if saved { code } else { exit::ERROR })
        }
        Command::Validate(files) => {
            let budgets = load(&files.paths, files.recursive)?;
            let written = match output {
                Output::Json => render::json(stdout, &budgets.all().list_report()),
                Output::Text | Output::Markdown => writeln!(
                    stdout,
                    "valid: {} budget(s) in {} file(s)",
                    budgets.budget_count(),
                    budgets.files().len()
                ),
            };
            Ok(delivered(written, exit::WITHIN))
        }
        Command::List(args) => {
            let budgets = load(&args.files.paths, args.files.recursive)?;
            let report = budgets.select(&selection(&args.select))?.list_report();
            let written = match output {
                Output::Json => render::json(stdout, &report),
                Output::Text | Output::Markdown => render::list_text(stdout, &report),
            };
            Ok(delivered(written, exit::WITHIN))
        }
        Command::Report(args) => run_report(stdout, args, output),
        Command::Schema => Ok(delivered(
            render::json(stdout, &schema_bundle()),
            exit::WITHIN,
        )),
    }
}

/// Save the report to `path`, or name the reason on stderr and answer false.
fn saved(report: &CheckReport, path: &Path) -> bool {
    let outcome = report.save(path);
    if let Err(error) = &outcome {
        eprintln!(
            "onebudgetspec: cannot save the results to {}: {error}; re-run with a writable --save-results path",
            path.display()
        );
    }
    outcome.is_ok()
}

/// Refuse a `--save-results` path that could never name a file, before anything is measured.
fn refuse_unusable_results_file(path: &Path) -> Result<(), Error> {
    if path.file_name().is_some() && !path.is_dir() {
        return Ok(());
    }
    Err(Error::Invocation {
        message: format!(
            "--save-results {:?} does not name a file",
            path.display().to_string()
        ),
        next: "pass a file path, for example --save-results results/check.json".into(),
    })
}

/// The exit status once the report is written: `outcome` when it was, and `3` (an error)
/// with the reason on stderr when it could not be, because an answer nobody received must
/// never read as a pass.
fn delivered(written: std::io::Result<()>, outcome: i32) -> i32 {
    match written {
        Ok(()) => outcome,
        Err(error) => {
            eprintln!(
                "onebudgetspec: cannot write the report: {error}; re-run with stdout writable, for example redirected to a file"
            );
            exit::ERROR
        }
    }
}

fn selection(args: &cli::SelectArgs) -> Selection {
    Selection {
        ids: args.ids.clone(),
        labels: args.labels.clone(),
        exclude_labels: args.exclude_labels.clone(),
    }
}

fn run_report(
    stdout: &mut impl Write,
    args: &cli::ReportArgs,
    output: Output,
) -> Result<i32, Error> {
    use onebudgetspec_core::delivery::{
        ExternalReporter, HistoricalFlag, PublishRequest, Reporter,
    };
    let invalid = |message: String| Error::Invocation {
        message,
        next: "provide a valid saved ComparisonInput JSON file".into(),
    };
    let bytes = std::fs::read(&args.input)
        .map_err(|e| invalid(format!("{}: {e}", args.input.display())))?;
    let mut report = onebudgetspec_core::comparison::compare_json(&bytes).map_err(invalid)?;
    let markdown = onebudgetspec_core::render_markdown(&report);
    if let Some(executable) = &args.reporter {
        let request = PublishRequest {
            schema_version: 1,
            destination: args.destination.clone(),
            key: args.key.clone(),
            head_sha: report.current.head_sha.clone(),
            run_id: report.current.run_id.clone(),
            run_url: report.current.run_url.clone(),
            attempt: report.current.attempt,
            markdown,
            flags: report
                .rows
                .iter()
                .filter(|r| r.flag)
                .map(|r| HistoricalFlag {
                    file: r.file.clone(),
                    id: r.id.clone(),
                    sigma_change: r.sigma_change,
                    constant_baseline_change: r.sd == Some(0.0),
                })
                .collect(),
        };
        let delivery = ExternalReporter {
            executable: executable.clone(),
        }
        .publish(&request);
        if let Some(diagnostic) = delivery.diagnostic {
            report.diagnostics.push(diagnostic);
        }
        if !delivery.delivered {
            report
                .diagnostics
                .push("report not delivered; saved comparison remains available offline".into());
        }
    }
    let written = if output == Output::Json {
        render::json(stdout, &report)
    } else {
        write!(stdout, "{}", onebudgetspec_core::render_markdown(&report))
    };
    Ok(delivered(written, report.exit_code()))
}
