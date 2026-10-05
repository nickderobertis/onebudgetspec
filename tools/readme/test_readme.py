"""The README's example budgets files are valid, as the built binary reads them."""

import subprocess
from pathlib import Path

import examples as readme
import pytest
from examples import ROOT


@pytest.fixture(scope="session")
def binary() -> Path:
    """The cargo-built onebudgetspec; Nx's readme-examples:test depends on that build."""
    path = ROOT / "target" / "debug" / "onebudgetspec"
    assert path.is_file(), f"{path} is missing; build it with `cargo build -p onebudgetspec`"
    return path


def copy_tree(into: Path, entry: str) -> Path:
    """Copy one file of the repository into ``into``."""
    (into / entry).parent.mkdir(parents=True, exist_ok=True)
    (into / entry).write_text((ROOT / entry).read_text())
    return into


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


def test_an_example_path_that_leaves_the_tree_is_refused(tmp_path: Path) -> None:
    for path in ("../escape.yaml", "/etc/budgets.yaml", "a/../../b.yaml"):
        with pytest.raises(ValueError, match="leaves the example tree"):
            readme.lay_out({path: "x"}, tmp_path / "tree")
    assert not (tmp_path / "escape.yaml").exists()
