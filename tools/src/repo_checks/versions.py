"""Every manifest releases at the one workspace version, or the check names which does not.

``python -m repo_checks.versions check`` reports disagreement; ``set <version>`` writes the
version into every place this reads, which the release pull request runs.
"""

import json
import re
import sys
import tomllib
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from repo_checks.paths import ROOT

PLATFORMS = ("linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64")


@dataclass(frozen=True)
class Place:
    """One place a version is written: a file and a pattern whose group 2 is the version."""

    path: str
    pattern: str

    def read(self, root: Path) -> str | None:
        """The version written here, or None when the pattern finds none."""
        match = re.search(self.pattern, (root / self.path).read_text(), re.MULTILINE)
        return match.group(2) if match else None

    def write(self, root: Path, version: str) -> None:
        """Write ``version`` here."""
        path = root / self.path
        replace: Callable[[re.Match[str]], str] = lambda m: f"{m.group(1)}{version}{m.group(3)}"  # noqa: E731
        path.write_text(
            re.sub(self.pattern, replace, path.read_text(), count=1, flags=re.MULTILINE)
        )


def places() -> list[Place]:
    """Every version this repository writes outside ``[workspace.package]``."""
    found = [
        Place("Cargo.toml", r'^(onebudgetspec-core = \{[^}]*version = "=)([^"]+)(")'),
        Place("pyproject.toml", r'^(version = ")([^"]+)(")'),
        Place("sdks/python/pyproject.toml", r'^(version = ")([^"]+)(")'),
        Place("sdks/python/src/onebudgetspec_sdk/__init__.py", r'^(__version__ = ")([^"]+)(")'),
        Place("sdks/typescript/package.json", r'^(  "version": ")([^"]+)(")'),
        Place("sdks/typescript/src/index.ts", r'^(export const VERSION = ")([^"]+)(")'),
        Place("npm/cli/package.json", r'^(  "version": ")([^"]+)(")'),
    ]
    for platform in PLATFORMS:
        found.append(
            Place("npm/platforms/" + platform + "/package.json", r'^(  "version": ")([^"]+)(")')
        )
        found.append(
            Place("npm/cli/package.json", rf'^(    "@onebudgetspec/cli-{platform}": ")([^"]+)(")')
        )
    return found


def workspace_version(root: Path = ROOT) -> str:
    """The ``[workspace.package] version`` every crate inherits."""
    return tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def disagreements(root: Path = ROOT) -> list[str]:
    """One line per place whose version is not the workspace's; empty when all agree."""
    version = workspace_version(root)
    problems = []
    for place in places():
        found = place.read(root)
        if found != version:
            problems.append(f"{place.path}: {found or 'no version found'} (workspace is {version})")
    launcher = json.loads((root / "npm/cli/package.json").read_text())
    carriers = sorted(launcher.get("optionalDependencies", {}))
    expected = sorted(f"@onebudgetspec/cli-{platform}" for platform in PLATFORMS)
    if carriers != expected:
        problems.append(f"npm/cli/package.json: carriers {carriers}, expected {expected}")
    return problems


def set_version(version: str, root: Path = ROOT) -> None:
    """Write ``version`` everywhere :func:`places` reads."""
    for place in places():
        place.write(root, version)


def main(argv: list[str], root: Path = ROOT) -> int:
    """``check`` or ``set <version>``; returns the exit status."""
    if argv == ["check"]:
        problems = disagreements(root)
        for problem in problems:
            print(f"versions: {problem}", file=sys.stderr)
        if problems:
            print(
                "versions: next: run 'python -m repo_checks.versions set <version>'",
                file=sys.stderr,
            )
        return 1 if problems else 0
    if len(argv) == 2 and argv[0] == "set":
        set_version(argv[1], root)
        return 0
    print("usage: python -m repo_checks.versions check | set <version>", file=sys.stderr)
    return 64


if __name__ == "__main__":  # pragma: no cover
    sys.exit(main(sys.argv[1:]))
