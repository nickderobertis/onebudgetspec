"""Run the ``onebudgetspec`` binary once per call and read the JSON report it prints.

Nothing here measures, loads or selects a budget: the binary does all of it, and this
module turns a call into its argv and its stdout into the generated report type.
"""

import importlib.metadata
import json
import os
import shutil
import subprocess
from collections.abc import Sequence
from pathlib import Path
from typing import Any, TypeVar

from pydantic import BaseModel, ValidationError

from ._generated.check_report import CheckReport
from ._generated.list_report import ListReport

#: The environment variable naming the binary when no explicit one is passed.
BINARY_ENV = "ONEBUDGETSPEC_BIN"
#: The distribution whose wheel carries the binary.
CLI_DISTRIBUTION = "onebudgetspec-cli"
_EXECUTABLE = "onebudgetspec"
#: The statuses whose stdout is a report: within, over and error. A verdict is in the
#: report, so none of them is an exception.
_REPORTED = frozenset({0, 1, 3})

#: A filesystem path, as a string or a path object.
StrPath = str | os.PathLike[str]
_Report = TypeVar("_Report", bound=BaseModel)


class OnebudgetspecError(RuntimeError):
    """The binary refused the call, could not run, or printed no report.

    ``str(error)`` is the binary's own message when it gave one. ``exit_code`` is its exit
    status, ``2`` for an invalid invocation or budgets file, and ``None`` when no binary ran.
    """

    def __init__(self, message: str, *, exit_code: int | None) -> None:
        """Keep the message and the exit status, ``None`` when nothing ran."""
        super().__init__(message)
        self.exit_code = exit_code


def _bundled() -> Path | None:
    """The binary the ``onebudgetspec-cli`` wheel installed, when it is installed."""
    try:
        distribution = importlib.metadata.distribution(CLI_DISTRIBUTION)
    except importlib.metadata.PackageNotFoundError:
        return None
    for file in distribution.files or ():
        if file.name == _EXECUTABLE and file.parent.name == "bin":
            path = Path(os.path.normpath(Path(str(distribution.locate_file(file)))))
            if path.is_file():
                return path
    return None


def resolve_binary(binary: StrPath | None = None) -> Path:
    """The binary a call runs.

    In order: ``binary`` when given, then ``ONEBUDGETSPEC_BIN`` when set and non-empty, then
    the binary the ``onebudgetspec-cli`` wheel installed, then ``onebudgetspec`` on ``PATH``.

    Raises:
        OnebudgetspecError: none of them names a binary.
    """
    if binary is not None:
        return Path(binary)
    from_environment = os.environ.get(BINARY_ENV)
    if from_environment:
        return Path(from_environment)
    bundled = _bundled()
    if bundled is not None:
        return bundled
    on_path = shutil.which(_EXECUTABLE)
    if on_path is not None:
        return Path(on_path)
    raise OnebudgetspecError(
        f"onebudgetspec: no binary found; install {CLI_DISTRIBUTION}, put {_EXECUTABLE} "
        f"on PATH, set {BINARY_ENV}, or pass binary=",
        exit_code=None,
    )


def _values(name: str, values: Sequence[str] | None) -> list[str]:
    """``values`` as a list, refusing a lone string, which would split into characters."""
    if values is None:
        return []
    if isinstance(values, str) or not all(isinstance(value, str) for value in values):
        raise TypeError(f"{name} must be a sequence of strings, not {values!r}")
    return list(values)


def _paths(paths: Sequence[StrPath] | None) -> list[str]:
    if paths is None:
        return []
    if isinstance(paths, (str, os.PathLike)):
        raise TypeError(f"paths must be a sequence of paths, not {paths!r}")
    return [os.fspath(path) for path in paths]


def _files(paths: Sequence[StrPath] | None, recursive: bool) -> list[str]:
    """The flags and operands naming the budgets files, after ``--`` so none reads as a flag."""
    if not isinstance(recursive, bool):
        raise TypeError(f"recursive must be True or False, not {recursive!r}")
    return [*(["--recursive"] if recursive else []), "--", *_paths(paths)]


def _selection(
    ids: Sequence[str] | None,
    labels: Sequence[str] | None,
    exclude_labels: Sequence[str] | None,
) -> list[str]:
    """The selection flags, each value joined to its flag so none can read as a flag."""
    return [
        f"{flag}={value}"
        for flag, name, values in (
            ("--id", "ids", ids),
            ("--label", "labels", labels),
            ("--exclude-label", "exclude_labels", exclude_labels),
        )
        for value in _values(name, values)
    ]


def _run(binary: StrPath | None, args: list[str], cwd: StrPath | None) -> bytes:
    """Run the binary with ``args`` and return its stdout, or raise with its message."""
    program = resolve_binary(binary)
    try:
        completed = subprocess.run(
            [os.fspath(program), *args],
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        raise OnebudgetspecError(
            f"onebudgetspec: cannot run {program}: {error}", exit_code=None
        ) from error
    if completed.returncode in _REPORTED:
        return completed.stdout
    status = completed.returncode
    message = completed.stderr.decode(errors="replace").strip()
    if not message:
        ended = f"was terminated by signal {-status}" if status < 0 else f"exited {status}"
        message = f"onebudgetspec: {program} {ended} with no message"
    raise OnebudgetspecError(message, exit_code=status)


def _report(model: type[_Report], stdout: bytes) -> _Report:
    try:
        # Strict: a report field of the wrong JSON type is refused, never coerced.
        return model.model_validate_json(stdout, strict=True)
    except ValidationError as error:
        raise OnebudgetspecError(
            f"onebudgetspec: the binary printed no valid {model.__name__}: {error}",
            exit_code=None,
        ) from error


def check(
    paths: Sequence[StrPath] | None = None,
    ids: Sequence[str] | None = None,
    labels: Sequence[str] | None = None,
    exclude_labels: Sequence[str] | None = None,
    recursive: bool = False,
    cwd: StrPath | None = None,
    *,
    binary: StrPath | None = None,
) -> CheckReport:
    """Measure every selected budget once: ``onebudgetspec check``.

    Args:
        paths: budgets files, or directories with ``recursive``; ``./budgets.yaml`` when
            omitted. Relative paths are read from ``cwd``.
        ids: keep only these budgets; an id no file registers is refused.
        labels: keep budgets carrying at least one of these labels.
        exclude_labels: drop budgets carrying any of these labels.
        recursive: search directories for files named ``budgets.yaml``.
        cwd: the directory to run from; the current one when omitted.
        binary: the binary to run; see :func:`resolve_binary`.

    Returns:
        The report, whatever its verdicts: within, over and error are all in it.

    Raises:
        OnebudgetspecError: the invocation or a budgets file is invalid (exit status 2),
            with the binary's message, or the binary could not run.
    """
    args = ["check", "--json", *_selection(ids, labels, exclude_labels), *_files(paths, recursive)]
    return _report(CheckReport, _run(binary, args, cwd))


def validate(
    paths: Sequence[StrPath] | None = None,
    recursive: bool = False,
    cwd: StrPath | None = None,
    *,
    binary: StrPath | None = None,
) -> ListReport:
    """Check the files' shape and id uniqueness, running no command: ``onebudgetspec validate``.

    Args:
        paths: as for :func:`check`.
        recursive: as for :func:`check`.
        cwd: as for :func:`check`.
        binary: as for :func:`check`.

    Returns:
        Every budget the valid files register, as ``list`` reports them.

    Raises:
        OnebudgetspecError: a file is invalid (exit status 2), with the binary's message.
    """
    return _report(ListReport, _run(binary, ["validate", "--json", *_files(paths, recursive)], cwd))


def list_budgets(
    paths: Sequence[StrPath] | None = None,
    ids: Sequence[str] | None = None,
    labels: Sequence[str] | None = None,
    exclude_labels: Sequence[str] | None = None,
    recursive: bool = False,
    cwd: StrPath | None = None,
    *,
    binary: StrPath | None = None,
) -> ListReport:
    """Report the selected budgets, running no command: ``onebudgetspec list``.

    Takes the arguments of :func:`check`.

    Raises:
        OnebudgetspecError: the invocation or a budgets file is invalid (exit status 2).
    """
    args = ["list", "--json", *_selection(ids, labels, exclude_labels), *_files(paths, recursive)]
    return _report(ListReport, _run(binary, args, cwd))


def schema(*, binary: StrPath | None = None) -> dict[str, Any]:
    """The JSON Schema bundle the binary prints: ``onebudgetspec schema``.

    Raises:
        OnebudgetspecError: the binary could not run or printed no JSON object.
    """
    stdout = _run(binary, ["schema"], None)
    try:
        bundle = json.loads(stdout)
    except ValueError as error:
        raise OnebudgetspecError(
            f"onebudgetspec: the schema is not JSON: {error}", exit_code=None
        ) from error
    if not isinstance(bundle, dict):
        raise OnebudgetspecError("onebudgetspec: the schema is not a JSON object", exit_code=None)
    return bundle
