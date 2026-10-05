"""scripts/ci-base.sh picks the commit affected selection compares against, over real git."""

import os
import subprocess
from pathlib import Path
from typing import NamedTuple

import pytest
from repo_checks.paths import ROOT

SCRIPT = ROOT / "scripts/ci-base.sh"


def git(cwd: Path, *args: str) -> str:
    env = {
        **os.environ,
        "GIT_AUTHOR_NAME": "t",
        "GIT_AUTHOR_EMAIL": "t@t",
        "GIT_COMMITTER_NAME": "t",
        "GIT_COMMITTER_EMAIL": "t@t",
    }
    return subprocess.run(
        ["git", *args], cwd=cwd, env=env, capture_output=True, text=True, check=True
    ).stdout.strip()


def commit(cwd: Path, name: str) -> str:
    (cwd / name).write_text(name)
    git(cwd, "add", name)
    git(cwd, "commit", "-q", "-m", name)
    return git(cwd, "rev-parse", "HEAD")


class Derived(NamedTuple):
    """What one run of ci-base.sh did: the process, and what it wrote to GITHUB_OUTPUT."""

    completed: subprocess.CompletedProcess[str]
    output: str


def base(cwd: Path, tmp_path: Path, **env: str) -> Derived:
    output = tmp_path / "github-output"
    output.write_text("")
    completed = subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=cwd,
        env={
            "PATH": os.environ["PATH"],
            "HOME": str(tmp_path),
            "GITHUB_OUTPUT": str(output),
            **env,
        },
        capture_output=True,
        text=True,
        check=False,
    )
    return Derived(completed, output.read_text())


@pytest.fixture
def clone(tmp_path: Path) -> Path:
    """A clone of a repository whose main has two commits, on a branch with two more."""
    origin = tmp_path / "origin"
    origin.mkdir()
    git(origin, "init", "-q", "-b", "main")
    commit(origin, "a")
    commit(origin, "b")
    work = tmp_path / "work"
    git(tmp_path, "clone", "-q", str(origin), str(work))
    git(work, "checkout", "-q", "-b", "feature")
    commit(work, "c")
    commit(work, "d")
    commit(origin, "e")  # main moves on after the branch forked
    return work


def test_a_pull_request_compares_against_its_merge_base(clone: Path, tmp_path: Path) -> None:
    forked = git(clone, "rev-parse", "HEAD~2")
    git(clone, "fetch", "-q", "origin", "main")
    main = git(clone, "rev-parse", "origin/main")
    completed, output = base(clone, tmp_path, BASE_REF="main", BASE_SHA=main)
    assert completed.returncode == 0, completed.stderr
    assert completed.stdout.strip() == forked
    assert output == f"base={forked}\n"


def test_a_push_compares_against_the_commit_it_replaced(clone: Path, tmp_path: Path) -> None:
    before = git(clone, "rev-parse", "HEAD~2")
    completed, output = base(clone, tmp_path, BEFORE_SHA=before)
    assert (completed.returncode, output) == (0, f"base={before}\n")


def test_an_unreadable_predecessor_falls_back_to_the_parent(clone: Path, tmp_path: Path) -> None:
    parent = git(clone, "rev-parse", "HEAD~1")
    for before in ("0" * 40, ""):
        completed, output = base(clone, tmp_path, BEFORE_SHA=before)
        assert (completed.returncode, output) == (0, f"base={parent}\n")


def test_no_derivable_base_and_a_half_given_pull_request_are_refused(tmp_path: Path) -> None:
    lone = tmp_path / "lone"
    lone.mkdir()
    git(lone, "init", "-q", "-b", "main")
    commit(lone, "only")
    completed, output = base(lone, tmp_path)
    assert completed.returncode == 1
    assert "no base can be derived" in completed.stderr
    assert output == ""
    completed, _ = base(lone, tmp_path, BASE_SHA="abc")
    assert completed.returncode == 64
    assert "without BASE_REF" in completed.stderr
