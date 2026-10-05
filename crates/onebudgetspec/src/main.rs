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
            match output {
                Output::Json => render::json(stdout, &report),
                Output::Text => render::check_text(stdout, &report),
            }
            Ok(report.exit_code())
        }
        Command::Validate(files) => {
            let budgets = load(&files.paths, files.recursive)?;
            match output {
                Output::Json => render::json(stdout, &budgets.all().list_report()),
                Output::Text => {
                    let _ = writeln!(
                        stdout,
                        "valid: {} budget(s) in {} file(s)",
                        budgets.budget_count(),
                        budgets.files().len()
                    );
                }
            }
            Ok(exit::WITHIN)
        }
        Command::List(args) => {
            let budgets = load(&args.files.paths, args.files.recursive)?;
            let report = budgets.select(&selection(&args.select))?.list_report();
            match output {
                Output::Json => render::json(stdout, &report),
                Output::Text => render::list_text(stdout, &report),
            }
            Ok(exit::WITHIN)
        }
        Command::Schema => {
            render::json(stdout, &schema_bundle());
            Ok(exit::WITHIN)
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
