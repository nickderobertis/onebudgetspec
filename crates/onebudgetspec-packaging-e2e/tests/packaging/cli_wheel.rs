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
