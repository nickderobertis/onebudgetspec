"""Which binary a call runs, and how a call that gets no report fails.

Every candidate is a real executable that records its name and then runs the built binary,
so each test reads which one ran. The wheel's candidate is laid out as installing the
``onebudgetspec-cli`` wheel lays it out: a ``.dist-info`` on ``sys.path`` whose RECORD names
the script in the environment's ``bin``.
"""

import json
from dataclasses import dataclass
from pathlib import Path

import pytest
from conftest import run_cli

from onebudgetspec_sdk import (
    BINARY_ENV,
    OnebudgetspecError,
    check,
    list_budgets,
    resolve_binary,
    schema,
    validate,
)


def recording(directory: Path, name: str, log: Path, body: str) -> Path:
    """An executable ``onebudgetspec`` in ``directory`` that logs ``name``, then runs ``body``."""
    directory.mkdir(parents=True, exist_ok=True)
    program = directory / "onebudgetspec"
    program.write_text(f'#!/bin/sh\necho {name} >> "{log}"\n{body}\n')
    program.chmod(0o755)
    return program


@dataclass
class Candidates:
    """One recording binary in each place a call can find one, and the log they write."""

    log: Path
    explicit: Path
    variable: Path
    wheel: Path
    on_path: Path

    def ran(self) -> list[str]:
        """The names of the candidates that ran, in order."""
        return self.log.read_text().split() if self.log.exists() else []


def install_wheel_layout(environment: Path, body: str, log: Path) -> Path:
    """Lay out an installed ``onebudgetspec-cli``: its dist-info and its script."""
    site = environment / "lib" / "site-packages"
    info = site / "onebudgetspec_cli-0.1.0.dist-info"
    info.mkdir(parents=True)
    (info / "METADATA").write_text(
        "Metadata-Version: 2.4\nName: onebudgetspec-cli\nVersion: 0.1.0\n"
    )
    (info / "RECORD").write_text(
        "../../bin/onebudgetspec,,\nonebudgetspec_cli-0.1.0.dist-info/METADATA,,\n"
    )
    return recording(environment / "bin", "wheel", log, body)


@pytest.fixture
def candidates(tmp_path: Path, built_binary: Path, monkeypatch: pytest.MonkeyPatch) -> Candidates:
    """Every candidate present: an explicit one, the variable's, the wheel's and PATH's."""
    log = tmp_path / "ran.log"
    body = f'exec "{built_binary}" "$@"'
    found = Candidates(
        log=log,
        explicit=recording(tmp_path / "explicit", "explicit", log, body),
        variable=recording(tmp_path / "variable", "variable", log, body),
        wheel=install_wheel_layout(tmp_path / "venv", body, log),
        on_path=recording(tmp_path / "path", "path", log, body),
    )
    monkeypatch.setenv(BINARY_ENV, str(found.variable))
    monkeypatch.syspath_prepend(str(tmp_path / "venv" / "lib" / "site-packages"))
    monkeypatch.setenv("PATH", str(found.on_path.parent))
    return found


def test_an_explicit_binary_wins_over_the_variable_the_wheel_and_path(
    candidates: Candidates, built_binary: Path, tmp_path: Path
) -> None:
    """``binary=`` runs that binary, though the variable, the wheel and PATH each name one."""
    bundle = schema(binary=candidates.explicit)
    assert candidates.ran() == ["explicit"]
    assert bundle == json.loads(run_cli(built_binary, ["schema"], tmp_path).stdout)


def test_the_variable_wins_over_the_wheel_and_path(candidates: Candidates) -> None:
    """``ONEBUDGETSPEC_BIN`` runs over the wheel's binary and PATH's."""
    assert resolve_binary() == candidates.variable
    schema()
    assert candidates.ran() == ["variable"]


def test_the_wheel_s_binary_wins_over_path(
    candidates: Candidates, monkeypatch: pytest.MonkeyPatch
) -> None:
    """With no explicit binary and no variable, the cli wheel's binary runs over PATH's."""
    monkeypatch.delenv(BINARY_ENV)
    assert resolve_binary() == candidates.wheel
    schema()
    assert candidates.ran() == ["wheel"]


def test_an_empty_variable_is_not_a_binary(
    candidates: Candidates, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An empty ``ONEBUDGETSPEC_BIN`` is passed over, as if unset."""
    monkeypatch.setenv(BINARY_ENV, "")
    schema()
    assert candidates.ran() == ["wheel"]


def test_path_is_used_when_nothing_else_names_a_binary(
    tmp_path: Path, built_binary: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """With no explicit binary, no variable and no wheel, PATH's ``onebudgetspec`` checks."""
    log = tmp_path / "ran.log"
    on_path = recording(tmp_path / "path", "path", log, f'exec "{built_binary}" "$@"')
    monkeypatch.setenv("PATH", str(on_path.parent))
    assert resolve_binary() == on_path
    (tmp_path / "budgets.yaml").write_text(
        "schema_version: 1\nbudgets:\n  - id: quick\n    measure: elapsed\n"
        '    command: ["/bin/sh", "-c", "exit 0"]\n    unit: seconds\n'
        "    direction: max\n    threshold: 60\n"
    )
    report = check(cwd=tmp_path)
    assert [result.verdict for result in report.results] == ["within"]
    assert log.read_text().split() == ["path"]


def test_a_wheel_whose_script_is_gone_falls_through_to_path(
    candidates: Candidates, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A wheel whose recorded script is missing is passed over for PATH."""
    monkeypatch.delenv(BINARY_ENV)
    candidates.wheel.unlink()
    assert resolve_binary() == candidates.on_path


def test_no_binary_anywhere_is_an_error_naming_the_ways_to_provide_one(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Nothing to run raises, naming every way to provide a binary."""
    monkeypatch.setenv("PATH", str(tmp_path))
    with pytest.raises(OnebudgetspecError) as missing:
        schema()
    assert missing.value.exit_code is None
    for way in ("onebudgetspec-cli", "PATH", BINARY_ENV, "binary="):
        assert way in str(missing.value)


def test_a_binary_that_cannot_run_is_an_error(tmp_path: Path) -> None:
    """A binary that cannot be executed raises with no exit status."""
    with pytest.raises(OnebudgetspecError, match="cannot run") as failed:
        check(binary=tmp_path / "absent", cwd=tmp_path)
    assert failed.value.exit_code is None


def test_an_invalid_file_raises_with_the_cli_s_own_message(
    tmp_path: Path, built_binary: Path
) -> None:
    """Status 2 raises from every call with exactly the CLI's stderr."""
    (tmp_path / "budgets.yaml").write_text("schema_version: 1\nbudgets: nope\n")
    cli = run_cli(built_binary, ["validate"], tmp_path)
    assert cli.returncode == 2
    for call in (check, validate, list_budgets):
        with pytest.raises(OnebudgetspecError) as refused:
            call(cwd=tmp_path, binary=built_binary)
        assert refused.value.exit_code == 2
        assert str(refused.value) == cli.stderr.strip()
        assert "budgets.yaml" in str(refused.value)


def test_a_status_that_is_no_report_raises_with_what_the_binary_said(tmp_path: Path) -> None:
    """A status other than 0, 1 or 3 raises with the binary's stderr, or says how it ended."""
    log = tmp_path / "ran.log"
    refusing = recording(tmp_path / "a", "a", log, 'echo "launcher: no carrier" >&2; exit 69')
    with pytest.raises(OnebudgetspecError) as refused:
        check(binary=refusing, cwd=tmp_path)
    assert (refused.value.exit_code, str(refused.value)) == (69, "launcher: no carrier")

    silent = recording(tmp_path / "b", "b", log, "exit 70")
    with pytest.raises(OnebudgetspecError, match="exited 70 with no message"):
        check(binary=silent, cwd=tmp_path)

    killed = recording(tmp_path / "c", "c", log, "kill -9 $$")
    with pytest.raises(OnebudgetspecError, match="terminated by signal 9") as ended:
        validate(binary=killed, cwd=tmp_path)
    assert ended.value.exit_code == -9


@pytest.mark.parametrize(
    ("printed", "call", "reason"),
    [
        ("not json", check, "no valid CheckReport"),
        ('{"schema_version": 1, "budgets": [], "extra": 1}', list_budgets, "no valid ListReport"),
        (
            '{"schema_version": 1, "budgets": [{"id": "a", "file": "budgets.yaml", '
            '"description": null, "labels": [], "measure": "elapsed", "command": ["true"], '
            '"unit": "seconds", "direction": "max", "threshold": "60", "timeout_seconds": null}]}',
            list_budgets,
            "no valid ListReport",
        ),
        ("not json", schema, "not JSON"),
        ("[]", schema, "not a JSON object"),
    ],
)
def test_stdout_that_is_not_the_report_raises(
    tmp_path: Path, printed: str, call: object, reason: str
) -> None:
    """Stdout that is not the expected report raises rather than returning it."""
    program = recording(tmp_path / "bin", "liar", tmp_path / "ran.log", f"echo '{printed}'")
    assert callable(call)
    with pytest.raises(OnebudgetspecError, match=reason):
        call(binary=program)


@pytest.mark.parametrize(
    "arguments",
    [{"ids": "api"}, {"labels": ["api", 3]}, {"paths": "budgets.yaml"}, {"paths": Path("x")}],
)
def test_a_lone_string_or_a_non_string_is_refused_before_anything_runs(
    tmp_path: Path, arguments: dict
) -> None:
    """A bare string where a sequence belongs, or a non-string in one, is a TypeError."""
    with pytest.raises(TypeError):
        check(cwd=tmp_path, binary=tmp_path / "never-run", **arguments)


def test_a_value_shaped_like_a_flag_is_passed_as_a_value(
    tmp_path: Path, built_binary: Path
) -> None:
    """A label or path starting with ``-`` reaches the binary as a value, never a flag."""
    (tmp_path / "budgets.yaml").write_text(
        "schema_version: 1\nbudgets:\n  - id: quick\n    labels: [api]\n    measure: elapsed\n"
        '    command: ["/bin/sh", "-c", "exit 0"]\n    unit: seconds\n'
        "    direction: max\n    threshold: 60\n"
    )
    listed = list_budgets(labels=["--recursive"], cwd=tmp_path, binary=built_binary)
    assert listed.budgets == []
    with pytest.raises(OnebudgetspecError, match="--version"):
        list_budgets(paths=["--version"], cwd=tmp_path, binary=built_binary)
