"""Shared fixtures: the built binary and temporary copies of the repository."""

import shutil
import subprocess
from pathlib import Path

import pytest

from repo_checks.paths import ROOT


@pytest.fixture(scope="session")
def binary() -> Path:
    """The cargo-built ``onebudgetspec``; Nx's repo-checks:test depends on that build."""
    path = ROOT / "target" / "debug" / "onebudgetspec"
    assert path.is_file(), f"{path} is missing; build it with `cargo build -p onebudgetspec`"
    return path


@pytest.fixture(scope="session")
def schema(binary: Path) -> str:
    """The schema bundle the binary emits."""
    return subprocess.run([binary, "schema"], capture_output=True, text=True, check=True).stdout


def other_version(version: str) -> str:
    """A release version that is not ``version``: the next major after it."""
    other = f"{int(version.split('.')[0]) + 1}.0.0"
    assert other != version
    return other


def copy_tree(into: Path, *entries: str) -> Path:
    """Copy ``entries`` (files or directories) of the repository into ``into``."""
    for entry in entries:
        source = ROOT / entry
        target = into / entry
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_dir():
            shutil.copytree(
                source,
                target,
                ignore=shutil.ignore_patterns(
                    "node_modules", "dist", ".venv", "__pycache__", "target"
                ),
            )
        else:
            shutil.copy(source, target)
    return into
