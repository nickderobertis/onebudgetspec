//! Standing reports retain compact per-head adverse flags within the host's limit.
use onebudgetspec_core::delivery::{HistoricalFlag, PublishRequest, merge_standing_report};

fn request(head: &str, attempt: u32) -> PublishRequest {
    PublishRequest {
        schema_version: 1,
        destination: "https://example.test/pr/1".into(),
        key: "budget".into(),
        head_sha: head.into(),
        run_id: format!("run-{head}"),
        run_url: "https://example.test/summary".into(),
        markdown: "## Current\n\n| Budget | Value |\n| --- | --- |\n| latency | 15 |".into(),
        attempt,
        flags: vec![HistoricalFlag {
            file: "budgets.yaml".into(),
            id: "latency".into(),
            sigma_change: Some(2.0),
            constant_baseline_change: false,
        }],
    }
}

#[test]
fn latest_attempt_per_head_replaces_flags_and_old_attempts_do_not_regress_history() {
    let first = merge_standing_report(None, &request("abc", 2), 65536);
    let mut stale = request("abc", 1);
    stale.markdown = "stale current table".into();
    let duplicate = merge_standing_report(Some(&first), &stale, 65536);
    assert_eq!(first, duplicate);
    let mut improved = request("abc", 3);
    improved.flags.clear();
    let updated = merge_standing_report(Some(&first), &improved, 65536);
    assert!(updated.contains("abc (attempt 3): 0 adverse flag(s)"));
    assert!(!updated.contains("attempt 2"));
    let another = merge_standing_report(Some(&updated), &request("def", 1), 65536);
    assert!(another.contains("abc (attempt 3)"));
    assert!(another.contains("def (attempt 1)"));
}

#[test]
fn history_is_dropped_with_notice_and_oversized_current_links_the_complete_run() {
    let mut body = String::new();
    for i in 0..30 {
        body = merge_standing_report(Some(&body), &request(&format!("head-{i}"), 1), 1800);
    }
    assert!(body.len() <= 1800);
    assert!(body.contains("oldest history head(s) omitted"));
    assert!(body.contains("head-29"));
    let mut huge = request("huge", 1);
    huge.markdown = "測".repeat(10000);
    let fallback = merge_standing_report(Some(&body), &huge, 1800);
    assert!(fallback.len() <= 1800);
    assert!(fallback.contains("https://example.test/summary"));
    assert!(fallback.contains("may expire"));
    assert!(merge_standing_report(None, &huge, 7).len() <= 7);
}
