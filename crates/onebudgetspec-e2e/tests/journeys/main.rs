//! The journeys: the built `onebudgetspec` binary driven as a subprocess over temporary
//! directories holding real `budgets.yaml` files and real commands. AGENTS.md lists what
//! each journey file proves, and a test there fails when that list and these files part.

mod common;

mod budget_id;
mod command_environment;
mod command_stderr;
mod conditions;
mod detail;
mod discovery;
mod elapsed;
mod errors;
mod files;
mod host;
mod invalid_files;
mod json_flag;
mod list;
mod measurement_order;
mod output_streams;
mod returned_conditions;
mod rust_report;
mod selection;
mod text_output;
mod validate;
mod verdicts;
