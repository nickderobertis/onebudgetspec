"""The onebudgetspec Python SDK: check, validate and list budgets through the binary.

Each call runs the ``onebudgetspec`` binary once and returns its JSON report as a model
generated from ``onebudgetspec schema``. ``report`` is the one call that runs no binary: a
``reported`` budget's command uses it to write its result.
"""

from ._client import (
    BINARY_ENV,
    CLI_DISTRIBUTION,
    OnebudgetspecError,
    StrPath,
    check,
    list_budgets,
    resolve_binary,
    schema,
    validate,
)
from ._generated import SCHEMA_BUNDLE_VERSION
from ._generated.check_report import CheckReport, CheckResult, Direction, Host, Verdict
from ._generated.list_report import ListedBudget, ListReport, Measure
from ._report import report

__all__ = [
    "BINARY_ENV",
    "CLI_DISTRIBUTION",
    "SCHEMA_BUNDLE_VERSION",
    "CheckReport",
    "CheckResult",
    "Direction",
    "Host",
    "ListReport",
    "ListedBudget",
    "Measure",
    "OnebudgetspecError",
    "StrPath",
    "Verdict",
    "__version__",
    "check",
    "list_budgets",
    "report",
    "resolve_binary",
    "schema",
    "validate",
]

#: The version of this package, which releases in lock step with the ``onebudgetspec`` binary.
__version__ = "0.1.2"
