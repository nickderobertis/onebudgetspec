#!/usr/bin/env python3
"""What a public registry serves right now for one release target of this repository.

    usage: scripts/release-probe.py <registry>:<name>

Three answers, kept apart on purpose:

* exit 0 with one line on stdout: the version that registry serves now;
* exit 0 with nothing on stdout: the registry says it has no release of it;
* a non-zero exit with the reason on stderr: not answered (2 is a usage error).

A lookup that failed is never reported as "nothing published". It answers only for the
``[[target]]`` ids of release-targets.toml. Standard library only; one request, no retry.
The registry base URLs can be pointed elsewhere with ONEBUDGETSPEC_PROBE_<REGISTRY>_URL,
which is how the tests drive it against a local server.
"""

import json
import os
import sys
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASES = {
    "crate": "https://crates.io/api/v1/crates/",
    "pypi": "https://pypi.org/pypi/",
    "npm": "https://registry.npmjs.org/",
}
AGENT = "onebudgetspec-release-probe (+https://github.com/nickderobertis/onebudgetspec)"


def refuse(reason: str, status: int = 1) -> None:
    """Exit without an answer, saying why."""
    print(f"release-probe: {reason}", file=sys.stderr)
    sys.exit(status)


def url(registry: str, name: str) -> str:
    """Where ``registry`` describes ``name``."""
    base = os.environ.get(f"ONEBUDGETSPEC_PROBE_{registry.upper()}_URL", BASES[registry])
    if registry == "crate":
        return f"{base}{name}"
    if registry == "pypi":
        return f"{base}{name}/json"
    return base + urllib.parse.quote(name, safe="@")


def served(registry: str, document: dict) -> str | None:
    """The version a registry document says is served, or None for none."""
    if registry == "crate":
        return document["crate"]["max_stable_version"]
    if registry == "pypi":
        return document["info"]["version"]
    return document["dist-tags"]["latest"]


def main(argv: list[str]) -> None:
    """Answer for ``argv[0]``."""
    if len(argv) != 1:
        refuse("takes exactly one registry-qualified id, e.g. pypi:onebudgetspec-cli", 2)
    identifier = argv[0]
    targets = tomllib.loads((ROOT / "release-targets.toml").read_text())["target"]
    if identifier not in {target["id"] for target in targets}:
        refuse(f"{identifier} is not a target id of release-targets.toml", 2)
    registry, _, name = identifier.partition(":")
    request = urllib.request.Request(url(registry, name), headers={"User-Agent": AGENT})
    try:
        with urllib.request.urlopen(request, timeout=25) as response:
            document = json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return
        refuse(f"{registry} answered HTTP {error.code} for {name}; re-ask later")
    except (OSError, ValueError) as error:
        refuse(f"{registry} could not be read for {name} ({error}); re-ask later")
    try:
        version = served(registry, document)
    except (KeyError, TypeError):
        refuse(f"{registry}'s document for {name} has no version where one is expected")
    if version is not None:
        print(version)


if __name__ == "__main__":
    main(sys.argv[1:])
