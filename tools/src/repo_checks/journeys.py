"""AGENTS.md lists exactly the journey files the end-to-end tests hold."""

import re
from pathlib import Path

from repo_checks.paths import ROOT

#: The end-to-end crates; every `.rs` under their `tests/` is a journey file, except the
#: test binary's root and its shared helpers.
E2E_CRATES = ("crates/onebudgetspec-e2e", "crates/onebudgetspec-packaging-e2e")
NOT_JOURNEYS = {"main.rs", "common.rs"}
_ENTRY = re.compile(r"^- `([^`]+\.rs)` — \S")


def on_disk(root: Path = ROOT) -> set[str]:
    """The journey files under the end-to-end tests, relative to ``root``."""
    return {
        path.relative_to(root).as_posix()
        for crate in E2E_CRATES
        for path in (root / crate / "tests").rglob("*.rs")
        if path.name not in NOT_JOURNEYS
    }


def listed(root: Path = ROOT) -> set[str]:
    """The journey files AGENTS.md's "Journeys" section lists, one entry each."""
    text = (root / "AGENTS.md").read_text()
    section = text.split("\n## Journeys\n", 1)[1].split("\n## ", 1)[0]
    return {match.group(1) for line in section.splitlines() if (match := _ENTRY.match(line))}


def differences(root: Path = ROOT) -> list[str]:
    """One line per journey file listed but absent, or present but unlisted."""
    disk, agents = on_disk(root), listed(root)
    return [f"AGENTS.md lists {path}, which does not exist" for path in sorted(agents - disk)] + [
        f"{path} is a journey file AGENTS.md does not list" for path in sorted(disk - agents)
    ]
