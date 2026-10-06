"""Shared fixtures: the built binary and temporary copies of the repository."""

import os
import shutil
import subprocess
from pathlib import Path

import pytest

from repo_checks.paths import ROOT


@pytest.fixture(scope="session")
def binary() -> Path:
    """The cargo-built ``onebudgetspec``; Nx's repo-checks:test depends on that build."""
    path = ROOT / "target" / "debug" / ("onebudgetspec.exe" if os.name == "nt" else "onebudgetspec")
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


# Build outputs, and the files a test run writes and removes while other tests copy: a
# coverage data file listed by one copy can be gone before it is read.
TEST_RUN_ARTIFACTS = shutil.ignore_patterns(
    "node_modules",
    "dist",
    ".venv",
    "__pycache__",
    "target",
    ".coverage",
    ".coverage.*",
    ".pytest_cache",
    ".ruff_cache",
)


def copy_tree(into: Path, *entries: str, source_root: Path = ROOT) -> Path:
    """Copy ``entries`` (files or directories) of ``source_root`` into ``into``."""
    for entry in entries:
        source = source_root / entry
        target = into / entry
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_dir():
            shutil.copytree(source, target, ignore=TEST_RUN_ARTIFACTS)
        else:
            shutil.copy(source, target)
    return into
