"""The package's version is the one its manifest releases."""

import tomllib
from pathlib import Path

import onebudgetspec_sdk


def test_version_matches_the_manifest() -> None:
    """``__version__`` is the ``[project] version`` of this package's pyproject.toml."""
    manifest = tomllib.loads((Path(__file__).parents[1] / "pyproject.toml").read_text())
    assert onebudgetspec_sdk.__version__ == manifest["project"]["version"]
