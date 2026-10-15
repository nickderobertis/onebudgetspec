"""measure.sh, checked as its budget: one timed pass over one run's superseded builds.

Each test lays out a one-crate cargo workspace holding this repository's own justfile, cargo
configuration, toolchain pin, budgets file and measure.sh, and checks that budgets file with
the onebudgetspec this workspace builds, so the declared `just pool-maintenance` recipe and
the installed pruner do the work. The crate builds in seconds; the repository's own
measurement, which builds this workspace twice, runs only on the release pull request.
"""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest
from repo_checks.paths import ROOT

# The budget names its script directly, by its `#!` line, which Windows does not read, and
# pool maintenance runs on the Linux and macOS hosts that keep warm slots.
pytestmark = pytest.mark.skipif(
    sys.platform == "win32", reason="the budget runs measure.sh by its #! line"
)

BINARY = ROOT / "target/debug/onebudgetspec"
CRATE = "fixture_crate"
LIB = """pub fn answer() -> u32 {
    42
}

#[cfg(test)]
mod tests {
    #[test]
    fn answers() {
        assert_eq!(super::answer(), 42);
    }
}
"""


def entries(root: Path) -> set[str]:
    """Every entry of the profile's build containers, as measure.sh lists them."""
    profile = root / "target/debug"
    return {
        f"{container}/{entry.name}"
        for container in ("deps", ".fingerprint", "build", "incremental")
        if (profile / container).is_dir()
        for entry in (profile / container).iterdir()
    }


def cargo(root: Path, *args: str) -> None:
    subprocess.run(["cargo", *args], cwd=root, check=True, capture_output=True)


@pytest.fixture
def workspace(tmp_path: Path) -> Path:
    for name in ("justfile", "rust-toolchain.toml", ".cargo/config.toml"):
        (tmp_path / name).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / name, tmp_path / name)
    shutil.copytree(
        ROOT / "pool-maintenance",
        tmp_path / "pool-maintenance",
        ignore=shutil.ignore_patterns("tests"),
    )
    (tmp_path / "Cargo.toml").write_text(f'[workspace]\nmembers = ["{CRATE}"]\nresolver = "2"\n')
    (tmp_path / CRATE / "src").mkdir(parents=True)
    (tmp_path / CRATE / "Cargo.toml").write_text(
        f'[package]\nname = "{CRATE}"\nversion = "0.1.0"\nedition = "2021"\n'
    )
    (tmp_path / CRATE / "src/lib.rs").write_text(LIB)
    cargo(tmp_path, "generate-lockfile", "--offline")
    return tmp_path


def check(root: Path, env: dict[str, str]) -> tuple[int, dict]:
    run = subprocess.run(
        [str(BINARY), "check", "--json", "pool-maintenance/budgets.yaml"],
        cwd=root,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    return run.returncode, json.loads(run.stdout)


def environment(**extra: str) -> dict[str, str]:
    env = {k: v for k, v in os.environ.items() if not k.startswith("CARGO_TARGET_DIR")}
    return env | extra


def test_the_timed_pass_removes_exactly_the_superseded_build(
    workspace: Path, tmp_path_factory: pytest.TempPathFactory
) -> None:
    # A backlog: an older configuration's build, past the window and already superseded.
    cargo(workspace, "test", "--no-run", "--locked")
    ordinary = entries(workspace)
    cargo(
        workspace,
        "test",
        "--no-run",
        "--locked",
        "--config",
        f"profile.dev.package.{CRATE}.opt-level=1",
    )
    backlog = entries(workspace) - ordinary
    assert backlog
    for entry in backlog:
        for path in [
            workspace / "target/debug" / entry,
            *(workspace / "target/debug" / entry).rglob("*"),
        ]:
            os.utime(path, (946_684_800, 946_684_800), follow_symlinks=False)
    evidence = tmp_path_factory.mktemp("evidence")

    status, report = check(workspace, environment(MEASURE_EVIDENCE_DIR=str(evidence)))

    [result] = report["results"]
    # A figure was reported; whether it is within the budget is the check's to judge.
    assert (status, result["verdict"]) in ((0, "within"), (1, "over")), result.get("error")
    assert "untimed backlog pass seconds (not in the figure): " in result["detail"]
    removed = set((evidence / "removed").read_text().split())
    alternative = set((evidence / "alternative").read_text().split())
    built = set((evidence / "built").read_text().split())
    # The backlog went in the untimed pass, so the timed one removed only the alternative
    # build, every unit of it this repository's crate, and kept every ordinary entry.
    assert backlog.isdisjoint(set((evidence / "before").read_text().split()))
    assert removed == alternative
    assert {entry.split("/", 1)[1].removeprefix("lib").split("-")[0] for entry in removed} == {
        CRATE
    }
    assert "builds removed: 0;" not in result["detail"]
    assert entries(workspace) == built - removed
    assert ordinary <= entries(workspace)


def test_a_missing_pruner_is_an_error_naming_the_fix(workspace: Path, tmp_path: Path) -> None:
    # PATH keeps what the script runs, but not the pruner beside cargo.
    shims = tmp_path / "shims"
    shims.mkdir()
    for name in ("cargo", "rustc", "rustup", "just", "node"):
        found = shutil.which(name)
        if found:
            (shims / name).symlink_to(found)
    env = environment(PATH=os.pathsep.join([str(shims), "/usr/bin", "/bin"]))

    status, report = check(workspace, env)

    [result] = report["results"]
    assert (status, result["verdict"]) == (3, "error")
    assert "the untimed backlog pass failed" in result["error"]
    assert "run 'just bootstrap'" in result["error"]
