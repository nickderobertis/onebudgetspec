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
from datetime import UTC, datetime, timedelta
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


def vendor_dependency(root: Path) -> None:
    """Give the fixture crate a registry-style dependency that uses a proc macro, like clap."""
    (root / "vendor/macro_dep/src").mkdir(parents=True)
    (root / "vendor/macro_dep/Cargo.toml").write_text(
        '[package]\nname = "macro_dep"\nversion = "1.0.0"\nedition = "2021"\n'
        "[lib]\nproc-macro = true\n"
    )
    (root / "vendor/macro_dep/src/lib.rs").write_text(
        "use proc_macro::TokenStream;\n"
        "#[proc_macro]\n"
        'pub fn seven(_: TokenStream) -> TokenStream { "7".parse().unwrap() }\n'
    )
    (root / "vendor/facade/src").mkdir(parents=True)
    (root / "vendor/facade/Cargo.toml").write_text(
        '[package]\nname = "facade"\nversion = "1.0.0"\nedition = "2021"\n'
        '[dependencies]\nmacro_dep = "1"\n'
    )
    (root / "vendor/facade/src/lib.rs").write_text(
        "pub fn seven() -> u32 { macro_dep::seven!() }\n"
    )
    for name in ("macro_dep", "facade"):
        (root / "vendor" / name / ".cargo-checksum.json").write_text(
            '{"files": {}, "package": null}'
        )
    with (root / ".cargo/config.toml").open("a") as config:
        config.write(
            '\n[source.crates-io]\nreplace-with = "vendored"\n'
            '[source.vendored]\ndirectory = "vendor"\n'
        )
    with (root / CRATE / "Cargo.toml").open("a") as manifest:
        manifest.write('\n[dependencies]\nfacade = "1"\n')
    cargo(root, "generate-lockfile", "--offline")


def backdate(root: Path, names: set[str], age: timedelta) -> None:
    moment = (datetime.now(UTC) - age).timestamp()
    for entry in names:
        path = root / "target/debug" / entry
        for item in [path, *path.rglob("*")]:
            os.utime(item, (moment, moment), follow_symlinks=False)


def test_a_lint_build_newer_than_the_test_build_still_gives_a_figure(
    workspace: Path, tmp_path_factory: pytest.TempPathFactory
) -> None:
    # The gate's restored cache past the window: a lint build of each dependency newer than
    # its test build, so a backlog pass removes that dependency from the ordinary build.
    vendor_dependency(workspace)
    cargo(workspace, "test", "--no-run", "--locked")
    tests = entries(workspace)
    cargo(workspace, "check", "--all-targets", "--locked")
    lints = entries(workspace) - tests
    assert any(entry.startswith(".fingerprint/facade-") for entry in lints)
    backdate(workspace, tests, timedelta(days=3))
    backdate(workspace, lints, timedelta(days=2, hours=3))
    evidence = tmp_path_factory.mktemp("evidence")

    status, report = check(workspace, environment(MEASURE_EVIDENCE_DIR=str(evidence)))

    [result] = report["results"]
    assert (status, result["verdict"]) in ((0, "within"), (1, "over")), result.get("error")
    # The figure is the stated workload: this repository's crate alone, however much the
    # backlog held, with the whole ordinary build still in place.
    removed = set((evidence / "removed").read_text().split())
    assert removed == set((evidence / "alternative").read_text().split())
    assert {entry.split("/", 1)[1].removeprefix("lib").split("-")[0] for entry in removed} == {
        CRATE
    }
    assert tests <= entries(workspace)
    assert lints.isdisjoint(entries(workspace))


def test_a_timed_pass_removing_an_entry_of_the_current_build_is_refused(
    workspace: Path, tmp_path: Path, tmp_path_factory: pytest.TempPathFactory
) -> None:
    # The real pruner, then in the timed pass alone (the only one after the alternative build
    # backdated its entries past this script) the ordinary build of the crate goes as well.
    pruner = shutil.which("cargo-prune-superseded")
    assert pruner, "run 'just bootstrap'"
    shims = tmp_path / "shims"
    shims.mkdir()
    shim = shims / "cargo-prune-superseded"
    shim.write_text(
        "#!/bin/sh\n"
        'units() { find target/debug/.fingerprint -mindepth 1 -maxdepth 1 "$@"; }\n'
        'if [ -n "$(units ! -newer "$0")" ]; then\n'
        f'  units -name "{CRATE}-*" -newer "$0" -exec rm -rf {{}} +\n'
        "fi\n"
        f'exec {pruner} "$@"\n'
    )
    shim.chmod(0o755)
    evidence = tmp_path_factory.mktemp("evidence")
    env = environment(
        MEASURE_EVIDENCE_DIR=str(evidence),
        PATH=os.pathsep.join([str(shims), os.environ["PATH"]]),
    )

    status, report = check(workspace, env)

    [result] = report["results"]
    assert (status, result["verdict"]) == (3, "error")
    assert "the timed pass removed entries the alternative build did not create" in result["error"]
    removed = set((evidence / "removed").read_text().split())
    assert removed & set((evidence / "ordinary").read_text().split())


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
