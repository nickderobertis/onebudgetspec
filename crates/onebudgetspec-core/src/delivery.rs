//! Reporter transport and bounded standing reports.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The saved `PublishRequest` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishRequest {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// Host-specific standing report destination.
    pub destination: String,
    /// Stable standing report identity at the destination.
    pub key: String,
    /// Commit measured by this run; never substituted with another head.
    pub head_sha: String,
    /// Host workflow run identity.
    pub run_id: String,
    /// Host run holding the complete summary and artifacts; links may expire.
    pub run_url: String,
    /// Current rendered budget report.
    pub markdown: String,
    /// Positive host attempt number; the latest completed attempt wins.
    #[schemars(range(min = 1))]
    pub attempt: u32,
    /// Adverse movements only; improvements create no history flag.
    pub flags: Vec<HistoricalFlag>,
}

/// The saved `HistoricalFlag` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoricalFlag {
    /// Normalized repository-relative budgets file path.
    pub file: String,
    /// Budget id within its file.
    pub id: String,
    /// Signed change in sample SD units; absent for changed constant baselines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sigma_change: Option<f64>,
    /// Whether a constant baseline changed adversely.
    pub constant_baseline_change: bool,
}

/// The saved `DeliveryResult` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryResult {
    /// Whether the host accepted the report.
    pub delivered: bool,
    /// Published report location when supplied by the host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Why delivery failed or degraded; independent of budget verdicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

/// The saved `HistoryEntry` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    /// Commit measured by this run; never substituted with another head.
    pub head_sha: String,
    /// Host workflow run identity.
    pub run_id: String,
    /// Host run holding the complete summary and artifacts; links may expire.
    pub run_url: String,
    /// Positive host attempt number; the latest completed attempt wins.
    #[schemars(range(min = 1))]
    pub attempt: u32,
    /// Adverse movements only; improvements create no history flag.
    pub flags: Vec<HistoricalFlag>,
}

/// The saved `ReportHistory` wire contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportHistory {
    /// Always `1`.
    #[schemars(schema_with = "crate::model::schema_version_schema")]
    pub schema_version: u32,
    /// Compact per-head flags, oldest first.
    pub entries: Vec<HistoryEntry>,
    /// Number of older heads dropped to fit host capacity.
    pub omitted_heads: u32,
}

/// A delivery adapter; delivery failures never change ordinary budget verdicts.
pub trait Reporter {
    /// Publish one standing report request.
    fn publish(&self, request: &PublishRequest) -> DeliveryResult;
}

/// An executable speaking JSON on stdin/stdout, with no shell interpretation.
pub struct ExternalReporter {
    /// The executable path.
    pub executable: std::path::PathBuf,
}

impl Reporter for ExternalReporter {
    fn publish(&self, request: &PublishRequest) -> DeliveryResult {
        let result = (|| -> Result<DeliveryResult, String> {
            use std::io::Write;
            use std::process::{Command, Stdio};
            let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
            let mut input =
                tempfile::tempfile().map_err(|e| format!("cannot prepare reporter stdin: {e}"))?;
            input
                .write_all(&bytes)
                .map_err(|e| format!("cannot write reporter stdin: {e}"))?;
            std::io::Seek::rewind(&mut input).map_err(|e| e.to_string())?;
            let output = Command::new(&self.executable)
                .stdin(Stdio::from(input))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| format!("cannot start reporter: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "reporter exited {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            serde_json::from_slice(&output.stdout)
                .map_err(|e| format!("malformed reporter output: {e}"))
        })();
        result.unwrap_or_else(|diagnostic| DeliveryResult {
            delivered: false,
            url: None,
            diagnostic: Some(diagnostic),
        })
    }
}

const START: &str = "<!-- onebudgetspec-history\n";
const END: &str = "\n-->";

/// Merge per-head flags and the current report into one bounded standing comment.
#[must_use]
pub fn merge_standing_report(
    previous: Option<&str>,
    request: &PublishRequest,
    max_body_bytes: usize,
) -> String {
    use std::fmt::Write;
    let mut history = previous
        .and_then(|body| body.split_once(START)?.1.split_once(END))
        .and_then(|(json, _)| serde_json::from_str::<ReportHistory>(json).ok())
        .filter(|h| h.schema_version == 1)
        .unwrap_or(ReportHistory {
            schema_version: 1,
            entries: Vec::new(),
            omitted_heads: 0,
        });
    if let Some(body) = previous.filter(|body| body.len() <= max_body_bytes)
        && history
            .entries
            .iter()
            .any(|entry| entry.head_sha == request.head_sha && entry.attempt > request.attempt)
    {
        return body.to_string();
    }
    if let Some(entry) = history
        .entries
        .iter_mut()
        .find(|entry| entry.head_sha == request.head_sha)
    {
        if request.attempt >= entry.attempt {
            *entry = history_entry(request);
        }
    } else {
        history.entries.push(history_entry(request));
    }
    let mut current = request.markdown.clone();
    loop {
        let mut body = format!("{current}\n\n### Per-head adverse flags\n");
        for entry in &history.entries {
            let _ = writeln!(
                body,
                "\n{} (attempt {}): {} adverse flag(s)",
                safe_text(&entry.head_sha),
                entry.attempt,
                entry.flags.len()
            );
            for flag in &entry.flags {
                let _ = writeln!(
                    body,
                    "- {}/{}: {}",
                    safe_text(&flag.file),
                    safe_text(&flag.id),
                    if flag.constant_baseline_change {
                        "constant-baseline-change".into()
                    } else {
                        flag.sigma_change.map_or_else(
                            || "sigma unavailable".into(),
                            |value| format!("{value:.2} SD"),
                        )
                    }
                );
            }
        }
        if history.omitted_heads > 0 {
            let _ = write!(
                body,
                "\n{} oldest history head(s) omitted to fit host capacity.\n",
                history.omitted_heads
            );
        }
        body.push_str(START);
        body.push_str(
            &serde_json::to_string(&history)
                .unwrap_or_default()
                .replace('<', "\\u003c")
                .replace('>', "\\u003e"),
        );
        body.push_str(END);
        if body.len() <= max_body_bytes {
            return body;
        }
        if history.entries.len() > 1 {
            history.entries.remove(0);
            history.omitted_heads = history.omitted_heads.saturating_add(1);
            continue;
        }
        let fallback = format!(
            "## Budget report\n\nComplete summary/artifact: {}\n\nArtifact links may expire.\n",
            safe_text(&request.run_url)
        );
        if current != fallback {
            current = fallback;
            continue;
        }
        if !history.entries.is_empty() {
            history.entries.clear();
            history.omitted_heads = history.omitted_heads.saturating_add(1);
            continue;
        }
        // Tiny capacities cannot hold even the fallback; truncate at a UTF-8 boundary.
        let mut end = max_body_bytes.min(body.len());
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
        return body;
    }
}

fn safe_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace(['\r', '\n'], " ")
}
fn history_entry(request: &PublishRequest) -> HistoryEntry {
    HistoryEntry {
        head_sha: request.head_sha.clone(),
        run_id: request.run_id.clone(),
        run_url: request.run_url.clone(),
        attempt: request.attempt,
        flags: request.flags.clone(),
    }
}
