//! The `onebudgetspec-cli` wheel puts a working `onebudgetspec` on a fresh environment's PATH.

use crate::common::{artifact, behaves_like_the_binary, venv_with};

#[test]
fn the_wheel_installs_a_binary_that_behaves_like_the_cargo_build() {
    let wheel = artifact("cli-wheel");
    let name = wheel.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with(&format!("onebudgetspec_cli-{}-", crate::common::VERSION)),
        "{name}"
    );
    let dir = tempfile::tempdir().unwrap();
    let bin = venv_with(dir.path(), &wheel);
    let installed = bin.join(crate::common::exe("onebudgetspec"));
    assert!(
        installed.is_file(),
        "the wheel installed no {}",
        installed.display()
    );
    behaves_like_the_binary(&installed, dir.path());
}

// The sequential pre-publication installer targets Darwin; its host-native subprocess
// proof uses the same Unix layout on Linux. Windows keeps its existing wheel journey.
#[cfg(unix)]
#[test]
fn staged_native_products_reuse_one_binary_and_run_every_local_install() {
    use crate::common::{bash, root, run, succeed};
    let dir = tempfile::tempdir().unwrap();
    let launcher = artifact("npm-launcher");
    let target = succeed("rustc", &["-vV"], &root());
    let target = target
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap();
    succeed(
        bash(),
        &[
            "scripts/release/native/native-products.sh",
            target,
            dir.path().to_str().unwrap(),
            launcher.to_str().unwrap(),
        ],
        &root(),
    );
    let hashes: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("payload-hashes.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(hashes["wheel"], hashes["npm"]);
    assert_eq!(hashes["wheel"], hashes["cargo"]);
    assert_eq!(hashes["wheel"].as_str().unwrap().len(), 64);
    let refused = run(
        bash(),
        &[
            "scripts/release/native/native-products.sh",
            "unsupported-target",
            dir.path().to_str().unwrap(),
            launcher.to_str().unwrap(),
        ],
        &root(),
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("not a Rust target"));
}
