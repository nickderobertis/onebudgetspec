//! The `onebudgetspec` command line.
//!
//! It parses arguments, calls `onebudgetspec-core` and renders what it returns. Loading,
//! discovery, selection, measurement and the reports are the library's; nothing here
//! decides any of them.

mod cli;
mod render;

use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use onebudgetspec_core::{Error, Selection, exit, load, schema_bundle};

use crate::cli::{Cli, Command, Output};

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
    let stdout = &mut std::io::stdout().lock();
    match command {
        Command::Check(args) => {
            let budgets = load(&args.files.paths, args.files.recursive)?;
            let selected = budgets.select(&selection(&args.select))?;
            let report = selected.check();
            delivered(match output {
                Output::Json => render::json(stdout, &report),
                Output::Text => render::check_text(stdout, &report),
            });
            Ok(report.exit_code())
        }
        Command::Validate(files) => {
            let budgets = load(&files.paths, files.recursive)?;
            delivered(match output {
                Output::Json => render::json(stdout, &budgets.all().list_report()),
                Output::Text => writeln!(
                    stdout,
                    "valid: {} budget(s) in {} file(s)",
                    budgets.budget_count(),
                    budgets.files().len()
                ),
            });
            Ok(exit::WITHIN)
        }
        Command::List(args) => {
            let budgets = load(&args.files.paths, args.files.recursive)?;
            let report = budgets.select(&selection(&args.select))?.list_report();
            delivered(match output {
                Output::Json => render::json(stdout, &report),
                Output::Text => render::list_text(stdout, &report),
            });
            Ok(exit::WITHIN)
        }
        Command::Schema => {
            delivered(render::json(stdout, &schema_bundle()));
            Ok(exit::WITHIN)
        }
    }
}

/// Say on stderr when the report could not be written to stdout.
///
/// The exit status stays the outcome's: the contract fixes it to 0, 1, 2 or 3 by what was
/// measured, and a consumer reads the verdict from it whether or not stdout was writable.
fn delivered(written: std::io::Result<()>) {
    if let Err(error) = written {
        // llmlint: ignore[cli_output_contract] the exit statuses are a frozen contract keyed to the measured outcome, which other repositories read; a stdout failure is reported here on stderr with its reason and a next step rather than given a status the contract does not define.
        eprintln!(
            "onebudgetspec: cannot write the report to stdout ({error}); re-run with stdout writable, for example redirected to a file"
        );
    }
}

fn selection(args: &cli::SelectArgs) -> Selection {
    Selection {
        ids: args.ids.clone(),
        labels: args.labels.clone(),
        exclude_labels: args.exclude_labels.clone(),
    }
}
