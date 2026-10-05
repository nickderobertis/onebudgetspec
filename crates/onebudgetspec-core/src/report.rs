//! The check report and the list report: what `onebudgetspec check` and `list` answer
//! with under `--output json`.
//!
//! Every field is always present, `null` where it has no value, so a consumer never has
//! to tell an absent key from a null one.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::{Direction, Measure};

/// What `check` answers with: one result per selected budget, in measurement order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "check-report")]
pub struct CheckReport {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// One result per selected budget.
    pub results: Vec<CheckResult>,
}

// llmlint: ignore-block[invalid_states_unrepresentable] The flat, nullable fields are the check report's frozen wire shape. measure.rs builds every result in one place from the measurement's Result, so a verdict always arrives with its value or its error, which the verdict and error journeys assert.
/// One budget's measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = every_field_required)]
pub struct CheckResult {
    /// The budget's id.
    pub id: String,
    /// The budgets file's path, as discovered or given.
    pub file: String,
    /// The budget's labels, empty when it has none.
    pub labels: Vec<String>,
    /// The budget's unit.
    pub unit: String,
    /// The budget's direction.
    pub direction: Direction,
    /// The budget's threshold.
    pub threshold: f64,
    /// `within`, `over` or `error`.
    pub verdict: Verdict,
    /// The measured value; null on an error.
    pub actual: Option<f64>,
    /// `threshold - actual` under `max`, `actual - threshold` under `min`; negative when
    /// over, null on an error.
    pub headroom: Option<f64>,
    /// `headroom / threshold * 100`; null on an error or when the threshold is 0.
    pub headroom_percent: Option<f64>,
    /// The `detail` a `reported` command wrote; null when it wrote none.
    pub detail: Option<String>,
    /// Why the measurement failed; null unless `verdict` is `error`.
    pub error: Option<String>,
    /// When the measurement started (RFC 3339).
    pub started_at: DateTime<Utc>,
    /// When the measurement ended (RFC 3339), never before `started_at`.
    pub ended_at: DateTime<Utc>,
    /// The host conditions recorded beside this result.
    pub host: Host,
}

// llmlint: ignore-end[invalid_states_unrepresentable]
/// A result's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The actual value is on the budget's side of the threshold.
    Within,
    /// The actual value is past the threshold.
    Over,
    /// No value was measured; `error` says why.
    Error,
}

impl Verdict {
    /// The spelling the reports use.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Within => "within",
            Self::Over => "over",
            Self::Error => "error",
        }
    }
}

/// The host conditions recorded beside a result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = every_field_required)]
pub struct Host {
    /// The 1-minute load average; null where unreadable.
    pub load1: Option<f64>,
    /// The CPUs available to this process.
    pub cpus: u32,
    /// Available memory in MiB; null where unreadable.
    pub mem_available_mib: Option<u64>,
    /// Each condition the file declares, and for a `reported` result each condition its
    /// command returned, by name.
    pub conditions: BTreeMap<String, String>,
}

/// What `list` answers with: the selected budgets, in file order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "list-report")]
pub struct ListReport {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// The selected budgets.
    pub budgets: Vec<ListedBudget>,
}

/// One budget as `list` reports it: every field the file gives, plus the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = every_field_required)]
pub struct ListedBudget {
    /// The budget's id.
    pub id: String,
    /// The budgets file's path, as discovered or given.
    pub file: String,
    /// The budget's description; null when it has none.
    pub description: Option<String>,
    /// The budget's labels, empty when it has none.
    pub labels: Vec<String>,
    /// How the value is measured.
    pub measure: Measure,
    /// The argv that measures it.
    pub command: Vec<String>,
    /// The budget's unit.
    pub unit: String,
    /// The budget's direction.
    pub direction: Direction,
    /// The budget's threshold.
    pub threshold: f64,
    /// The budget's timeout; null when it has none.
    pub timeout_seconds: Option<u64>,
}

/// Mark every property required, nullable ones included: a report always carries every
/// key, `null` where it has no value.
fn every_field_required(schema: &mut schemars::Schema) {
    let keys: Vec<serde_json::Value> = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .keys()
                .cloned()
                .map(serde_json::Value::String)
                .collect()
        })
        .unwrap_or_default();
    schema.insert("required".into(), serde_json::Value::Array(keys));
}

/// The exit statuses `onebudgetspec` and this library's callers share.
pub mod exit {
    /// Every selected budget is within.
    pub const WITHIN: i32 = 0;
    /// At least one budget is over and none errored.
    pub const OVER: i32 = 1;
    /// The invocation or a file is invalid; nothing was run.
    pub const INVALID: i32 = 2;
    /// At least one measurement errored.
    pub const ERROR: i32 = 3;
}

impl CheckReport {
    /// The exit status this report earns: `3` when any result errored, else `1` when any
    /// is over, else `0` — including when there are no results at all.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        let verdicts = || self.results.iter().map(|result| result.verdict);
        if verdicts().any(|verdict| verdict == Verdict::Error) {
            exit::ERROR
        } else if verdicts().any(|verdict| verdict == Verdict::Over) {
            exit::OVER
        } else {
            exit::WITHIN
        }
    }
}
