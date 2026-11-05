//! Saved ordinary budget observations and their comparison.
use crate::{CheckReport, Direction, ListedBudget, Verdict};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// llmlint: ignore-block[invalid_states_unrepresentable] These are the fixed serde wire structs shared with installed consumers; changing String, Option or bool fields to domain newtypes or outcome enums would change that agreed Rust API and schema. validate_run/compare_json and validate_history validate saved observations at entry; validate_request and validated_report_history guard delivery inputs. Arithmetic and rendering preserve the ordinary report rather than replacing its types.
/// The saved `BudgetRun` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BudgetRun {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// Repository identity recorded by the producer.
    pub repository: String,
    /// Commit measured by this run; never substituted with another head.
    pub head_sha: String,
    /// Base branch reference used to select the history.
    pub base_ref: String,
    /// Host workflow run identity.
    pub run_id: String,
    /// Host run holding the complete summary and artifacts; links may expire.
    pub run_url: String,
    /// Positive host attempt number; the latest completed attempt wins.
    #[schemars(range(min = 1))]
    pub attempt: u32,
    /// Completion timestamp in RFC 3339.
    #[schemars(with = "chrono::DateTime<chrono::Utc>")]
    pub completed_at: String,
    /// Every discovered production budget, including unavailable measurements.
    pub catalog: Vec<ListedBudget>,
    /// Unmodified ordinary check results from completed measurements.
    pub results: CheckReport,
    /// Explicit missing-data reasons; no values are inferred.
    pub unavailable: Vec<UnavailableBudget>,
    /// Saved generic selection: matching catalog labels render as report-only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enforcement_exclude_labels: Vec<String>,
}

/// The saved `UnavailableBudget` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnavailableBudget {
    /// Normalized repository-relative budgets file path.
    pub file: String,
    /// Budget id within its file.
    pub id: String,
    /// Why the command could not supply an ordinary result.
    pub reason: String,
}

/// The saved `ComparisonInput` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparisonInput {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// Current ordinary observations, independent of the baseline.
    pub current: BudgetRun,
    /// Saved base observations; at most 20 unique heads are selected.
    pub baseline: Vec<BudgetRun>,
}

/// The saved `ComparisonRow` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparisonRow {
    /// Normalized repository-relative budgets file path.
    pub file: String,
    /// Budget id within its file.
    pub id: String,
    /// Declared measurement unit.
    pub unit: String,
    /// Which side of the threshold is within.
    pub direction: Direction,
    /// Ordinary declared threshold, excluded from baseline identity.
    pub threshold: f64,
    /// Current ordinary observations, independent of the baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<f64>,
    /// The observed ordinary verdict; absent when no measurement completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// Number of eligible unique base commits, at most 20.
    pub baseline_n: u32,
    /// Baseline arithmetic mean, available from five samples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean: Option<f64>,
    /// Sample standard deviation using n minus one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sd: Option<f64>,
    /// Signed change divided by absolute mean times 100; absent for zero mean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent_change: Option<f64>,
    /// Signed change divided by sample SD; absent for changed constant baselines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sigma_change: Option<f64>,
    /// Adverse movement of at least one SD, or an adverse constant-baseline change.
    pub flag: bool,
    /// Explicit missing-data reasons; no values are inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

/// The saved `ComparisonReport` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparisonReport {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// Current ordinary observations, independent of the baseline.
    pub current: BudgetRun,
    /// Every catalog budget, with ordinary errors and failures first.
    pub rows: Vec<ComparisonRow>,
    /// Rejected baseline entries, saved provenance and delivery diagnostics.
    pub diagnostics: Vec<String>,
}

/// The saved `RollingHistory` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RollingHistory {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// Repository identity recorded by the producer.
    pub repository: String,
    /// Base branch reference used to select the history.
    pub base_ref: String,
    /// At most 20 unique base commits, each with its latest completed attempt.
    #[schemars(length(max = 20))]
    pub runs: Vec<BudgetRun>,
}

// llmlint: ignore-end[invalid_states_unrepresentable]

/// Compare saved observations without executing a measurement.
#[must_use]
pub fn compare(input: &ComparisonInput) -> ComparisonReport {
    let mut report = ComparisonReport {
        schema_version: 1,
        current: input.current.clone(),
        rows: Vec::new(),
        diagnostics: Vec::new(),
    };
    if input.schema_version != 1 {
        report
            .diagnostics
            .push("unsupported comparison input schema_version".into());
    }
    let mut commits = std::collections::BTreeMap::<&str, &BudgetRun>::new();
    for run in &input.baseline {
        if let Err(reason) = validate_run(run) {
            report
                .diagnostics
                .push(format!("baseline {}: {reason}", run.head_sha));
            continue;
        }
        if run.repository != input.current.repository
            || run.base_ref != input.current.base_ref
            || run.head_sha == input.current.head_sha
        {
            report.diagnostics.push(format!(
                "baseline {}: repository, base_ref or head provenance mismatch",
                run.head_sha
            ));
            continue;
        }
        let previous = commits.entry(&run.head_sha).or_insert(run);
        if (
            run.attempt,
            chrono::DateTime::parse_from_rfc3339(&run.completed_at).ok(),
            &run.run_id,
        ) > (
            previous.attempt,
            chrono::DateTime::parse_from_rfc3339(&previous.completed_at).ok(),
            &previous.run_id,
        ) {
            *previous = run;
        }
    }
    let mut runs: Vec<_> = commits.into_values().collect();
    runs.sort_by_key(|run| {
        std::cmp::Reverse((
            chrono::DateTime::parse_from_rfc3339(&run.completed_at).ok(),
            &run.head_sha,
        ))
    });
    runs.truncate(20);
    for run in &runs {
        report.diagnostics.push(format!("baseline provenance: head {} run {} attempt {} completed {} URL {} (artifact links may expire)", run.head_sha, run.run_id, run.attempt, run.completed_at, run.run_url));
    }
    let current_error = if input.schema_version == 1 {
        validate_run(&input.current).err()
    } else {
        Some("unsupported comparison input schema_version".into())
    };
    if let Some(reason) = &current_error {
        report.diagnostics.push(format!("current: {reason}"));
    }
    report.rows = input
        .current
        .catalog
        .iter()
        .map(|budget| comparison_row(&input.current, budget, &runs, current_error.as_ref()))
        .collect();
    report.rows.sort_by_key(|row| match row.verdict {
        Some(Verdict::Error) => 0,
        Some(Verdict::Over) => 1,
        _ => 2,
    });
    report
}

fn comparison_row(
    current: &BudgetRun,
    budget: &ListedBudget,
    runs: &[&BudgetRun],
    current_error: Option<&String>,
) -> ComparisonRow {
    let result = current
        .results
        .results
        .iter()
        .find(|r| r.file == budget.file && r.id == budget.id);
    let mut row = ComparisonRow {
        file: budget.file.clone(),
        id: budget.id.clone(),
        unit: budget.unit.clone(),
        direction: budget.direction,
        threshold: budget.threshold,
        current: result.and_then(|r| r.actual),
        verdict: result.map(|r| r.verdict),
        baseline_n: 0,
        mean: None,
        sd: None,
        percent_change: None,
        sigma_change: None,
        flag: false,
        unavailable: current_error.cloned(),
    };
    if row.unavailable.is_none() {
        row.unavailable = current
            .unavailable
            .iter()
            .find(|r| r.file == budget.file && r.id == budget.id)
            .map(|r| r.reason.clone());
    }
    if row.current.is_none() && row.unavailable.is_none() {
        row.unavailable = Some(
            result
                .and_then(|r| r.error.clone())
                .unwrap_or_else(|| "current measurement unavailable or incomplete".into()),
        );
    }
    let mut missing = Vec::new();
    let samples: Vec<f64> = runs
        .iter()
        .filter_map(|run| match sample(run, budget) {
            Ok(value) => Some(value),
            Err(reason) => {
                missing.push(format!("{}: {reason}", run.head_sha));
                None
            }
        })
        .collect();
    if !missing.is_empty() {
        append_reason(&mut row, &missing.join("; "));
    }
    row.baseline_n = u32::try_from(samples.len()).unwrap_or(20);
    if samples.len() < 5 {
        let reason = format!(
            "baseline unavailable: {} eligible samples; minimum 5; additional usable baseline observations were not supplied",
            samples.len()
        );
        row.unavailable = Some(
            row.unavailable
                .map_or_else(|| reason.clone(), |old| format!("{old}; {reason}")),
        );
    } else if current_error.is_none() {
        statistics(&mut row, &samples);
    }
    row
}

fn append_reason(row: &mut ComparisonRow, reason: &str) {
    row.unavailable = Some(
        row.unavailable
            .take()
            .map_or_else(|| reason.to_string(), |old| format!("{old}; {reason}")),
    );
}

fn sample(run: &BudgetRun, budget: &ListedBudget) -> Result<f64, String> {
    let scope = run
        .catalog
        .iter()
        .find(|b| b.file == budget.file && b.id == budget.id)
        .ok_or("catalog entry absent")?;
    if scope.unit != budget.unit
        || scope.direction != budget.direction
        || scope.measure != budget.measure
        || scope.command != budget.command
        || scope.timeout_seconds != budget.timeout_seconds
    {
        return Err("measurement/command scope mismatch".into());
    }
    if let Some(item) = run
        .unavailable
        .iter()
        .find(|b| b.file == budget.file && b.id == budget.id)
    {
        return Err(item.reason.clone());
    }
    let result = run
        .results
        .results
        .iter()
        .find(|r| r.file == budget.file && r.id == budget.id)
        .ok_or("measurement absent or incomplete")?;
    if result.verdict == Verdict::Error {
        return Err(result
            .error
            .clone()
            .unwrap_or_else(|| "measurement error".into()));
    }
    result
        .actual
        .ok_or_else(|| "measurement value absent".into())
}

fn statistics(row: &mut ComparisonRow, samples: &[f64]) {
    let n = f64::from(row.baseline_n);
    let constant = samples.iter().all(|x| x.total_cmp(&samples[0]).is_eq());
    let mean = if constant {
        samples[0]
    } else {
        samples.iter().map(|x| x / n).sum::<f64>()
    };
    let scale = samples
        .iter()
        .map(|x| (x - mean).abs())
        .fold(0.0_f64, f64::max);
    let sd = if scale == 0.0 {
        0.0
    } else {
        scale
            * (samples
                .iter()
                .map(|x| ((x - mean) / scale).powi(2))
                .sum::<f64>()
                / (n - 1.0))
                .sqrt()
    };
    row.mean = mean.is_finite().then_some(mean);
    row.sd = sd.is_finite().then_some(sd);
    if row.mean.is_none() || row.sd.is_none() {
        append_reason(row, "baseline arithmetic exceeds finite range");
        return;
    }
    let Some(current) = row.current else {
        return;
    };
    let delta = current - mean;
    if mean == 0.0 {
        append_reason(row, "percent change unavailable: baseline mean is zero");
    }
    if sd == 0.0 && delta != 0.0 {
        append_reason(row, "sigma change unavailable: constant baseline changed");
    }
    row.percent_change = (mean != 0.0)
        .then(|| delta / mean.abs() * 100.0)
        .filter(|x| x.is_finite());
    row.sigma_change = if sd == 0.0 {
        (delta == 0.0).then_some(0.0)
    } else {
        Some(delta / sd).filter(|x| x.is_finite())
    };
    let adverse = match row.direction {
        Direction::Max => delta > 0.0,
        Direction::Min => delta < 0.0,
    };
    row.flag = adverse && (sd == 0.0 || row.sigma_change.is_some_and(|x| x.abs() >= 1.0));
    if row.percent_change.is_none() && mean != 0.0 {
        append_reason(row, "percent change exceeds finite range");
    }
    if row.sigma_change.is_none() && sd != 0.0 {
        append_reason(row, "sigma change exceeds finite range");
    }
}

/// Validate a saved run at the untrusted-input boundary.
///
/// # Errors
/// Returns the first invalid field, without running any command.
pub fn validate_run(run: &BudgetRun) -> Result<(), String> {
    if run.schema_version != 1 || run.results.schema_version != 1 {
        return Err("unsupported schema_version".into());
    }
    if run.repository.trim().is_empty()
        || run.base_ref.trim().is_empty()
        || run.head_sha.trim().is_empty()
        || run.run_id.trim().is_empty()
        || run.run_url.trim().is_empty()
        || run.attempt == 0
    {
        return Err("repository, base_ref, head_sha, run_id and positive attempt required".into());
    }
    chrono::DateTime::parse_from_rfc3339(&run.completed_at)
        .map_err(|e| format!("completed_at: {e}"))?;
    let id_pattern = regex::Regex::new(crate::model::ID_PATTERN).map_err(|e| e.to_string())?;
    let unit_pattern = regex::Regex::new(crate::model::UNIT_PATTERN).map_err(|e| e.to_string())?;
    if run
        .enforcement_exclude_labels
        .iter()
        .any(|label| !id_pattern.is_match(label))
    {
        return Err("invalid enforcement exclusion label".into());
    }
    let mut keys = std::collections::BTreeSet::new();
    for b in &run.catalog {
        if !valid_relative_path(&b.file) {
            return Err(format!(
                "{}: path must be normalized repository-relative",
                b.file
            ));
        }
        if !id_pattern.is_match(&b.id)
            || !unit_pattern.is_match(&b.unit)
            || b.command.is_empty()
            || b.command[0].is_empty()
            || b.threshold < 0.0
            || b.timeout_seconds == Some(0)
            || b.labels.iter().any(|label| !id_pattern.is_match(label))
        {
            return Err(format!("{}: invalid catalog measurement declaration", b.id));
        }
        if !b.threshold.is_finite() || !keys.insert((&b.file, &b.id)) {
            return Err("nonfinite threshold or duplicate catalog entry".into());
        }
    }
    let mut measured = std::collections::BTreeSet::new();
    for r in &run.results.results {
        let Some(b) = run
            .catalog
            .iter()
            .find(|b| b.file == r.file && b.id == r.id)
        else {
            return Err("result absent from catalog".into());
        };
        validate_result(r)?;
        if !measured.insert((&r.file, &r.id))
            || r.labels != b.labels
            || r.ended_at < r.started_at
            || r.unit != b.unit
            || r.direction != b.direction
            || r.threshold.total_cmp(&b.threshold).is_ne()
            || !r.threshold.is_finite()
            || [r.actual, r.headroom, r.headroom_percent, r.host.load1]
                .into_iter()
                .flatten()
                .any(|x| !x.is_finite())
            || (r.verdict == Verdict::Error) != r.actual.is_none()
        {
            return Err(format!("{}: invalid or mismatched result", r.id));
        }
    }
    let mut unavailable = std::collections::BTreeSet::new();
    for item in &run.unavailable {
        if !keys.contains(&(&item.file, &item.id))
            || !unavailable.insert((&item.file, &item.id))
            || item.reason.trim().is_empty()
            || measured.contains(&(&item.file, &item.id))
        {
            return Err("invalid unavailable entry".into());
        }
    }
    Ok(())
}

fn validate_result(result: &crate::CheckResult) -> Result<(), String> {
    let valid = if let Some(actual) = result.actual {
        let (verdict, headroom, percent) = crate::judge(result.direction, result.threshold, actual);
        result.verdict == verdict
            && result.error.is_none()
            && same_number(result.headroom, Some(headroom))
            && same_number(result.headroom_percent, percent)
    } else {
        result.verdict == Verdict::Error
            && result.headroom.is_none()
            && result.headroom_percent.is_none()
            && result
                .error
                .as_ref()
                .is_some_and(|error| !error.trim().is_empty())
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{}: ordinary verdict, headroom or error contradicts the saved measurement",
            result.id
        ))
    }
}

fn same_number(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            // JSON decoders can round a final mantissa bit; verdict comparisons stay exact.
            left.is_finite()
                && right.is_finite()
                && (left - right).abs() <= 8.0 * f64::EPSILON * left.abs().max(right.abs()).max(1.0)
        }
        (None, None) => true,
        _ => false,
    }
}

/// Validate the single saved rolling file, including its bounded unique provenance.
///
/// # Errors
/// Returns a diagnostic for malformed or mismatched saved history.
pub fn validate_history(history: &RollingHistory) -> Result<(), String> {
    if history.schema_version != 1
        || history.runs.len() > 20
        || history.repository.trim().is_empty()
        || history.base_ref.trim().is_empty()
    {
        return Err("rolling history requires schema_version 1 and at most 20 runs".into());
    }
    let mut heads = std::collections::BTreeSet::new();
    for run in &history.runs {
        validate_run(run)?;
        if run.repository != history.repository
            || run.base_ref != history.base_ref
            || !heads.insert(&run.head_sha)
        {
            return Err("rolling history repository, base_ref or duplicate head mismatch".into());
        }
    }
    Ok(())
}

fn cell(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace(['\r', '\n'], " ")
}
fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "unavailable".into(), |v| format!("{v:.3}"))
}

/// Render every catalog row, with ordinary failures first and saved enforcement labels.
#[must_use]
pub fn render_markdown(report: &ComparisonReport) -> String {
    use std::fmt::Write;
    let mut out = format!(
        "## Budget report\n\nRepository: {} · head: {} · run: {} · attempt: {}\n\n| Budget | Verdict | Current | Mean | SD | Change % | Sigma | Samples | Status |\n| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n",
        cell(&report.current.repository),
        cell(&report.current.head_sha),
        cell(&report.current.run_id),
        report.current.attempt
    );
    for row in &report.rows {
        let only = report
            .current
            .catalog
            .iter()
            .find(|b| b.file == row.file && b.id == row.id)
            .is_some_and(|b| {
                b.labels
                    .iter()
                    .any(|l| report.current.enforcement_exclude_labels.contains(l))
            });
        let mut status = Vec::new();
        if only {
            status.push("report-only".to_string());
        }
        if row.flag {
            status.push(
                if row.sd == Some(0.0) {
                    "constant-baseline-change"
                } else {
                    "adverse change ≥1 SD"
                }
                .into(),
            );
        }
        if (5..20).contains(&row.baseline_n) {
            status.push("provisional".into());
        }
        if let Some(reason) = &row.unavailable {
            status.push(cell(reason));
        }
        let _ = writeln!(
            out,
            "| {} / {} ({}) | {} | {} | {} | {} | {} | {} | {} | {} |",
            cell(&row.file),
            cell(&row.id),
            cell(&row.unit),
            row.verdict.map_or("unavailable", Verdict::as_str),
            number(row.current),
            number(row.mean),
            number(row.sd),
            number(row.percent_change),
            number(row.sigma_change),
            row.baseline_n,
            status.join("; ")
        );
    }
    out.push_str("\nBaseline provenance (saved observations; artifact links may expire):\n");
    for diagnostic in &report.diagnostics {
        let _ = writeln!(out, "\nDiagnostic: {}", cell(diagnostic));
    }
    out
}

impl ComparisonReport {
    /// Ordinary exit semantics applied only to results enforced by the saved selection.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        let mut enforced = self.current.results.clone();
        enforced.results.retain(|result| {
            !self.current.catalog.iter().any(|budget| {
                budget.file == result.file
                    && budget.id == result.id
                    && budget
                        .labels
                        .iter()
                        .any(|label| self.current.enforcement_exclude_labels.contains(label))
            })
        });
        enforced.exit_code()
    }
}

/// Parse untrusted saved input, retaining malformed baseline entries as diagnostics.
///
/// # Errors
/// Refuses malformed current input; baseline failures never mask current verdicts.
pub fn compare_json(bytes: &[u8]) -> Result<ComparisonReport, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SavedInput {
        schema_version: u32,
        current: BudgetRun,
        baseline: Vec<serde_json::Value>,
    }
    let saved: SavedInput = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if saved.schema_version != 1 {
        return Err("unsupported comparison input schema_version".into());
    }
    validate_run(&saved.current)?;
    let mut diagnostics = Vec::new();
    let baseline = saved
        .baseline
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| match serde_json::from_value(value) {
            Ok(run) => Some(run),
            Err(error) => {
                diagnostics.push(format!(
                    "baseline entry {index}: invalid saved run: {error}"
                ));
                None
            }
        })
        .collect();
    let mut report = compare(&ComparisonInput {
        schema_version: 1,
        current: saved.current,
        baseline,
    });
    report.diagnostics.extend(diagnostics);
    Ok(report)
}

pub(crate) fn valid_relative_path(file: &str) -> bool {
    !file.is_empty()
        && !file.starts_with('/')
        && !file.contains('\\')
        && !file.contains(':')
        && !file
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}
