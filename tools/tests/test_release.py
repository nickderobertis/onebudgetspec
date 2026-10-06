"""release-targets.toml and every platform list agree with what actually releases."""

import json
from pathlib import Path

import pytest
from conftest import copy_tree

from repo_checks import release

FILES = (
    "release-targets.toml",
    "rust-toolchain.toml",
    "README.md",
    "scripts",
    ".github/workflows/release.yml",
    "crates",
    "pyproject.toml",
    "sdks/python/pyproject.toml",
    "sdks/typescript/package.json",
    "npm",
)


def test_the_release_declaration_matches_the_workflow() -> None:
    assert release.target_problems() == []


def test_the_platform_lists_agree() -> None:
    assert release.platform_problems() == []
    assert release.carrier_platforms() == {
        "linux-x64",
        "linux-arm64",
        "darwin-x64",
        "darwin-arm64",
        "win32-x64",
        "win32-arm64",
    }


def test_a_renamed_target_and_a_wrong_manifest_are_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    path = root / "release-targets.toml"
    text = path.read_text().replace('name = "sdk-npm"', 'name = "typescript"')
    text = text.replace('manifest = "pyproject.toml"', 'manifest = "sdks/python/pyproject.toml"')
    path.write_text(text)
    problems = release.target_problems(root)
    assert any("not exactly" in problem for problem in problems), problems
    assert (
        "pypi: id names onebudgetspec-cli, sdks/python/pyproject.toml declares onebudgetspec-sdk"
        in problems
    )


def test_an_unpublished_target_and_a_stale_cover_are_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    workflow = root / ".github/workflows/release.yml"
    workflow.write_text(workflow.read_text().replace("scripts/release/publish.sh sdk-pypi", "true"))
    path = root / "release-targets.toml"
    path.write_text(path.read_text().replace('    "npm:@onebudgetspec/cli-darwin-x64",\n', ""))
    path.write_text(
        path.read_text().replace('probe = "scripts/release/release-probe.py"', 'probe = "nope"')
    )
    problems = release.target_problems(root)
    assert any(problem.startswith("release.yml publishes") for problem in problems), problems
    assert any(problem.startswith("sdk-pypi: published_by") for problem in problems), problems
    assert any(problem.startswith("npm: covers") for problem in problems), problems
    assert any("probe nope" in problem for problem in problems), problems


def test_a_platform_missing_anywhere_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    launcher = root / "npm/cli/lib/launcher.js"
    launcher.write_text(launcher.read_text().replace(', "darwin-x64"', ""))
    manifest = root / "npm/cli/package.json"
    document = json.loads(manifest.read_text())
    del document["optionalDependencies"]["@onebudgetspec/cli-linux-arm64"]
    manifest.write_text(json.dumps(document))
    build = root / "scripts/build-dist.sh"
    build.write_text(
        build.read_text().replace("    aarch64-apple-darwin) echo darwin-arm64 ;;\n", "")
    )
    problems = release.platform_problems(root)
    # The launcher, its manifest, the build script, and the release matrix and toolchain
    # (which the build script no longer maps), five places in all.
    assert len(problems) == 5, problems


def test_a_release_target_missing_from_the_toolchain_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    toolchain = root / "rust-toolchain.toml"
    toolchain.write_text(toolchain.read_text().replace('    "x86_64-apple-darwin",\n', ""))
    assert release.platform_problems(root) == [
        "rust-toolchain.toml lists ['darwin-arm64', 'linux-arm64', 'linux-x64', 'win32-arm64', "
        "'win32-x64']; npm/platforms holds ['darwin-arm64', 'darwin-x64', 'linux-arm64', "
        "'linux-x64', 'win32-arm64', 'win32-x64']"
    ]


@pytest.mark.parametrize(
    ("path", "line", "where"),
    [
        (
            "rust-toolchain.toml",
            '    "aarch64-pc-windows-msvc",\n',
            "rust-toolchain.toml",
        ),
        (
            "scripts/build-dist.sh",
            "    aarch64-pc-windows-msvc) echo win32-arm64 ;;\n",
            "scripts/build-dist.sh",
        ),
        (
            "scripts/build-dist.sh",
            "    MINGW*-aarch64 | MSYS*-aarch64 | CYGWIN*-aarch64 | MINGW*-arm64 | MSYS*-arm64 "
            "| CYGWIN*-arm64) echo aarch64-pc-windows-msvc ;;\n",
            "scripts/build-dist.sh's host_target",
        ),
        (
            ".github/workflows/release.yml",
            "          - { os: windows-11-arm, target: aarch64-pc-windows-msvc }\n",
            ".github/workflows/release.yml",
        ),
        (
            "npm/cli/package.json",
            '    "@onebudgetspec/cli-win32-x64": "workspace:*",\n',
            "npm/cli/package.json",
        ),
        (
            "crates/onebudgetspec-packaging-e2e/project.json",
            '    "npm-cli-win32-arm64",\n',
            "crates/onebudgetspec-packaging-e2e/project.json",
        ),
        (
            "crates/onebudgetspec-packaging-e2e/tests/packaging/npm_launcher.rs",
            '        ("windows", "aarch64") => "win32-arm64",\n',
            "crates/onebudgetspec-packaging-e2e/tests/packaging/npm_launcher.rs",
        ),
    ],
)
def test_a_windows_platform_missing_from_one_place_is_named(
    tmp_path: Path, path: str, line: str, where: str
) -> None:
    root = copy_tree(tmp_path, *FILES)
    listing = root / path
    text = listing.read_text()
    assert line in text, f"{path} no longer holds {line!r}; update this test"
    listing.write_text(text.replace(line, ""))
    problems = release.platform_problems(root)
    named = [problem for problem in problems if problem.startswith(f"{where} lists")]
    assert named, problems
    # The listing named lacks the dropped platform, which npm/platforms still holds.
    dropped = "win32-x64" if "win32-x64" in line else "win32-arm64"
    assert all(f"'{dropped}'" not in problem.split(";")[0] for problem in named), named


def test_a_windows_platform_missing_from_the_version_check_is_named(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        release.versions,
        "PLATFORMS",
        tuple(p for p in release.versions.PLATFORMS if p != "win32-x64"),
    )
    assert release.platform_problems() == [
        "tools/src/repo_checks/versions.py lists ['darwin-arm64', 'darwin-x64', 'linux-arm64', "
        "'linux-x64', 'win32-arm64']; npm/platforms holds ['darwin-arm64', 'darwin-x64', "
        "'linux-arm64', 'linux-x64', 'win32-arm64', 'win32-x64']"
    ]


def test_the_readme_documents_the_launcher_s_statuses() -> None:
    assert release.launcher_status_problems() == []


def test_a_launcher_status_the_readme_omits_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    readme = root / "README.md"
    readme.write_text(readme.read_text().replace(", and `70` when", ", and when"))
    assert release.launcher_status_problems(root) == [
        "README.md documents launcher statuses [64, 69]; "
        "npm/cli/lib/launcher.js returns [64, 69, 70]"
    ]


def test_a_carrier_whose_manifest_names_another_platform_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    manifest = root / "npm/platforms/darwin-arm64/package.json"
    document = json.loads(manifest.read_text())
    document["cpu"] = ["x64"]
    manifest.write_text(json.dumps(document))
    assert release.platform_problems(root) == [
        "npm/platforms/darwin-arm64/package.json declares "
        "('@onebudgetspec/cli-darwin-arm64', ['darwin'], ['x64']), "
        "not ('@onebudgetspec/cli-darwin-arm64', ['darwin'], ['arm64'])"
    ]


@pytest.mark.parametrize(
    ("declaration", "reason"),
    [
        ('target = "x"\n', "`target` is not a list of tables"),
        ("schema_version = 2\n", "`target` is not a list of tables"),
        ('[[target]]\nid = "crate:x"\n', "target 1 lacks a string `name`"),
        (
            '[[target]]\nid = "crate:x"\nname = "crate"\nwhat = "w"\npublished_by = "p"\n'
            "manifest = 3\n",
            "target 1 lacks a string `manifest`",
        ),
        (
            '[[target]]\nid = "crate:x"\nname = "crate"\nwhat = "w"\npublished_by = "p"\n'
            'manifest = "m"\ncovers = "crate:y"\n',
            "target 1's `covers` is not a list of strings",
        ),
    ],
)
def test_a_malformed_release_declaration_is_named(
    tmp_path: Path, declaration: str, reason: str
) -> None:
    (tmp_path / "release-targets.toml").write_text(declaration)
    with pytest.raises(release.InvalidDeclaration, match=reason):
        release.targets(tmp_path)


def test_each_publish_job_uploads_what_its_target_names() -> None:
    assert release.artifact_problems() == []
    by_name = {target["name"]: target for target in release.targets()}
    assert by_name[release.TargetName("sdk-pypi")]["id"] == "pypi:onebudgetspec-sdk"
    assert by_name[release.TargetName("sdk-npm")]["id"] == "npm:@onebudgetspec/sdk"


def test_a_job_publishing_another_package_s_artifact_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    workflow = root / ".github/workflows/release.yml"
    workflow.write_text(
        workflow.read_text()
        .replace("publish.sh sdk-pypi dist/sdk-pypi", "publish.sh sdk-pypi dist/sdk-npm")
        .replace("publish.sh sdk-npm dist/sdk-npm", "publish.sh sdk-npm dist/elsewhere")
    )
    assert release.artifact_problems(root) == [
        "sdk-pypi: publishes dist/sdk-npm, built as sdk-typescript (@onebudgetspec/sdk), "
        "not ['onebudgetspec-sdk']",
        "sdk-npm: publishes dist/elsewhere, which no build-dist.sh step fills",
    ]


def test_an_artifact_built_from_another_package_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    build = root / "scripts/build-dist.sh"
    build.write_text(
        build.read_text()
        .replace("--package onebudgetspec-sdk ", "--package onebudgetspec-repo-checks ")
        .replace("$ROOT/sdks/typescript", "$ROOT/npm/cli")
    )
    assert release.artifact_problems(root) == [
        "scripts/build-dist.sh sdk-python does not build onebudgetspec-sdk",
        "scripts/build-dist.sh sdk-typescript does not build @onebudgetspec/sdk",
    ]
    build.write_text(build.read_text().replace("  sdk-python)\n", "  sdk-py)\n"))
    assert any("release.py knows" in problem for problem in release.artifact_problems(root))


def test_a_target_whose_probe_asks_for_another_package_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    path = root / "release-targets.toml"
    path.write_text(
        path.read_text().replace('"npm:@onebudgetspec/sdk"', '"npm:@onebudgetspec/cli"')
    )
    assert (
        "sdk-npm: id names @onebudgetspec/cli, sdks/typescript/package.json declares "
        "@onebudgetspec/sdk" in release.target_problems(root)
    )
    assert release.artifact_problems(root) == [
        "sdk-npm: publishes dist/sdk-npm, built as sdk-typescript (@onebudgetspec/sdk), "
        "not ['@onebudgetspec/cli']"
    ]


def test_a_release_workflow_or_wheel_config_of_another_shape_is_named(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    pyproject = root / "pyproject.toml"
    pyproject.write_text(pyproject.read_text().replace("[tool.maturin]", "[tool.not-maturin]"))
    workflow = root / ".github/workflows/release.yml"
    workflow.write_text("on: release\njobs: [build, publish]\n")
    assert release.artifact_problems(root) == [
        "scripts/build-dist.sh cli-wheel does not build onebudgetspec-cli",
        ".github/workflows/release.yml: `jobs` is not a mapping of jobs",
    ]


def test_a_job_or_step_of_another_shape_publishes_nothing(tmp_path: Path) -> None:
    root = copy_tree(tmp_path, *FILES)
    workflow = root / ".github/workflows/release.yml"
    workflow.write_text(
        "on: release\njobs:\n  odd: just a string\n  stepless:\n    steps: none\n"
        "  publish:\n    steps:\n      - bash scripts/release/publish.sh sdk-npm dist/sdk-npm\n"
    )
    assert release.artifact_problems(root) == []
