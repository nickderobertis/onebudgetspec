"""generate.py, run as ``just generate`` and ``just lint`` run it, over a copy of the package.

Each test copies the generator, its configuration and the committed generated files into a
temporary package and runs the generator there against the built binary, so nothing in the
tree is written.
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest
from conftest import ROOT

PACKAGE = ROOT / "sdks" / "python"
GENERATED = Path("src") / "onebudgetspec_sdk" / "_generated"


@pytest.fixture
def package(tmp_path: Path) -> Path:
    """A copy of the generator, its pyproject.toml and the committed generated files."""
    copy = tmp_path / "sdks" / "python"
    (copy / GENERATED).mkdir(parents=True)
    for name in ("generate.py", "pyproject.toml"):
        shutil.copy(PACKAGE / name, copy / name)
    for path in (PACKAGE / GENERATED).glob("*.py"):
        shutil.copy(path, copy / GENERATED / path.name)
    return copy


def generate(package: Path, binary: Path, *args: str) -> subprocess.CompletedProcess[str]:
    """Run the copied generator with ``binary`` as ``ONEBUDGETSPEC_BIN``."""
    return subprocess.run(
        [sys.executable, str(package / "generate.py"), *args],
        cwd=package,
        env={**os.environ, "ONEBUDGETSPEC_BIN": str(binary)},
        capture_output=True,
        text=True,
        check=False,
    )


def contents(package: Path) -> dict[str, str]:
    """Every entry under the generated directory, by relative path, outside ``__pycache__``.

    A regular file maps to its text and a symlink to ``-> <target>``, never followed, so any
    change the generator makes to either shows.
    """
    root = package / GENERATED
    found: dict[str, str] = {}
    for directory, subdirectories, names in os.walk(root, followlinks=False):
        here = Path(directory)
        subdirectories[:] = [name for name in subdirectories if name != "__pycache__"]
        for name in [*names, *subdirectories]:
            path = here / name
            relative = path.relative_to(root).as_posix()
            if path.is_symlink():
                found[relative] = f"-> {os.readlink(path)}"
            elif path.is_file():
                found[relative] = path.read_text()
    return found


def test_check_passes_on_the_committed_files_and_writes_nothing(
    package: Path, built_binary: Path
) -> None:
    """The committed models are what the current schema generates."""
    before = contents(package)
    checked = generate(package, built_binary, "--check")
    assert (checked.returncode, checked.stderr) == (0, "")
    assert contents(package) == before == contents(PACKAGE)


def test_check_names_stale_missing_and_extra_files_and_generate_repairs_them(
    package: Path, built_binary: Path
) -> None:
    """``--check`` names every difference and changes nothing; generating repairs them all."""
    committed = contents(package)
    (package / GENERATED / "check_report.py").write_text("# edited by hand\n")
    (package / GENERATED / "list_report.py").unlink()
    (package / GENERATED / "stray.py").write_text("")
    drifted = contents(package)

    checked = generate(package, built_binary, "--check")
    assert checked.returncode == 1
    for name in ("check_report.py", "list_report.py", "stray.py"):
        assert f"_generated/{name} differs" in checked.stderr
    assert "run 'just generate'" in checked.stderr
    assert contents(package) == drifted, "--check wrote to the generated files"

    assert generate(package, built_binary).returncode == 0
    assert contents(package) == committed
    assert generate(package, built_binary, "--check").returncode == 0


def fake_binary(directory: Path, printed: str) -> Path:
    """An executable that prints ``printed`` as its schema."""
    program = directory / "onebudgetspec"
    program.write_text(f"#!/bin/sh\nprintf '%s' '{printed}'\n")
    program.chmod(0o755)
    return program


@pytest.mark.parametrize(
    ("printed", "reason"),
    [
        ("not json", "the binary's schema is not JSON"),
        ('{"version": 1, "roots": {"check-report": {}}}', "lacks an integer `version`"),
        ('{"version": "1", "roots": {}}', "lacks an integer `version`"),
        ('{"version": true, "roots": {}}', "lacks an integer `version`"),
    ],
)
def test_a_bundle_of_another_shape_is_refused_with_a_next_step(
    package: Path, tmp_path: Path, printed: str, reason: str
) -> None:
    """A binary printing no usable bundle fails generation, naming what to do."""
    before = contents(package)
    refused = generate(package, fake_binary(tmp_path, printed))
    assert refused.returncode == 1
    assert reason in refused.stderr
    assert "run 'just generate'" in refused.stderr
    assert contents(package) == before


def test_a_missing_binary_is_refused_with_a_next_step(package: Path, tmp_path: Path) -> None:
    """A missing binary is named, with how to build it."""
    refused = generate(package, tmp_path / "absent", "--check")
    assert refused.returncode == 1
    assert "absent is missing" in refused.stderr
    assert "run 'just generate', which builds the binary first" in refused.stderr


def test_a_binary_that_fails_is_refused_with_its_message(package: Path, tmp_path: Path) -> None:
    """A binary exiting non-zero fails generation with its own stderr."""
    program = tmp_path / "onebudgetspec"
    program.write_text("#!/bin/sh\necho 'schema: broken' >&2\nexit 3\n")
    program.chmod(0o755)
    refused = generate(package, program)
    assert refused.returncode == 1
    assert "schema: broken" in refused.stderr


def test_an_unwritable_generated_file_is_refused_with_a_next_step(
    package: Path, built_binary: Path
) -> None:
    """A generated file the generator cannot write is named, with what to do."""
    stale = package / GENERATED / "check_report.py"
    stale.write_text("# stale\n")
    stale.chmod(0o444)
    refused = generate(package, built_binary)
    assert refused.returncode == 1
    assert "make it writable" in refused.stderr


def test_a_binary_that_cannot_execute_is_refused_with_a_next_step(
    package: Path, tmp_path: Path
) -> None:
    """A binary present but not executable is named, with what to do."""
    program = tmp_path / "onebudgetspec"
    program.write_text("not a program\n")
    program.chmod(0o644)
    refused = generate(package, program)
    assert refused.returncode == 1
    assert f"cannot run {program}" in refused.stderr
    assert "run 'just generate'" in refused.stderr


def test_a_missing_formatter_is_refused_with_a_next_step(
    package: Path, built_binary: Path, tmp_path: Path
) -> None:
    """Without ruff on PATH the generator says how to install it, and writes nothing."""
    before = contents(package)
    refused = subprocess.run(
        [sys.executable, str(package / "generate.py")],
        cwd=package,
        env={**os.environ, "ONEBUDGETSPEC_BIN": str(built_binary), "PATH": str(tmp_path)},
        capture_output=True,
        text=True,
        check=False,
    )
    assert refused.returncode == 1
    assert "ruff is not on PATH; run 'just bootstrap'" in refused.stderr
    assert contents(package) == before


def test_check_names_unexpected_entries_of_any_kind_and_generate_removes_them(
    package: Path, built_binary: Path, tmp_path: Path
) -> None:
    """Extras of any kind are named and removed; a link's target and the cache are kept."""
    committed = contents(package)
    generated = package / GENERATED
    (generated / "stray.json").write_text("{}\n")
    (generated / "notes" / "inner").mkdir(parents=True)
    (generated / "notes" / "inner" / "readme.txt").write_text("not a model\n")
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "keep.txt").write_text("keep me\n")
    (generated / "link").symlink_to(outside, target_is_directory=True)
    (generated / "__pycache__").mkdir()
    (generated / "__pycache__" / "x.cpython.pyc").write_bytes(b"cache")
    drifted = contents(package)

    checked = generate(package, built_binary, "--check")
    assert checked.returncode == 1, checked.stderr
    for name in ("stray.json", "notes", "link"):
        assert f"_generated/{name} differs" in checked.stderr
    assert "__pycache__" not in checked.stderr
    assert contents(package) == drifted, "--check changed the generated directory"

    regenerated = generate(package, built_binary)
    assert (regenerated.returncode, regenerated.stderr) == (0, "")
    assert contents(package) == committed
    assert not (generated / "link").is_symlink()
    assert (outside / "keep.txt").read_text() == "keep me\n", "the link's target was followed"
    assert (generated / "__pycache__" / "x.cpython.pyc").read_bytes() == b"cache"
    assert generate(package, built_binary, "--check").returncode == 0


def test_a_directory_or_link_where_a_model_belongs_is_stale_and_replaced(
    package: Path, built_binary: Path, tmp_path: Path
) -> None:
    """A model's name held by a directory or a symlink is named stale, never read through."""
    committed = contents(package)
    generated = package / GENERATED
    (generated / "check_report.py").unlink()
    (generated / "check_report.py").mkdir()
    (generated / "check_report.py" / "inside.txt").write_text("in the way\n")
    elsewhere = tmp_path / "elsewhere.py"
    elsewhere.write_text((PACKAGE / GENERATED / "list_report.py").read_text())
    (generated / "list_report.py").unlink()
    (generated / "list_report.py").symlink_to(elsewhere)

    checked = generate(package, built_binary, "--check")
    assert checked.returncode == 1, checked.stderr
    assert "_generated/check_report.py differs" in checked.stderr
    assert "_generated/list_report.py differs" in checked.stderr
    assert "cannot read or write" not in checked.stderr

    assert generate(package, built_binary).returncode == 0
    assert contents(package) == committed
    assert not (generated / "list_report.py").is_symlink()
    assert elsewhere.read_text() == (PACKAGE / GENERATED / "list_report.py").read_text()
    assert generate(package, built_binary, "--check").returncode == 0
