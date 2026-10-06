//! The budgets file: what a `budgets.yaml` declares.
//!
//! These types are the one authoritative statement of the file's shape. The JSON Schema
//! `onebudgetspec schema` emits under the `budgets-file` root is derived from them, and
//! [`crate::load`] holds every file to the patterns and rules written here.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The only `schema_version` a budgets file, a check report or a list report carries.
pub const SCHEMA_VERSION: u32 = 1;

/// A budget id: lowercase, starting with a letter, then letters, digits and hyphens.
pub const ID_PATTERN: &str = "^[a-z][a-z0-9-]*$";
/// A label: the same shape as an id.
pub const LABEL_PATTERN: &str = "^[a-z][a-z0-9-]*$";
/// A unit: a free label echoed in results, such as `seconds`, `requests/min` or `%`.
pub const UNIT_PATTERN: &str = "^[a-z][a-z0-9_/%-]*$";
/// A condition name, declared by a file or returned by a `reported` command.
pub const CONDITION_NAME_PATTERN: &str = "^[a-z][a-z0-9_]*$";
/// The host values every result records, which no condition may be named.
pub const RESERVED_CONDITION_NAMES: [&str; 3] = ["load1", "cpus", "mem_available_mib"];
/// The environment variable naming the file a `reported` command writes its result to.
pub const RESULT_ENV: &str = "ONEBUDGETSPEC_RESULT";
/// The environment variable holding the id of the budget whose command is running, set for
/// every budget's command and for no condition's.
pub const BUDGET_ID_ENV: &str = "ONEBUDGETSPEC_BUDGET_ID";
/// The only file name `--recursive` discovery picks up.
pub const FILE_NAME: &str = "budgets.yaml";

// llmlint: ignore-block[invalid_states_unrepresentable] These structs are the serde shape of an untrusted file and the source the contract's schema is emitted from. load.rs holds every field to the patterns and rules above and reports every problem in one refusal naming the file and key path; validating newtypes would refuse at deserialization, one problem at a time, and change the emitted schema other repositories build against.
/// One `budgets.yaml` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "budgets-file")]
pub struct BudgetsFile {
    /// Always `1`.
    #[schemars(schema_with = "schema_version_schema")]
    pub schema_version: u32,
    /// Host conditions recorded beside every result measured from this file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    /// The budgets this file registers, measured in this order.
    pub budgets: Vec<Budget>,
}

/// A host condition a file declares: a command whose trimmed stdout is recorded beside
/// every result measured from the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// Unique in the file, and never `load1`, `cpus` or `mem_available_mib`.
    #[schemars(pattern(CONDITION_NAME_PATTERN))]
    pub name: String,
    /// The argv to run, once per file per check, from the directory holding the file. A
    /// failure records `unknown`.
    #[schemars(length(min = 1))]
    pub command: Vec<String>,
}

/// One budget: a command that measures a value, and the threshold it must stay within.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Unique in the file, and across every file one discovery finds.
    #[schemars(pattern(ID_PATTERN))]
    pub id: String,
    /// Free text: what is measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Free tags a consumer selects on; the library attaches no meaning to them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(inner(pattern(LABEL_PATTERN)), extend("uniqueItems" = true))]
    pub labels: Vec<String>,
    /// How the value is measured.
    pub measure: Measure,
    /// The argv to run, once, from the directory holding the file, with no shell.
    #[schemars(length(min = 1))]
    pub command: Vec<String>,
    /// A free label echoed in results. An `elapsed` budget's unit is `seconds`.
    #[schemars(pattern(UNIT_PATTERN))]
    pub unit: String,
    /// Which side of the threshold is within.
    pub direction: Direction,
    /// A finite, non-negative number in `unit`.
    #[schemars(range(min = 0))]
    pub threshold: f64,
    /// Past this many seconds the measurement is an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub timeout_seconds: Option<u64>,
}

// llmlint: ignore-end[invalid_states_unrepresentable]
/// How a budget's value is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// The command's wall clock, in seconds; a non-zero exit is an error.
    Elapsed,
    /// The value the command writes to the file `ONEBUDGETSPEC_RESULT` names.
    Reported,
}

/// Which side of the threshold is within.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Within when `actual <= threshold`.
    Max,
    /// Within when `actual >= threshold`.
    Min,
}

impl Measure {
    /// The spelling the file and the reports use.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Elapsed => "elapsed",
            Self::Reported => "reported",
        }
    }
}

impl Direction {
    /// The spelling the file and the reports use.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Max => "max",
            Self::Min => "min",
        }
    }
}

pub(crate) fn schema_version_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({ "type": "integer", "const": SCHEMA_VERSION })
}
