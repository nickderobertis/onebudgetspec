"""``check_subjects`` against the built binary, over real directories and real commands.

Each subject's budgets file is read from its own directory, and its commands run there with
its own environment: one reads ``SUBJECT_MINUTES`` from the environment the subject gives,
the other a number from ``./measure.txt``. The SDK's typed answer must hold what the binary
prints when run directly.
"""

import json
from pathlib import Path
from typing import Any

import pytest
from conftest import run_cli

from onebudgetspec_sdk import (
    OnebudgetspecError,
    Subject,
    Subjects,
    SubjectsReport,
    check_subjects,
)

DELIVERY = (
    'require("fs").writeFileSync(process.env.ONEBUDGETSPEC_RESULT, '
    "JSON.stringify({ value: Number(process.env.SUBJECT_MINUTES) }));"
)
GATE_RUNS = (
    'const fs = require("fs"); fs.writeFileSync("gate-runs.ran", ""); '
    "fs.writeFileSync(process.env.ONEBUDGETSPEC_RESULT, "
    'JSON.stringify({ value: Number(fs.readFileSync("./measure.txt", "utf8")) }));'
)


def _budget(budget_id: str, label: str, script: str, unit: str, threshold: int) -> str:
    return (
        f"  - id: {budget_id}\n"
        f"    labels: [{label}]\n"
        "    measure: reported\n"
        f"    command: {json.dumps(['node', '-e', script])}\n"
        f"    unit: {unit}\n"
        "    direction: max\n"
        f"    threshold: {threshold}\n"
    )


DELIVERY_TIME = _budget("delivery-time", "delivery", DELIVERY, "minutes", 10)
GATE_RUNS_BUDGET = _budget("gate-runs", "gates", GATE_RUNS, "runs", 5)


@pytest.fixture
def subjects_dir(tmp_path: Path) -> Path:
    """Lay out the four subjects' directories under ``tmp_path``.

    Two hold both budgets and their own ``measure.txt``, one no budgets file, and one only
    the ``gates`` budget.
    """
    for name, runs in (("change-a", "2"), ("change-b", "7")):
        (tmp_path / name).mkdir()
        (tmp_path / name / "budgets.yaml").write_text(
            f"schema_version: 1\nbudgets:\n{DELIVERY_TIME}{GATE_RUNS_BUDGET}"
        )
        (tmp_path / name / "measure.txt").write_text(runs)
    (tmp_path / "change-c").mkdir()
    (tmp_path / "change-d").mkdir()
    (tmp_path / "change-d" / "budgets.yaml").write_text(
        f"schema_version: 1\nbudgets:\n{GATE_RUNS_BUDGET}"
    )
    (tmp_path / "change-d" / "measure.txt").write_text("1")
    return tmp_path


def document(*, with_d: bool = False) -> dict[str, Any]:
    """The subjects document, with directories relative to the fixture."""
    subjects: list[dict[str, Any]] = [
        {
            "id": "change-a",
            "directory": "change-a",
            "env": {"SUBJECT_MINUTES": "30"},
            "context": {"node": 1, "title": "first"},
        },
        {
            "id": "change-b",
            "directory": "change-b",
            "env": {"SUBJECT_MINUTES": "4"},
            "files": ["budgets.yaml"],
            "context": ["second", 2],
        },
        {"id": "change-c", "directory": "change-c", "context": "third"},
    ]
    if with_d:
        subjects.append({"id": "change-d", "directory": "change-d"})
    return {"schema_version": 1, "subjects": subjects}


def verdicts(answer: SubjectsReport) -> dict[str, list[tuple[str, str, float | None]]]:
    """Each checked subject's ``(budget, verdict, actual)`` triples, by subject id."""
    return {
        subject.id: [
            (result.id, result.verdict, result.actual) for result in subject.report.results
        ]
        for subject in answer.subjects
    }


def cli_answer(
    built_binary: Path, cwd: Path, document: dict[str, Any], *args: str
) -> SubjectsReport:
    """What the binary prints for ``document``, run directly."""
    (cwd / "subjects.json").write_text(json.dumps(document))
    ran = run_cli(built_binary, ["check", "--subjects", "subjects.json", "--json", *args], cwd)
    assert ran.returncode == 1, ran.stderr
    return SubjectsReport.model_validate_json(ran.stdout)


def test_each_subject_is_answered_as_the_binary_answers_it(
    built_binary: Path, subjects_dir: Path
) -> None:
    """Each verdict, context and unanswered reason is the binary's own."""
    answer = check_subjects(document(), cwd=subjects_dir, binary=built_binary)
    assert isinstance(answer, SubjectsReport)
    assert verdicts(answer) == {
        "change-a": [("delivery-time", "over", 30.0), ("gate-runs", "within", 2.0)],
        "change-b": [("delivery-time", "within", 4.0), ("gate-runs", "over", 7.0)],
    }
    assert [(subject.id, subject.context) for subject in answer.subjects] == [
        ("change-a", {"node": 1, "title": "first"}),
        ("change-b", ["second", 2]),
    ]
    [unanswered] = answer.unanswered
    assert (unanswered.id, unanswered.context) == ("change-c", "third")
    assert "no such file" in unanswered.reason

    cli = cli_answer(built_binary, subjects_dir, document())
    assert verdicts(answer) == verdicts(cli)
    assert [(s.id, s.context) for s in answer.subjects] == [(s.id, s.context) for s in cli.subjects]
    assert answer.unanswered == cli.unanswered


def test_a_label_selection_and_an_empty_one(built_binary: Path, subjects_dir: Path) -> None:
    """A label keeps one budget per subject; a subject it keeps none of is unanswered."""
    answer = check_subjects(
        document(with_d=True), labels=["delivery"], cwd=subjects_dir, binary=built_binary
    )
    assert verdicts(answer) == {
        "change-a": [("delivery-time", "over", 30.0)],
        "change-b": [("delivery-time", "within", 4.0)],
    }
    assert not list(subjects_dir.glob("*/gate-runs.ran")), "an unselected command ran"
    assert [entry.id for entry in answer.unanswered] == ["change-c", "change-d"]
    empty = answer.unanswered[1]
    assert empty.context is None
    assert "selection is empty" in empty.reason
    cli = cli_answer(built_binary, subjects_dir, document(with_d=True), "--label", "delivery")
    assert verdicts(answer) == verdicts(cli)
    assert answer.unanswered == cli.unanswered


def test_the_typed_document_and_over_only(built_binary: Path, subjects_dir: Path) -> None:
    """The generated model is accepted, and ``over_only`` keeps only over results."""
    typed = Subjects(
        schema_version=1,
        subjects=[
            Subject(id="change-a", directory="change-a", env={"SUBJECT_MINUTES": "30"}),
            Subject(id="change-b", directory="change-b", env={"SUBJECT_MINUTES": "4"}),
        ],
    )
    answer = check_subjects(typed, over_only=True, cwd=subjects_dir, binary=built_binary)
    assert verdicts(answer) == {
        "change-a": [("delivery-time", "over", 30.0)],
        "change-b": [("gate-runs", "over", 7.0)],
    }
    assert [subject.context for subject in answer.subjects] == [None, None]
    assert answer.unanswered == []


def test_an_invalid_document_raises_with_the_binarys_message(
    built_binary: Path, subjects_dir: Path
) -> None:
    """A document the binary refuses raises with exit status 2, running nothing."""
    duplicated = {
        "schema_version": 1,
        "subjects": [
            {"id": "x", "directory": "change-a"},
            {"id": "x", "directory": "change-b"},
        ],
    }
    with pytest.raises(OnebudgetspecError, match=r"subjects\[1\]\.id") as raised:
        check_subjects(duplicated, cwd=subjects_dir, binary=built_binary)
    assert raised.value.exit_code == 2
    assert not list(subjects_dir.glob("*/gate-runs.ran"))


@pytest.mark.parametrize(
    "arguments",
    [
        {"subjects": "subjects.json"},
        {"subjects": {"schema_version": 1, "subjects": []}, "over_only": "yes"},
    ],
)
def test_arguments_of_the_wrong_type_are_refused(arguments: dict) -> None:
    """A path in place of the document, or a flag that is not a bool, is refused unrun."""
    with pytest.raises(TypeError):
        check_subjects(binary="never-run", **arguments)
