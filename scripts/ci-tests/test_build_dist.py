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
