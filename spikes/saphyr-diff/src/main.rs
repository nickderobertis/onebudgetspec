//! Differential harness: serde_norway (today) against serde-saphyr 1.3 and saphyr 0.2 on
//! this repository's real YAML. Measurement only; nothing here lands.
//!
//! Usage: saphyr-diff <repo root> <journey corpus dir> [external budgets.yaml ...]
//!
//! The journey corpus is what `ONEBUDGETSPEC_SPIKE_CORPUS` makes the e2e fixture writer
//! keep: `values/*.json` (the JSON values `Fixture::budgets` renders) and `texts/*.yaml`
//! (every budgets.yaml a journey writes). `run.sh` produces it and runs this.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use onebudgetspec_core::BudgetsFile;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy, Options, SerializerOptions};

// ---------------------------------------------------------------------------------------
// Reading through each crate the way load.rs does: serde_path_to_error around the crate's
// deserializer, the error rendered as `<file>: <path>: <message>` (path dropped at `.`).

#[derive(Debug, Clone, PartialEq)]
struct Failure {
    path: String,
    message: String,
}

impl Failure {
    fn rendered(&self) -> String {
        if self.path == "." {
            format!("budgets.yaml: {}", self.message)
        } else {
            format!("budgets.yaml: {}: {}", self.path, self.message)
        }
    }
}

type Parsed<T> = Result<T, Failure>;

fn norway<T: DeserializeOwned>(text: &str) -> Parsed<T> {
    let deserializer = serde_norway::Deserializer::from_str(text);
    serde_path_to_error::deserialize(deserializer).map_err(|error| Failure {
        path: error.path().to_string(),
        message: error.into_inner().to_string(),
    })
}

fn saphyr<T: DeserializeOwned>(text: &str, options: Options) -> Parsed<T> {
    let mut path = None;
    serde_saphyr::with_deserializer_from_str_with_options(text, options, |deserializer| {
        serde_path_to_error::deserialize(deserializer).map_err(|error| {
            path = Some(error.path().to_string());
            error.into_inner()
        })
    })
    .map_err(|error| Failure {
        // An error raised after the closure returned (trailing documents) has no path.
        path: path.unwrap_or_else(|| ".".into()),
        message: error.to_string(),
    })
}

/// The read options tried, by name, when the default disagrees with serde_norway.
fn read_variants() -> Vec<(&'static str, fn() -> Options)> {
    vec![
        ("default", || Options::default()),
        ("with_snippet: false", || serde_saphyr::options! { with_snippet: false }),
        ("strict_booleans: true", || serde_saphyr::options! { strict_booleans: true }),
        ("no_schema: true", || serde_saphyr::options! { no_schema: true }),
        ("legacy_octal_numbers: true", || {
            serde_saphyr::options! { legacy_octal_numbers: true }
        }),
        ("duplicate_keys: LastWins", || {
            serde_saphyr::options! { duplicate_keys: DuplicateKeyPolicy::LastWins }
        }),
        ("duplicate_keys: FirstWins", || {
            serde_saphyr::options! { duplicate_keys: DuplicateKeyPolicy::FirstWins }
        }),
        ("merge_keys: AsOrdinary", || {
            serde_saphyr::options! { merge_keys: MergeKeyPolicy::AsOrdinary }
        }),
        ("reject_non_finite_typeless_float: false", || {
            serde_saphyr::options! { reject_non_finite_typeless_float: false }
        }),
    ]
}

/// How two outcomes compare. Typed values compare by their `Debug`, which prints every
/// f64 exactly and lets NaN equal NaN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Agreement {
    Same,
    /// Both refuse at the same key path; the message words differ.
    SamePathOtherMessage,
    Different,
}

fn agreement<T: std::fmt::Debug>(a: &Parsed<T>, b: &Parsed<T>) -> Agreement {
    match (a, b) {
        (Ok(x), Ok(y)) if format!("{x:?}") == format!("{y:?}") => Agreement::Same,
        (Err(x), Err(y)) if x == y => Agreement::Same,
        (Err(x), Err(y)) if x.path == y.path => Agreement::SamePathOtherMessage,
        _ => Agreement::Different,
    }
}

fn show<T: std::fmt::Debug>(outcome: &Parsed<T>) -> String {
    match outcome {
        Ok(value) => format!("Ok {value:?}"),
        Err(failure) => format!("Err path={} :: {}", failure.path, indent(&failure.rendered())),
    }
}

fn indent(text: &str) -> String {
    text.trim_end().replace('\n', "\n        | ")
}

// ---------------------------------------------------------------------------------------
// saphyr's node tree, turned into JSON so it compares with the serde reads.

fn tree(text: &str) -> Result<Vec<Value>, String> {
    use saphyr::LoadableYamlNode;
    let documents = saphyr::Yaml::load_from_str(text).map_err(|error| error.to_string())?;
    documents.iter().map(node).collect()
}

fn node(yaml: &saphyr::Yaml) -> Result<Value, String> {
    use saphyr::{Scalar, Yaml};
    Ok(match yaml {
        Yaml::Value(Scalar::Null) => Value::Null,
        Yaml::Value(Scalar::Boolean(b)) => json!(b),
        Yaml::Value(Scalar::Integer(i)) => json!(i),
        Yaml::Value(Scalar::FloatingPoint(f)) => serde_json::Number::from_f64(f.into_inner())
            .map_or_else(|| json!(format!("<non-finite {}>", f.into_inner())), Value::Number),
        Yaml::Value(Scalar::String(s)) => json!(s),
        Yaml::Sequence(items) => Value::Array(items.iter().map(node).collect::<Result<_, _>>()?),
        Yaml::Mapping(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                let key = match node(key)? {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                out.insert(key, node(value)?);
            }
            Value::Object(out)
        }
        Yaml::Representation(text, ..) => json!(format!("<unresolved {text}>")),
        Yaml::Tagged(tag, inner) => json!({ format!("<tag {tag}>"): node(inner)? }),
        Yaml::Alias(id) => json!(format!("<alias {id}>")),
        Yaml::BadValue => return Err("BadValue".into()),
    })
}

// ---------------------------------------------------------------------------------------
// Writing.

fn write_variants() -> Vec<(String, SerializerOptions)> {
    let mut variants = Vec::new();
    for compact in [true, false] {
        for braces in [true, false] {
            for yaml_12 in [false, true] {
                for blocks in [true, false] {
                    for quote_all in [false, true] {
                        variants.push((
                            format!(
                                "compact_list_indent: {compact}, empty_as_braces: {braces}, yaml_12: {yaml_12}, prefer_block_scalars: {blocks}, quote_all: {quote_all}"
                            ),
                            serde_saphyr::ser_options! {
                                compact_list_indent: compact,
                                empty_as_braces: braces,
                                yaml_12: yaml_12,
                                prefer_block_scalars: blocks,
                                quote_all: quote_all,
                            },
                        ));
                    }
                }
            }
        }
    }
    variants
}

fn saphyr_write(value: &Value, options: SerializerOptions) -> Result<String, String> {
    serde_saphyr::to_string_with_options(value, options).map_err(|error| error.to_string())
}

/// What kind of emitted difference a line pair is: the same scalar quoted another way, a
/// long scalar folded onto several lines, or anything else.
fn classify(before: &str, after: &str) -> &'static str {
    let scalar = |line: &str| -> Option<Value> {
        // A sequence item's scalar is all of it; a mapping entry's follows its key.
        let trimmed = line.trim_start();
        let body = match trimmed.strip_prefix("- ") {
            Some(item) => item,
            None => trimmed.split_once(": ").map_or(trimmed, |(_, v)| v),
        };
        serde_norway::from_str::<Value>(body).ok()
    };
    if after.contains(">-\n") || after.contains("|-\n") || after.contains(">\n") {
        let a = serde_norway::from_str::<Value>(before).ok();
        let b = serde_norway::from_str::<Value>(after).ok();
        if a.is_some() && a == b {
            return "folded block scalar, same value";
        }
    }
    let quote = |line: &str| line.trim_start().trim_start_matches("- ").chars().next();
    if !before.contains('\n') && scalar(before).is_some() && scalar(before) == scalar(after) {
        return match (quote(before), quote(after)) {
            (Some('\''), Some('"')) => "single-quoted before, double-quoted after, same value",
            _ => "other quoting, same value",
        };
    }
    "OTHER"
}

/// The lines two texts differ in, paired up when they have as many lines, else whole.
fn line_diff(before: &str, after: &str) -> Vec<(String, String)> {
    let a: Vec<_> = before.lines().collect();
    let b: Vec<_> = after.lines().collect();
    if a.len() != b.len() {
        return vec![(before.to_owned(), after.to_owned())];
    }
    a.iter()
        .zip(&b)
        .filter(|(x, y)| x != y)
        .map(|(x, y)| ((*x).to_owned(), (*y).to_owned()))
        .collect()
}

// ---------------------------------------------------------------------------------------
// Corpus.

struct Doc {
    kind: &'static str,
    name: String,
    text: String,
}

fn read_dir_sorted(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}; run run.sh, which writes it", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == extension))
        .collect();
    paths.sort();
    paths
}

fn conformance(root: &Path) -> Vec<Doc> {
    let mut found = Vec::new();
    let mut stack = vec![root.join("conformance/cases")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "budgets.yaml") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
        .into_iter()
        .map(|path| Doc {
            kind: "conformance",
            name: path.strip_prefix(root).unwrap().display().to_string(),
            text: std::fs::read_to_string(&path).unwrap(),
        })
        .collect()
}

fn readme(root: &Path) -> Vec<Doc> {
    let text = std::fs::read_to_string(root.join("README.md")).unwrap();
    let mut docs = Vec::new();
    let mut current: Option<(usize, String)> = None;
    for (number, line) in text.lines().enumerate() {
        match &mut current {
            None if line.starts_with("```yaml") => current = Some((number + 1, String::new())),
            Some((start, body)) if line == "```" => {
                docs.push(Doc {
                    kind: "readme",
                    name: format!("README.md:{start}"),
                    text: std::mem::take(body),
                });
                current = None;
            }
            Some((_, body)) => {
                body.push_str(line);
                body.push('\n');
            }
            None => {}
        }
    }
    docs
}

fn api_literals(root: &Path) -> Vec<Doc> {
    let source =
        std::fs::read_to_string(root.join("crates/onebudgetspec-core/tests/api.rs")).unwrap();
    let start = source.find("const TWO_BUDGETS: &str = r#\"").unwrap() + 29;
    let end = start + source[start..].find("\"#;").unwrap();
    vec![
        Doc {
            kind: "api.rs",
            name: "TWO_BUDGETS".into(),
            text: source[start..end].to_owned(),
        },
        Doc {
            kind: "api.rs",
            name: "refuses_an_invalid_file (copied)".into(),
            text: "schema_version: 1\nbudgets:\n  - id: Bad\n    measure: elapsed\n    command: [node, -e, \"\"]\n    unit: ms\n    direction: max\n    threshold: -1\n".into(),
        },
        Doc {
            kind: "api.rs",
            name: "a_refusal_reads_as_its_problems (copied)".into(),
            text: "schema_version: 3\nbudgets: []\n".into(),
        },
    ]
}

/// Keys sorted, as the e2e crate's serde_json (no `preserve_order`) holds them, so the
/// harness renders what the journeys render.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// invalid_files.rs's valid base, with its scripts' paths as the fixture would make them.
fn base_value() -> Value {
    sorted(json!({
        "schema_version": 1,
        "conditions": [{ "name": "probe", "command": ["node", "condition.js"] }],
        "budgets": [{
            "id": "ok",
            "measure": "reported",
            "command": ["node", "budget.js"],
            "unit": "ms",
            "direction": "max",
            "threshold": 5,
        }],
    }))
}

type Defect = fn(&mut Value);

/// invalid_files.rs's DEFECTS, case for case, with the key each refusal must name.
const DEFECTS: &[(&str, Defect, &str)] = &[
    ("unknown top-level key", |f| f["owner"] = json!("me"), "owner"),
    ("unknown budget key", |f| f["budgets"][0]["workload"] = json!("x"), "workload"),
    ("unknown condition key", |f| f["conditions"][0]["shell"] = json!(true), "shell"),
    ("malformed id", |f| f["budgets"][0]["id"] = json!("Gate_Time"), "budgets[0].id"),
    ("malformed label", |f| f["budgets"][0]["labels"] = json!(["Fast"]), "budgets[0].labels[0]"),
    ("repeated label", |f| f["budgets"][0]["labels"] = json!(["fast", "fast"]), "budgets[0].labels[1]"),
    ("malformed unit", |f| f["budgets"][0]["unit"] = json!("Seconds"), "budgets[0].unit"),
    ("empty command", |f| f["budgets"][0]["command"] = json!([]), "budgets[0].command"),
    ("negative threshold", |f| f["budgets"][0]["threshold"] = json!(-1), "budgets[0].threshold"),
    ("zero timeout", |f| f["budgets"][0]["timeout_seconds"] = json!(0), "budgets[0].timeout_seconds"),
    (
        "id repeated in one file",
        |f| {
            let copy = f["budgets"][0].clone();
            f["budgets"].as_array_mut().unwrap().push(copy);
        },
        "budgets[1].id",
    ),
    ("elapsed not in seconds", |f| f["budgets"][0]["measure"] = json!("elapsed"), "budgets[0].unit"),
    ("unsupported schema_version", |f| f["schema_version"] = json!(2), "schema_version"),
    ("empty condition command", |f| f["conditions"][0]["command"] = json!([]), "conditions[0].command"),
    (
        "missing budgets",
        |f| {
            f.as_object_mut().unwrap().remove("budgets");
        },
        "budgets",
    ),
    ("malformed condition name", |f| f["conditions"][0]["name"] = json!("Probe"), "conditions[0].name"),
    (
        "condition name repeated",
        |f| {
            let copy = f["conditions"][0].clone();
            f["conditions"].as_array_mut().unwrap().push(copy);
        },
        "conditions[1].name",
    ),
    ("condition named load1", |f| f["conditions"][0]["name"] = json!("load1"), "conditions[0].name"),
    ("condition named cpus", |f| f["conditions"][0]["name"] = json!("cpus"), "conditions[0].name"),
    (
        "condition named mem_available_mib",
        |f| f["conditions"][0]["name"] = json!("mem_available_mib"),
        "conditions[0].name",
    ),
];

/// Synthetic edge probes: not real documents, the YAML corners a hand-written budgets file
/// can reach. Each is a small valid file with one thing changed.
fn probes() -> Vec<(&'static str, String)> {
    const BASE: &str = "schema_version: 1\nbudgets:\n  - id: ok\n    measure: reported\n    command: [node, budget.js]\n    unit: ms\n    direction: max\n    threshold: 5\n";
    let swap = |from: &str, to: &str| {
        assert!(BASE.contains(from));
        BASE.replacen(from, to, 1)
    };
    let add = |line: &str| format!("{BASE}    {line}\n");
    vec![
        ("threshold: 1_000", swap("threshold: 5", "threshold: 1_000")),
        ("threshold: 0x10", swap("threshold: 5", "threshold: 0x10")),
        ("threshold: 0o17", swap("threshold: 5", "threshold: 0o17")),
        ("threshold: 017", swap("threshold: 5", "threshold: 017")),
        ("threshold: 1e3", swap("threshold: 5", "threshold: 1e3")),
        ("threshold: +5", swap("threshold: 5", "threshold: +5")),
        ("threshold: .5", swap("threshold: 5", "threshold: .5")),
        ("threshold: 5.", swap("threshold: 5", "threshold: 5.")),
        ("threshold: \"5\"", swap("threshold: 5", "threshold: \"5\"")),
        ("threshold: !!float 5", swap("threshold: 5", "threshold: !!float 5")),
        ("threshold: 1e999", swap("threshold: 5", "threshold: 1e999")),
        ("threshold: .Inf", swap("threshold: 5", "threshold: .Inf")),
        ("threshold: .NaN", swap("threshold: 5", "threshold: .NaN")),
        ("threshold: 1:30 (sexagesimal)", swap("threshold: 5", "threshold: 1:30")),
        ("schema_version: \"1\"", swap("schema_version: 1", "schema_version: \"1\"")),
        ("schema_version: 1.0", swap("schema_version: 1", "schema_version: 1.0")),
        ("schema_version: 0x1", swap("schema_version: 1", "schema_version: 0x1")),
        ("timeout_seconds: 1.0", add("timeout_seconds: 1.0")),
        ("timeout_seconds: \"60\"", add("timeout_seconds: \"60\"")),
        ("timeout_seconds: ~", add("timeout_seconds: ~")),
        ("timeout_seconds: -1", add("timeout_seconds: -1")),
        ("description: yes", add("description: yes")),
        ("description: on", add("description: on")),
        ("description: true", add("description: true")),
        ("description: 123", add("description: 123")),
        ("description: 1.50", add("description: 1.50")),
        ("description: 0x1F", add("description: 0x1F")),
        ("description: 2026-10-12", add("description: 2026-10-12")),
        ("description: ~", add("description: ~")),
        ("description: null", add("description: null")),
        ("description: \"\"", add("description: \"\"")),
        ("description: (empty)", add("description:")),
        ("labels: ~", add("labels: ~")),
        ("labels: []", add("labels: []")),
        ("command: [node, x, 1395]", swap("[node, budget.js]", "[node, budget.js, 1395]")),
        ("command: [node, x, true]", swap("[node, budget.js]", "[node, budget.js, true]")),
        ("command: [node, x, yes]", swap("[node, budget.js]", "[node, budget.js, yes]")),
        ("command: [node, x, ~]", swap("[node, budget.js]", "[node, budget.js, ~]")),
        ("command: [node, x, 1.50]", swap("[node, budget.js]", "[node, budget.js, 1.50]")),
        ("command: [node, x, 0o17]", swap("[node, budget.js]", "[node, budget.js, 0o17]")),
        ("unit: y (YAML 1.1 bool spelling)", swap("unit: ms", "unit: y")),
        ("unit: no", swap("unit: ms", "unit: no")),
        ("measure: Reported (case)", swap("measure: reported", "measure: Reported")),
        ("measure: bogus", swap("measure: reported", "measure: bogus")),
        ("threshold: abc", swap("threshold: 5", "threshold: abc")),
        ("budgets: ~", "schema_version: 1\nbudgets: ~\n".into()),
        ("budgets: (empty)", "schema_version: 1\nbudgets:\n".into()),
        ("budgets: {}", "schema_version: 1\nbudgets: {}\n".into()),
        ("duplicate top-level key", format!("{BASE}schema_version: 1\n")),
        ("duplicate budget key", add("unit: ms")),
        ("anchor and alias", format!("schema_version: 1\nbudgets:\n  - &b\n    id: ok\n    measure: reported\n    command: &c [node, budget.js]\n    unit: ms\n    direction: max\n    threshold: 5\n  - id: two\n    measure: reported\n    command: *c\n    unit: ms\n    direction: max\n    threshold: 5\n")),
        ("merge key <<", "schema_version: 1\nbudgets:\n  - &b\n    id: ok\n    measure: reported\n    command: [node, budget.js]\n    unit: ms\n    direction: max\n    threshold: 5\n  - <<: *b\n    id: two\n".into()),
        ("unknown tag !custom", swap("unit: ms", "unit: !custom ms")),
        ("leading ---", format!("---\n{BASE}")),
        ("trailing ...", format!("{BASE}...\n")),
        ("two documents", format!("{BASE}---\n{BASE}")),
        ("UTF-8 BOM", format!("\u{feff}{BASE}")),
        ("CRLF line ends", BASE.replace('\n', "\r\n")),
        ("tab indentation", BASE.replacen("  - id: ok", "\t- id: ok", 1)),
        ("syntax error (unclosed [)", swap("[node, budget.js]", "[node, budget.js")),
        ("bad indentation", swap("    unit: ms", "   unit: ms")),
        ("empty file", String::new()),
        ("comment only", "# nothing\n".into()),
        ("scalar document", "hello\n".into()),
        ("%YAML 1.1 directive", format!("%YAML 1.1\n---\n{BASE}")),
        ("%YAML 1.2 directive", format!("%YAML 1.2\n---\n{BASE}")),
        ("flow mapping", "{schema_version: 1, budgets: [{id: ok, measure: reported, command: [node, budget.js], unit: ms, direction: max, threshold: 5}]}\n".into()),
        ("block literal description", add("description: |\n      two\n      lines")),
        ("quoted key", BASE.replacen("unit: ms", "\"unit\": ms", 1)),
        ("non-string key", add("1: x")),
    ]
}

// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct Tally {
    docs: usize,
    same: usize,
    path_only: usize,
    different: usize,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("usage: saphyr-diff <repo root> <corpus> [external ...]"));
    let corpus = PathBuf::from(args.next().expect("usage: saphyr-diff <repo root> <corpus> [external ...]"));
    let external: Vec<PathBuf> = args.map(PathBuf::from).collect();

    let values: Vec<(String, Value)> = read_dir_sorted(&corpus.join("values"), "json")
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            (path.file_name().unwrap().to_string_lossy().into_owned(), serde_json::from_str(&text).unwrap())
        })
        .collect();

    // Every real document read.
    let mut docs = Vec::new();
    docs.extend(conformance(&root));
    docs.extend(readme(&root));
    docs.extend(api_literals(&root));
    for path in read_dir_sorted(&corpus.join("texts"), "yaml") {
        docs.push(Doc {
            kind: "journey-written",
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            text: std::fs::read_to_string(&path).unwrap(),
        });
    }
    // invalid_files.rs's non-finite rewrite of serde_norway's text.
    let base_text = serde_norway::to_string(&base_value()).unwrap();
    for spelling in [".inf", "-.inf", ".nan"] {
        docs.push(Doc {
            kind: "non-finite",
            name: format!("threshold: {spelling}"),
            text: base_text.replace("threshold: 5", &format!("threshold: {spelling}")),
        });
    }
    // packaging-e2e's write_reporting_budgets: compact JSON as the budgets file ("JSON is
    // YAML"), with the two commands its journeys pass. The Python one is an absolute venv
    // path there; a stand-in of the same shape here.
    for command in [vec!["/tmp/venv/bin/python", "measure.py"], vec!["node", "measure.mjs"]] {
        let budgets: Vec<Value> = ["api-requests", "worker-requests"]
            .iter()
            .map(|id| {
                json!({ "id": id, "measure": "reported", "command": command, "unit": "requests",
                        "direction": "max", "threshold": 20 })
            })
            .collect();
        docs.push(Doc {
            kind: "packaging-written (JSON)",
            name: format!("write_reporting_budgets {command:?}"),
            text: sorted(json!({ "schema_version": 1, "budgets": budgets })).to_string(),
        });
    }
    // Every journey value as compact JSON too, the same JSON-is-YAML route.
    for (name, value) in &values {
        docs.push(Doc {
            kind: "journey values as JSON",
            name: name.clone(),
            text: value.to_string(),
        });
    }
    for path in &external {
        docs.push(Doc {
            kind: "external (read-only)",
            name: path.display().to_string(),
            text: std::fs::read_to_string(path).unwrap(),
        });
    }

    let mut out = String::new();
    let o = &mut out;
    writeln!(o, "# saphyr-diff: serde_norway 0.9.42 vs serde-saphyr 1.3.0 (+ saphyr 0.2.1 tree)\n").unwrap();

    // ---- READS ------------------------------------------------------------------------
    writeln!(o, "## READS into BudgetsFile (serde_path_to_error), serde_json::Value (preserve_order) and saphyr's tree\n").unwrap();
    let mut tallies: BTreeMap<&str, (Tally, Tally, Tally)> = BTreeMap::new();
    for doc in &docs {
        let typed_before = norway::<BudgetsFile>(&doc.text);
        let typed_after = saphyr::<BudgetsFile>(&doc.text, Options::default());
        let value_before = norway::<Value>(&doc.text);
        let value_after = saphyr::<Value>(&doc.text, Options::default());
        let tree_after = tree(&doc.text);
        let tree_vs_value = match (&value_before, &tree_after) {
            (Ok(v), Ok(t)) if t.len() == 1 && &t[0] == v => Agreement::Same,
            (Err(_), Err(_)) => Agreement::SamePathOtherMessage,
            _ => Agreement::Different,
        };

        let entry = tallies.entry(doc.kind).or_default();
        for (tally, verdict) in [
            (&mut entry.0, agreement(&typed_before, &typed_after)),
            (&mut entry.1, agreement(&value_before, &value_after)),
            (&mut entry.2, tree_vs_value),
        ] {
            tally.docs += 1;
            match verdict {
                Agreement::Same => tally.same += 1,
                Agreement::SamePathOtherMessage => tally.path_only += 1,
                Agreement::Different => tally.different += 1,
            }
        }

        let typed = agreement(&typed_before, &typed_after);
        if typed != Agreement::Same {
            writeln!(o, "- [{}] {} — typed read differs ({typed:?})", doc.kind, doc.name).unwrap();
            writeln!(o, "    before: {}", show(&typed_before)).unwrap();
            writeln!(o, "    after:  {}", show(&typed_after)).unwrap();
            let fixes: Vec<_> = read_variants()
                .into_iter()
                .filter(|(_, make)| agreement(&typed_before, &saphyr::<BudgetsFile>(&doc.text, make())) == Agreement::Same)
                .map(|(name, _)| name)
                .collect();
            writeln!(o, "    options reproducing today exactly: {fixes:?}").unwrap();
        }
        let value = agreement(&value_before, &value_after);
        if value != Agreement::Same {
            writeln!(o, "- [{}] {} — Value read differs ({value:?})", doc.kind, doc.name).unwrap();
            writeln!(o, "    before: {}", show(&value_before)).unwrap();
            writeln!(o, "    after:  {}", show(&value_after)).unwrap();
            let fixes: Vec<_> = read_variants()
                .into_iter()
                .filter(|(_, make)| agreement(&value_before, &saphyr::<Value>(&doc.text, make())) == Agreement::Same)
                .map(|(name, _)| name)
                .collect();
            writeln!(o, "    options reproducing today exactly: {fixes:?}").unwrap();
        }
        if tree_vs_value != Agreement::Same {
            writeln!(o, "- [{}] {} — saphyr tree differs from serde_norway's Value", doc.kind, doc.name).unwrap();
            writeln!(o, "    before (norway Value): {}", show(&value_before)).unwrap();
            writeln!(o, "    after  (saphyr tree):  {tree_after:?}").unwrap();
        }
    }
    writeln!(o, "\nRead tallies (docs / same / same-path-other-message / different):").unwrap();
    let mut total = 0;
    for (kind, (typed, value, tree)) in &tallies {
        total += typed.docs;
        writeln!(
            o,
            "  {kind:<22} typed {}/{}/{}/{}   Value {}/{}/{}/{}   tree-vs-Value {}/{}/{}/{}",
            typed.docs, typed.same, typed.path_only, typed.different,
            value.docs, value.same, value.path_only, value.different,
            tree.docs, tree.same, tree.path_only, tree.different,
        )
        .unwrap();
    }
    writeln!(o, "  total documents read: {total}").unwrap();

    // ---- WRITES -----------------------------------------------------------------------
    writeln!(o, "\n## WRITES: Fixture::budgets' serde_json::Value rendered as block YAML ({} values)\n", values.len()).unwrap();
    let mut written = values.clone();
    written.push(("invalid_files base".into(), base_value()));
    for (case, defect, _) in DEFECTS {
        let mut v = base_value();
        defect(&mut v);
        written.push((format!("invalid_files defect: {case}"), sorted(v)));
    }
    let variants = write_variants();
    let mut matches_per_variant = vec![0usize; variants.len()];
    let mut default_diffs: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    let mut identical_default = 0;
    let mut round_trip_problems = Vec::new();
    for (name, value) in &written {
        let before = serde_norway::to_string(value).unwrap();
        let after = saphyr_write(value, SerializerOptions::default()).unwrap();
        if before == after {
            identical_default += 1;
        }
        for pair in line_diff(&before, &after) {
            default_diffs.entry(pair).or_insert((0, name.clone())).0 += 1;
        }
        for (index, (_, options)) in variants.iter().enumerate() {
            if saphyr_write(value, options.clone()).as_deref() == Ok(before.as_str()) {
                matches_per_variant[index] += 1;
            }
        }
        // What the emitted text means: each crate's text read back by each crate.
        let original = Ok::<_, Failure>(value.clone());
        for (writer, text) in [("norway", &before), ("saphyr", &after)] {
            for (reader, read) in [
                ("norway", norway::<Value>(text)),
                ("saphyr", saphyr::<Value>(text, Options::default())),
            ] {
                if agreement(&original, &read) != Agreement::Same {
                    round_trip_problems.push(format!("{name}: {writer}-written, {reader}-read: {}", show(&read)));
                }
            }
            let typed_n = norway::<BudgetsFile>(text);
            let typed_s = saphyr::<BudgetsFile>(text, Options::default());
            if agreement(&typed_n, &typed_s) == Agreement::Different {
                round_trip_problems.push(format!(
                    "{name}: {writer}-written, typed reads differ:\n        norway {}\n        saphyr {}",
                    show(&typed_n),
                    show(&typed_s)
                ));
            }
        }
    }
    writeln!(o, "values written: {}; byte-identical under SerializerOptions::default(): {identical_default}", written.len()).unwrap();
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for (before, after) in default_diffs.keys() {
        *classes.entry(classify(before, after)).or_default() += 1;
    }
    writeln!(o, "Distinct differing line pairs under the default, by class: {classes:?}").unwrap();
    writeln!(o, "\nDistinct differing line pairs under the default (count, first value, class):").unwrap();
    for ((before, after), (count, first)) in &default_diffs {
        writeln!(o, "  x{count} [{first}] {}\n    before: {:?}\n    after:  {:?}", classify(before, after), before, after).unwrap();
    }
    writeln!(o, "\nByte-identical values per SerializerOptions variant (of {}):", written.len()).unwrap();
    for ((name, _), count) in variants.iter().zip(&matches_per_variant) {
        writeln!(o, "  {count:>4}  {name}").unwrap();
    }
    let best = matches_per_variant.iter().enumerate().max_by_key(|(_, c)| **c).unwrap().0;
    writeln!(o, "\nRemaining differences under the best variant ({}):", variants[best].0).unwrap();
    let mut best_diffs: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for (name, value) in &written {
        let before = serde_norway::to_string(value).unwrap();
        let after = saphyr_write(value, variants[best].1.clone()).unwrap();
        for pair in line_diff(&before, &after) {
            best_diffs.entry(pair).or_insert((0, name.clone())).0 += 1;
        }
    }
    if best_diffs.is_empty() {
        writeln!(o, "  none").unwrap();
    }
    let mut best_classes: BTreeMap<&str, usize> = BTreeMap::new();
    for (before, after) in best_diffs.keys() {
        *best_classes.entry(classify(before, after)).or_default() += 1;
    }
    writeln!(o, "  by class: {best_classes:?}").unwrap();
    for ((before, after), (count, first)) in &best_diffs {
        writeln!(o, "  x{count} [{first}] {}\n    before: {:?}\n    after:  {:?}", classify(before, after), before, after).unwrap();
    }
    writeln!(o, "\nEmitted-text round trips that do not give back the value or agree when typed:").unwrap();
    if round_trip_problems.is_empty() {
        writeln!(o, "  none").unwrap();
    }
    for problem in &round_trip_problems {
        writeln!(o, "  {problem}").unwrap();
    }
    let base_after = saphyr_write(&base_value(), variants[best].1.clone()).unwrap();
    writeln!(
        o,
        "\ninvalid_files.rs rewrite: serde_norway's base text holds `threshold: 5`: {}; serde-saphyr's (best variant) does: {}; (default) does: {}",
        base_text.contains("threshold: 5"),
        base_after.contains("threshold: 5"),
        saphyr_write(&base_value(), SerializerOptions::default()).unwrap().contains("threshold: 5"),
    )
    .unwrap();
    writeln!(o, "  base, serde_norway:\n{}", fence(&base_text)).unwrap();
    writeln!(o, "  base, serde-saphyr default:\n{}", fence(&saphyr_write(&base_value(), SerializerOptions::default()).unwrap())).unwrap();

    // ---- ERRORS -----------------------------------------------------------------------
    writeln!(o, "\n## ERRORS: invalid_files.rs DEFECTS (written by serde_norway as today, then by serde-saphyr) and api.rs refusals\n").unwrap();
    let mut error_cases: Vec<(String, String, String)> = Vec::new();
    for (case, defect, key) in DEFECTS {
        let mut v = base_value();
        defect(&mut v);
        let v = sorted(v);
        error_cases.push((format!("{case} (norway-written)"), serde_norway::to_string(&v).unwrap(), (*key).into()));
        error_cases.push((
            format!("{case} (saphyr-written)"),
            saphyr_write(&v, variants[best].1.clone()).unwrap(),
            (*key).into(),
        ));
    }
    error_cases.push(("a list, not a mapping".into(), "- just\n- a list\n".into(), "budgets.yaml".into()));
    for doc in api_literals(&root).into_iter().skip(1) {
        error_cases.push((doc.name, doc.text, "budgets.yaml".into()));
    }
    for spelling in [".inf", "-.inf", ".nan"] {
        error_cases.push((
            format!("non-finite {spelling}"),
            base_text.replace("threshold: 5", &format!("threshold: {spelling}")),
            "budgets[0].threshold".into(),
        ));
    }
    for (case, text, key) in &error_cases {
        let before = norway::<BudgetsFile>(text);
        let after = saphyr::<BudgetsFile>(text, Options::default());
        let verdict = agreement(&before, &after);
        match (&before, &after) {
            (Ok(_), Ok(_)) if verdict == Agreement::Same => {
                writeln!(o, "- {case}: both parse to the same BudgetsFile; refused afterwards by validate() alike (names {key})").unwrap();
            }
            _ => {
                writeln!(o, "- {case}: {verdict:?}").unwrap();
                writeln!(o, "    before: {}", show(&before)).unwrap();
                writeln!(o, "    after:  {}", show(&after)).unwrap();
                if let Err(f) = &after {
                    writeln!(o, "    after still names `{key}`: {}", f.rendered().contains(key.as_str())).unwrap();
                    let bare = saphyr::<BudgetsFile>(text, serde_saphyr::options! { with_snippet: false });
                    writeln!(o, "    after, with_snippet: false: {}", show(&bare)).unwrap();
                }
            }
        }
    }

    // ---- PROBES -----------------------------------------------------------------------
    writeln!(o, "\n## PROBES (synthetic edge cases, not real documents): typed read\n").unwrap();
    let mut probe_same = 0;
    let probe_list = probes();
    for (name, text) in &probe_list {
        let before = norway::<BudgetsFile>(text);
        let after = saphyr::<BudgetsFile>(text, Options::default());
        let verdict = agreement(&before, &after);
        if verdict == Agreement::Same {
            probe_same += 1;
            writeln!(o, "- {name}: same ({})", if before.is_ok() { "accepted" } else { "refused" }).unwrap();
            continue;
        }
        writeln!(o, "- {name}: {verdict:?}").unwrap();
        writeln!(o, "    before: {}", show(&before)).unwrap();
        writeln!(o, "    after:  {}", show(&after)).unwrap();
        let fixes: Vec<_> = read_variants()
            .into_iter()
            .filter(|(_, make)| agreement(&before, &saphyr::<BudgetsFile>(text, make())) == Agreement::Same)
            .map(|(name, _)| name)
            .collect();
        let path_fixes: Vec<_> = read_variants()
            .into_iter()
            .filter(|(_, make)| agreement(&before, &saphyr::<BudgetsFile>(text, make())) <= Agreement::SamePathOtherMessage)
            .map(|(name, _)| name)
            .collect();
        writeln!(o, "    options reproducing today exactly: {fixes:?}; same outcome and path: {path_fixes:?}").unwrap();
    }
    writeln!(o, "\nprobes: {} run, {probe_same} identical", probe_list.len()).unwrap();

    // ---- TIMING -----------------------------------------------------------------------
    writeln!(o, "\n## PARSE TIME (release build; 15 rounds of 2000 parses after 200 warm-up)\n").unwrap();
    let largest_repo = docs
        .iter()
        .filter(|d| d.kind != "external (read-only)" && norway::<BudgetsFile>(&d.text).is_ok())
        .max_by_key(|d| d.text.len())
        .unwrap();
    let largest_committed = docs
        .iter()
        .filter(|d| matches!(d.kind, "conformance" | "readme" | "api.rs") && norway::<BudgetsFile>(&d.text).is_ok())
        .max_by_key(|d| d.text.len())
        .unwrap();
    let largest_external = docs
        .iter()
        .filter(|d| d.kind == "external (read-only)")
        .max_by_key(|d| d.text.len());
    writeln!(o, "host: {} CPUs; timings are wall clock on a shared host, so compare within a row set\n", std::thread::available_parallelism().map_or(0, usize::from)).unwrap();
    for doc in [largest_committed, largest_repo].into_iter().chain(largest_external) {
        writeln!(o, "{} [{}], {} bytes:", doc.name, doc.kind, doc.text.len()).unwrap();
        let text = doc.text.as_str();
        let rounds: [(&str, Box<dyn Fn()>); 3] = [
            ("serde_norway -> BudgetsFile", Box::new(move || {
                std::hint::black_box(norway::<BudgetsFile>(text).unwrap());
            })),
            ("serde-saphyr -> BudgetsFile", Box::new(move || {
                std::hint::black_box(saphyr::<BudgetsFile>(text, Options::default()).unwrap());
            })),
            ("saphyr tree", Box::new(move || {
                use saphyr::LoadableYamlNode;
                std::hint::black_box(saphyr::Yaml::load_from_str(text).unwrap());
            })),
        ];
        for (name, run) in &rounds {
            let (min, median) = timed_micros(run.as_ref());
            writeln!(o, "  {name:<30} min {min:>8.1} µs/parse   median {median:>8.1} µs/parse").unwrap();
        }
    }

    print!("{out}");
}

fn fence(text: &str) -> String {
    text.lines().map(|l| format!("      | {l}")).collect::<Vec<_>>().join("\n")
}

fn timed_micros(run: &dyn Fn()) -> (f64, f64) {
    const PER_ROUND: u32 = 2000;
    for _ in 0..200 {
        run();
    }
    let mut rounds: Vec<f64> = (0..15)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..PER_ROUND {
                run();
            }
            start.elapsed().as_secs_f64() * 1e6 / f64::from(PER_ROUND)
        })
        .collect();
    rounds.sort_by(f64::total_cmp);
    (rounds[0], rounds[rounds.len() / 2])
}
