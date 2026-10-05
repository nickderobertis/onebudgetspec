"""The project graph's edges follow the allowed directions, and Cargo's edges are in it."""

import json
from pathlib import Path

import pytest
from conftest import copy_tree

from repo_checks import graph


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
