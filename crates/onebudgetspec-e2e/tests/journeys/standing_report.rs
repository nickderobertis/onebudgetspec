//! One change's standing comment across successive heads, through the real CLI and the
//! real reporter subprocess: earlier heads' flags collapse after Details, the current head
//! is never listed there, and the hidden history keeps its v0.2.0 bytes.
use crate::common::{Fixture, run_in};
use onebudgetspec_core::comparison::{BudgetRun, ComparisonInput};
use onebudgetspec_core::delivery::ReportHistory;

const START: &str = "<!-- onebudgetspec-history\n";
const END: &str = "\n-->";

/// The history a standing body carries in its hidden block.
pub fn hidden_history(body: &str) -> ReportHistory {
    serde_json::from_str(hidden_block(body)).unwrap()
}

fn hidden_block(body: &str) -> &str {
    let block = body.split_once(START).expect("a hidden history block").1;
    block
        .strip_suffix(END)
        .expect("the hidden block ends the body")
}

/// Two budgets over twenty constant base commits; each id in `flagged` moves adversely.
fn input(head: &str, flagged: &[&str]) -> ComparisonInput {
    let mut current: BudgetRun = serde_json::from_str(include_str!(
        "../../../onebudgetspec-core/tests/fixtures/budget-run.json"
    ))
    .unwrap();
    current.enforcement_exclude_labels.clear();
    current.head_sha = head.into();
    current.run_id = format!("run-{head}");
    let mut second = current.catalog[0].clone();
    second.id = "throughput".into();
    current.catalog.push(second);
    let mut result = current.results.results[0].clone();
    result.id = "throughput".into();
    current.results.results.push(result);
    let set = |run: &mut BudgetRun, id: &str, actual: f64| {
        let result = run.results.results.iter_mut().find(|r| r.id == id).unwrap();
        let (verdict, headroom, percent) =
            onebudgetspec_core::judge(result.direction, result.threshold, actual);
        result.actual = Some(actual);
        result.verdict = verdict;
        result.headroom = Some(headroom);
        result.headroom_percent = percent;
    };
    let baseline = (0..20)
        .map(|i| {
            let mut run = current.clone();
            run.head_sha = format!("base-{i}");
            set(&mut run, "latency", 10.0);
            set(&mut run, "throughput", 10.0);
            run
        })
        .collect();
    for id in ["latency", "throughput"] {
        let actual = if flagged.contains(&id) { 11.0 } else { 10.0 };
        set(&mut current, id, actual);
    }
    ComparisonInput {
        schema_version: 1,
        current,
        baseline,
    }
}

/// Report `input` through the reporter subprocess and return the standing body it wrote.
fn publish(fixture: &Fixture, input: &ComparisonInput) -> String {
    fixture.write("input.json", &serde_json::to_string(input).unwrap());
    let reporter = env!("CARGO_BIN_EXE_onebudgetspec-e2e-publish");
    run_in(
        fixture.path(),
        ["report", "--input", "input.json", "--reporter", reporter],
        &[],
    )
    .expect_status(0);
    std::fs::read_to_string(fixture.path().join("standing.md")).unwrap()
}

/// The collapsed earlier-heads section, asserting it sits after Details and before the
/// hidden block.
fn earlier_heads(body: &str) -> &str {
    let (_, after_details) = body
        .split_once("<details><summary>Details</summary>")
        .expect("a Details section");
    let (_, after_close) = after_details.split_once("\n</details>\n").unwrap();
    let section = after_close
        .trim_start()
        .strip_prefix("<details><summary>Flags on earlier heads (")
        .unwrap_or_else(|| panic!("no collapsed earlier-heads section after Details in\n{body}"));
    let (section, rest) = section.split_once("\n</details>").unwrap();
    assert!(rest.trim_start().starts_with(START), "{body}");
    section.trim_end()
}

#[test]
fn a_later_head_shows_the_first_heads_flags_collapsed_after_details() {
    let fixture = Fixture::new();
    publish(&fixture, &input("head-1", &["latency"]));
    let body = publish(&fixture, &input("head-2", &[]));
    assert!(!body.contains("<details open><summary>Flags"));
    assert_eq!(
        earlier_heads(&body),
        "1)</summary>\n\nhead-1 (attempt 1): 1 adverse flag(s)\n- budgets.yaml/latency: constant-baseline-change"
    );
}

#[test]
fn a_flagged_later_head_lists_only_the_earlier_heads_flags() {
    let fixture = Fixture::new();
    publish(&fixture, &input("head-1", &["latency"]));
    let body = publish(&fixture, &input("head-2", &["throughput"]));
    let section = earlier_heads(&body);
    assert!(section.contains("head-1 (attempt 1): 1 adverse flag(s)\n- budgets.yaml/latency:"));
    assert!(!section.contains("head-2"), "{section}");
    assert!(!section.contains("throughput"), "{section}");
    // The current head's own flag is in its report and its history entry, not listed.
    assert!(body.contains("| **throughput** |"));
    let history = hidden_history(&body);
    assert_eq!(history.entries[1].head_sha, "head-2");
    assert_eq!(history.entries[1].flags[0].id, "throughput");
}

#[test]
fn only_the_current_head_flagged_or_no_head_flagged_shows_no_section() {
    for flagged in [&["latency"][..], &[]] {
        let fixture = Fixture::new();
        publish(&fixture, &input("head-1", &[]));
        let body = publish(&fixture, &input("head-2", flagged));
        assert!(!body.contains("Flags on earlier heads"), "{body}");
        assert!(!body.contains("adverse flag(s)"), "{body}");
        assert_eq!(hidden_history(&body).entries.len(), 2);
    }
}

#[test]
fn heads_left_out_for_capacity_leave_only_the_capacity_line() {
    let fixture = Fixture::new();
    let mut body = String::new();
    for head in 0..40 {
        body = publish(&fixture, &input(&format!("head-{head}"), &[]));
    }
    assert!(body.len() <= 4096);
    let omitted = hidden_history(&body).omitted_heads;
    assert!(omitted > 0);
    assert_eq!(
        earlier_heads(&body),
        format!("0)</summary>\n\n{omitted} oldest history head(s) omitted to fit host capacity.")
    );
}

/// The bytes v0.2.0's `merge_standing_report` wrote for one flagged head.
const V0_2_0_BODY: &str = r#"## Budget report

Repository: owner/repo · head: old-head · run: run-old · attempt: 1

| Budget | Verdict | Current | Mean | SD | Change % | Sigma | Samples | Status |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| budgets.yaml / latency (ms) | over | 15.000 | 10.000 | 0.000 | 50.000 | unavailable | 20 | constant-baseline-change; sigma change unavailable: constant baseline changed |

Baseline provenance (saved observations; artifact links may expire):


### Per-head adverse flags

old-head (attempt 1): 1 adverse flag(s)
- budgets.yaml/latency: constant-baseline-change
<!-- onebudgetspec-history
{"schema_version":1,"entries":[{"head_sha":"old-head","run_id":"run-old","run_url":"https://example.test/run/old","attempt":1,"flags":[{"file":"budgets.yaml","id":"latency","constant_baseline_change":true}]}],"omitted_heads":0}
-->"#;

#[test]
fn a_body_written_by_v0_2_0_is_read_and_its_flags_carried() {
    let fixture = Fixture::new();
    fixture.write("standing.md", V0_2_0_BODY);
    let body = publish(&fixture, &input("head-2", &[]));
    assert!(
        earlier_heads(&body).contains(
            "old-head (attempt 1): 1 adverse flag(s)\n- budgets.yaml/latency: constant-baseline-change"
        ),
        "{body}"
    );
    assert!(!body.contains("### Per-head adverse flags"));
    let history = hidden_history(&body);
    let heads: Vec<_> = history
        .entries
        .iter()
        .map(|e| e.head_sha.as_str())
        .collect();
    assert_eq!(heads, ["old-head", "head-2"]);
    assert!(history.entries[0].flags[0].constant_baseline_change);
}

#[test]
fn the_hidden_block_round_trips_byte_for_byte() {
    let fixture = Fixture::new();
    publish(&fixture, &input("head-<1>", &["latency"]));
    let body = publish(&fixture, &input("head-2", &["throughput"]));
    let block = hidden_block(&body);
    let history: ReportHistory = serde_json::from_str(block).unwrap();
    let encoded = serde_json::to_string(&history)
        .unwrap()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    assert_eq!(block, encoded);
    assert!(block.contains("head-\\u003c1\\u003e"));
    assert_eq!(body.matches(START).count(), 1);
}
