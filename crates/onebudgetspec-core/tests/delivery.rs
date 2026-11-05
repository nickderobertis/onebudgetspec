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

#[test]
fn malformed_unsupported_and_semantically_invalid_history_cannot_hide_current_flags() {
    let request = request("current", 2);
    let valid = merge_standing_report(None, &request, 65536);
    let start = "<!-- onebudgetspec-history\n";
    let json = valid
        .split_once(start)
        .unwrap()
        .1
        .split_once("\n-->")
        .unwrap()
        .0;
    let history: serde_json::Value = serde_json::from_str(json).unwrap();
    for change in [
        "malformed",
        "version",
        "attempt",
        "duplicate",
        "flag",
        "provenance",
    ] {
        let mut history = history.clone();
        match change {
            "version" => history["schema_version"] = 99.into(),
            "attempt" => history["entries"][0]["attempt"] = 0.into(),
            "duplicate" => {
                let duplicate = history["entries"][0].clone();
                history["entries"].as_array_mut().unwrap().push(duplicate);
            }
            "flag" => history["entries"][0]["flags"][0]["sigma_change"] = 0.5.into(),
            "provenance" => history["entries"][0]["run_id"] = "".into(),
            _ => {}
        }
        let previous = format!(
            "old\n{start}{}\n-->",
            if change == "malformed" {
                "not JSON".into()
            } else {
                history.to_string()
            }
        );
        assert_eq!(
            merge_standing_report(Some(&previous), &request, 65536),
            valid,
            "{change}"
        );
    }
}

#[test]
fn request_validation_and_embedded_history_escaping_preserve_the_protocol() {
    use onebudgetspec_core::delivery::{ExternalReporter, Reporter, validate_request};
    let mut request = request("head-->tail<&>", 1);
    request.flags[0].file = "folder-->tail/budgets.yaml".into();
    let body = merge_standing_report(None, &request, 65536);
    assert!(body.contains("2.00 SD"));
    let encoded = body
        .split_once("<!-- onebudgetspec-history\n")
        .unwrap()
        .1
        .split_once("\n-->")
        .unwrap()
        .0;
    assert!(!encoded.contains("-->"));
    let history: onebudgetspec_core::delivery::ReportHistory =
        serde_json::from_str(encoded).unwrap();
    assert_eq!(history.entries[0].head_sha, request.head_sha);
    for invalid in [
        "version",
        "attempt",
        "provenance",
        "duplicate",
        "both",
        "neither",
        "path",
    ] {
        let mut bad = request.clone();
        match invalid {
            "version" => bad.schema_version = 2,
            "attempt" => bad.attempt = 0,
            "provenance" => bad.run_url.clear(),
            "duplicate" => bad.flags.push(bad.flags[0].clone()),
            "both" => bad.flags[0].constant_baseline_change = true,
            "neither" => bad.flags[0].sigma_change = None,
            _ => bad.flags[0].file = "../bad".into(),
        }
        assert!(validate_request(&bad).is_err(), "{invalid}");
        let delivery = ExternalReporter {
            executable: "not-invoked".into(),
        }
        .publish(&bad);
        assert!(!delivery.delivered);
        assert!(!delivery.diagnostic.unwrap().contains("cannot start"));
        assert!(merge_standing_report(Some(&body), &bad, 65536).contains("delivery diagnostic"));
    }
}
