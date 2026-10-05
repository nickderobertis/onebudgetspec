"""The library names none of the stack's libraries and depends on none of them.

It mentions no plans, design documents or approvals either, and the scan fails when any of
that is planted.
"""

import json
from pathlib import Path

import pytest
from conftest import copy_tree

from repo_checks import boundary

LIBRARY = (*boundary.SHIPPED, "Cargo.lock", "uv.lock", "bun.lock")


def test_the_library_is_clean(schema: str) -> None:
    assert boundary.scan(schema=schema) == []


def test_it_reads_every_shipped_part_and_resolves_dependencies() -> None:
    scanned = {path.relative_to(boundary.ROOT).as_posix() for path in boundary.files()}
    for expected in (
        "crates/onebudgetspec-core/src/lib.rs",
        "crates/onebudgetspec-core/Cargo.toml",
        "crates/onebudgetspec/src/main.rs",
        "crates/onebudgetspec/Cargo.toml",
        "pyproject.toml",
        "npm/cli/bin/onebudgetspec.js",
        "npm/platforms/linux-x64/package.json",
        "sdks/python/src/onebudgetspec_sdk/__init__.py",
        "sdks/typescript/src/index.ts",
        "README.md",
    ):
        assert expected in scanned
    assert not any("node_modules" in path for path in scanned)
    cargo = boundary.cargo_dependencies()
    assert {"onebudgetspec", "onebudgetspec-core", "serde", "schemars"} <= cargo
    assert {"onebudgetspec-cli", "onebudgetspec-sdk"} <= boundary.uv_dependencies()
    assert boundary.launcher_dependencies() == {
        f"@onebudgetspec/cli-{platform}"
        for platform in ("linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64")
    }


@pytest.mark.parametrize("name", boundary.STACK)
def test_a_name_planted_in_a_source_file_fails_the_scan(tmp_path: Path, name: str) -> None:
    root = copy_tree(tmp_path, *LIBRARY)
    source = root / "crates/onebudgetspec-core/src/lib.rs"
    source.write_text(source.read_text() + f"\n// Works with {name}.\n")
    findings = boundary.scan(root)
    assert any(
        f.startswith("crates/onebudgetspec-core/src/lib.rs:") and name in f for f in findings
    ), findings


@pytest.mark.parametrize("name", boundary.STACK)
def test_a_name_planted_in_a_manifest_dependency_list_fails_the_scan(
    tmp_path: Path, name: str
) -> None:
    root = copy_tree(tmp_path, *LIBRARY)
    manifest = root / "crates/onebudgetspec/Cargo.toml"
    text = manifest.read_text().replace("[dependencies]\n", f'[dependencies]\n{name} = "1"\n', 1)
    manifest.write_text(text)
    package = root / "sdks/typescript/package.json"
    document = json.loads(package.read_text())
    document["dependencies"] = {name: "1.0.0"}
    package.write_text(json.dumps(document))
    findings = boundary.scan(root)
    assert any(f.startswith("crates/onebudgetspec/Cargo.toml:") and name in f for f in findings), (
        findings
    )
    assert any(f.startswith("sdks/typescript/package.json:") and name in f for f in findings), (
        findings
    )


@pytest.mark.parametrize("name", boundary.STACK)
def test_a_name_planted_in_a_resolved_dependency_fails_the_scan(tmp_path: Path, name: str) -> None:
    root = copy_tree(tmp_path, *LIBRARY)
    lock = root / "Cargo.lock"
    text = lock.read_text()
    text = text.replace(
        'name = "onebudgetspec-core"\nversion = "0.1.0"\ndependencies = [\n',
        f'name = "onebudgetspec-core"\nversion = "0.1.0"\ndependencies = [\n "{name}-sys",\n',
    )
    lock.write_text(text + f'\n[[package]]\nname = "{name}-sys"\nversion = "1.0.0"\n')
    assert f"Cargo.lock: resolves dependency {name}-sys" in boundary.scan(root)


@pytest.mark.parametrize(
    "line",
    [
        "the release plan",
        "see the design document",
        "pending approval",
        "once approved",
        "planning ahead",
    ],
)
def test_the_topics_are_refused(line: str) -> None:
    assert boundary.scan_text("x", line)


def test_the_schema_is_scanned() -> None:
    assert boundary.scan_text("onebudgetspec schema", '{"description": "an oneharness id"}') == [
        "onebudgetspec schema:1: names oneharness"
    ]


def test_crediting_the_model_layout_is_the_one_exception() -> None:
    assert boundary.scan_text("x", "The layout is modelled on onetaskgraph.") == []
    assert boundary.scan_text("x", "Uses onetaskgraph.") == ["x:1: names onetaskgraph"]
    assert boundary.scan_text("x", "modelled on onetaskgraph and onevcs") == ["x:1: names onevcs"]


def test_python_and_bun_dependencies_are_followed(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *LIBRARY)
    uv = root / "uv.lock"
    sdk = 'name = "onebudgetspec-sdk"\nversion = "0.1.0"\nsource = { editable = "sdks/python" }\n'
    planted = 'dependencies = [{ name = "onejudge-client" }]\n'
    uv.write_text(uv.read_text().replace(sdk, sdk + planted))
    bun = root / "bun.lock"
    bun.write_text(
        bun.read_text()
        .replace(
            '"name": "@onebudgetspec/sdk",',
            '"name": "@onebudgetspec/sdk",\n      "dependencies": { "a": "1" },',
        )
        .replace(
            '"packages": {',
            '"packages": {\n    "a": ["a@1", "", { "dependencies": { "onemessagebus": "1" } }],',
        )
    )
    findings = boundary.scan(root)
    assert "uv.lock: resolves dependency onejudge-client" in findings
    assert "bun.lock: resolves dependency onemessagebus" in findings


def test_a_binary_file_is_skipped(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *LIBRARY)
    (root / "npm/cli/bin/blob").write_bytes(b"\xff\xfe onevcs")
    assert boundary.scan(root) == []
