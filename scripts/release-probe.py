#!/usr/bin/env python3
"""What a public registry serves right now for one release target of this repository.

    usage: scripts/release-probe.py <registry>:<name>

Three answers, kept apart on purpose:

* exit 0 with one line on stdout: the version that registry serves now;
* exit 0 with nothing on stdout: the registry says it has no release of it;
* a non-zero exit with the reason on stderr: not answered (2 is a usage error).

A lookup that failed is never reported as "nothing published". It answers only for the
``[[target]]`` ids of release-targets.toml. Standard library only, so it runs on any host
with Python 3.11; one request, no retry. ONEBUDGETSPEC_PROBE_<REGISTRY>_URL points a
registry's base URL elsewhere, which is how the tests drive it against a local server.
"""

import json
import os
import re
import sys
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Literal, NoReturn

ROOT = Path(__file__).resolve().parents[1]
Registry = Literal["crate", "pypi", "npm"]
BASES: dict[Registry, str] = {
    "crate": "https://crates.io/api/v1/crates/",
    "pypi": "https://pypi.org/pypi/",
    "npm": "https://registry.npmjs.org/",
}
REGISTRIES: dict[str, Registry] = {"crate": "crate", "pypi": "pypi", "npm": "npm"}
AGENT = "onebudgetspec-release-probe (+https://github.com/nickderobertis/onebudgetspec)"
VERSION = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:[-+.][0-9A-Za-z.+-]+)?$")
LATER = "re-ask later; a registry that did not answer is not one with no release"


def refuse(reason: str, next_step: str, status: int = 1) -> NoReturn:
    """Exit without an answer, saying why and what to do."""
    print(f"release-probe: {reason}", file=sys.stderr)
    print(f"release-probe: next: {next_step}", file=sys.stderr)
    sys.exit(status)


def url(registry: Registry, name: str) -> str:
    """Where ``registry`` describes ``name``."""
    base = os.environ.get(f"ONEBUDGETSPEC_PROBE_{registry.upper()}_URL", BASES[registry])
    match registry:
        case "crate":
            return f"{base}{name}"
        case "pypi":
            return f"{base}{name}/json"
        case "npm":
            return base + urllib.parse.quote(name, safe="@")


def served(registry: Registry, document: object) -> object:
    """The version a registry document says is served; None when it serves none."""
    match registry, document:
        case "crate", {"crate": {"max_stable_version": version}}:
            return version
        case "pypi", {"info": {"version": version}}:
            return version
        case "npm", {"dist-tags": {"latest": version}}:
            return version
        case _:
            refuse(f"{registry}'s document has no version where one is expected", LATER)


def main(argv: list[str]) -> None:
    """Answer for ``argv[0]``."""
    match argv:
        case [identifier]:
            pass
        case _:
            refuse(
                "takes exactly one registry-qualified id",
                "run 'scripts/release-probe.py pypi:onebudgetspec-cli'",
                2,
            )
    targets = tomllib.loads((ROOT / "release-targets.toml").read_text())["target"]
    ids = sorted(target["id"] for target in targets)
    if identifier not in ids:
        refuse(
            f"{identifier} is not a target id of release-targets.toml", f"ask for one of {ids}", 2
        )
    prefix, _, name = identifier.partition(":")
    registry = REGISTRIES.get(prefix)
    if registry is None:
        refuse(
            f"release-targets.toml names {identifier} on no registry this probe reads", "fix its id"
        )
    request = urllib.request.Request(url(registry, name), headers={"User-Agent": AGENT})
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] one blocking stdlib request
        # is this short-lived command's whole job, keeping it runnable on any host with no
        # install; the answer is checked by pattern below.
        with urllib.request.urlopen(request, timeout=25) as response:
            document = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return
        refuse(f"{registry} answered HTTP {error.code} for {name}", LATER)
    except (OSError, ValueError) as error:
        refuse(f"{registry} could not be read for {name} ({error})", LATER)
    version = served(registry, document)
    if version is None:
        return
    if not isinstance(version, str) or not VERSION.match(version):
        refuse(f"{registry} serves {name} at {version!r}, which is not a version", LATER)
    print(version)


if __name__ == "__main__":
    main(sys.argv[1:])
