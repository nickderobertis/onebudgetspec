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
    compare,
    list_budgets,
    render_markdown,
    resolve_binary,
    schema,
    validate,
)
from ._generated import SCHEMA_BUNDLE_VERSION
from ._generated.budget_run import BudgetRun, ResultNote, UnavailableBudget
from ._generated.check_report import CheckReport, CheckResult, Direction, Host, Verdict
from ._generated.comparison_input import ComparisonInput
from ._generated.comparison_report import ComparisonReport, ComparisonRow
from ._generated.delivery_result import DeliveryResult
from ._generated.list_report import ListedBudget, ListReport, Measure
from ._generated.publish_request import HistoricalFlag, PublishRequest
from ._generated.report_history import HistoryEntry, ReportHistory
from ._generated.rolling_history import RollingHistory
from ._report import report

__all__ = [
    "BINARY_ENV",
    "BudgetRun",
    "UnavailableBudget",
    "ResultNote",
    "ComparisonRow",
    "HistoricalFlag",
    "HistoryEntry",
    "ComparisonInput",
    "ComparisonReport",
    "RollingHistory",
    "PublishRequest",
    "DeliveryResult",
    "ReportHistory",
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
    "compare",
    "list_budgets",
    "report",
    "resolve_binary",
    "render_markdown",
    "schema",
    "validate",
]

#: The version of this package, which releases in lock step with the ``onebudgetspec`` binary.
__version__ = "0.2.1"
