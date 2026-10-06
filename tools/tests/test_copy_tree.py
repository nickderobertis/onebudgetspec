"""The tests' repository copies never read the files a concurrent test run writes and removes."""

from pathlib import Path

from conftest import copy_tree


def test_a_copy_leaves_out_coverage_data_and_test_caches(tmp_path: Path) -> None:
    source = tmp_path / "repo"
    tests = source / "scripts/release/tests"
    tests.mkdir(parents=True)
    (tests / "test_release.py").write_text("")
    for artifact in (".coverage", ".coverage.runnervm8df0l.pid10995.XZ0YeVNx"):
        (tests / artifact).write_text("")
    for cache in (".pytest_cache", ".ruff_cache"):
        (tests / cache).mkdir()

    copied = copy_tree(tmp_path / "copy", "scripts", source_root=source)

    assert sorted(p.name for p in (copied / "scripts/release/tests").iterdir()) == [
        "test_release.py"
    ]
