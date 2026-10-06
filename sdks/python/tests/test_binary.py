"""Which binary a call runs, and how a call that gets no report fails.

Every candidate is a real executable that records its name and then runs the built binary,
so each test reads which one ran. The wheel's candidate is laid out as installing the
``onebudgetspec-cli`` wheel lays it out: a ``.dist-info`` on ``sys.path`` whose RECORD names
the binary in the environment's scripts directory, ``bin`` on Linux and macOS and ``Scripts``
on Windows. On Windows that binary is ``onebudgetspec.exe``, which only a real executable can
be, so there it is a copy of the built binary and records nothing.
"""

import json
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

import pytest
from conftest import WINDOWS, delegating, printing, run_cli, stand_in

import onebudgetspec_sdk
from onebudgetspec_sdk import (
    BINARY_ENV,
    OnebudgetspecError,
    check,
    list_budgets,
    resolve_binary,
    schema,
    validate,
)
from onebudgetspec_sdk._client import _require_validators
from onebudgetspec_sdk._generated.schemas import REPORT_SCHEMAS


def recording(
    directory: Path, name: str, log: Path, source: str, program: str = "onebudgetspec"
) -> Path:
    """An executable ``program`` in ``directory`` that logs ``name``, then runs ``source``."""
    logged = f"with open({str(log)!r}, 'a') as log:\n    log.write({name + chr(10)!r})\n"
    return stand_in(directory, logged + source, program)


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


@dataclass(frozen=True)
class Layout:
    """Where installing a wheel puts its site-packages and its binary, under the environment."""

    site: str
    scripts: str
    binary: str


#: A Linux or macOS environment's layout, and a Windows one's.
POSIX = Layout(site="lib/site-packages", scripts="bin", binary="onebudgetspec")
NT = Layout(site="Lib/site-packages", scripts="Scripts", binary="onebudgetspec.exe")
#: The layout installing a wheel on this host makes.
HOST = NT if WINDOWS else POSIX
#: Whether the wheel's candidate records that it ran: on Windows it is the built binary.
WHEEL_RECORDS = not WINDOWS


def lay_out_wheel(environment: Path, layout: Layout) -> Path:
    """Lay out an installed ``onebudgetspec-cli``'s dist-info; returns where its binary goes.

    Its RECORD names the binary as installers write it, relative to site-packages and with
    ``/`` separators on every platform.
    """
    # The SDK pins the CLI at its own version, so that is the version installed beside it.
    version = onebudgetspec_sdk.__version__
    site = environment / layout.site
    info = site / f"onebudgetspec_cli-{version}.dist-info"
    info.mkdir(parents=True)
    (info / "METADATA").write_text(
        f"Metadata-Version: 2.4\nName: onebudgetspec-cli\nVersion: {version}\n"
    )
    record = f"../../{layout.scripts}/{layout.binary},,\n{info.name}/METADATA,,\n"
    (info / "RECORD").write_text(record)
    return environment / layout.scripts / layout.binary


def install_wheel(environment: Path, log: Path, built_binary: Path) -> Path:
    """Install ``onebudgetspec-cli`` as this host lays it out; returns its binary."""
    binary = lay_out_wheel(environment, HOST)
    if WINDOWS:
        binary.parent.mkdir(parents=True)
        shutil.copy(built_binary, binary)
        return binary
    return recording(binary.parent, "wheel", log, delegating(built_binary), binary.name)


@pytest.fixture
def candidates(tmp_path: Path, built_binary: Path, monkeypatch: pytest.MonkeyPatch) -> Candidates:
    """Every candidate present: an explicit one, the variable's, the wheel's and PATH's."""
    log = tmp_path / "ran.log"
    body = delegating(built_binary)
    found = Candidates(
        log=log,
        explicit=recording(tmp_path / "explicit", "explicit", log, body),
        variable=recording(tmp_path / "variable", "variable", log, body),
        wheel=install_wheel(tmp_path / "venv", log, built_binary),
        on_path=recording(tmp_path / "path", "path", log, body),
    )
    monkeypatch.setenv(BINARY_ENV, str(found.variable))
    monkeypatch.syspath_prepend(str(tmp_path / "venv" / HOST.site))
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
    # On Windows the wheel's binary is the built one, which records nothing; that no other
    # candidate recorded shows it was the one that ran.
    assert candidates.ran() == (["wheel"] if WHEEL_RECORDS else [])


def test_an_empty_variable_is_not_a_binary(
    candidates: Candidates, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An empty ``ONEBUDGETSPEC_BIN`` is passed over, as if unset."""
    monkeypatch.setenv(BINARY_ENV, "")
    assert resolve_binary() == candidates.wheel
    schema()
    assert candidates.ran() == (["wheel"] if WHEEL_RECORDS else [])


def test_path_is_used_when_nothing_else_names_a_binary(
    tmp_path: Path, built_binary: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """With no explicit binary, no variable and no wheel, PATH's ``onebudgetspec`` checks."""
    log = tmp_path / "ran.log"
    on_path = recording(tmp_path / "path", "path", log, delegating(built_binary))
    monkeypatch.setenv("PATH", str(on_path.parent))
    assert resolve_binary() == on_path
    (tmp_path / "budgets.yaml").write_text(
        "schema_version: 1\nbudgets:\n  - id: quick\n    measure: elapsed\n"
        f"    command: {json.dumps([sys.executable, '-c', 'pass'])}\n    unit: seconds\n"
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


@pytest.mark.parametrize("layout", [POSIX, NT], ids=["bin", "Scripts"])
def test_the_wheel_s_binary_is_found_where_each_platform_installs_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, layout: Layout
) -> None:
    """The wheel's binary is found as each platform installs it, whichever host reads it.

    Linux and macOS install it as ``bin/onebudgetspec`` and Windows as
    ``Scripts/onebudgetspec.exe``. Only where it is found is checked, so the binary is a
    placeholder, never run.
    """
    binary = lay_out_wheel(tmp_path / "venv", layout)
    binary.parent.mkdir(parents=True)
    binary.write_text("the wheel's binary\n")
    monkeypatch.syspath_prepend(str(tmp_path / "venv" / layout.site))
    monkeypatch.setenv("PATH", str(tmp_path / "nothing-on-path"))
    assert resolve_binary() == binary


@pytest.mark.parametrize(
    "binary", ["bin/onebudgetspec.exe", "Scripts/onebudgetspec", "lib/onebudgetspec"]
)
def test_a_wheel_file_named_like_the_binary_elsewhere_is_not_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, binary: str
) -> None:
    """Only the two installed shapes are the binary; any other file the RECORD names is not."""
    shape = Layout(site=POSIX.site, scripts=binary.split("/")[0], binary=binary.split("/")[1])
    placed = lay_out_wheel(tmp_path / "venv", shape)
    placed.parent.mkdir(parents=True, exist_ok=True)
    placed.write_text("not the wheel's binary\n")
    monkeypatch.syspath_prepend(str(tmp_path / "venv" / POSIX.site))
    monkeypatch.setenv("PATH", str(tmp_path / "nothing-on-path"))
    with pytest.raises(OnebudgetspecError, match="no binary found"):
        resolve_binary()


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
    refusing = stand_in(
        tmp_path / "a", "import sys\nsys.stderr.write('launcher: no carrier\\n')\nsys.exit(69)\n"
    )
    with pytest.raises(OnebudgetspecError) as refused:
        check(binary=refusing, cwd=tmp_path)
    assert (refused.value.exit_code, str(refused.value)) == (69, "launcher: no carrier")

    silent = stand_in(tmp_path / "b", "raise SystemExit(70)\n")
    with pytest.raises(OnebudgetspecError, match="exited 70 with no message"):
        check(binary=silent, cwd=tmp_path)


# Windows has no signals: a process ended there by TerminateProcess has an exit status, which
# the case above covers, so no Windows run can end the binary this way.
@pytest.mark.skipif(WINDOWS, reason="Windows ends no process by a signal")
def test_a_binary_ended_by_a_signal_raises_naming_it(tmp_path: Path) -> None:
    """A binary killed by a signal raises naming the signal, its negated number the status."""
    killed = stand_in(tmp_path / "c", "import os\nos.kill(os.getpid(), 9)\n")
    with pytest.raises(OnebudgetspecError, match="terminated by signal 9") as ended:
        validate(binary=killed, cwd=tmp_path)
    assert ended.value.exit_code == -9


@pytest.mark.parametrize(
    ("printed", "call", "reason"),
    [
        ("not json", check, "printed no JSON"),
        ('{"schema_version": 1, "budgets": [], "extra": 1}', list_budgets, "no valid list-report"),
        (
            '{"schema_version": 1, "budgets": [{"id": "a", "file": "budgets.yaml", '
            '"description": null, "labels": [], "measure": "elapsed", "command": ["true"], '
            '"unit": "seconds", "direction": "max", "threshold": "60", "timeout_seconds": null}]}',
            list_budgets,
            "no valid list-report",
        ),
        ("not json", schema, "not JSON"),
        ("[]", schema, "not a JSON object"),
    ],
)
def test_stdout_that_is_not_the_report_raises(
    tmp_path: Path, printed: str, call: object, reason: str
) -> None:
    """Stdout that is not the expected report raises rather than returning it."""
    program = printing(tmp_path / "bin", printed + "\n")
    assert callable(call)
    with pytest.raises(OnebudgetspecError, match=reason):
        call(binary=program)


@pytest.mark.parametrize(
    "arguments",
    [
        {"ids": "api"},
        {"labels": ["api", 3]},
        {"paths": "budgets.yaml"},
        {"paths": Path("x")},
        {"recursive": "false"},
        {"recursive": 1},
    ],
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


def report_with(result: dict[str, str], host: dict[str, str] | None = None) -> str:
    """A check report with one result, its fields overridden by ``result`` and ``host``.

    It is written raw, so a value JSON allows but the schema's formats refuse can be printed.
    """
    fields = {
        "id": '"late"',
        "file": '"budgets.yaml"',
        "labels": "[]",
        "unit": '"seconds"',
        "direction": '"max"',
        "threshold": "1.0",
        "verdict": '"within"',
        "actual": "0.5",
        "headroom": "0.5",
        "headroom_percent": "50.0",
        "detail": "null",
        "error": "null",
        "started_at": '"2026-10-05T10:00:00.123456789Z"',
        "ended_at": '"2026-10-05T10:00:01.5+02:00"',
        **result,
    }
    host_fields = {
        "load1": "null",
        "cpus": "1",
        "mem_available_mib": "18446744073709551615",
        "conditions": "{}",
        **(host or {}),
    }

    def text(entries: dict[str, str]) -> str:
        return "{" + ", ".join(f'"{key}": {value}' for key, value in entries.items()) + "}"

    report = {
        "schema_version": "1",
        "results": "[" + text({**fields, "host": text(host_fields)}) + "]",
    }
    return text(report)


@pytest.mark.parametrize(
    ("result", "host"),
    [
        ({"started_at": '"2024-02-29T00:00:00Z"'}, {}),
        ({}, {"cpus": "4294967295"}),
        ({"ended_at": '"2026-10-05T23:59:59-23:59"'}, {}),
    ],
)
def test_a_report_at_the_edge_of_its_formats_is_returned(
    tmp_path: Path, result: dict[str, str], host: dict[str, str]
) -> None:
    """A leap day, the largest uint32 and the widest offset are all within the contract."""
    assert len(check(binary=printing(tmp_path, report_with(result, host))).results) == 1


def test_a_report_whose_every_format_holds_is_returned(tmp_path: Path) -> None:
    """Nanosecond timestamps, offsets and the largest uint64 are all within the contract."""
    report = check(binary=printing(tmp_path, report_with({})))
    [result] = report.results
    assert result.host.mem_available_mib == 2**64 - 1
    assert result.ended_at.utcoffset() is not None


@pytest.mark.parametrize(
    ("result", "host", "reason"),
    [
        ({"started_at": '"yesterday"'}, {}, "is not a 'date-time'"),
        ({"started_at": '"2026-02-30T00:00:00Z"'}, {}, "is not a 'date-time'"),
        ({"started_at": '"2100-02-29T00:00:00Z"'}, {}, "is not a 'date-time'"),
        ({"started_at": '"2026-13-01T00:00:00Z"'}, {}, "is not a 'date-time'"),
        ({"started_at": '"2026-10-00T00:00:00Z"'}, {}, "is not a 'date-time'"),
        ({"ended_at": '"2026-10-05T24:00:00Z"'}, {}, "is not a 'date-time'"),
        ({"ended_at": '"2026-10-05T10:60:00Z"'}, {}, "is not a 'date-time'"),
        ({"ended_at": '"2026-10-05T10:00:60Z"'}, {}, "is not a 'date-time'"),
        ({"ended_at": '"2026-10-05T10:00:00+24:00"'}, {}, "is not a 'date-time'"),
        ({"ended_at": '"2026-10-05T10:00:00+02:60"'}, {}, "is not a 'date-time'"),
        ({"started_at": "3"}, {}, "3 is not of type 'string'"),
        ({"actual": "1e400"}, {}, "is not a 'double'"),
        ({}, {"cpus": "4294967296"}, "is not a 'uint32'"),
        ({}, {"cpus": "-1"}, "is not a 'uint32'"),
        ({}, {"mem_available_mib": "18446744073709551616"}, "is not a 'uint64'"),
        ({}, {"mem_available_mib": "1.5"}, "is not of type 'integer', 'null'"),
        # An integral float is a JSON integer to the schema; the model refuses it as one.
        ({}, {"mem_available_mib": "1e30"}, "mem_available_mib"),
        ({"threshold": "true"}, {}, "True is not of type 'number'"),
        # Valid by the schema's own grammar, but not an instant Python can hold.
        ({"started_at": '"0000-01-01T00:00:00Z"'}, {}, "started_at"),
    ],
)
def test_a_report_breaking_its_schema_raises_naming_the_field(
    tmp_path: Path, result: dict[str, str], host: dict[str, str], reason: str
) -> None:
    """Each value the schema's types or formats refuse raises, naming what was wrong."""
    with pytest.raises(OnebudgetspecError, match="no valid check-report") as refused:
        check(binary=printing(tmp_path, report_with(result, host)))
    assert reason in str(refused.value)


def test_a_schema_version_of_true_is_not_1(tmp_path: Path) -> None:
    """``true`` is a JSON boolean, never the integer 1 the schema requires."""
    printed = report_with({}).replace('"schema_version": 1', '"schema_version": true')
    with pytest.raises(OnebudgetspecError, match="schema_version: 1 was expected"):
        check(binary=printing(tmp_path, printed))


def test_a_measurement_and_threshold_past_integer_range_come_back_as_doubles(
    tmp_path: Path, built_binary: Path
) -> None:
    """The binary prints ``1e20`` and ``1e300`` as doubles, and the SDK returns them as such."""
    (tmp_path / "report.py").write_text(
        "import os\n"
        "with open(os.environ['ONEBUDGETSPEC_RESULT'], 'w') as result:\n"
        "    result.write('{\"value\": 1e20}')\n"
    )
    (tmp_path / "budgets.yaml").write_text(
        "schema_version: 1\nbudgets:\n  - id: huge\n    measure: reported\n"
        f"    command: {json.dumps([sys.executable, 'report.py'])}\n"
        "    unit: bytes\n    direction: max\n    threshold: 1e300\n"
    )
    assert '"actual": 1e+20' in run_cli(built_binary, ["check", "--json"], tmp_path).stdout
    [result] = check(cwd=tmp_path, binary=built_binary).results
    assert (result.verdict, result.actual, result.threshold) == ("within", 1e20, 1e300)


def _bundle_without(built_binary: Path, tmp_path: Path, change: str) -> str:
    """The real bundle the binary prints, with one ``change`` applied to its shape."""
    bundle = json.loads(run_cli(built_binary, ["schema"], tmp_path).stdout)
    match change:
        case "empty":
            return "{}"
        case "boolean version":
            bundle["version"] = True
        case "roots array":
            bundle["roots"] = list(bundle["roots"].values())
        case "missing root":
            del bundle["roots"]["list-report"]
        case "root not an object":
            bundle["roots"]["check-report"] = "a schema"
    return json.dumps(bundle)


@pytest.mark.parametrize(
    "change", ["empty", "boolean version", "roots array", "missing root", "root not an object"]
)
def test_a_bundle_of_another_shape_raises_naming_what_to_do(
    built_binary: Path, tmp_path: Path, change: str
) -> None:
    """A bundle without an integer version and the three roots is never returned."""
    printed = _bundle_without(built_binary, tmp_path, change)
    program = printing(tmp_path / "bin", printed + "\n")
    with pytest.raises(OnebudgetspecError) as refused:
        schema(binary=program)
    message = str(refused.value)
    assert "is not a schema bundle" in message
    assert "budgets-file" in message and "list-report" in message
    assert "reinstall onebudgetspec-cli" in message


def test_a_generated_format_the_sdk_cannot_check_fails_naming_it(tmp_path: Path) -> None:
    """A report schema naming a format with no validator here is refused, never skipped.

    The format is a test-only stand-in planted in a copy of the generated schemas; the copy
    is imported and driven through ``check`` as a user would.
    """
    package = tmp_path / "onebudgetspec_sdk"
    shutil.copytree(
        Path(onebudgetspec_sdk.__file__).parent,
        package,
        ignore=shutil.ignore_patterns("__pycache__"),
    )
    schemas = package / "_generated" / "schemas.py"
    schemas.write_text(schemas.read_text().replace('"format": "uint32"', '"format": "uint16"', 1))
    program = printing(tmp_path, report_with({}, {"cpus": "70000"}))
    driven = subprocess.run(
        [
            sys.executable,
            "-c",
            "import sys; sys.path.insert(0, sys.argv[1]); import onebudgetspec_sdk as sdk; "
            "assert sdk.__file__.startswith(sys.argv[1]); "
            "print(sdk.check(binary=sys.argv[2]).results[0].host.cpus)",
            str(tmp_path),
            str(program),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert driven.returncode != 0, f"cpus {driven.stdout.strip()} passed an unchecked format"
    assert "'uint16'" in driven.stderr
    assert "has no validator" in driven.stderr
    assert "onebudgetspec-sdk release that matches the binary" in driven.stderr


def test_the_generated_schemas_name_only_checked_formats() -> None:
    """Every format the shipped report schemas name has a validator.

    A format without one is refused by the same check the import runs.
    """
    _require_validators(REPORT_SCHEMAS)
    with pytest.raises(OnebudgetspecError, match=r"\['uint16'\].*no validator"):
        _require_validators({"properties": {"n": {"type": "integer", "format": "uint16"}}})
