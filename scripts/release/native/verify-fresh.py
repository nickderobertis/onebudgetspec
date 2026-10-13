"""Refuse a Cargo install that recompiles instead of reusing the wheel build."""

import argparse
import json
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class CompilerArtifact:
    """The admitted identity and freshness evidence from one Cargo artifact message."""

    fresh: bool
    name: str
    kinds: tuple[str, ...]
    manifest: Path
    executable: Path | None


def artifact(record: dict[str, object]) -> CompilerArtifact:
    """Validate Cargo's relevant fields before constructing the internal projection."""
    fresh, target = record.get("fresh"), record.get("target")
    manifest, executable = record.get("manifest_path"), record.get("executable")
    if not isinstance(fresh, bool):
        raise ValueError("compiler-artifact lacks a boolean fresh field; rerun Cargo JSON output")
    if not isinstance(target, dict) or not isinstance(target.get("name"), str):
        raise ValueError("compiler-artifact lacks target identity; rerun Cargo JSON output")
    kinds = target.get("kind")
    if not isinstance(kinds, list) or not all(isinstance(kind, str) for kind in kinds):
        raise ValueError("compiler-artifact lacks target kinds; rerun Cargo JSON output")
    if not isinstance(manifest, str) or (
        executable is not None and not isinstance(executable, str)
    ):
        raise ValueError(
            "compiler-artifact lacks manifest/executable identity; rerun Cargo JSON output"
        )
    return CompilerArtifact(
        fresh,
        target["name"],
        tuple(kinds),
        Path(manifest),
        Path(executable) if executable else None,
    )


def verify(path: Path) -> None:
    """Require a complete successful CLI build whose every artifact was already fresh."""
    compiled: list[CompilerArtifact] = []
    finished = False
    for line in path.read_text().splitlines():
        if not line.startswith("{"):
            continue
        record: object = json.loads(line)
        if not isinstance(record, dict):
            raise ValueError("Cargo message is not an object; rerun Cargo JSON output")
        match record.get("reason"):
            case "compiler-artifact":
                compiled.append(artifact(record))
            case "build-finished":
                finished = record.get("success") is True
    if not compiled:
        raise ValueError("no compiler-artifact evidence; rerun Cargo install with JSON messages")
    if any(not compiled_artifact.fresh for compiled_artifact in compiled):
        raise ValueError("Cargo install recompiled; every artifact must reuse the wheel build")
    manifest = (Path(__file__).resolve().parents[3] / "crates/onebudgetspec/Cargo.toml").resolve()
    if not finished or not any(
        record.name == "onebudgetspec"
        and "bin" in record.kinds
        and record.executable is not None
        and record.manifest.resolve() == manifest
        for record in compiled
    ):
        raise ValueError("incomplete successful onebudgetspec build evidence; rerun Cargo install")


def main() -> None:
    """Check the install ledger, with an actionable diagnostic on failure."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    args = parser.parse_args()
    try:
        verify(args.path)
    except (ValueError, OSError) as error:
        parser.exit(
            1, f"verify-fresh: {error}; restore the shared target directory and build options\n"
        )


if __name__ == "__main__":
    main()
