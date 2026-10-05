"""release-targets.toml and every platform list agree with what actually releases."""

import json
from pathlib import Path

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
    assert release.carrier_platforms() == {"linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64"}


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
        "rust-toolchain.toml lists ['darwin-arm64', 'linux-arm64', 'linux-x64']; "
        "npm/platforms holds ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64']"
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
