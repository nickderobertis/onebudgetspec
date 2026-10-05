//! The JSON Schema bundle `onebudgetspec schema` prints.
//!
//! It is emitted from the types in [`crate::model`] and [`crate::report`] rather than
//! committed, so a consumer generating against it can never drift from what this library
//! reads and writes: they are the same types.

use schemars::schema_for;
use serde_json::{Value, json};

use crate::model::BudgetsFile;
use crate::report::{CheckReport, ListReport};

/// The bundle's own version, bumped whenever any root's schema changes — added, removed,
/// renamed or altered inside. The golden digest in `tests/schema.rs` fails until it moves.
pub const SCHEMA_BUNDLE_VERSION: u32 = 1;

/// The bundle: `{"version": ..., "roots": {"budgets-file", "check-report", "list-report"}}`,
/// each root a self-contained JSON Schema (draft 2020-12).
#[must_use]
pub fn schema_bundle() -> Value {
    json!({
        "version": SCHEMA_BUNDLE_VERSION,
        "roots": {
            "budgets-file": schema_for!(BudgetsFile),
            "check-report": schema_for!(CheckReport),
            "list-report": schema_for!(ListReport),
        },
    })
}
