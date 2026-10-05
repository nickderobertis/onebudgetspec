"""Every job that needs a credential is guarded, and its guard skips it until provisioned.

No hosted run can show this before the secrets exist, so each guard's own logic runs here:
the guard job's `scripts/ci-guard.sh` step with each secret unset and set, and each
job-level `if:` evaluated with each `*_PUBLISH` variable unset, set to something other than
`true`, and `true`.
"""

import itertools
import shutil
import subprocess
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import pytest
import yaml

from repo_checks import workflows
from repo_checks.paths import ROOT


@dataclass(frozen=True)
class Requirement:
    """What a credentialed job needs before it may run."""

    secrets: tuple[str, ...]
    events: tuple[str, ...]
    variable: str | None = None
    any_secret: bool = False


#: Every credentialed job, and exactly what enables it. A job missing here fails the test.
REQUIRED = {
    "ci.yml:llmlint": Requirement(
        ("CLAUDE_CODE_OAUTH_TOKEN", "OPENAI_API_KEY"), ("pull_request",), any_secret=True
    ),
    "release-plz.yml:release-plz": Requirement(
        ("RELEASE_PLZ_TOKEN",), ("push", "workflow_dispatch")
    ),
    "release.yml:publish-crates": Requirement(
        ("CARGO_REGISTRY_TOKEN",), ("release",), "CARGO_PUBLISH"
    ),
    "release.yml:publish-pypi": Requirement(("PYPI_TOKEN",), ("release",), "PYPI_PUBLISH"),
    "release.yml:publish-sdk-pypi": Requirement(("PYPI_TOKEN",), ("release",), "PYPI_PUBLISH"),
    "release.yml:publish-npm": Requirement(("NPM_TOKEN",), ("release",), "NPM_PUBLISH"),
    "release.yml:publish-sdk-npm": Requirement(("NPM_TOKEN",), ("release",), "NPM_PUBLISH"),
}
VARIABLE_STATES = (None, "false", "1", "true")


def jobs() -> dict[str, workflows.Job]:
    return {job.key: job for job in workflows.credentialed(workflows.load())}


def test_every_credentialed_job_is_known_and_guarded() -> None:
    assert sorted(jobs()) == sorted(REQUIRED)
    assert workflows.unguarded(workflows.load()) == []


def test_every_listed_secret_and_variable_is_read_by_some_job() -> None:
    read = set()
    for job in workflows.load():
        read |= job.secrets() | job.variables()
    assert read == set(workflows.SECRETS) | set(workflows.PUBLISH_VARIABLES)


@pytest.mark.parametrize("key", sorted(REQUIRED))
def test_the_job_runs_only_when_provisioned(key: str) -> None:
    job = jobs()[key]
    requirement = REQUIRED[key]
    assert job.secrets() == set(requirement.secrets)
    triggers = workflows.triggers(ROOT, job.workflow)
    assert set(requirement.events) <= triggers
    states = VARIABLE_STATES if requirement.variable else (None,)
    every = {name: "provisioned" for name in requirement.secrets}
    every_on = {requirement.variable: "true"} if requirement.variable else {}
    for event in sorted(triggers - set(requirement.events)):
        assert not workflows.runs(job, event=event, secrets=every, variables=every_on), (
            f"{key} runs on {event}"
        )
    for event in requirement.events:
        for present in itertools.product((False, True), repeat=len(requirement.secrets)):
            secrets = {
                name: "provisioned"
                for name, on in zip(requirement.secrets, present, strict=True)
                if on
            }
            for state in states:
                variables = {requirement.variable: state} if requirement.variable and state else {}
                enabled = (any(present) if requirement.any_secret else all(present)) and (
                    requirement.variable is None or state == "true"
                )
                assert (
                    workflows.runs(job, event=event, secrets=secrets, variables=variables)
                    is enabled
                ), f"{key} on {event} with {sorted(secrets)} and {variables}"


def test_the_guard_script_exits_zero_and_answers_both_ways(tmp_path: Path) -> None:
    output = tmp_path / "output"

    def guard(*args: str, **env: str) -> str:
        output.write_text("")
        completed = subprocess.run(
            ["bash", "scripts/ci-guard.sh", *args],
            cwd=ROOT,
            env={"PATH": "/usr/bin:/bin", "GITHUB_OUTPUT": str(output), **env},
            capture_output=True,
            text=True,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr
        return output.read_text()

    assert guard("NPM_TOKEN") == "enabled=false\n"
    assert guard("NPM_TOKEN", NPM_TOKEN="") == "enabled=false\n"
    assert guard("NPM_TOKEN", NPM_TOKEN="t") == "enabled=true\n"
    assert guard("A", "B", A="t") == "enabled=false\n"
    assert guard("--any", "A", "B", A="t") == "enabled=true\n"
    assert guard("--any", "A", "B") == "enabled=false\n"


def test_the_guard_script_refuses_a_malformed_call(tmp_path: Path) -> None:
    for args, env in (([], {"GITHUB_OUTPUT": str(tmp_path / "o")}), (["NPM_TOKEN"], {})):
        completed = subprocess.run(
            ["bash", "scripts/ci-guard.sh", *args],
            cwd=ROOT,
            env={"PATH": "/usr/bin:/bin", **env},
            capture_output=True,
            text=True,
            check=False,
        )
        assert completed.returncode == 64
        assert "ci-guard:" in completed.stderr


def _copy_with(tmp_path: Path, workflow: str, edit: Callable[[dict], None]) -> Path:
    """A copy of the workflows and the guard script, with one workflow edited."""
    root = tmp_path / "repo"
    shutil.copytree(ROOT / ".github", root / ".github")
    (root / "scripts").mkdir()
    shutil.copy(ROOT / "scripts/ci-guard.sh", root / "scripts/ci-guard.sh")
    path = root / ".github/workflows" / workflow
    document = yaml.safe_load(path.read_text())
    edit(document)
    path.write_text(yaml.safe_dump(document))
    return root


def test_an_unguarded_job_is_found_and_runs_without_its_secret(tmp_path: Path) -> None:
    def unguard(document: dict) -> None:
        del document["jobs"]["publish-npm"]["if"]

    root = _copy_with(tmp_path, "release.yml", unguard)
    loaded = workflows.load(root)
    assert workflows.unguarded(loaded) == [
        "release.yml:publish-npm reads ['NPM_TOKEN'] but its `if:` reads no guard job's output",
    ]
    job = next(job for job in loaded if job.key == "release.yml:publish-npm")
    assert workflows.runs(job, event="release", secrets={}, variables={}, root=root) is True


def test_a_publish_variable_the_condition_does_not_test_is_found(tmp_path: Path) -> None:
    def untested(document: dict) -> None:
        job = document["jobs"]["publish-npm"]
        job["if"] = "needs.guard.outputs.npm == 'true'"
        job["env"] = {"ENABLED": "${{ vars.NPM_PUBLISH }}"}

    root = _copy_with(tmp_path, "release.yml", untested)
    assert workflows.unguarded(workflows.load(root)) == [
        "release.yml:publish-npm reads NPM_PUBLISH but its `if:` does not test it",
    ]


def test_a_guard_that_fails_is_reported(tmp_path: Path) -> None:
    def break_guard(document: dict) -> None:
        document["jobs"]["guard"]["steps"][1]["run"] = "bash scripts/ci-guard.sh"

    root = _copy_with(tmp_path, "release-plz.yml", break_guard)
    job = next(job for job in workflows.load(root) if job.key == "release-plz.yml:release-plz")
    with pytest.raises(workflows.GuardFailed, match="exited 64"):
        workflows.runs(job, event="push", secrets={}, variables={}, root=root)


def test_a_skipped_guard_job_skips_what_needs_it(tmp_path: Path) -> None:
    def skip_guard(document: dict) -> None:
        document["jobs"]["guard"]["if"] = "github.event_name == 'never'"

    root = _copy_with(tmp_path, "release-plz.yml", skip_guard)
    job = next(job for job in workflows.load(root) if job.key == "release-plz.yml:release-plz")
    assert (
        workflows.runs(
            job, event="push", secrets={"RELEASE_PLZ_TOKEN": "t"}, variables={}, root=root
        )
        is False
    )


def test_triggers_read_every_spelling(tmp_path: Path) -> None:
    workflows_dir = tmp_path / ".github/workflows"
    workflows_dir.mkdir(parents=True)
    (workflows_dir / "one.yml").write_text("on: push\njobs: {}\n")
    (workflows_dir / "two.yml").write_text("on: [push, release]\njobs: {}\n")
    assert workflows.triggers(tmp_path, "one.yml") == {"push"}
    assert workflows.triggers(tmp_path, "two.yml") == {"push", "release"}
    assert workflows.load(tmp_path) == []


def test_the_guard_script_refuses_a_name_that_is_not_a_variable(tmp_path: Path) -> None:
    completed = subprocess.run(
        ["bash", "scripts/ci-guard.sh", "NPM_TOKEN", "$(id)"],
        cwd=ROOT,
        env={"PATH": "/usr/bin:/bin", "GITHUB_OUTPUT": str(tmp_path / "o")},
        capture_output=True,
        text=True,
        check=False,
    )
    assert completed.returncode == 64
    assert "is not an environment variable name" in completed.stderr
    assert not (tmp_path / "o").exists()


@pytest.mark.parametrize(
    ("text", "reason"),
    [
        ("- just\n- a list\n", "is not a mapping"),
        ("on: push\njobs: [build]\n", "`jobs` is not a mapping of job ids"),
        ("on: push\njobs:\n  build: run it\n", "job build is not a mapping"),
        (
            "on: push\njobs:\n  build:\n    steps: run it\n",
            "job build's `steps` is not a list of mappings",
        ),
        ("on: push\njobs:\n  build:\n    if: [1]\n", "job build's `if` is not a string"),
    ],
)
def test_a_malformed_workflow_is_named(tmp_path: Path, text: str, reason: str) -> None:
    directory = tmp_path / ".github/workflows"
    directory.mkdir(parents=True)
    (directory / "bad.yml").write_text(text)
    with pytest.raises(workflows.InvalidWorkflow, match=reason):
        workflows.load(tmp_path)


@pytest.mark.parametrize("on", ["5", "[push, 7]", "{push: {}, 7: {}}", "null"])
def test_malformed_triggers_are_named(tmp_path: Path, on: str) -> None:
    directory = tmp_path / ".github/workflows"
    directory.mkdir(parents=True)
    (directory / "bad.yml").write_text(f"on: {on}\njobs: {{}}\n")
    with pytest.raises(workflows.InvalidWorkflow, match="`on` is not an event name"):
        workflows.triggers(tmp_path, "bad.yml")


@pytest.mark.parametrize(
    ("text", "reason"),
    [
        (
            "on: push\njobs:\n  build:\n    steps:\n      - id: [1]\n",
            "job build's step 1 `id` is not a string",
        ),
        (
            "on: push\njobs:\n  build:\n    steps:\n      - run: [ls]\n",
            "job build's step 1 `run` is not a string",
        ),
        (
            "on: push\njobs:\n  build:\n    steps:\n      - env: [A]\n",
            "job build's step 1 `env` is not a mapping of names to values",
        ),
        (
            "on: push\njobs:\n  build:\n    outputs:\n      x: [1]\n",
            "job build's `outputs` is not a mapping of names to values",
        ),
        (
            "on: push\njobs:\n  build:\n    env:\n      A: {b: 1}\n",
            "job build's `env` is not a mapping of names to values",
        ),
    ],
)
def test_malformed_step_and_mapping_fields_are_named(
    tmp_path: Path, text: str, reason: str
) -> None:
    directory = tmp_path / ".github/workflows"
    directory.mkdir(parents=True)
    (directory / "bad.yml").write_text(text)
    with pytest.raises(workflows.InvalidWorkflow, match=reason):
        workflows.load(tmp_path)


def test_yaml_scalars_read_as_github_reads_them(tmp_path: Path) -> None:
    directory = tmp_path / ".github/workflows"
    directory.mkdir(parents=True)
    (directory / "w.yml").write_text(
        "on: push\njobs:\n  never:\n    if: false\n  always:\n    if: true\n"
        "  numbered:\n    env:\n      COUNT: 3\n    steps:\n      - env:\n          ENABLED: true\n"
    )
    jobs = {job.name: job for job in workflows.load(tmp_path)}
    assert (
        workflows.runs(jobs["never"], event="push", secrets={}, variables={}, root=tmp_path)
        is False
    )
    assert (
        workflows.runs(jobs["always"], event="push", secrets={}, variables={}, root=tmp_path)
        is True
    )
    assert jobs["numbered"].body["env"] == {"COUNT": "3"}
    assert jobs["numbered"].steps()[0]["env"] == {"ENABLED": "true"}


@pytest.mark.parametrize(
    "run",
    [
        "bash scripts/ci-guard.sh NPM_TOKEN; touch {marker}",
        "bash scripts/ci-guard.sh NPM_TOKEN && touch {marker}",
        "bash scripts/ci-guard.sh $(touch {marker})",
        "touch {marker} # bash scripts/ci-guard.sh NPM_TOKEN",
    ],
)
def test_a_guard_step_that_runs_more_than_the_guard_is_refused_unrun(
    tmp_path: Path, run: str
) -> None:
    marker = tmp_path / "ran"

    def smuggle(document: dict) -> None:
        document["jobs"]["guard"]["steps"][1]["run"] = run.format(marker=marker)

    root = _copy_with(tmp_path, "release-plz.yml", smuggle)
    job = next(job for job in workflows.load(root) if job.key == "release-plz.yml:release-plz")
    with pytest.raises(workflows.InvalidWorkflow, match="runs more than scripts/ci-guard.sh"):
        workflows.runs(job, event="push", secrets={}, variables={}, root=root)
    assert not marker.exists(), "the smuggled command ran"
