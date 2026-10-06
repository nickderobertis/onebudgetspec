//! The `@onebudgetspec/cli` launcher resolves the host's platform carrier and runs it; without
//! the carrier it refuses with a reason and exit status 69.

use crate::common::{artifact, behaves_like_the_binary, exe, npm_bin, npm_project_with, run};

#[test]
fn the_launcher_runs_the_host_carrier_like_the_cargo_build() {
    let launcher = artifact("npm-launcher");
    let carrier = artifact("npm-carrier");
    let dir = tempfile::tempdir().unwrap();
    let project = npm_project_with(dir.path(), &[&carrier, &launcher]);
    let installed = npm_bin(&project, "onebudgetspec");
    assert!(installed.exists(), "npm linked no {}", installed.display());
    behaves_like_the_binary(&installed, dir.path());
}

#[test]
fn the_launcher_without_its_carrier_refuses_with_a_reason() {
    let launcher = artifact("npm-launcher");
    let dir = tempfile::tempdir().unwrap();
    let project = npm_project_with(dir.path(), &[&launcher]);
    let output = run(npm_bin(&project, "onebudgetspec"), &["--version"], &project);
    assert_eq!(output.status.code(), Some(69));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not installed"), "{stderr}");
    assert!(stderr.contains("reinstall @onebudgetspec/cli"), "{stderr}");
    assert!(output.stdout.is_empty());
}

/// The npm platform name of this host, as the carriers are named.
fn host_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("macos", "aarch64") => "darwin-arm64",
        ("windows", "x86_64") => "win32-x64",
        ("windows", "aarch64") => "win32-arm64",
        (os, arch) => panic!("{os}-{arch} is not a platform the release ships"),
    }
}

/// A packed carrier for this host whose binary is `contents`, executable on Unix when
/// `executable`, made from the committed carrier manifest the way scripts/build-dist.sh makes
/// the real one.
fn carrier_with(dir: &std::path::Path, contents: &str, executable: bool) -> std::path::PathBuf {
    let stage = dir.join("stage");
    std::fs::create_dir_all(stage.join("bin")).unwrap();
    std::fs::copy(
        crate::common::root().join(format!("npm/platforms/{}/package.json", host_platform())),
        stage.join("package.json"),
    )
    .unwrap();
    let binary = stage.join("bin").join(exe("onebudgetspec"));
    std::fs::write(&binary, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = executable;
    let packed = dir.join("packed");
    std::fs::create_dir_all(&packed).unwrap();
    let name = crate::common::succeed(
        "npm",
        &[
            "pack",
            stage.to_str().unwrap(),
            "--silent",
            "--pack-destination",
            packed.to_str().unwrap(),
        ],
        dir,
    );
    packed.join(name.trim())
}

/// On Unix the binary lacks execute permission; on Windows it is not a program at all.
#[test]
fn a_carrier_that_cannot_execute_is_refused_with_69() {
    let launcher = artifact("npm-launcher");
    let dir = tempfile::tempdir().unwrap();
    let carrier = carrier_with(dir.path(), "not a program\n", false);
    let project = npm_project_with(dir.path(), &[&carrier, &launcher]);
    let output = run(npm_bin(&project, "onebudgetspec"), &["--version"], &project);
    assert_eq!(output.status.code(), Some(69));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("onebudgetspec: cannot run"), "{stderr}");
    assert!(stderr.contains("reinstall @onebudgetspec/cli-"), "{stderr}");
}

// Unix only: a process ended by a signal exists only there; on Windows every process that
// ends has an exit code, which the launcher passes on.
#[cfg(unix)]
#[test]
fn a_binary_ended_by_a_signal_exits_70() {
    let launcher = artifact("npm-launcher");
    let dir = tempfile::tempdir().unwrap();
    let carrier = carrier_with(dir.path(), "#!/bin/sh\nkill -9 $$\n", true);
    let project = npm_project_with(dir.path(), &[&carrier, &launcher]);
    let output = run(npm_bin(&project, "onebudgetspec"), &["--version"], &project);
    assert_eq!(output.status.code(), Some(70));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("terminated by SIGKILL"), "{stderr}");
}

/// An installed launcher and host carrier, the carrier then damaged in place: its binary
/// removed, and its binary replaced by a link that escapes the package.
#[test]
fn the_installed_launcher_refuses_a_damaged_carrier() {
    let launcher = artifact("npm-launcher");
    let carrier = artifact("npm-carrier");
    let dir = tempfile::tempdir().unwrap();
    let project = npm_project_with(dir.path(), &[&carrier, &launcher]);
    let installed = npm_bin(&project, "onebudgetspec");
    let binary = project
        .join("node_modules/@onebudgetspec")
        .join(format!("cli-{}", host_platform()))
        .join("bin")
        .join(exe("onebudgetspec"));

    std::fs::remove_file(&binary).unwrap();
    let output = run(&installed, &["--version"], &project);
    assert_eq!(output.status.code(), Some(69));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not installed (ENOENT)"), "{stderr}");

    let outside = dir.path().join("outside");
    std::fs::write(&outside, "#!/bin/sh\necho hijacked\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &binary).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&outside, &binary)
        .expect("a symlink is created; on Windows that needs Developer Mode or an elevated user");
    let output = run(&installed, &["--version"], &project);
    assert_eq!(output.status.code(), Some(69));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("outside the package"), "{stderr}");
    assert!(output.stdout.is_empty(), "the escaping binary ran");
}
