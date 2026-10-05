"""AGENTS.md lists exactly the journey files on disk, and a difference either way fails."""

from pathlib import Path

from conftest import copy_tree

from repo_checks import journeys


def test_agents_md_lists_every_journey_file() -> None:
    assert journeys.differences() == []
    assert journeys.listed()


def test_an_unlisted_and_a_missing_journey_are_both_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, "AGENTS.md", *journeys.E2E_CRATES)
    added = root / "crates/onebudgetspec-e2e/tests/journeys/new_journey.rs"
    added.write_text("")
    removed = sorted(journeys.on_disk(root))[0]
    (root / removed).unlink()
    new = "crates/onebudgetspec-e2e/tests/journeys/new_journey.rs"
    assert journeys.differences(root) == [
        f"AGENTS.md lists {removed}, which does not exist",
        f"{new} is a journey file AGENTS.md does not list",
    ]
