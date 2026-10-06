"""scripts/build-dist.sh names the artifact it just built, even beside earlier builds."""

import os
import subprocess
import time
from pathlib import Path

from repo_checks.paths import ROOT
from repo_checks.versions import workspace_version


def build(out: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(ROOT / "scripts/build-dist.sh"), "npm-launcher", str(out)],
        env=os.environ,
        capture_output=True,
        text=True,
        check=False,
    )


def test_the_new_artifact_is_named_and_earlier_ones_are_not(tmp_path: Path) -> None:
    out = tmp_path / "dist"
    out.mkdir()
    version = workspace_version()
    # A pre-release of the workspace's version precedes it, whatever the version is.
    earlier = f"{version}-rc.1"
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
    assert built.stdout == f"{same_name}\n"
    assert same_name.stat().st_size > len("an earlier build of this version")
    assert older.read_text() == "an earlier build"


def test_an_unknown_artifact_is_refused_with_the_usage(tmp_path: Path) -> None:
    refused = subprocess.run(
        ["bash", str(ROOT / "scripts/build-dist.sh"), "wheelbarrow", str(tmp_path)],
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
            "bash",
            str(ROOT / "scripts/build-dist.sh"),
            "cli-wheel",
            str(tmp_path),
            "x86_64-pc-windows-msvc",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert refused.returncode == 64
    assert "x86_64-pc-windows-msvc is not a Rust target the release ships" in refused.stderr
    assert list(tmp_path.iterdir()) == []
