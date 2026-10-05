//! The schema bundle's golden digest.
//!
//! Every consumer generates against the bundle `onebudgetspec schema` prints, so it may
//! change only on purpose: when this test fails, the change was to the contract. Bump
//! `SCHEMA_BUNDLE_VERSION`, then record the new digest and version in `schema.golden`.

use std::fmt::Write;

use onebudgetspec_core::{SCHEMA_BUNDLE_VERSION, schema_bundle};
use serde_json::Value;
use sha2::{Digest, Sha256};

const GOLDEN: &str = include_str!("../schema.golden");

#[test]
fn the_bundle_matches_its_golden_digest() {
    let text = canonical(&schema_bundle());
    let digest = Sha256::digest(text.as_bytes())
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    let actual = format!("version {SCHEMA_BUNDLE_VERSION}\nsha256 {digest}\n");
    assert_eq!(
        GOLDEN, actual,
        "the schema bundle changed; if that is intended, bump SCHEMA_BUNDLE_VERSION and write\n{actual}into crates/onebudgetspec-core/schema.golden"
    );
}

/// The bundle with every object's keys sorted, so the digest does not depend on whether a
/// build happens to enable `serde_json`'s insertion-ordered maps.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(fields) => {
            let mut keys: Vec<_> = fields.keys().collect();
            keys.sort();
            let members: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", Value::String(key.clone()), canonical(&fields[key])))
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", items.join(","))
        }
        scalar => scalar.to_string(),
    }
}
