//! The `onebudgetspec-sdk` wheel installs into a fresh environment and imports as
//! `onebudgetspec_sdk` at the release version.

use crate::common::{VERSION, artifact, succeed, venv_with};

#[test]
fn the_python_sdk_installs_and_imports() {
    let wheel = artifact("sdk-python");
    let dir = tempfile::tempdir().unwrap();
    let bin = venv_with(dir.path(), &wheel);
    let version = succeed(
        bin.join("python"),
        &[
            "-c",
            "import onebudgetspec_sdk; print(onebudgetspec_sdk.__version__)",
        ],
        dir.path(),
    );
    assert_eq!(version.trim(), VERSION);
    let typed = succeed(
        bin.join("python"),
        &[
            "-c",
            "import importlib.resources as r; print(r.files('onebudgetspec_sdk').joinpath('py.typed').is_file())",
        ],
        dir.path(),
    );
    assert_eq!(typed.trim(), "True", "the wheel ships no py.typed marker");
}
