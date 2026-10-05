"""What the release declares, reconciled against what releases it.

``release-targets.toml`` names five targets, their manifests, what each covers and the job
that publishes it; the platform carriers are listed in the launcher, its manifest, the
build script and the release matrix. Each fact is checked against its source here.
"""

import json
import re
import tomllib
from pathlib import Path

import yaml

from repo_checks.paths import ROOT

#: The target names other repositories wait on; they never change.
TARGET_NAMES = ("crate", "pypi", "npm", "sdk-pypi", "sdk-npm")


def _manifest_name(root: Path, manifest: str) -> str:
    path = root / manifest
    if path.name == "Cargo.toml":
        return tomllib.loads(path.read_text())["package"]["name"]
    if path.name == "pyproject.toml":
        return tomllib.loads(path.read_text())["project"]["name"]
    return json.loads(path.read_text())["name"]


def carrier_platforms(root: Path = ROOT) -> set[str]:
    """The platforms with a carrier manifest under ``npm/platforms``."""
    return {path.parent.name for path in (root / "npm/platforms").glob("*/package.json")}


def target_problems(root: Path = ROOT) -> list[str]:
    """Every way release-targets.toml and the release workflow disagree."""
    document = tomllib.loads((root / "release-targets.toml").read_text())
    targets = document["target"]
    problems: list[str] = []
    names = [target["name"] for target in targets]
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
    for target in targets:
        registry, _, name = target["id"].partition(":")
        declared = _manifest_name(root, target["manifest"])
        if declared != name:
            problems.append(
                f"{target['name']}: id names {name}, {target['manifest']} declares {declared}"
            )
        job = published.get(target["name"])
        if job is None or f"the {job} job" not in target["published_by"]:
            problems.append(
                f"{target['name']}: published_by does not name {job}, the job that publishes it"
            )
        covers = sorted(target.get("covers", []))
        if target["name"] == "crate":
            expected = sorted(f"crate:{crate}" for crate in publishable_crates if crate != name)
        elif target["name"] == "npm":
            expected = sorted(f"npm:@onebudgetspec/cli-{p}" for p in carrier_platforms(root))
        else:
            expected = []
        if covers != expected:
            problems.append(f"{target['name']}: covers {covers}, expected {expected}")
        if registry not in ("crate", "pypi", "npm"):
            problems.append(f"{target['name']}: {registry} is not a registry the probe reads")
    return problems


def platform_problems(root: Path = ROOT) -> list[str]:
    """Every place the shipped platforms are listed that disagrees with the carriers."""
    carriers = sorted(carrier_platforms(root))
    launcher = (root / "npm/cli/bin/onebudgetspec.js").read_text()
    listed = re.search(r"const carriers = \[([^\]]*)\]", launcher)
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
    problems = []
    for where, found in (
        ("npm/cli/bin/onebudgetspec.js", in_launcher),
        ("npm/cli/package.json", in_manifest),
        ("scripts/build-dist.sh", in_build),
        (".github/workflows/release.yml", in_release),
    ):
        if found != carriers:
            problems.append(f"{where} lists {found}; npm/platforms holds {carriers}")
    return problems
