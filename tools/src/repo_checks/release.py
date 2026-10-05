"""What the release declares, reconciled against what releases it.

``release-targets.toml`` names five targets, their manifests, what each covers and the job
that publishes it; the platform carriers are listed in the launcher, its manifest, the
build script and the release matrix. Each fact is checked against its source here.
"""

import json
import re
import tomllib
from pathlib import Path
from typing import NamedTuple, NewType, NotRequired, TypedDict, cast

import yaml

from repo_checks.paths import ROOT

#: A registry-qualified target id, ``<registry>:<name>``.
TargetId = NewType("TargetId", str)
#: One of the five short target names other repositories wait on.
TargetName = NewType("TargetName", str)


class Target(TypedDict):
    """One ``[[target]]`` of release-targets.toml."""

    id: TargetId
    name: TargetName
    what: str
    published_by: str
    manifest: str
    covers: NotRequired[list[TargetId]]


class InvalidDeclaration(ValueError):
    """release-targets.toml is not the shape the release reads."""


#: The fields every target must carry, read from Target so the two cannot part.
_TARGET_FIELDS = tuple(
    field for field in Target.__annotations__ if field in Target.__required_keys__
)


def targets(root: Path = ROOT) -> list[Target]:
    """The targets release-targets.toml declares, each checked for the fields it must carry.

    Raises:
        InvalidDeclaration: the declaration has no list of targets, or one lacks a field.
    """
    declared = tomllib.loads((root / "release-targets.toml").read_text()).get("target")
    if not isinstance(declared, list) or not all(isinstance(r, dict) for r in declared):
        raise InvalidDeclaration("release-targets.toml: `target` is not a list of tables")
    checked: list[Target] = []
    for number, record in enumerate(declared, 1):
        for field in _TARGET_FIELDS:
            if not isinstance(record.get(field), str):
                raise InvalidDeclaration(
                    f"release-targets.toml: target {number} lacks a string `{field}`"
                )
        covers = record.get("covers", [])
        if not isinstance(covers, list) or not all(isinstance(c, str) for c in covers):
            raise InvalidDeclaration(
                f"release-targets.toml: target {number}'s `covers` is not a list of strings"
            )
        checked.append(cast(Target, record))  # its fields were checked above
    return checked


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
            for match in re.finditer(
                r"scripts/release/publish\.sh (\S+)", str(step.get("run", ""))
            ):
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


#: What each scripts/build-dist.sh artifact packs: the manifest (a glob, for the carriers)
#: declaring the package it builds.
ARTIFACT_MANIFESTS = {
    "cli-wheel": "pyproject.toml",
    "npm-carrier": "npm/platforms/*/package.json",
    "npm-launcher": "npm/cli/package.json",
    "sdk-python": "sdks/python/pyproject.toml",
    "sdk-typescript": "sdks/typescript/package.json",
}


def _build_arms(root: Path) -> dict[str, str]:
    """Each artifact scripts/build-dist.sh builds, and the text of its case arm."""
    script = (root / "scripts/build-dist.sh").read_text()
    body = script.split('case "$ARTIFACT" in', 1)[-1]
    arms = re.split(r"^  ([a-z-]+)\)\n", body, flags=re.M)
    return dict(zip(arms[1::2], arms[2::2], strict=True))


def _arm_builds(root: Path, arm: str, manifest: str, name: str) -> bool:
    """Whether a build-dist.sh arm builds ``manifest``'s package.

    For the root wheel, maturin over the crate its ``[tool.maturin]`` names; for a uv
    workspace member, ``--package`` with its name; otherwise the manifest's own directory.
    """
    if manifest == "pyproject.toml":
        maturin = tomllib.loads((root / manifest).read_text()).get("tool", {}).get("maturin")
        crate = maturin.get("manifest-path") if isinstance(maturin, dict) else None
        return isinstance(crate, str) and "maturin build" in arm and f'"$ROOT/{crate}"' in arm
    directory = manifest.rsplit("/", 1)[0].removesuffix("/*")
    return f"$ROOT/{directory}" in arm or f"--package {name} " in arm


def _step_runs(root: Path) -> list[str] | None:
    """Every step's ``run`` text in release.yml, or None when it has no mapping of jobs.

    A job or step of another shape contributes nothing, so it can never stand in for a
    build or a publish.
    """
    document = yaml.safe_load((root / ".github/workflows/release.yml").read_text())
    jobs = document.get("jobs") if isinstance(document, dict) else None
    if not isinstance(jobs, dict):
        return None
    return [
        str(step.get("run", ""))
        for body in jobs.values()
        if isinstance(body, dict) and isinstance(body.get("steps"), list)
        for step in body["steps"]
        if isinstance(step, dict)
    ]


def artifact_problems(root: Path = ROOT) -> list[str]:
    """Every publish job that would upload an artifact built from another package.

    Each directory a ``publish.sh <target>`` step names must be one release.yml fills with
    ``build-dist.sh <artifact> <directory>``, and that artifact must build a package the
    target publishes (its id or what it covers).
    """
    problems: list[str] = []
    arms = _build_arms(root)
    if sorted(arms) != sorted(ARTIFACT_MANIFESTS):
        known = sorted(ARTIFACT_MANIFESTS)
        problems.append(f"scripts/build-dist.sh builds {sorted(arms)}; release.py knows {known}")
    packages: dict[str, set[str]] = {}
    for artifact, pattern in ARTIFACT_MANIFESTS.items():
        names = {
            _manifest_name(root, path.relative_to(root).as_posix())
            for path in sorted(root.glob(pattern))
        }
        packages[artifact] = names
        arm = arms.get(artifact, "")
        for name in sorted(names):
            if not _arm_builds(root, arm, pattern, name):
                problems.append(f"scripts/build-dist.sh {artifact} does not build {name}")
    runs = _step_runs(root)
    if runs is None:
        return [*problems, ".github/workflows/release.yml: `jobs` is not a mapping of jobs"]
    built: dict[str, str] = {}
    for run in runs:
        for match in re.finditer(r"build-dist\.sh (\S+) (\S+)", run):
            built[match.group(2)] = match.group(1)
    for target in targets(root):
        publishes = {target["id"].partition(":")[2]} | {
            cover.partition(":")[2] for cover in target.get("covers", [])
        }
        for run in runs:
            match = re.search(
                rf"scripts/release/publish\.sh {re.escape(target['name'])}((?: \S+)*)", run
            )
            if match is None:
                continue
            for directory in match.group(1).split():
                artifact = built.get(directory, "")
                if artifact not in packages:
                    problems.append(
                        f"{target['name']}: publishes {directory}, "
                        "which no build-dist.sh step fills"
                    )
                elif not packages[artifact] or not packages[artifact] <= publishes:
                    problems.append(
                        f"{target['name']}: publishes {directory}, built as {artifact} "
                        f"({', '.join(sorted(packages[artifact]))}), not {sorted(publishes)}"
                    )
    return problems
