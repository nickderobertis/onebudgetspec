"""The real Nx wrapper preserves diagnostics after large budget details."""

import json
import os
import shutil
import subprocess
from pathlib import Path

from repo_checks.paths import ROOT
from repo_checks.programs import bash


def test_a_failed_target_after_a_large_detail_remains_visible(tmp_path: Path) -> None:
    (tmp_path / "scripts").mkdir()
    shutil.copy2(ROOT / "scripts/nx", tmp_path / "scripts/nx")
    # Resolve the real installed executor and its dependencies from an isolated workspace.
    (tmp_path / "node_modules").symlink_to(ROOT / "node_modules", target_is_directory=True)
    (tmp_path / "nx.json").write_text("{}")
    (tmp_path / "package.json").write_text('{"name":"log-journey","private":true}')
    (tmp_path / "project.json").write_text(
        json.dumps(
            {
                "name": "log-journey",
                "targets": {
                    "budgets": {
                        "executor": "nx:run-commands",
                        "options": {"command": "node measure.cjs"},
                    }
                },
            }
        )
    )
    (tmp_path / "measure.cjs").write_text(
        'console.log("detail: " + "x".repeat(130000));\n'
        'console.error("budget broken: error — measurement exited 7");\nprocess.exit(3);\n'
    )
    failed = subprocess.run(
        [bash(), str(tmp_path / "scripts/nx"), "run", "log-journey:budgets"],
        env={**os.environ, "NX_DAEMON": "false", "NX_ISOLATE_PLUGINS": "false"},
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    assert failed.returncode == 1
    assert "budget broken: error — measurement exited 7" in failed.stderr
    assert "log-journey:budgets" in failed.stderr
    assert max(map(len, failed.stderr.splitlines())) <= 1000
    (tmp_path / "measure.cjs").write_text('console.log("budget recovered: within");\n')
    recovered = subprocess.run(
        [bash(), str(tmp_path / "scripts/nx"), "run", "log-journey:budgets"],
        env={**os.environ, "NX_DAEMON": "false", "NX_ISOLATE_PLUGINS": "false"},
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    assert recovered.returncode == 0, recovered.stderr
    assert recovered.stdout == ""
