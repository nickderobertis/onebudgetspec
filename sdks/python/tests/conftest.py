"""What the SDK's tests share: the built binary, the conformance cases, and stand-ins."""

import os
import subprocess
import sys
from pathlib import Path

import pytest

from onebudgetspec_sdk import BINARY_ENV

ROOT = Path(__file__).resolve().parents[3]
CASES = ROOT / "conformance" / "cases"
#: Whether this host is Windows, which runs a program only by its extension.
WINDOWS = os.name == "nt"
#: The suffix this host's executables carry: cargo builds ``onebudgetspec.exe`` on Windows.
EXE = ".exe" if WINDOWS else ""


@pytest.fixture(scope="session")
def built_binary() -> Path:
    """The ``onebudgetspec`` cargo built, which Nx's sdk-python:test builds first."""
    binary = ROOT / "target" / "debug" / f"onebudgetspec{EXE}"
    assert binary.is_file(), f"{binary} is missing; build it with `cargo build -p onebudgetspec`"
    return binary


@pytest.fixture(autouse=True)
def no_binary_variable(monkeypatch: pytest.MonkeyPatch) -> None:
    """Start every test without ``ONEBUDGETSPEC_BIN``, whatever the caller's shell sets."""
    monkeypatch.delenv(BINARY_ENV, raising=False)


def run_cli(binary: Path, args: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    """Run the binary directly, as a person would, to compare the SDK with."""
    return subprocess.run(
        [str(binary), *args],
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        check=False,
    )


def stand_in(directory: Path, source: str, name: str = "onebudgetspec") -> Path:
    """An executable in ``directory`` that runs the Python ``source`` with its arguments.

    The source is a script beside it that this interpreter runs. On Linux and macOS the
    executable is ``name``, a ``/bin/sh`` script; Windows runs no script without an
    extension, so there it is ``name.cmd``, which ``PATH`` lookup finds through ``PATHEXT``
    and ``subprocess`` runs as Windows runs any batch file.
    """
    directory.mkdir(parents=True, exist_ok=True)
    script = directory / f"{name}-stand-in.py"
    script.write_text(source)
    if WINDOWS:
        program = directory / f"{name}.cmd"
        program.write_text(f'@"{sys.executable}" "{script}" %*\n')
        return program
    program = directory / name
    program.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{script}" "$@"\n')
    program.chmod(0o755)
    return program


def printing(directory: Path, stdout: str) -> Path:
    """An executable that prints ``stdout`` verbatim and exits 0."""
    return stand_in(directory, f"import sys\nsys.stdout.write({stdout!r})\n")


def delegating(binary: Path) -> str:
    """Stand-in source that runs ``binary`` with the stand-in's arguments and exits as it did."""
    return (
        "import subprocess, sys\n"
        f"sys.exit(subprocess.run([{str(binary)!r}, *sys.argv[1:]]).returncode)\n"
    )


def require_symlinks(directory: Path) -> None:
    """Skip the calling test when this host cannot make a symlink.

    Linux and macOS always can. Windows lets only an elevated process, or one with Developer
    Mode on, make one; what the test checks of a link is the same on every platform, and
    every Linux and macOS run checks it.
    """
    probe = directory / "symlink-probe"
    try:
        probe.symlink_to(directory, target_is_directory=True)
    except OSError as error:
        pytest.skip(f"this host cannot make a symlink ({error})")
    probe.unlink()
