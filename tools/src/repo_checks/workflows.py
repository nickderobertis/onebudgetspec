"""Find the workflow jobs that need a credential, and run their guards locally.

No hosted run can show a guard before the repository is provisioned, so this executes the
guard's own logic instead: each guard job's ``scripts/ci-guard.sh`` step is run with the
secrets it reads set or unset, and each job-level ``if:`` is evaluated over those outputs
and the ``*_PUBLISH`` variables.
"""

import json
import os
import re
import subprocess
import tempfile
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Literal, TypedDict, cast

import yaml

from repo_checks.expressions import Status, Value, evaluate
from repo_checks.paths import ROOT
from repo_checks.programs import bash

Secret = Literal[
    "CARGO_REGISTRY_TOKEN",
    "PYPI_TOKEN",
    "NPM_TOKEN",
    "RELEASE_PLZ_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "OPENAI_API_KEY",
]
PublishVariable = Literal["CARGO_PUBLISH", "PYPI_PUBLISH", "NPM_PUBLISH"]


class Step(TypedDict, total=False):
    """The fields of a workflow step this module reads."""

    id: str
    run: str
    env: dict[str, str]


#: The fields of a workflow job this module reads (`if` is a keyword, hence this form).
JobBody = TypedDict(
    "JobBody",
    {
        "if": str,
        "needs": str | list[str],
        "steps": list[Step],
        "outputs": dict[str, str],
        "env": dict[str, str],
    },
    total=False,
)

#: The secrets publishing and the judged lint need; none exists until provisioning.
SECRETS: tuple[Secret, ...] = (
    "CARGO_REGISTRY_TOKEN",
    "PYPI_TOKEN",
    "NPM_TOKEN",
    "RELEASE_PLZ_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "OPENAI_API_KEY",
)
#: The repository variables that switch each registry's publication on.
PUBLISH_VARIABLES: tuple[PublishVariable, ...] = ("CARGO_PUBLISH", "PYPI_PUBLISH", "NPM_PUBLISH")
GUARD_SCRIPT = "scripts/ci-guard.sh"
#: The one shape a guard step's `run` may take, so running it locally runs the guard alone.
_GUARD_RUN = re.compile(
    r"bash scripts/ci-guard\.sh(?P<args>(?: --any)?(?: [A-Za-z_][A-Za-z0-9_]*)*)"
)

_SECRET = re.compile(r"secrets\.([A-Z0-9_]+)")
_VARIABLE = re.compile(r"vars\.([A-Z0-9_]+)")
_REFERENCE = re.compile(r"\$\{\{\s*([^}]*?)\s*\}\}")


class GuardFailed(AssertionError):
    """A guard step exited non-zero, which would fail the job instead of skipping it."""


@dataclass(frozen=True)
class Job:
    """One job of one workflow."""

    workflow: str
    name: str
    body: JobBody
    siblings: Mapping[str, JobBody]

    @property
    def key(self) -> str:
        """``<workflow file>:<job id>``."""
        return f"{self.workflow}:{self.name}"

    @property
    def condition(self) -> str:
        """The job-level ``if:``, or ``""`` when it has none."""
        return str(self.body.get("if", ""))

    @property
    def needs(self) -> list[str]:
        """The jobs this one needs."""
        needs = self.body.get("needs", [])
        return [needs] if isinstance(needs, str) else list(needs)

    def text(self) -> str:
        """The job as text, for finding what it reads."""
        return json.dumps(self.body)

    def secrets(self) -> set[str]:
        """The listed secrets this job reads anywhere in its steps or environment."""
        return set(_SECRET.findall(self.text())) & set(SECRETS)

    def variables(self) -> set[str]:
        """The ``*_PUBLISH`` variables this job reads."""
        return set(_VARIABLE.findall(self.text())) & set(PUBLISH_VARIABLES)

    def is_guard(self) -> bool:
        """Whether this job is a guard: one whose steps run ``scripts/ci-guard.sh``."""
        return any(GUARD_SCRIPT in str(step.get("run", "")) for step in self.steps())

    def steps(self) -> list[Step]:
        """The job's steps."""
        return list(self.body.get("steps", []))


class InvalidWorkflow(ValueError):
    """A workflow file is not the shape the guard checks read."""


def _document(path: Path) -> dict:
    document = yaml.safe_load(path.read_text())
    if not isinstance(document, dict):
        raise InvalidWorkflow(f"{path.name} is not a mapping")
    return document


def _scalar(value: object) -> str | None:
    """A YAML scalar as GitHub reads it in an expression context, or None for any other value."""
    match value:
        case bool():
            return "true" if value else "false"
        case str() | int() | float():
            return str(value)
        case _:
            return None


def _strings(value: object, where: str) -> dict[str, str]:
    """``value`` as a mapping of names to scalars, each read as a string."""
    if not isinstance(value, dict):
        raise InvalidWorkflow(f"{where} is not a mapping of names to values")
    read = {name: _scalar(item) for name, item in value.items()}
    if not all(isinstance(name, str) for name in read) or None in read.values():
        raise InvalidWorkflow(f"{where} is not a mapping of names to values")
    return {str(name): item for name, item in read.items() if item is not None}


def _step(where: str, step: object) -> Step:
    """``step``, its id, run and env checked and read as strings; other keys kept."""
    if not isinstance(step, dict):
        raise InvalidWorkflow(f"{where} is not a mapping")
    for field in ("id", "run"):
        if field in step and not isinstance(step[field], str):
            raise InvalidWorkflow(f"{where} `{field}` is not a string")
    read: dict = dict(step)
    if "env" in step:
        read["env"] = _strings(step["env"], f"{where} `env`")
    return cast(Step, read)


def _job(workflow: str, name: object, body: object) -> JobBody:
    """``body`` with each field this module reads checked and read as GitHub reads it.

    Keys this module does not model are kept, so a secret read anywhere in the job (in a
    step's `with:`, say) is still found by scanning it.
    """
    where = f"{workflow}: job {name}"
    if not isinstance(body, dict):
        raise InvalidWorkflow(f"{where} is not a mapping")
    read: dict = dict(body)
    match body.get("if", ""):
        case bool() as condition:
            read["if"] = "true" if condition else "false"
        case str():
            pass
        case _:
            raise InvalidWorkflow(f"{where}'s `if` is not a string")
    needs = body.get("needs", [])
    if not (
        isinstance(needs, str)
        or (isinstance(needs, list) and all(isinstance(n, str) for n in needs))
    ):
        raise InvalidWorkflow(f"{where}'s `needs` is not a job id or a list of them")
    steps = body.get("steps", [])
    if not isinstance(steps, list):
        raise InvalidWorkflow(f"{where}'s `steps` is not a list of mappings")
    if not all(isinstance(step, dict) for step in steps):
        raise InvalidWorkflow(f"{where}'s `steps` is not a list of mappings")
    read["steps"] = [_step(f"{where}'s step {n}", step) for n, step in enumerate(steps, 1)]
    for field in ("outputs", "env"):
        if field in body:
            read[field] = _strings(body[field], f"{where}'s `{field}`")
    return cast(JobBody, read)


def load(root: Path = ROOT) -> list[Job]:
    """Every job of every workflow under ``root/.github/workflows``.

    Raises:
        InvalidWorkflow: a workflow, its jobs or a job is not the shape read here.
    """
    jobs: list[Job] = []
    for path in sorted((root / ".github" / "workflows").glob("*.yml")):
        raw_jobs = _document(path).get("jobs", {})
        if not isinstance(raw_jobs, dict):
            raise InvalidWorkflow(f"{path.name}: `jobs` is not a mapping of job ids")
        workflow_jobs = {str(name): _job(path.name, name, body) for name, body in raw_jobs.items()}
        jobs.extend(
            Job(path.name, name, body, workflow_jobs) for name, body in workflow_jobs.items()
        )
    return jobs


def triggers(root: Path, workflow: str) -> set[str]:
    """The events ``workflow`` runs on.

    Raises:
        InvalidWorkflow: `on` is not an event name, a list of them, or a mapping keyed by them.
    """
    document = _document(root / ".github" / "workflows" / workflow)
    # YAML 1.1 reads the bare key `on` as the boolean true.
    on = document.get("on", document.get(True))
    match on:
        case str():
            return {on}
        case list() | dict() if on and all(isinstance(event, str) for event in on):
            return set(on)
        case _:
            raise InvalidWorkflow(
                f"{workflow}: `on` is not an event name, a list or a mapping of them"
            )


def credentialed(jobs: list[Job]) -> list[Job]:
    """The jobs that read a listed secret or ``*_PUBLISH`` variable, guards aside."""
    return [job for job in jobs if not job.is_guard() and (job.secrets() or job.variables())]


def unguarded(jobs: list[Job]) -> list[str]:
    """Why each credentialed job lacks a guard: one line per job, empty when all carry one.

    A job reading a secret must be skipped by its job-level ``if:`` over a guard job's
    output; a job reading a ``*_PUBLISH`` variable must name that variable in its ``if:``.
    """
    by_name = {(job.workflow, job.name): job for job in jobs}
    problems: list[str] = []
    for job in credentialed(jobs):
        guards = [
            need
            for need in job.needs
            if by_name.get((job.workflow, need), job).is_guard()
            and f"needs.{need}.outputs." in job.condition
        ]
        if job.secrets() and not guards:
            problems.append(
                f"{job.key} reads {sorted(job.secrets())} but its `if:` reads no guard job's output"
            )
        for variable in sorted(job.variables()):
            if f"vars.{variable}" not in job.condition:
                problems.append(f"{job.key} reads {variable} but its `if:` does not test it")
    return problems


def _resolve(text: str, context: Mapping[str, Value]) -> str:
    """Substitute every ``${{ name }}`` in ``text`` with its context value."""
    return _REFERENCE.sub(lambda match: str(context.get(match.group(1)) or ""), text)


def run_guard(job: Job, secrets: Mapping[str, str], root: Path = ROOT) -> dict[str, str]:
    """Run ``job``'s guard steps with ``secrets`` set (and every other listed one unset).

    Returns:
        The job's outputs, resolved from what its steps wrote.

    Raises:
        GuardFailed: a guard step exited non-zero.
    """
    context: dict[str, Value] = {f"secrets.{name}": value for name, value in secrets.items()}
    base = {name: value for name, value in os.environ.items() if name not in SECRETS}
    for step in job.steps():
        run = str(step.get("run", ""))
        if GUARD_SCRIPT not in run:
            continue
        guard = _GUARD_RUN.fullmatch(run.strip())
        if guard is None:
            raise InvalidWorkflow(
                f"{job.key}: step {step.get('id', '?')} runs more than scripts/ci-guard.sh: {run!r}"
            )
        env = {name: _resolve(str(value), context) for name, value in step.get("env", {}).items()}
        env = {name: value for name, value in env.items() if value}
        # Not deleted on close: Windows lets no other process open a file that is.
        with tempfile.NamedTemporaryFile("r", suffix=".out", delete_on_close=False) as output:
            completed = subprocess.run(
                [bash(), GUARD_SCRIPT, *guard.group("args").split()],
                cwd=root,
                env={**base, **env, "GITHUB_OUTPUT": output.name},
                capture_output=True,
                text=True,
                check=False,
            )
            if completed.returncode != 0:
                raise GuardFailed(
                    f"{job.key} guard step exited {completed.returncode}: {completed.stderr}"
                )
            for line in output.read().splitlines():
                name, _, value = line.partition("=")
                context[f"steps.{step.get('id', '')}.outputs.{name}"] = value
    return {
        name: _resolve(str(value), context) for name, value in job.body.get("outputs", {}).items()
    }


def runs(
    job: Job,
    *,
    event: str,
    secrets: Mapping[str, str],
    variables: Mapping[str, str],
    root: Path = ROOT,
) -> bool:
    """Whether ``job`` runs on ``event`` with these secrets and repository variables.

    Every job it needs is taken to run successfully, except a guard job, whose ``if:`` is
    evaluated and whose guard steps are actually run to produce the outputs this job reads.
    """
    context: dict[str, Value] = {
        "github.event_name": event,
        "github.event.action": "opened",
        **{f"vars.{name}": value for name, value in variables.items()},
    }
    succeeded = True
    for need in job.needs:
        needed = Job(job.workflow, need, job.siblings[need], job.siblings)
        if not needed.is_guard():
            continue
        if not runs(needed, event=event, secrets=secrets, variables=variables, root=root):
            succeeded = False
            continue
        for name, value in run_guard(needed, secrets, root).items():
            context[f"needs.{need}.outputs.{name}"] = value
    return evaluate(job.condition or "success()", context, Status(success=succeeded))
