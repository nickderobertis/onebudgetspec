//! The packed `@onebudgetspec/sdk`, installed into a fresh project beside the
//! `@onebudgetspec/cli` launcher and this host's carrier, checks, validates and lists a
//! conformance case and prints the schema through that launcher, with type declarations a
//! consumer compiles against.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::common::{
    SDK_CASE, VERSION, artifact, assert_sdk_answers, conformance_case, npm_project_with, root,
    succeed,
};

/// Each call over the case named by argv[1], with the selection its case.json gives.
const DRIVE: &str = r#"
import { readFileSync } from "node:fs";
import { join } from "node:path";
import * as sdk from "@onebudgetspec/sdk";

const cwd = process.argv[1];
const [, ...args] = JSON.parse(readFileSync(join(cwd, "case.json"), "utf8")).args;
const flags = { "--id": [], "--label": [], "--exclude-label": [] };
for (let index = 0; index < args.length; index++) {
  if (args[index] in flags) flags[args[index]].push(args[++index]);
}
const selection = {
  ids: flags["--id"],
  labels: flags["--label"],
  excludeLabels: flags["--exclude-label"],
};
console.log(JSON.stringify({
  check: await sdk.check({ cwd, ...selection }),
  list: await sdk.listBudgets({ cwd, ...selection }),
  validate: await sdk.validate({ cwd }),
  schema: await sdk.schema(),
  binary: sdk.resolveBinary()[1],
}));
"#;

/// A consumer the installed declarations must type-check: every call and the report types.
const CONSUMER: &str = r#"
import {
  type CheckReport,
  type ListReport,
  check,
  listBudgets,
  schema,
  validate,
} from "@onebudgetspec/sdk";

const report: CheckReport = await check({ ids: ["a"], labels: ["b"], excludeLabels: ["c"], recursive: true, cwd: "." });
const conditions: string | undefined = report.results[0]?.host.conditions["region"];
const listed: ListReport = await listBudgets({ paths: ["budgets.yaml"] });
const validated: ListReport = await validate({ paths: ["budgets.yaml"], recursive: false });
const bundle: Record<string, unknown> = await schema();
console.log(conditions, listed.budgets.length, validated.schema_version, Object.keys(bundle));
"#;

/// The manifest packed into `tarball`. Named relative to its directory, since a `C:` in a
/// path is a remote host to GNU tar, which Git Bash puts on a Windows PATH.
fn packed_manifest(tarball: &Path) -> Value {
    let manifest = succeed(
        "tar",
        &[
            "-xOzf",
            tarball.file_name().unwrap().to_str().unwrap(),
            "package/package.json",
        ],
        tarball.parent().unwrap(),
    );
    serde_json::from_str(&manifest).expect("the packed package.json is JSON")
}

#[test]
fn the_tarball_takes_the_cli_as_an_optional_dependency_at_the_workspace_version() {
    let manifest = packed_manifest(&artifact("sdk-typescript"));
    assert_eq!(manifest["name"], "@onebudgetspec/sdk");
    assert_eq!(manifest["version"], VERSION);
    assert_eq!(
        manifest["optionalDependencies"],
        serde_json::json!({ "@onebudgetspec/cli": VERSION })
    );
    assert!(
        manifest["dependencies"].get("@onebudgetspec/cli").is_none(),
        "the CLI is a dependency, not only an optional one: {manifest}"
    );
}

#[test]
fn the_installed_sdk_answers_a_conformance_case_through_the_launcher() {
    let sdk = artifact("sdk-typescript");
    let launcher = artifact("npm-launcher");
    let carrier = artifact("npm-carrier");
    let dir = tempfile::tempdir().unwrap();
    let project = npm_project_with(dir.path(), &[&carrier, &launcher, &sdk]);
    let version = succeed(
        "node",
        &[
            "--input-type=module",
            "-e",
            "import { VERSION } from '@onebudgetspec/sdk'; console.log(VERSION)",
        ],
        &project,
    );
    assert_eq!(version.trim(), VERSION);

    let (case_dir, case) = conformance_case(dir.path(), SDK_CASE);
    let output = Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            DRIVE,
            case_dir.to_str().unwrap(),
        ])
        .env_remove("ONEBUDGETSPEC_BIN")
        .current_dir(&project)
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "the SDK failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answers: Value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    let launcher = project
        .join("node_modules/@onebudgetspec/cli/bin/onebudgetspec.js")
        .canonicalize()
        .unwrap();
    let mut answers = answers;
    let resolved = Path::new(answers["binary"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    answers["binary"] = resolved.to_string_lossy().into_owned().into();
    assert_sdk_answers(&answers, &case_dir, &case, &launcher);
}

#[test]
fn a_consumer_type_checks_against_the_installed_declarations() {
    let sdk = artifact("sdk-typescript");
    let dir = tempfile::tempdir().unwrap();
    let project = npm_project_with(dir.path(), &[&sdk]);
    assert!(
        project
            .join("node_modules/@onebudgetspec/sdk/dist/index.d.ts")
            .is_file(),
        "the package ships no type declarations"
    );
    fs::write(project.join("consumer.ts"), CONSUMER).unwrap();
    fs::write(
        project.join("tsconfig.json"),
        r#"{"compilerOptions": {"target": "ES2023", "module": "NodeNext", "moduleResolution": "NodeNext", "strict": true, "noEmit": true, "skipLibCheck": false, "types": []}, "files": ["consumer.ts"]}"#,
    )
    .unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"name": "consumer", "private": true, "type": "module"}"#,
    )
    .unwrap();
    let tsc = root().join("node_modules/typescript/bin/tsc");
    succeed("node", &[tsc.to_str().unwrap(), "-p", "."], &project);
}
