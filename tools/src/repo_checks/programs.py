"""The programs the repository's scripts and checks start, found as a shell finds them."""

import shutil
import subprocess
import sys
from pathlib import Path


def program(name: str) -> str:
    """``name`` as found on PATH, or ``name`` itself when PATH holds no such program.

    Starting a bare name on Windows tries no extension but ``.exe``, so ``npm``
    (``npm.cmd``) is not found unless it is looked up first. Elsewhere this changes nothing.
    """
    return shutil.which(name) or name


def bash() -> str:
    """The bash the repository's scripts are written for: on Windows, Git's.

    Windows holds another ``bash`` in System32, the WSL launcher, which a bare ``bash``
    finds before PATH and which runs nothing without a Linux distribution. Git for
    Windows keeps its bash two directories above its exec path, under ``bin``.
    """
    if sys.platform != "win32":
        return program("bash")
    exec_path = subprocess.run(
        [program("git"), "--exec-path"], capture_output=True, text=True, check=True
    ).stdout.strip()
    found = Path(exec_path).parents[2] / "bin" / "bash.exe"
    if not found.is_file():
        raise FileNotFoundError(
            f"Git for Windows keeps no bash at {found}; install Git for Windows, whose bash"
            " runs the repository's scripts"
        )
    return str(found)
