"""The project graph's edges follow the allowed directions, and Cargo's edges are in it."""

import json
import os
import subprocess
from pathlib import Path

import pytest
from conftest import copy_tree

from repo_checks import graph
from repo_checks.paths import ROOT


def test_the_graph_holds_to_its_boundaries() -> None:
    assert graph.problems() == []
    assert {"onebudgetspec-core", "onebudgetspec", "npm-cli", "repo-checks"} <= set(
        graph.projects()
    )


def _edit(root: Path, project: str, change: dict) -> None:
    path = root / project / "project.json"
    document = json.loads(path.read_text())
    document.update(change)
    path.write_text(json.dumps(document))


def test_an_edge_back_into_the_contract_is_refused(tmp_path: Path) -> None:
    root = copy_tree(
        tmp_path, "crates", "sdks/python/project.json", "npm", "conformance/project.json"
    )
    _edit(root, "crates/onebudgetspec-core", {"implicitDependencies": ["onebudgetspec"]})
    assert (
        "onebudgetspec-core (contract) may not depend on onebudgetspec (binary)"
        in graph.problems(root)
    )


def test_an_untyped_project_an_unknown_edge_and_a_missing_cargo_edge_are_refused(
    tmp_path: Path,
) -> None:
    root = copy_tree(
        tmp_path, "crates", "sdks/python/project.json", "npm", "conformance/project.json"
    )
    _edit(root, "sdks/python", {"tags": ["lang:python"], "implicitDependencies": ["nowhere"]})
    _edit(root, "crates/onebudgetspec", {"implicitDependencies": []})
    found = graph.problems(root)
    assert any(problem.startswith("sdk-python: needs exactly one type tag") for problem in found), (
        found
    )
    assert "sdk-python: depends on nowhere, which is not a project" in found
    assert (
        "onebudgetspec: Cargo depends on ['onebudgetspec-core'], which its project.json omits"
        in found
    )


def test_a_malformed_or_repeated_project_is_refused(tmp_path: Path) -> None:
    (tmp_path / "a").mkdir()
    (tmp_path / "a/project.json").write_text(json.dumps({"name": "x", "tags": "type:sdk"}))
    with pytest.raises(graph.InvalidProject, match="`tags` is not a list of strings"):
        graph.projects(tmp_path)
    (tmp_path / "a/project.json").write_text(json.dumps({"tags": []}))
    with pytest.raises(graph.InvalidProject, match="no string `name`"):
        graph.projects(tmp_path)
    (tmp_path / "a/project.json").write_text(json.dumps({"name": "x", "tags": ["type:sdk"]}))
    (tmp_path / "b").mkdir()
    (tmp_path / "b/project.json").write_text(json.dumps({"name": "x", "tags": ["type:sdk"]}))
    with pytest.raises(graph.InvalidProject, match="repeats the project name x"):
        graph.projects(tmp_path)


def test_an_sdk_may_depend_on_the_binary_and_the_contract_and_nothing_further(
    tmp_path: Path,
) -> None:
    assert graph.ALLOWED["sdk"] == {"binary", "contract"}
    sdks = {"sdk-python", "sdk-typescript"}
    for name in sdks:
        assert set(graph.projects()[graph.ProjectName(name)].dependencies) == {
            "onebudgetspec",
            "conformance",
        }
    root = copy_tree(
        tmp_path,
        "crates",
        "sdks/python/project.json",
        "sdks/typescript/project.json",
        "npm",
        "conformance/project.json",
    )
    assert graph.problems(root) == []
    _edit(root, "sdks/python", {"implicitDependencies": ["onebudgetspec", "npm-cli"]})
    assert graph.problems(root) == ["sdk-python (sdk) may not depend on npm-cli (distribution)"]
    _edit(root, "sdks/python", {"implicitDependencies": ["onebudgetspec-e2e"]})
    assert graph.problems(root) == ["sdk-python (sdk) may not depend on onebudgetspec-e2e (e2e)"]


@pytest.mark.parametrize(
    "changed",
    ["crates/onebudgetspec-core/src/report.rs", "conformance/cases/selection/case.json"],
)
def test_a_contract_or_case_change_selects_both_sdks(changed: str) -> None:
    affected = subprocess.run(
        [
            str(ROOT / "node_modules/.bin/nx"),
            "show",
            "projects",
            "--affected",
            f"--files={changed}",
            "--json",
        ],
        cwd=ROOT,
        env={**os.environ, "NX_DAEMON": "false"},
        capture_output=True,
        text=True,
        check=True,
    )
    assert {"sdk-python", "sdk-typescript"} <= set(json.loads(affected.stdout)), affected.stdout
