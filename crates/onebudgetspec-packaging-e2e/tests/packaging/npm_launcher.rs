//! The `@onebudgetspec/cli` launcher resolves the host's platform carrier and runs it; without
//! the carrier it refuses with a reason and exit status 69.

use crate::common::{artifact, behaves_like_the_binary, npm_project_with, run};

#[test]
fn the_launcher_runs_the_host_carrier_like_the_cargo_build() {
    let launcher = artifact("npm-launcher");
    let carrier = artifact("npm-carrier");
    let dir = tempfile::tempdir().unwrap();
    let project = npm_project_with(dir.path(), &[&carrier, &launcher]);
    let installed = project.join("node_modules/.bin/onebudgetspec");
    assert!(installed.exists(), "npm linked no {}", installed.display());
    behaves_like_the_binary(&installed, dir.path());
}

#[test]
fn the_launcher_without_its_carrier_refuses_with_a_reason() {
    let launcher = artifact("npm-launcher");
    let dir = tempfile::tempdir().unwrap();
    let project = npm_project_with(dir.path(), &[&launcher]);
    let output = run(
        project.join("node_modules/.bin/onebudgetspec"),
        &["--version"],
        &project,
    );
    assert_eq!(output.status.code(), Some(69));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not installed"), "{stderr}");
    assert!(stderr.contains("reinstall @onebudgetspec/cli"), "{stderr}");
    assert!(output.stdout.is_empty());
}
