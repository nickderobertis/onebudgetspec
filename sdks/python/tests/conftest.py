"""What the SDK's tests share: the built binary and the conformance cases."""

import subprocess
from pathlib import Path

import pytest

from onebudgetspec_sdk import BINARY_ENV

ROOT = Path(__file__).resolve().parents[3]
CASES = ROOT / "conformance" / "cases"


@pytest.fixture(scope="session")
def built_binary() -> Path:
    """The ``onebudgetspec`` cargo built, which Nx's sdk-python:test builds first."""
    binary = ROOT / "target" / "debug" / "onebudgetspec"
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
