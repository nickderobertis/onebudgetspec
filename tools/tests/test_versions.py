"""Every manifest and SDK releases at the workspace version, and drift is named."""

import json
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest
from conftest import copy_tree

from repo_checks import versions
from repo_checks.paths import ROOT

FILES = (
    "Cargo.toml",
    "clippy.toml",
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


def test_each_sdk_s_pin_on_the_cli_is_held_to_the_workspace_version() -> None:
    version = versions.workspace_version()
    manifest = tomllib.loads((ROOT / "sdks/python/pyproject.toml").read_text())
    assert f"onebudgetspec-cli=={version}" in manifest["project"]["dependencies"]
    pins = [place for place in versions.places() if "onebudgetspec-cli==" in place.pattern]
    assert [(place.path, place.read(ROOT)) for place in pins] == [
        ("sdks/python/pyproject.toml", version)
    ]
    # The TypeScript SDK names the workspace's own launcher, which packing writes as the
    # release version (crates/onebudgetspec-packaging-e2e reads it from the tarball).
    package = json.loads((ROOT / "sdks/typescript/package.json").read_text())
    assert package["optionalDependencies"] == {"@onebudgetspec/cli": "workspace:*"}


@pytest.mark.parametrize(
    "change",
    [
        {"optionalDependencies": {"@onebudgetspec/cli": "0.0.9"}},
        {"optionalDependencies": {}, "dependencies": {"@onebudgetspec/cli": "workspace:*"}},
        {"dependencies": {"@onebudgetspec/cli": "workspace:*"}},
    ],
)
def test_a_typescript_sdk_pin_other_than_the_workspace_s_optional_launcher_is_named(
    tmp_path: Path, change: dict
) -> None:
    root = copy_tree(tmp_path, *FILES)
    manifest = root / "sdks/typescript/package.json"
    document = json.loads(manifest.read_text())
    document.update(change)
    manifest.write_text(json.dumps(document, indent=2) + "\n")
    [problem] = versions.disagreements(root)
    assert problem.startswith("sdks/typescript/package.json: optional dependencies")


def test_set_brings_every_place_and_the_workspace_to_one_version(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    assert versions.main(["set", "0.2.0"], root) == 0
    assert versions.workspace_version(root) == "0.2.0"
    assert versions.disagreements(root) == []
    assert all(place.read(root) == "0.2.0" for place in versions.places())


def test_set_refuses_what_is_not_a_version(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    root = copy_tree(tmp_path, *FILES)
    for bad in (
        "1.2",
        "v1.2.3",
        '1.2.3"\nevil = "x',
        "",
        "1.2.3\n",
        "01.2.3",
        "1.2.3-..",
        "1.2.3-01",
    ):
        assert versions.main(["set", bad], root) == 64
        assert "is not a version" in capsys.readouterr().err
    assert versions.disagreements(root) == []


def test_a_drifted_msrv_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    (root / "clippy.toml").write_text('msrv = "1.80"\n')
    assert versions.disagreements(root) == [
        "clippy.toml: msrv 1.80 (Cargo.toml's rust-version is 1.97)"
    ]


def test_a_missing_version_and_a_missing_carrier_are_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    sdk = root / "sdks/typescript/src/index.ts"
    sdk.write_text("export const NOTHING = 1;\n")
    launcher = root / "npm/cli/package.json"
    launcher.write_text(
        launcher.read_text().replace('    "@onebudgetspec/cli-darwin-x64": "workspace:*",\n', "")
    )
    problems = versions.disagreements(root)
    assert "sdks/typescript/src/index.ts: no version found (workspace is 0.1.0)" in problems
    assert any("carriers" in problem for problem in problems)


def test_set_refuses_a_place_with_no_version_field_and_writes_nothing(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    root = copy_tree(tmp_path, *FILES)
    (root / "sdks/typescript/src/index.ts").write_text("export const NOTHING = 1;\n")
    assert versions.main(["set", "0.2.0"], root) == 1
    err = capsys.readouterr().err
    assert "sdks/typescript/src/index.ts: no version found to write; nothing was written" in err
    assert versions.workspace_version(root) == "0.1.0"
    assert all(
        place.read(root) == "0.1.0"
        for place in versions.places()
        if place.path != "sdks/typescript/src/index.ts"
    )


@pytest.mark.parametrize("line", ['version = "banana"', "version = 7", ""])
def test_a_workspace_version_that_is_not_a_version_is_named(
    tmp_path: Path, line: str, capsys: pytest.CaptureFixture[str]
) -> None:
    root = copy_tree(tmp_path, *FILES)
    cargo = root / "Cargo.toml"
    cargo.write_text(cargo.read_text().replace('version = "0.1.0"\n', f"{line}\n", 1))
    [problem] = versions.disagreements(root)
    assert problem.startswith("Cargo.toml: workspace version ")
    assert problem.endswith("is not a version such as 1.2.3")
    assert versions.main(["check"], root) == 1
    assert "Cargo.toml: workspace version" in capsys.readouterr().err


def test_usage(capsys: pytest.CaptureFixture[str]) -> None:
    assert versions.main([]) == 64
    assert "usage" in capsys.readouterr().err


def test_the_probe_and_this_check_share_one_version_grammar() -> None:
    def grammar(path: Path) -> str:
        text = path.read_text()
        start = text.index("VERSION = re.compile(")
        return text[start : text.index("\n)\n", start)]

    assert grammar(ROOT / "scripts/release/release-probe.py") == grammar(
        ROOT / "tools/src/repo_checks/versions.py"
    )
    for good in ("0.1.0", "1.2.3-rc.1", "1.2.3+build.5", "10.20.30-alpha.beta"):
        assert versions.VERSION.fullmatch(good), good


def test_the_module_runs_as_a_command() -> None:
    def command(*args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, "-m", "repo_checks.versions", *args],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )

    assert command("check").returncode == 0
    refused = command("set", "not-a-version")
    assert refused.returncode == 64
    assert "is not a version such as 1.2.3" in refused.stderr
    usage = command()
    assert usage.returncode == 64
    assert "usage: python -m repo_checks.versions" in usage.stderr
