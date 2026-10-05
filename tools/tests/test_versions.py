"""Every manifest and SDK releases at the workspace version, and drift is named."""

from pathlib import Path

import pytest
from conftest import copy_tree

from repo_checks import versions

FILES = (
    "Cargo.toml",
    "pyproject.toml",
    "sdks/python/pyproject.toml",
    "sdks/python/src/onebudgetspec_sdk/__init__.py",
    "sdks/typescript/package.json",
    "sdks/typescript/src/index.ts",
    "npm",
)


def test_the_tree_agrees() -> None:
    assert versions.disagreements() == []
    assert versions.main(["check"]) == 0


@pytest.mark.parametrize("place", range(len(versions.places())))
def test_each_place_that_drifts_is_named(
    tmp_path: Path, place: int, capsys: pytest.CaptureFixture[str]
) -> None:
    root = copy_tree(tmp_path, *FILES)
    chosen = versions.places()[place]
    chosen.write(root, "9.9.9")
    assert versions.disagreements(root) == [
        f"{chosen.path}: 9.9.9 (workspace is {versions.workspace_version()})"
    ]
    assert versions.main(["check"], root) == 1
    assert chosen.path in capsys.readouterr().err


def test_set_brings_every_place_to_one_version(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    cargo = root / "Cargo.toml"
    cargo.write_text(cargo.read_text().replace('version = "0.1.0"', 'version = "0.2.0"', 1))
    assert len(versions.disagreements(root)) == len(versions.places())
    assert versions.main(["set", "0.2.0"], root) == 0
    assert versions.disagreements(root) == []


def test_a_missing_version_and_a_missing_carrier_are_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    sdk = root / "sdks/typescript/src/index.ts"
    sdk.write_text("export const NOTHING = 1;\n")
    launcher = root / "npm/cli/package.json"
    launcher.write_text(
        launcher.read_text().replace('    "@onebudgetspec/cli-darwin-x64": "0.1.0",\n', "")
    )
    problems = versions.disagreements(root)
    assert "sdks/typescript/src/index.ts: no version found (workspace is 0.1.0)" in problems
    assert any("carriers" in problem for problem in problems)


def test_usage(capsys: pytest.CaptureFixture[str]) -> None:
    assert versions.main([]) == 64
    assert "usage" in capsys.readouterr().err
