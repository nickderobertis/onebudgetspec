"""Which projects may depend on which, enforced over the Nx graph.

Every project carries one ``type:`` tag, and each type may depend only on the types listed
for it below, so an edge drawn back toward a contract or into a shipped package from a
test tier fails here. Rust crates must also declare, as Nx dependencies, every workspace
crate their Cargo manifest depends on, or affected selection would miss them.
"""

import json
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Literal, NewType

from repo_checks.paths import ROOT

#: An Nx project's name, as project.json declares it and other projects depend on it.
ProjectName = NewType("ProjectName", str)
ProjectType = Literal[
    "contract", "sdk", "distribution", "binary", "e2e", "integration", "tooling", "workspace"
]
_EVERYTHING_BELOW_TOOLING: set[ProjectType] = {
    "binary",
    "contract",
    "distribution",
    "sdk",
    "e2e",
    "integration",
    "tooling",
}
#: For each project type, the types it may depend on.
ALLOWED: dict[ProjectType, set[ProjectType]] = {
    "contract": set(),
    "sdk": set(),
    "distribution": set(),
    "binary": {"contract"},
    "e2e": {"binary", "contract", "distribution", "sdk"},
    "integration": {"binary", "tooling"},
    "tooling": _EVERYTHING_BELOW_TOOLING,
    "workspace": _EVERYTHING_BELOW_TOOLING,
}


class InvalidProject(ValueError):
    """A project.json without the shape this check reads."""


@dataclass(frozen=True)
class Project:
    """What this check reads from one project.json."""

    name: ProjectName
    root: Path
    tags: tuple[str, ...]
    dependencies: tuple[ProjectName, ...]

    @classmethod
    def read(cls, path: Path) -> "Project":
        """Parse ``path``, refusing a document without a name or with ill-typed lists."""
        document = json.loads(path.read_text())
        match document:
            case {"name": str(name), **rest}:
                tags = rest.get("tags", [])
                dependencies = rest.get("implicitDependencies", [])
            case _:
                raise InvalidProject(f"{path}: has no string `name`")
        for field, values in (("tags", tags), ("implicitDependencies", dependencies)):
            if not isinstance(values, list) or not all(isinstance(v, str) for v in values):
                raise InvalidProject(f"{path}: `{field}` is not a list of strings")
        return cls(
            ProjectName(name),
            path.parent,
            tuple(tags),
            tuple(ProjectName(dependency) for dependency in dependencies),
        )

    def kinds(self) -> list[str]:
        """The values of this project's ``type:`` tags."""
        return [tag.removeprefix("type:") for tag in self.tags if tag.startswith("type:")]


def projects(root: Path = ROOT) -> dict[ProjectName, Project]:
    """Every project under ``root``, by name.

    Raises:
        InvalidProject: a project.json is malformed, or two share a name.
    """
    found: dict[ProjectName, Project] = {}
    for path in sorted(root.glob("**/project.json")):
        if {"node_modules", "target"} & set(path.parts):
            continue
        project = Project.read(path)
        if project.name in found:
            raise InvalidProject(f"{path}: repeats the project name {project.name}")
        found[project.name] = project
    return found


_BY_TAG: dict[str, ProjectType] = {kind: kind for kind in ALLOWED}


def _kind(project: Project) -> ProjectType | None:
    match project.kinds():
        case [kind]:
            return _BY_TAG.get(kind)
        case _:
            return None


def problems(root: Path = ROOT) -> list[str]:
    """Every project without one known type, and every edge its type does not allow."""
    found = projects(root)
    issues: list[str] = []
    kinds: dict[ProjectName, ProjectType] = {}
    for name, project in found.items():
        kind = _kind(project)
        if kind is None:
            issues.append(
                f"{name}: needs exactly one type tag of {sorted(ALLOWED)}, has {project.kinds()}"
            )
        else:
            kinds[name] = kind
    for name, project in found.items():
        for dependency in project.dependencies:
            if dependency not in found:
                issues.append(f"{name}: depends on {dependency}, which is not a project")
            elif (
                name in kinds
                and dependency in kinds
                and kinds[dependency] not in ALLOWED[kinds[name]]
            ):
                issues.append(
                    f"{name} ({kinds[name]}) may not depend on {dependency} ({kinds[dependency]})"
                )
        manifest = project.root / "Cargo.toml"
        if manifest.is_file():
            cargo = tomllib.loads(manifest.read_text())
            crates = {
                dependency
                for table in ("dependencies", "dev-dependencies", "build-dependencies")
                for dependency in cargo.get(table, {})
                if dependency in found
            }
            missing = sorted(crates - set(project.dependencies))
            if missing:
                issues.append(f"{name}: Cargo depends on {missing}, which its project.json omits")
    return issues
