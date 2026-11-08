"""Saved observations cross the real CLI boundary; no measurement command exists."""

import json
from pathlib import Path

import pytest
from conftest import ROOT, run_cli

from onebudgetspec_sdk import (
    ComparisonInput,
    OnebudgetspecError,
    RollingHistory,
    compare,
    render_markdown,
)


def test_saved_history_roundtrips_and_renders_through_the_binary(
    tmp_path: Path, built_binary: Path
) -> None:
    """All ordinary results survive, including report-only failures and delivery errors."""
    current = json.loads(
        (ROOT / "crates/onebudgetspec-core/tests/fixtures/budget-run.json").read_text()
    )
    runs = [{**current, "head_sha": f"base-{i}"} for i in range(5)]
    history = RollingHistory.model_validate(
        {"schema_version": 1, "repository": "owner/repo", "base_ref": "main", "runs": runs}
    )
    history = RollingHistory.model_validate_json(history.model_dump_json())
    saved = ComparisonInput.model_validate(
        {"schema_version": 1, "current": current, "baseline": history.runs}
    )
    path = tmp_path / "input.json"
    path.write_text(saved.model_dump_json())
    report = compare(path, binary=built_binary)
    assert report.rows[0].verdict == "over"
    assert report.rows[0].baseline_n == 5
    assert report.rows[0].sigma_change == 0
    assert not report.rows[0].flag
    markdown = render_markdown(path, binary=built_binary)
    assert (
        markdown
        == run_cli(
            built_binary, ["report", "--input", str(path), "--output", "markdown"], tmp_path
        ).stdout
    )
    assert "| **latency** | 15.0 ms<br>+0.0% (+0.0 SD) | Over limit (report-only) |" in markdown
    failed = compare(path, binary=built_binary, reporter=tmp_path / "missing")
    assert failed.rows == report.rows
    assert any("cannot start reporter" in diagnostic for diagnostic in failed.diagnostics)
    path.write_text("{}")
    with pytest.raises(OnebudgetspecError, match="missing field"):
        compare(path, binary=built_binary)
    with pytest.raises(OnebudgetspecError, match="missing field"):
        render_markdown(path, binary=built_binary)
