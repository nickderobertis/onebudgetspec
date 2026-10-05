"""What the release declares, reconciled against what releases it.

``release-targets.toml`` names five targets, their manifests, what each covers and the job
that publishes it; the platform carriers are listed in the launcher, its manifest, the
build script and the release matrix. Each fact is checked against its source here.
"""

import json
import re
import tomllib
from pathlib import Path
from typing import NamedTuple, NotRequired, TypedDict

import yaml

from repo_checks.paths import ROOT


class Target(TypedDict):
    """One ``[[target]]`` of release-targets.toml."""

    id: str
    name: str
    what: str
    published_by: str
    manifest: str
    covers: NotRequired[list[str]]


def targets(root: Path = ROOT) -> list[Target]:
    """The targets release-targets.toml declares."""
    return tomllib.loads((root / "release-targets.toml").read_text())["target"]


#: The target names other repositories wait on; they never change.
TARGET_NAMES = ("crate", "pypi", "npm", "sdk-pypi", "sdk-npm")


def _manifest_name(root: Path, manifest: str) -> str:
    path = root / manifest
    match path.name:
        case "Cargo.toml":
            return tomllib.loads(path.read_text())["package"]["name"]
        case "pyproject.toml":
            return tomllib.loads(path.read_text())["project"]["name"]
        case _:
            return json.loads(path.read_text())["name"]


class CarrierIdentity(NamedTuple):
    """What a carrier's manifest says it is: its package name and npm's os and cpu fields."""

    name: object
    os: object
    cpu: object


def carrier_platforms(root: Path = ROOT) -> set[str]:
    """The platforms with a carrier manifest under ``npm/platforms``."""
    return {path.parent.name for path in (root / "npm/platforms").glob("*/package.json")}


def target_problems(root: Path = ROOT) -> list[str]:
    """Every way release-targets.toml and the release workflow disagree."""
    document = tomllib.loads((root / "release-targets.toml").read_text())
    declared = targets(root)
    problems: list[str] = []
    names = [target["name"] for target in declared]
    if sorted(names) != sorted(TARGET_NAMES):
        problems.append(f"release-targets.toml names {names}, not exactly {list(TARGET_NAMES)}")
    if not (root / document["probe"]).is_file():
        problems.append(f"release-targets.toml's probe {document['probe']} does not exist")
    workflow = yaml.safe_load((root / ".github/workflows/release.yml").read_text())["jobs"]
    published: dict[str, str] = {}
    for job, body in workflow.items():
        for step in body.get("steps", []):
            for match in re.finditer(r"scripts/publish\.sh (\S+)", str(step.get("run", ""))):
                published[match.group(1)] = job
    if sorted(published) != sorted(TARGET_NAMES):
        problems.append(f"release.yml publishes {sorted(published)}, not {sorted(TARGET_NAMES)}")
    publishable_crates = sorted(
        tomllib.loads(path.read_text())["package"]["name"]
        for path in (root / "crates").glob("*/Cargo.toml")
        if tomllib.loads(path.read_text())["package"].get("publish", True)
    )
    for target in declared:
        registry, _, name = target["id"].partition(":")
        manifest_name = _manifest_name(root, target["manifest"])
        if manifest_name != name:
            problems.append(
                f"{target['name']}: id names {name}, {target['manifest']} declares {manifest_name}"
            )
        job = published.get(target["name"])
        if job is None or f"the {job} job" not in target["published_by"]:
            problems.append(
                f"{target['name']}: published_by does not name {job}, the job that publishes it"
            )
        covers = sorted(target.get("covers", []))
        match target["name"]:
            case "crate":
                expected = sorted(f"crate:{c}" for c in publishable_crates if c != name)
            case "npm":
                expected = sorted(f"npm:@onebudgetspec/cli-{p}" for p in carrier_platforms(root))
            case _:
                expected = []
        if covers != expected:
            problems.append(f"{target['name']}: covers {covers}, expected {expected}")
        if registry not in ("crate", "pypi", "npm"):
            problems.append(f"{target['name']}: {registry} is not a registry the probe reads")
    return problems


def platform_problems(root: Path = ROOT) -> list[str]:
    """Every place the shipped platforms are listed that disagrees with the carriers."""
    carriers = sorted(carrier_platforms(root))
    launcher = (root / "npm/cli/lib/launcher.js").read_text()
    listed = re.search(r"const CARRIERS = \[([^\]]*)\]", launcher)
    in_launcher = sorted(re.findall(r'"([^"]+)"', listed.group(1))) if listed else []
    manifest = json.loads((root / "npm/cli/package.json").read_text())
    in_manifest = sorted(
        name.removeprefix("@onebudgetspec/cli-")
        for name in manifest.get("optionalDependencies", {})
    )
    build = (root / "scripts/build-dist.sh").read_text()
    mapping = dict(
        re.findall(r"^\s+([a-z0-9_]+-[a-z0-9_-]+)\) echo ([a-z0-9]+-[a-z0-9]+) ;;$", build, re.M)
    )
    in_build = sorted(mapping.values())
    release = yaml.safe_load((root / ".github/workflows/release.yml").read_text())
    matrix = release["jobs"]["native"]["strategy"]["matrix"]["include"]
    in_release = sorted(mapping.get(row["target"], f"unmapped {row['target']}") for row in matrix)
    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]
    in_toolchain = sorted(
        mapping.get(target, f"unmapped {target}") for target in toolchain.get("targets", [])
    )
    problems = []
    for platform in carriers:
        carrier = json.loads((root / f"npm/platforms/{platform}/package.json").read_text())
        system, _, cpu = platform.partition("-")
        declared = CarrierIdentity(carrier.get("name"), carrier.get("os"), carrier.get("cpu"))
        expected = CarrierIdentity(f"@onebudgetspec/cli-{platform}", [system], [cpu])
        if declared != expected:
            problems.append(
                f"npm/platforms/{platform}/package.json declares {tuple(declared)}, "
                f"not {tuple(expected)}"
            )
    for where, found in (
        ("npm/cli/lib/launcher.js", in_launcher),
        ("npm/cli/package.json", in_manifest),
        ("scripts/build-dist.sh", in_build),
        (".github/workflows/release.yml", in_release),
        ("rust-toolchain.toml", in_toolchain),
    ):
        if found != carriers:
            problems.append(f"{where} lists {found}; npm/platforms holds {carriers}")
    return problems


def launcher_status_problems(root: Path = ROOT) -> list[str]:
    """Whether the README documents exactly the exit statuses the npm launcher returns."""
    launcher = (root / "npm/cli/lib/launcher.js").read_text()
    returned = sorted({int(code) for code in re.findall(r"status: (\d+),", launcher)})
    readme = (root / "README.md").read_text()
    paragraph = re.search(r"The npm launcher adds[^\n]*(?:\n[^\n]+)*", readme)
    documented = (
        sorted(int(code) for code in re.findall(r"`(\d+)`", paragraph.group(0)))
        if paragraph
        else []
    )
    if returned != documented:
        return [
            f"README.md documents launcher statuses {documented}; "
            f"npm/cli/lib/launcher.js returns {returned}"
        ]
    return []
