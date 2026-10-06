"""scripts/sync-release-pr.sh brings a release branch's other manifests and locks to its version.

The script runs from a copy of this repository whose `origin` is a local bare repository
holding a release branch bumped as release-plz bumps it: the Cargo manifests and lock only.
GitHub is the one stand-in, a `gh` that names that branch as the open release pull request's.
"""

import os
import re
import subprocess
import tarfile
from dataclasses import dataclass
from pathlib import Path

from repo_checks import versions
from repo_checks.paths import ROOT
from repo_checks.programs import bash

BRANCH = "release-plz-2026-01-01T00-00-00Z"
#: release-plz's bump of the workspace's own crates in Cargo.lock.
OWN_CRATE = re.compile(r'(?m)^(name = "onebudgetspec[^"]*"\nversion = ")[^"]*"')
#: Each workspace package bun.lock records, and the version it records for it.
LOCKED = re.compile(r'"name": "(@onebudgetspec/[^"]+)",\n\s*"version": "([^"]+)"')
#: The npm packages this repository publishes, each a bun workspace.
PACKAGES = ("cli", *(f"cli-{platform}" for platform in versions.PLATFORMS), "sdk")


def other_version(version: str) -> str:
    """A release version that is not ``version``: the next major after it."""
    other = f"{int(version.split('.')[0]) + 1}.0.0"
    assert other != version
    return other


@dataclass(frozen=True)
class Release:
    """A copy of the repository, its bare origin, and the version its release branch chose."""

    work: Path
    origin: Path
    env: dict[str, str]
    version: str


def git(cwd: Path, *args: str, env: dict[str, str]) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, env=env, capture_output=True, text=True, check=True
    ).stdout.strip()


def gh_naming(directory: Path, branch: str) -> Path:
    """A `gh` that answers `gh pr list` with ``branch`` as the open release PR's head."""
    directory.mkdir()
    gh = directory / "gh"
    gh.write_text(f'#!/bin/sh\nprintf "%s\\n" "{branch}"\n')
    gh.chmod(0o755)
    return directory


def bump_as_release_plz(work: Path, version: str) -> None:
    """Write ``version`` into the Cargo manifest and lock, and nowhere else."""
    versions.WORKSPACE.write(work, version)
    [pin] = [place for place in versions.places() if "onebudgetspec-core" in place.pattern]
    pin.write(work, version)
    lock = work / "Cargo.lock"
    bumped = OWN_CRATE.sub(lambda found: f'{found.group(1)}{version}"', lock.read_text())
    lock.write_text(bumped)


def release(tmp_path: Path, branch: str) -> Release:
    """This repository's tracked files at ``tmp_path/work``, pushed with a bumped release branch."""
    work, origin = tmp_path / "work", tmp_path / "origin.git"
    tracked = subprocess.run(
        ["git", "ls-files", "-z"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout
    for name in filter(None, tracked.split("\0")):
        source = ROOT / name
        if source.is_file():
            target = work / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(source.read_bytes())
            target.chmod(source.stat().st_mode)
    config = tmp_path / "gitconfig"
    config.write_text("[user]\n\tname = Test\n\temail = test@example.com\n")
    env = {
        **{key: value for key, value in os.environ.items() if key != "VIRTUAL_ENV"},
        "PATH": f"{gh_naming(tmp_path / 'gh', branch)}{os.pathsep}{os.environ['PATH']}",
        "GIT_CONFIG_GLOBAL": str(config),
        "GIT_CONFIG_NOSYSTEM": "1",
    }
    git(tmp_path, "init", "--quiet", "--bare", "--initial-branch=main", str(origin), env=env)
    git(work, "init", "--quiet", "--initial-branch=main", env=env)
    git(work, "add", "-A", env=env)
    git(work, "commit", "--quiet", "-m", "main", env=env)
    git(work, "remote", "add", "origin", str(origin), env=env)
    version = other_version(versions.workspace_version(work))
    git(work, "checkout", "--quiet", "-b", BRANCH, env=env)
    bump_as_release_plz(work, version)
    git(work, "commit", "--quiet", "-am", f"chore: release v{version}", env=env)
    git(work, "push", "--quiet", "origin", "main", BRANCH, env=env)
    git(work, "checkout", "--quiet", "main", env=env)
    return Release(work, origin, env, version)


def sync(release: Release) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [bash(), (release.work / "scripts/sync-release-pr.sh").as_posix()],
        cwd=release.work,
        env=release.env,
        capture_output=True,
        text=True,
        check=False,
    )


def packed_manifest(package: Path, out: Path) -> str:
    """The package.json bun packs for the workspace package at ``package``."""
    subprocess.run(
        ["bun", "pm", "pack", "--quiet", "--ignore-scripts", "--destination", str(out)],
        cwd=package,
        capture_output=True,
        check=True,
    )
    [tarball] = out.glob("*.tgz")
    with tarfile.open(tarball) as archive:
        member = archive.extractfile("package/package.json")
        assert member is not None
        return member.read().decode()


def test_the_release_branch_s_lock_and_packed_sdk_carry_its_version(tmp_path: Path) -> None:
    pushed = release(tmp_path, BRANCH)
    assert pushed.version != versions.workspace_version()

    synced = sync(pushed)
    assert synced.returncode == 0, synced.stderr
    head = git(pushed.origin, "rev-parse", BRANCH, env=pushed.env)
    assert git(pushed.work, "rev-parse", "HEAD", env=pushed.env) == head
    assert versions.disagreements(pushed.work) == []
    locked = dict(LOCKED.findall((pushed.work / "bun.lock").read_text()))
    assert locked == {f"@onebudgetspec/{name}": pushed.version for name in PACKAGES}

    manifest = packed_manifest(pushed.work / "sdks/typescript", tmp_path / "packed")
    assert f'"@onebudgetspec/cli": "{pushed.version}"' in manifest

    again = sync(pushed)
    assert again.returncode == 0, again.stderr
    assert git(pushed.origin, "rev-parse", BRANCH, env=pushed.env) == head


def test_no_open_release_pull_request_changes_nothing(tmp_path: Path) -> None:
    pushed = release(tmp_path, "")
    before = git(pushed.origin, "rev-parse", BRANCH, env=pushed.env)
    synced = sync(pushed)
    assert (synced.returncode, synced.stdout, synced.stderr) == (0, "", "")
    assert git(pushed.origin, "rev-parse", BRANCH, env=pushed.env) == before


def test_a_release_branch_gone_before_the_fetch_is_refused_and_nothing_is_pushed(
    tmp_path: Path,
) -> None:
    # gh lists an open release pull request whose branch the origin no longer holds.
    pushed = release(tmp_path, "release-plz-2026-01-02T00-00-00Z")
    before = git(pushed.origin, "rev-parse", BRANCH, env=pushed.env)
    synced = sync(pushed)
    assert synced.returncode != 0
    assert "the release PR is unchanged" in synced.stderr
    assert git(pushed.origin, "rev-parse", BRANCH, env=pushed.env) == before
