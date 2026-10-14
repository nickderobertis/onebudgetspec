"""Normalize only proven local version fields; all other parsed content must agree."""

import json
import sys
import tomllib
from collections.abc import Mapping
from typing import Any

from version_contract import VERSION


def table(value: object, site: str) -> dict[str, Any]:
    """Validate mappings; Any preserves heterogeneous TOML fields for exact comparison."""
    if not isinstance(value, dict) or not all(isinstance(key, str) for key in value):
        raise ValueError(f"{site} must be a table")
    return value


def local_source(source: Mapping[str, object], keys: tuple[str, ...]) -> bool:
    """Whether a source names a local checkout; a present key must hold a non-empty path."""
    present = [key for key in keys if key in source]
    for key in present:
        if not isinstance(source[key], str) or not source[key]:
            raise ValueError(f"dependency source {key} must be a non-empty path")
    return bool(present)


def erase_version(record: dict[str, Any], key: str = "version", *, allow_pin: bool = False) -> None:
    """Erase one version; Any retains all unrelated heterogeneous TOML fields unchanged."""
    if key in record:
        value = record[key]
        if not isinstance(value, str) or not VERSION.fullmatch(
            value.removeprefix("=") if allow_pin else value
        ):
            raise ValueError("invalid local version")
        record[key] = ("=" if value.startswith("=") else "") + "<local-version>"


def normalize(path: str, text: str) -> dict[str, Any]:
    """Normalize local versions; Any preserves the entire TOML tree for exact comparison."""
    doc = tomllib.loads(text)
    match path:
        case _ if path.endswith("Cargo.toml"):
            workspace = table(doc.get("workspace", {}), "Cargo workspace")
            erase_version(table(workspace.get("package", {}), "workspace package"))
            package = table(doc.get("package", {}), "Cargo package")
            if isinstance(package.get("version"), str):
                erase_version(package)
            targets = table(doc.get("target", {}), "Cargo targets")
            groups = [doc, workspace, *(table(value, "Cargo target") for value in targets.values())]
            for group in groups:
                for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
                    dependencies = table(group.get(kind, {}), "Cargo dependencies")
                    for dependency in dependencies.values():
                        if isinstance(dependency, str):
                            continue
                        dependency = table(dependency, "Cargo dependency")
                        if "path" in dependency:
                            if not isinstance(dependency["path"], str):
                                raise ValueError("Cargo path must be a string")
                            erase_version(dependency, allow_pin=True)
        case _ if path.endswith("pyproject.toml"):
            project = table(doc.get("project", {}), "Python project")
            erase_version(project)
            tool = table(doc.get("tool", {}), "Python tool")
            uv = table(tool.get("uv", {}), "uv tool")
            sources = table(uv.get("sources", {}), "Python sources")
            for source in sources.values():
                table(source, "Python dependency source")
            dependencies = project.get("dependencies", [])
            if not isinstance(dependencies, list) or not all(
                isinstance(dependency, str) for dependency in dependencies
            ):
                raise ValueError("Python dependencies must be a list of strings")
            for index, dependency in enumerate(dependencies):
                name, separator, value = dependency.partition("==")
                source = sources.get(name, {})
                if separator and (
                    source.get("workspace") is True or local_source(source, ("path",))
                ):
                    if not VERSION.fullmatch(value):
                        raise ValueError("invalid local dependency version")
                    dependencies[index] = name + "==<local-version>"
        case "Cargo.lock" | "uv.lock":
            packages = doc.get("package")
            if not isinstance(packages, list):
                raise ValueError("lockfile package must be a list of tables")
            for package in packages:
                package = table(package, "lockfile package")
                source = package.get("source")
                local = (
                    source is None
                    if path == "Cargo.lock"
                    else isinstance(source, dict)
                    and local_source(
                        table(source, "lockfile source"), ("editable", "virtual", "directory")
                    )
                )
                if local:
                    erase_version(package)
        case _:
            raise ValueError("unrecognized TOML surface")
    return doc


if __name__ == "__main__":
    request = json.load(sys.stdin)
    if (
        not isinstance(request, dict)
        or set(request) != {"path", "text"}
        or not all(isinstance(request[key], str) for key in ("path", "text"))
    ):
        raise ValueError("normalizer expects string path/text; invoke it through macos-policy.mjs")
    print(json.dumps(normalize(request["path"], request["text"]), sort_keys=True))
