"""Every conformance case, run through the SDK's own calls against the built binary.

Each case's ``args`` become a ``check`` call, and the same files and selection a
``list_budgets`` and a ``validate`` call. The check report, normalized as
``conformance/README.md`` defines, must equal the case's ``expected.json``; a case the CLI
refuses must raise with the CLI's own message; and ``list_budgets`` and ``validate`` must
answer exactly what the binary answers when run directly.
"""

import json
import shutil
from dataclasses import dataclass, field
from pathlib import Path

import pytest
from conftest import CASES, run_cli

from onebudgetspec_sdk import (
    BINARY_ENV,
    CheckReport,
    OnebudgetspecError,
    check,
    list_budgets,
    schema,
    validate,
)

CASE_DIRS = sorted(path for path in CASES.iterdir() if path.is_dir())
EPOCH = "1970-01-01T00:00:00Z"


@dataclass
class Invocation:
    """A case's ``args``, read as the SDK's parameters."""

    verb: str
    paths: list[str] = field(default_factory=list)
    ids: list[str] = field(default_factory=list)
    labels: list[str] = field(default_factory=list)
    exclude_labels: list[str] = field(default_factory=list)
    recursive: bool = False

    def files_args(self) -> list[str]:
        """The CLI flags and operands naming the files."""
        return [*(["--recursive"] if self.recursive else []), *self.paths]

    def selection_args(self) -> list[str]:
        """The CLI flags selecting budgets."""
        return [
            *(word for value in self.ids for word in ("--id", value)),
            *(word for value in self.labels for word in ("--label", value)),
            *(word for value in self.exclude_labels for word in ("--exclude-label", value)),
        ]


def parse(args: list[str]) -> Invocation:
    """Read ``args`` with the grammar the cases use; anything else fails, so a case using a
    flag this runner cannot pass to the SDK is never silently dropped."""
    verb, *rest = args
    invocation = Invocation(verb)
    words = iter(rest)
    for word in words:
        match word:
            case "--json":
                pass
            case "--recursive":
                invocation.recursive = True
            case "--id":
                invocation.ids.append(next(words))
            case "--label":
                invocation.labels.append(next(words))
            case "--exclude-label":
                invocation.exclude_labels.append(next(words))
            case _ if word.startswith("-"):
                raise AssertionError(f"the SDK runner cannot pass {word!r}; teach parse() it")
            case _:
                invocation.paths.append(word)
    return invocation


def normalize(report: dict, case: dict, name: str) -> dict:
    """The normalization conformance/README.md defines, applied to a report in place."""
    timed = case.get("timed", [])
    for result in report["results"]:
        result["started_at"] = EPOCH
        result["ended_at"] = EPOCH
        result["host"].update(load1=0.0, cpus=1, mem_available_mib=0)
        if result["error"] is not None:
            expected = case.get("error_contains", {}).get(result["id"])
            if expected is not None:
                assert expected in result["error"], f"{name}: {result['id']}: {result['error']}"
            result["error"] = "<error>"
        if result["id"] in timed:
            for key in ("actual", "headroom", "headroom_percent"):
                if result[key] is not None:
                    result[key] = 0.0
    return report


def exit_status(report: CheckReport) -> int:
    """The status the CLI exits with for ``report``: 3 on an error, 1 when over, else 0."""
    verdicts = {result.verdict for result in report.results}
    return 3 if "error" in verdicts else 1 if "over" in verdicts else 0


@pytest.fixture
def case(request: pytest.FixtureRequest, tmp_path: Path) -> tuple[str, dict, Path]:
    """The case's name, its case.json and a fresh copy of its directory to run in."""
    source: Path = request.param
    work = tmp_path / source.name
    shutil.copytree(source, work)
    return source.name, json.loads((source / "case.json").read_text()), work


def ids(path: Path) -> str:
    """The case's directory name, as its test id."""
    return path.name


@pytest.mark.parametrize("case", CASE_DIRS, ids=ids, indirect=True)
def test_check_returns_the_report_the_case_expects(
    case: tuple[str, dict, Path], built_binary: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    name, spec, work = case
    monkeypatch.setenv(BINARY_ENV, str(built_binary))
    invocation = parse(spec["args"])
    assert invocation.verb == "check", f"{name}: the cases are check invocations"

    def call() -> CheckReport:
        return check(
            paths=invocation.paths or None,
            ids=invocation.ids or None,
            labels=invocation.labels or None,
            exclude_labels=invocation.exclude_labels or None,
            recursive=invocation.recursive,
            cwd=work,
        )

    if spec["exit"] == 2:
        with pytest.raises(OnebudgetspecError) as refused:
            call()
        assert refused.value.exit_code == 2
        cli = run_cli(built_binary, spec["args"], work)
        assert cli.returncode == 2
        assert str(refused.value) == cli.stderr.strip(), f"{name}: not the CLI's own message"
        assert not (work / "expected.json").exists()
        return

    report = call()
    assert exit_status(report) == spec["exit"], f"{name}: the verdicts earn another status"
    expected = json.loads((work / "expected.json").read_text())
    actual = normalize(report.model_dump(mode="json"), spec, name)
    assert actual == expected, f"{name}: the normalized report differs from expected.json"


@pytest.mark.parametrize("case", CASE_DIRS, ids=ids, indirect=True)
def test_list_and_validate_answer_as_the_binary_does(
    case: tuple[str, dict, Path], built_binary: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    name, spec, work = case
    monkeypatch.setenv(BINARY_ENV, str(built_binary))
    invocation = parse(spec["args"])

    listing = run_cli(
        built_binary,
        ["list", "--json", *invocation.selection_args(), *invocation.files_args()],
        work,
    )
    if listing.returncode == 2:
        with pytest.raises(OnebudgetspecError) as refused:
            list_budgets(
                paths=invocation.paths or None,
                ids=invocation.ids,
                labels=invocation.labels,
                exclude_labels=invocation.exclude_labels,
                recursive=invocation.recursive,
                cwd=work,
            )
        assert (refused.value.exit_code, str(refused.value)) == (2, listing.stderr.strip())
        assert spec["exit"] == 2, f"{name}: list refused what check accepted"
    else:
        listed = list_budgets(
            paths=invocation.paths or None,
            ids=invocation.ids,
            labels=invocation.labels,
            exclude_labels=invocation.exclude_labels,
            recursive=invocation.recursive,
            cwd=work,
        )
        assert listed.model_dump(mode="json") == json.loads(listing.stdout)
        # What list selects is what check measured, in the same order.
        expected = json.loads((work / "expected.json").read_text())["results"]
        keys = ("id", "file", "labels", "unit", "direction", "threshold")
        assert [{key: getattr(budget, key) for key in keys} for budget in listed.budgets] == [
            {key: result[key] for key in keys} for result in expected
        ], name

    validation = run_cli(built_binary, ["validate", "--json", *invocation.files_args()], work)
    if validation.returncode == 2:
        with pytest.raises(OnebudgetspecError) as refused:
            validate(paths=invocation.paths or None, recursive=invocation.recursive, cwd=work)
        assert (refused.value.exit_code, str(refused.value)) == (2, validation.stderr.strip())
    else:
        assert validation.returncode == 0
        validated = validate(paths=invocation.paths, recursive=invocation.recursive, cwd=work)
        assert validated.model_dump(mode="json") == json.loads(validation.stdout)


def _case_report(name: str, built_binary: Path, tmp_path: Path) -> CheckReport:
    work = tmp_path / name
    shutil.copytree(CASES / name, work)
    return check(cwd=work, binary=built_binary)


def test_returned_conditions_are_read_beside_the_declared_ones(
    built_binary: Path, tmp_path: Path
) -> None:
    gate, plain = _case_report("returned-conditions", built_binary, tmp_path).results
    assert gate.verdict == "within"
    assert gate.host.conditions == {"dispatches": "3", "dispatches_max": "6", "gate_load": "2.1"}
    assert plain.host.conditions == {"dispatches": "3"}


def test_colliding_and_malformed_returned_conditions_are_error_results(
    built_binary: Path, tmp_path: Path
) -> None:
    collides, host_value, fine = _case_report(
        "returned-condition-collides", built_binary, tmp_path
    ).results
    for result in (collides, host_value):
        assert (result.verdict, result.actual) == ("error", None)
        assert result.host.conditions == {"dispatches": "3"}
    assert collides.error is not None and "dispatches" in collides.error
    assert host_value.error is not None and "load1" in host_value.error
    assert fine.verdict == "within"

    malformed = _case_report("returned-condition-malformed", built_binary, tmp_path).results
    assert [result.verdict for result in malformed] == ["error", "error", "error"]
    assert all(result.host.conditions == {} for result in malformed)


def test_schema_is_the_bundle_the_binary_prints(built_binary: Path, tmp_path: Path) -> None:
    printed = run_cli(built_binary, ["schema"], tmp_path)
    assert printed.returncode == 0
    assert schema(binary=built_binary) == json.loads(printed.stdout)
