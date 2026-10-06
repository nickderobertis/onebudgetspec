"""``report`` writes a ``reported`` budget's result, and ``onebudgetspec check`` reads it."""

import json
import math
import sys
from pathlib import Path

import pytest

from onebudgetspec_sdk import check, report

RESULT_ENV = "ONEBUDGETSPEC_RESULT"


def test_with_the_variable_set_the_file_is_replaced_by_the_value_and_detail(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The result file holds exactly the value, and the detail when one is given."""
    result = tmp_path / "result.json"
    result.write_text("what an earlier write left behind")
    monkeypatch.setenv(RESULT_ENV, str(result))
    assert report(2.5, "two and a half") is True
    assert json.loads(result.read_text()) == {"value": 2.5, "detail": "two and a half"}
    assert report(3) is True
    assert json.loads(result.read_text()) == {"value": 3}


@pytest.mark.parametrize("variable", [None, ""])
def test_outside_a_check_nothing_is_written_and_report_returns_false(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, variable: str | None
) -> None:
    """Unset or empty, the variable names no file, so nothing is written."""
    if variable is None:
        monkeypatch.delenv(RESULT_ENV, raising=False)
    else:
        monkeypatch.setenv(RESULT_ENV, variable)
    monkeypatch.chdir(tmp_path)
    assert report(7, "detail") is False
    assert list(tmp_path.iterdir()) == []


@pytest.mark.parametrize("value", [math.nan, math.inf, -math.inf])
def test_a_non_finite_value_is_refused_without_writing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, value: float
) -> None:
    """NaN and the infinities are no measurement; the file is left as it was."""
    result = tmp_path / "result.json"
    result.write_text("untouched")
    monkeypatch.setenv(RESULT_ENV, str(result))
    with pytest.raises(ValueError, match="finite"):
        report(value)
    assert result.read_text() == "untouched"


def test_a_failed_write_raises(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A file that cannot be written raises the OSError writing it."""
    result = tmp_path / "no-such-directory" / "result.json"
    monkeypatch.setenv(RESULT_ENV, str(result))
    with pytest.raises(OSError):
        report(1)
    assert not result.exists()


def test_a_check_reports_what_a_python_measurement_reported(
    tmp_path: Path, built_binary: Path
) -> None:
    """Under the binary, what ``report`` wrote is the result's ``actual`` and ``detail``."""
    measure = tmp_path / "measure.py"
    measure.write_text(
        "import sys\n"
        "from onebudgetspec_sdk import report\n"
        "report(float(sys.argv[1]), *sys.argv[2:])\n"
    )
    (tmp_path / "budgets.yaml").write_text(
        "schema_version: 1\n"
        "budgets:\n"
        "  - id: p95\n"
        "    measure: reported\n"
        f"    command: [{json.dumps(sys.executable)}, measure.py, '1395.5', p95 of 200 requests]\n"
        "    unit: ms\n"
        "    direction: max\n"
        "    threshold: 1500\n"
        "  - id: bare\n"
        "    measure: reported\n"
        f"    command: [{json.dumps(sys.executable)}, measure.py, '1600']\n"
        "    unit: ms\n"
        "    direction: max\n"
        "    threshold: 1500\n"
    )
    checked = check(cwd=tmp_path, binary=built_binary)
    results = {result.id: result for result in checked.results}
    assert (results["p95"].actual, results["p95"].detail) == (1395.5, "p95 of 200 requests")
    assert results["p95"].verdict == "within"
    assert (results["bare"].actual, results["bare"].detail) == (1600, None)
    assert results["bare"].verdict == "over"
