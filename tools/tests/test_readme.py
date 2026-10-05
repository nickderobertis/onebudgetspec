"""The README's example budgets files are valid, as the built binary reads them."""

import subprocess
from pathlib import Path

from conftest import copy_tree

from repo_checks import readme


def test_the_nested_examples_validate(binary: Path, tmp_path: Path) -> None:
    files = readme.examples()
    assert "budgets.yaml" in files, "the README shows no root budgets.yaml"
    nested = [path for path in files if path.endswith("/budgets.yaml")]
    assert nested, "the README shows no project's own budgets.yaml"
    readme.lay_out(files, tmp_path)
    completed = subprocess.run(
        [binary, "validate", "--recursive", "--json"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr
    listed = subprocess.run(
        [binary, "list", "--recursive", "--json"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    for path in ["budgets.yaml", *nested]:
        assert f'"file":"{path}"' in listed.replace(
            " ", ""
        ) or f'"file":"./{path}"' in listed.replace(" ", "")


def test_a_broken_example_fails_validation(binary: Path, tmp_path: Path) -> None:
    root = copy_tree(tmp_path / "repo", "README.md")
    text = (root / "README.md").read_text().replace("    threshold: 2\n", "    threshold: -2\n", 1)
    (root / "README.md").write_text(text)
    readme.lay_out(readme.examples(root), tmp_path / "tree")
    completed = subprocess.run(
        [binary, "validate", "--recursive"],
        cwd=tmp_path / "tree",
        capture_output=True,
        text=True,
        check=False,
    )
    assert completed.returncode == 2


def test_a_missing_example_is_noticed(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, "README.md")
    text = (root / "README.md").read_text().replace('```yaml title="budgets.yaml"', "```yaml")
    (root / "README.md").write_text(text)
    assert "budgets.yaml" not in readme.examples(root)
