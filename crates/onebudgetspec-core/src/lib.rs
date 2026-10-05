//! Register measurable budgets in `budgets.yaml` files and gate on them.
//!
//! This crate holds the behaviour; the `onebudgetspec` binary parses arguments, calls it
//! and renders what it returns.
//!
//! ```no_run
//! use std::path::PathBuf;
//!
//! use onebudgetspec_core::{Selection, load};
//!
//! let budgets = load(&[PathBuf::from("budgets.yaml")], false)?;
//! let selected = budgets.select(&Selection {
//!     labels: vec!["nightly".into()],
//!     ..Selection::default()
//! })?;
//! let report = selected.check();
//! std::process::exit(report.exit_code());
//! # Ok::<(), onebudgetspec_core::Error>(())
//! ```
//!
//! Every selected budget is measured exactly once, in file order, one at a time: there is
//! no retry, repeat, sampling or baseline. Over budget is a result, not something to
//! re-measure until it passes.

mod error;
mod host;
mod load;
mod measure;
pub mod model;
pub mod report;
mod schema;
mod select;

pub use error::Error;
pub use load::{Budgets, Discovered, LoadedFile, discover, load, load_discovered};
pub use measure::{UNKNOWN, judge};
pub use model::{Budget, BudgetsFile, Condition, Direction, Measure};
pub use report::{CheckReport, CheckResult, Host, ListReport, ListedBudget, Verdict, exit};
pub use schema::{SCHEMA_BUNDLE_VERSION, schema_bundle};
pub use select::{Selected, SelectedBudgets, Selection};
