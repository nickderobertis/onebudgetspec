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
from typing import Any

import yaml

from repo_checks.expressions import Status, Value, evaluate
from repo_checks.paths import ROOT

#: The secrets publishing and the judged lint need; none exists until provisioning.
SECRETS = (
    "CARGO_REGISTRY_TOKEN",
    "PYPI_TOKEN",
    "NPM_TOKEN",
    "RELEASE_PLZ_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "OPENAI_API_KEY",
)
#: The repository variables that switch each registry's publication on.
PUBLISH_VARIABLES = ("CARGO_PUBLISH", "PYPI_PUBLISH", "NPM_PUBLISH")
GUARD_SCRIPT = "scripts/ci-guard.sh"

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
    body: Mapping[str, Any]
    siblings: Mapping[str, Mapping[str, Any]]

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

    def steps(self) -> list[Mapping[str, Any]]:
        """The job's steps."""
        return list(self.body.get("steps", []))


def load(root: Path = ROOT) -> list[Job]:
    """Every job of every workflow under ``root/.github/workflows``."""
    jobs: list[Job] = []
    for path in sorted((root / ".github" / "workflows").glob("*.yml")):
        document = yaml.safe_load(path.read_text())
        workflow_jobs = document.get("jobs", {})
        for name, body in workflow_jobs.items():
            jobs.append(Job(path.name, name, body, workflow_jobs))
    return jobs


def triggers(root: Path, workflow: str) -> set[str]:
    """The events ``workflow`` runs on."""
    document = yaml.safe_load((root / ".github" / "workflows" / workflow).read_text())
    # YAML 1.1 reads the bare key `on` as the boolean true.
    on = document.get("on", document.get(True))
    if isinstance(on, str):
        return {on}
    return set(on)


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
        env = {name: _resolve(str(value), context) for name, value in step.get("env", {}).items()}
        env = {name: value for name, value in env.items() if value}
        with tempfile.NamedTemporaryFile("r", suffix=".out") as output:
            completed = subprocess.run(
                ["bash", "-c", run],
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
                context[f"steps.{step['id']}.outputs.{name}"] = value
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
