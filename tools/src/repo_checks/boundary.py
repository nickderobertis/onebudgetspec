"""The library knows nothing of the stack it is used beside.

What the repository ships and documents for its users (both crates, the wheel, the npm
launcher and carriers, both SDKs, the README, and the schema bundle the binary emits)
names none of the stack's libraries, depends on none of them directly or transitively,
and mentions no plans, design documents or approvals. Development tooling (this package,
the judged lint's configuration and CI job, AGENTS.md) is outside that boundary.
"""

import json
import re
import tomllib
from collections.abc import Iterable, Iterator
from pathlib import Path

from repo_checks.paths import ROOT

#: The stack's libraries, none of which the library may name or depend on.
STACK = (
    "onetaskgraph",
    "onepipeline",
    "onevcs",
    "oneagentgraph",
    "onejudge",
    "oneharness",
    "onemessagebus",
    "llmlint",
)
#: What the library ships, relative to the root: directories are scanned whole.
SHIPPED = (
    "crates/onebudgetspec-core",
    "crates/onebudgetspec",
    "pyproject.toml",
    "npm",
    "sdks/python",
    "sdks/typescript",
    "README.md",
)
#: Build and environment output inside a shipped directory, which is not source.
SKIPPED = {"node_modules", "dist", ".venv", "__pycache__", ".pytest_cache", ".ruff_cache", "target"}
#: The packages whose resolved dependencies ship, per lockfile.
CARGO_ROOTS = ("onebudgetspec", "onebudgetspec-core")
UV_ROOTS = ("onebudgetspec-cli", "onebudgetspec-sdk")
BUN_WORKSPACES = ("sdks/typescript",)

_NAMES = re.compile("|".join(STACK), re.IGNORECASE)
_TOPICS = re.compile(
    r"\b(plans?|planned|planning|design[ -]doc(ument)?s?|approvals?|approved?|approving)\b",
    re.IGNORECASE,
)
#: The one permitted mention: crediting onetaskgraph as the model the layout follows.
_CREDIT = re.compile(r"onetaskgraph.*\bmodel|\bmodel.*onetaskgraph", re.IGNORECASE)


def files(root: Path = ROOT) -> Iterator[Path]:
    """Every shipped file under ``root``."""
    for entry in SHIPPED:
        path = root / entry
        if path.is_file():
            yield path
            continue
        for found in sorted(path.rglob("*")):
            if found.is_file() and not SKIPPED & set(found.relative_to(root).parts):
                yield found


def scan_text(where: str, text: str) -> list[str]:
    """One finding per line of ``text`` that names the stack or mentions its topics."""
    findings = []
    for number, line in enumerate(text.splitlines(), 1):
        for match in _NAMES.finditer(line):
            if match.group(0).lower() == "onetaskgraph" and _CREDIT.search(line):
                continue
            findings.append(f"{where}:{number}: names {match.group(0)}")
        findings.extend(f"{where}:{number}: mentions {m.group(0)}" for m in _TOPICS.finditer(line))
    return findings


def _closure(graph: dict[str, set[str]], roots: Iterable[str]) -> set[str]:
    seen: set[str] = set()
    pending = [root for root in roots if root in graph]
    while pending:
        name = pending.pop()
        if name not in seen:
            seen.add(name)
            pending.extend(graph.get(name, set()) - seen)
    return seen


def cargo_dependencies(root: Path = ROOT) -> set[str]:
    """Every package Cargo.lock resolves for the two shipped crates, transitively."""
    graph: dict[str, set[str]] = {}
    for package in tomllib.loads((root / "Cargo.lock").read_text())["package"]:
        deps = {dependency.split()[0] for dependency in package.get("dependencies", [])}
        graph.setdefault(package["name"], set()).update(deps)
    return _closure(graph, CARGO_ROOTS)


def uv_dependencies(root: Path = ROOT) -> set[str]:
    """Every package uv.lock resolves for the wheel and the Python SDK, transitively."""
    graph: dict[str, set[str]] = {}
    for package in tomllib.loads((root / "uv.lock").read_text())["package"]:
        deps = {dependency["name"] for dependency in package.get("dependencies", [])}
        graph.setdefault(package["name"], set()).update(deps)
    return _closure(graph, UV_ROOTS)


def bun_dependencies(root: Path = ROOT) -> set[str]:
    """Every package bun.lock resolves for the TypeScript SDK, transitively."""
    text = re.sub(r",(\s*[}\]])", r"\1", (root / "bun.lock").read_text())
    lock = json.loads(text)
    graph: dict[str, set[str]] = {}
    for name, entry in lock.get("packages", {}).items():
        metadata = next((part for part in entry if isinstance(part, dict)), {})
        graph[name] = set(metadata.get("dependencies", {})) | set(
            metadata.get("optionalDependencies", {})
        )
    roots: set[str] = set()
    for workspace in BUN_WORKSPACES:
        declared = lock["workspaces"][workspace]
        roots |= set(declared.get("dependencies", {})) | set(
            declared.get("optionalDependencies", {})
        )
    return _closure(graph, roots) | roots


def launcher_dependencies(root: Path = ROOT) -> set[str]:
    """The packages the npm launcher pulls in: its platform carriers."""
    manifest = json.loads((root / "npm/cli/package.json").read_text())
    return set(manifest.get("dependencies", {})) | set(manifest.get("optionalDependencies", {}))


def scan(root: Path = ROOT, schema: str | None = None) -> list[str]:
    """Every finding over the shipped files, ``schema`` (the emitted bundle) and dependencies."""
    findings: list[str] = []
    for path in files(root):
        try:
            text = path.read_text()
        except UnicodeDecodeError:
            continue
        findings.extend(scan_text(path.relative_to(root).as_posix(), text))
    if schema is not None:
        findings.extend(scan_text("onebudgetspec schema", schema))
    for lockfile, names in (
        ("Cargo.lock", cargo_dependencies(root)),
        ("uv.lock", uv_dependencies(root)),
        ("bun.lock", bun_dependencies(root)),
        ("npm/cli/package.json", launcher_dependencies(root)),
    ):
        findings.extend(
            f"{lockfile}: resolves dependency {name}"
            for name in sorted(names)
            if _NAMES.search(name) or _TOPICS.search(name)
        )
    return findings
