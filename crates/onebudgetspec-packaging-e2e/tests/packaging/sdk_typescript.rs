//! The packed `@onebudgetspec/sdk` installs into a fresh project and imports at the release
//! version, with its type declarations.

use crate::common::{VERSION, artifact, npm_project_with, succeed};

#[test]
fn the_typescript_sdk_installs_and_imports() {
    let tarball = artifact("sdk-typescript");
    let dir = tempfile::tempdir().unwrap();
    let project = npm_project_with(dir.path(), &[&tarball]);
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
    assert!(
        project
            .join("node_modules/@onebudgetspec/sdk/dist/index.d.ts")
            .is_file(),
        "the package ships no type declarations"
    );
}
