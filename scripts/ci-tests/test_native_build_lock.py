"""The native build lock reports real setup and launch failures and recovers."""

import shutil
import subprocess
import sys
from pathlib import Path

from repo_checks.paths import ROOT


def test_lock_failures_name_the_cause_and_next_step(tmp_path: Path) -> None:
    scripts = tmp_path / "scripts"
    scripts.mkdir()
    helper = scripts / "native-build-lock.py"
    shutil.copy2(ROOT / "scripts/native-build-lock.py", helper)

    def invoke(*command: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(helper), *command],
            capture_output=True,
            text=True,
            check=False,
        )

    missing = invoke()
    assert missing.returncode == 64
    assert "usage:" in missing.stderr
    assert not (tmp_path / "target").exists()

    lock = tmp_path / "target/native-build.lock"
    lock.mkdir(parents=True)
    blocked = invoke(sys.executable, "-c", "raise SystemExit(0)")
    assert blocked.returncode == 1
    assert "cannot lock" in blocked.stderr
    assert str(lock) in blocked.stderr
    assert "ensure" in blocked.stderr
    lock.rmdir()

    absent = invoke(str(tmp_path / "missing-command"))
    assert absent.returncode == 1
    assert "cannot launch" in absent.stderr
    assert "missing-command" in absent.stderr
    assert "install" in absent.stderr

    failed = invoke(sys.executable, "-c", "raise SystemExit(37)")
    assert failed.returncode == 37
    recovered = invoke(sys.executable, "-c", "print('recovered')")
    assert recovered.returncode == 0, recovered.stderr
    assert recovered.stdout.strip() == "recovered"
