"""scripts/build-dist.sh names the artifact it just built, even beside earlier builds."""

import os
import re
import subprocess
import time
from pathlib import Path

from repo_checks.paths import ROOT
from repo_checks.programs import bash
from repo_checks.versions import workspace_version

#: The script, as bash on every platform reads a path.
BUILD_DIST = (ROOT / "scripts/build-dist.sh").as_posix()


def build(out: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [bash(), BUILD_DIST, "npm-launcher", str(out)],
        env=os.environ,
        capture_output=True,
        text=True,
        check=False,
    )


def test_the_new_artifact_is_named_and_earlier_ones_are_not(tmp_path: Path) -> None:
    out = tmp_path / "dist"
    out.mkdir()
    version = workspace_version()
    # `-0` is the lowest pre-release of the version's own release, so it precedes the version.
    earlier = f"{re.split('[-+]', version)[0]}-0"
    assert earlier != version
    older = out / f"onebudgetspec-cli-{earlier}.tgz"
    older.write_text("an earlier build")
    same_name = out / f"onebudgetspec-cli-{version}.tgz"
    same_name.write_text("an earlier build of this version")
    past = time.time() - 60
    for stale in (older, same_name):
        os.utime(stale, (past, past))

    built = build(out)
    assert built.returncode == 0, built.stderr
    # One line naming the file; on Windows the path is written as bash writes it.
    assert built.stdout.count("\n") == 1
    assert Path(built.stdout.strip()).samefile(same_name)
    assert same_name.stat().st_size > len("an earlier build of this version")
    assert older.read_text() == "an earlier build"


def test_an_unknown_artifact_is_refused_with_the_usage(tmp_path: Path) -> None:
    refused = subprocess.run(
        [bash(), BUILD_DIST, "wheelbarrow", str(tmp_path)],
        capture_output=True,
        text=True,
        check=False,
    )
    assert refused.returncode == 64
    assert "unknown artifact 'wheelbarrow'" in refused.stderr
    assert "usage: scripts/build-dist.sh" in refused.stderr


def test_a_wheel_target_the_release_does_not_ship_is_refused(tmp_path: Path) -> None:
    refused = subprocess.run(
        [
            bash(),
            BUILD_DIST,
            "cli-wheel",
            str(tmp_path),
            "i686-pc-windows-msvc",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert refused.returncode == 64
    assert "i686-pc-windows-msvc is not a Rust target the release ships" in refused.stderr
    assert list(tmp_path.iterdir()) == []


def test_an_intel_macos_target_is_refused_as_no_longer_supported(tmp_path: Path) -> None:
    for artifact in ("cli-wheel", "npm-carrier"):
        refused = subprocess.run(
            [bash(), BUILD_DIST, artifact, str(tmp_path), "x86_64-apple-darwin"],
            capture_output=True,
            text=True,
            check=False,
        )
        assert refused.returncode == 64
        assert "Intel macOS (x86_64-apple-darwin) is no longer supported" in refused.stderr
        assert "aarch64-apple-darwin" in refused.stderr
        assert list(tmp_path.iterdir()) == []


def test_a_missing_or_unrelated_precompiled_carrier_binary_is_refused(tmp_path: Path) -> None:
    """The carrier accepts only bytes from its target's compiled CLI."""
    for candidate in (tmp_path / "missing", tmp_path / "unrelated"):
        if candidate.name == "unrelated":
            candidate.write_text("an ordinary file, not the compiled CLI")
            candidate.chmod(0o755)
        refused = subprocess.run(
            [bash(), BUILD_DIST, "npm-carrier", str(tmp_path / "out"), "x86_64-unknown-linux-gnu"],
            env={**os.environ, "ONEBUDGETSPEC_BINARY": str(candidate)},
            capture_output=True,
            text=True,
            check=False,
        )
        assert refused.returncode == 64
        assert str(candidate) in refused.stderr
        assert "must match the compiled CLI" in refused.stderr
        assert "unset ONEBUDGETSPEC_BINARY" in refused.stderr
        assert list((tmp_path / "out").iterdir()) == []
