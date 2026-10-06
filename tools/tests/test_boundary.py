"""The library names none of the stack's libraries and depends on none of them.

It mentions no plans, design documents or approvals either, and the scan fails when any of
that is planted.
"""

import json
import re
from pathlib import Path

import pytest
from conftest import copy_tree, other_version

from repo_checks import boundary, versions

LIBRARY = (*boundary.SHIPPED, "Cargo.lock", "uv.lock", "bun.lock")

# A release rewrites this workspace's own package versions in its locks.
OWN_VERSION = re.compile(r'(?m)^(name = "onebudgetspec[^"]*"\nversion = ")[^"]*"')


def bump(lock: Path) -> None:
    """Rewrite the workspace's own versions in ``lock`` the way a release does."""
    text = lock.read_text()
    released = other_version(versions.workspace_version())
    bumped = OWN_VERSION.sub(lambda found: f'{found.group(1)}{released}"', text)
    assert bumped != text, f"{lock.name} carries none of the workspace's packages"
    lock.write_text(bumped)


def plant(lock: Path, package: str, entry: str) -> None:
    """Add ``entry`` to ``package``'s resolved dependencies in ``lock``, whatever its version.

    A planting that matches nothing fails here, so it is never read as a clean scan.
    """
    text = lock.read_text()
    head = re.compile(
        rf'(?m)^name = "{re.escape(package)}"\nversion = "[^"]*"\n(?:source = .*\n)?'
        r"dependencies = \[\n"
    )
    planted = head.sub(lambda found: found.group(0) + entry, text, count=1)
    assert planted != text, f"{lock.name} has no {package} entry with dependencies to plant in"
    lock.write_text(planted)


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
        for platform in (
            "linux-x64",
            "linux-arm64",
            "darwin-x64",
            "darwin-arm64",
            "win32-x64",
            "win32-arm64",
        )
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


@pytest.mark.parametrize("bumped", [False, True], ids=["current", "released"])
@pytest.mark.parametrize("name", boundary.STACK)
def test_a_name_planted_in_a_resolved_dependency_fails_the_scan(
    tmp_path: Path, name: str, bumped: bool
) -> None:
    root = copy_tree(tmp_path, *LIBRARY)
    lock = root / "Cargo.lock"
    if bumped:
        bump(lock)
    plant(lock, "onebudgetspec-core", f' "{name}-sys",\n')
    lock.write_text(lock.read_text() + f'\n[[package]]\nname = "{name}-sys"\nversion = "1.0.0"\n')
    assert f"Cargo.lock: resolves dependency {name}-sys" in boundary.scan(root)


def test_a_planting_that_matches_nothing_fails_loudly(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, "Cargo.lock")
    lock = root / "Cargo.lock"
    before = lock.read_text()
    with pytest.raises(AssertionError, match="no onebudgetspec-absent entry"):
        plant(lock, "onebudgetspec-absent", ' "onevcs-sys",\n')
    assert lock.read_text() == before


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


# Assembled at run time, so the judged lint does not read these samples as directives.
TOOL = "llm" + "lint"


@pytest.mark.parametrize(
    ("where", "line"),
    [
        ("crates/onebudgetspec-core/src/model.rs", f"// {TOOL}: ignore[rule_name] the reason"),
        ("crates/onebudgetspec-core/src/model.rs", f"    // {TOOL}: ignore-block[a, b] the reason"),
        ("crates/onebudgetspec-core/src/model.rs", f"// {TOOL}: ignore-end[rule_name]"),
        ("npm/cli/lib/launcher.js", f"  // {TOOL}: ignore[rule_name] the reason"),
        ("sdks/python/pyproject.toml", f"# {TOOL}: ignore[rule_name] the reason"),
        ("sdks/typescript/package.json", f'  "//": "{TOOL}: ignore-file[rule_name] the reason",'),
    ],
)
def test_a_reasoned_directive_in_its_file_s_comment_form_is_exempt(where: str, line: str) -> None:
    assert boundary.scan_text(where, line) == []


@pytest.mark.parametrize(
    ("where", "line"),
    [
        # Ordinary source strings shaped like a directive.
        ("crates/onebudgetspec-core/src/lib.rs", f'let s = "{TOOL}: ignore[r] the reason";'),
        ("crates/onebudgetspec-core/src/lib.rs", f'let s = "// {TOOL}: ignore[r] the reason";'),
        ("npm/cli/lib/launcher.js", f'const s = "{TOOL}: ignore[r] the reason";'),
        # Manifest values that are not the JSON comment key.
        ("npm/cli/package.json", f'  "description": "{TOOL}: ignore[r] the reason",'),
        ("pyproject.toml", f'description = "{TOOL}: ignore[r] the reason"'),
        # README prose, a README heading, and a code sample in it.
        ("README.md", f"See the {TOOL}: ignore[r] the reason syntax."),
        ("README.md", f"# {TOOL}: ignore[r] the reason"),
        ("README.md", f"// {TOOL}: ignore[r] the reason"),
        # A comment form the file's language does not use.
        ("crates/onebudgetspec-core/src/lib.rs", f"# {TOOL}: ignore[r] the reason"),
        # A directive with no reason, and a comment that only mentions the syntax.
        ("crates/onebudgetspec-core/src/lib.rs", f"// {TOOL}: ignore[rule_name]"),
        ("crates/onebudgetspec-core/src/lib.rs", f"// see {TOOL}: ignore[rule_name] the reason"),
    ],
)
def test_directive_shaped_text_outside_a_reasoned_comment_fails(where: str, line: str) -> None:
    assert boundary.scan_text(where, line) == [f"{where}:1: names {TOOL}"]


def test_a_directive_s_reason_is_scanned_like_any_other_text() -> None:
    rust = "crates/onebudgetspec-core/src/lib.rs"
    assert boundary.scan_text(rust, f"// {TOOL}: ignore[r] because {TOOL} says so") == [
        f"{rust}:1: names {TOOL}"
    ]
    assert boundary.scan_text(rust, f"// {TOOL}: ignore[r] onevcs needs it") == [
        f"{rust}:1: names onevcs"
    ]
    assert boundary.scan_text(rust, f"// {TOOL}: ignore[r] pending approval") == [
        f"{rust}:1: mentions approval"
    ]
    assert boundary.scan_text(rust, f"// {TOOL} ignore[r] reason") == [f"{rust}:1: names {TOOL}"]


@pytest.mark.parametrize(
    ("entry", "line"),
    [
        (
            "crates/onebudgetspec-core/src/lib.rs",
            f'pub const S: &str = "{TOOL}: ignore[r] the reason";',
        ),
        ("npm/cli/package.json", None),
        ("README.md", f"Suppress with `{TOOL}: ignore[rule] reason`."),
    ],
)
def test_directive_shaped_text_planted_in_a_shipped_file_fails_the_scan(
    tmp_path: Path, entry: str, line: str | None
) -> None:
    root = copy_tree(tmp_path, *LIBRARY)
    path = root / entry
    if line is None:
        document = json.loads(path.read_text())
        document["description"] = f"{TOOL}: ignore[r] the reason"
        path.write_text(json.dumps(document, indent=2))
    else:
        path.write_text(path.read_text() + f"\n{line}\n")
    findings = boundary.scan(root)
    assert any(f.startswith(f"{entry}:") and f.endswith(f"names {TOOL}") for f in findings), (
        findings
    )


def test_crediting_the_model_layout_is_the_one_exception() -> None:
    assert boundary.scan_text("x", "The layout is modelled on onetaskgraph.") == []
    assert boundary.scan_text("x", "Uses onetaskgraph.") == ["x:1: names onetaskgraph"]
    assert boundary.scan_text("x", "modelled on onetaskgraph and onevcs") == ["x:1: names onevcs"]


@pytest.mark.parametrize("bumped", [False, True], ids=["current", "released"])
def test_python_and_bun_dependencies_are_followed(tmp_path: Path, bumped: bool) -> None:
    root = copy_tree(tmp_path, *LIBRARY)
    uv = root / "uv.lock"
    if bumped:
        bump(uv)
    plant(uv, "onebudgetspec-sdk", '    { name = "onejudge-client" },\n')
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
