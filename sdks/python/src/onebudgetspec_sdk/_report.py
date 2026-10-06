"""Write a ``reported`` budget's result from inside the command that measures it.

Nothing here reads a budgets file or compares a value with a threshold: ``onebudgetspec
check`` reads the result this writes and is the only judge.
"""

import json
import math
import os
from pathlib import Path
from typing import NotRequired, TypedDict

#: The environment variable naming the file a ``reported`` budget's command writes to.
RESULT_ENV = "ONEBUDGETSPEC_RESULT"


class _Result(TypedDict):
    """The one JSON object a result file holds: a finite value, and a detail when given."""

    value: float
    detail: NotRequired[str]


def report(value: float, detail: str | None = None) -> bool:
    """Report ``value``, and ``detail`` when given, as the running budget's result.

    When ``ONEBUDGETSPEC_RESULT`` is set and non-empty, the file it names is replaced by one
    JSON object, ``{"value": value}`` with ``"detail": detail`` when a detail is given, and
    this returns ``True``. When it is unset or empty, nothing is written and this returns
    ``False``, so a test that measures behaves the same outside a check.

    Raises:
        TypeError: ``value`` is not a number or ``detail`` not a string; nothing is written.
        ValueError: ``value`` is not finite; nothing is written.
        OSError: the file could not be written.
    """
    # A bool is an int to Python but would be written as a JSON boolean, which is no value.
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise TypeError(f"a reported value must be a number, not {type(value).__name__}")
    if detail is not None and not isinstance(detail, str):
        raise TypeError(f"a reported detail must be a string, not {type(detail).__name__}")
    if not math.isfinite(value):
        raise ValueError(f"a reported value must be a finite number, not {value}")
    path = os.environ.get(RESULT_ENV)
    if not path:
        return False
    result: _Result = {"value": value}
    if detail is not None:
        result["detail"] = detail
    Path(path).write_text(json.dumps(result), encoding="utf-8")
    return True
