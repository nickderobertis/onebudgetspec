//! Committed budgets retain the values read before the YAML reader migration.
//! `yaml_values.json` was captured through `load` at c49215d982c53f871d1221e946c4fbc08a75b259;
//! the invalid-file document was already refused and therefore has no parsed value.
use std::path::{Path, PathBuf};

use onebudgetspec_core::load;

fn documents(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(documents(&path));
        } else if path.file_name().unwrap() == "budgets.yaml" {
            found.push(path);
        }
    }
    found.sort();
    found
}

#[test]
fn committed_documents_keep_their_values() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let expected: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("yaml_values.json")).unwrap();
    let paths = documents(&root.join("conformance/cases"));
    assert_eq!(paths.len(), expected.len());
    for path in paths {
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        let value = &expected[&name];
        if let Some(key) = value.get("refused") {
            let error = load(&[path], false).unwrap_err().to_string();
            assert!(error.contains("budgets.yaml"), "{error}");
            assert!(error.contains(key.as_str().unwrap()), "{error}");
            assert!(error.contains("unknown field"), "{error}");
            assert!(error.contains("budgets[0]"), "{error}");
        } else {
            let loaded = load(&[path], false).unwrap();
            let expected = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(loaded.files()[0].contents, expected, "{name}");
        }
    }
}
