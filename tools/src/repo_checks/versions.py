"""Every manifest releases at the one workspace version, or the check names which does not.

``python -m repo_checks.versions check`` reports disagreement; ``set <version>`` writes the
version into every place this reads, the workspace's included, which the release pull
request runs. The minimum supported Rust version is held to one value the same way.
"""

import json
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

from repo_checks.paths import ROOT

PLATFORMS = ("linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64")
#: A release version (Semantic Versioning 2.0.0), matched whole; scripts/release-probe.py
#: carries the same pattern, which tests/test_versions.py holds identical.
VERSION = re.compile(
    r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:-(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


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

        def replace(match: re.Match[str]) -> str:
            return f"{match.group(1)}{version}{match.group(3)}"

        path.write_text(
            re.sub(self.pattern, replace, path.read_text(), count=1, flags=re.MULTILINE)
        )


#: `[workspace.package] version`, the first `version =` line of the root Cargo.toml.
WORKSPACE = Place("Cargo.toml", r'^(version = ")([^"]+)(")')


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
            Place(f"npm/platforms/{platform}/package.json", r'^(  "version": ")([^"]+)(")')
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
    carriers = launcher.get("optionalDependencies", {})
    expected = {f"@onebudgetspec/cli-{platform}": "workspace:*" for platform in PLATFORMS}
    if carriers != expected:
        problems.append(
            f"npm/cli/package.json: carriers {carriers}, expected {expected}, which packing "
            "writes as the release version"
        )
    rust = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["rust-version"]
    msrv = tomllib.loads((root / "clippy.toml").read_text()).get("msrv")
    if msrv != rust:
        problems.append(f"clippy.toml: msrv {msrv} (Cargo.toml's rust-version is {rust})")
    return problems


def set_version(version: str, root: Path = ROOT) -> None:
    """Write ``version`` as the workspace's and everywhere :func:`places` reads.

    Raises:
        ValueError: ``version`` is not a release version.
    """
    if not VERSION.fullmatch(version):
        raise ValueError(f"{version!r} is not a version such as 1.2.3")
    for place in [WORKSPACE, *places()]:
        place.write(root, version)


def main(argv: list[str], root: Path = ROOT) -> int:
    """``check`` or ``set <version>``; returns the exit status."""
    match argv:
        case ["check"]:
            problems = disagreements(root)
            for problem in problems:
                print(f"versions: {problem}", file=sys.stderr)
            if problems:
                print("versions: next: run 'just set-version <version>'", file=sys.stderr)
            return 1 if problems else 0
        case ["set", version]:
            try:
                set_version(version, root)
            except ValueError as error:
                print(f"versions: {error}; next: pass MAJOR.MINOR.PATCH", file=sys.stderr)
                return 64
            return 0
        case _:
            print("usage: python -m repo_checks.versions check | set <version>", file=sys.stderr)
            return 64


if __name__ == "__main__":  # pragma: no cover - the entry point; tests call main() itself
    sys.exit(main(sys.argv[1:]))
