/** The onebudgetspec TypeScript SDK: check, validate and list budgets through the binary,
 * and `report` a `reported` budget's result from the command that measures it. */

import type { CheckResult } from "./generated/check-report.ts";
import type { ListedBudget } from "./generated/list-report.ts";

export {
  BINARY_ENV,
  type CheckOptions,
  CLI_PACKAGE,
  type CompareOptions,
  check,
  compare,
  type FileOptions,
  type ListOptions,
  listBudgets,
  OnebudgetspecError,
  type Program,
  renderMarkdown,
  resolveBinary,
  type SchemaOptions,
  type SelectionOptions,
  schema,
  type ValidateOptions,
  validate,
} from "./client.ts";
export type { CheckReport, CheckResult, Host } from "./generated/check-report.ts";
export type { ListedBudget, ListReport } from "./generated/list-report.ts";
export { SCHEMA_BUNDLE_VERSION } from "./generated/schemas.ts";
export { report } from "./report.ts";

/** A result's verdict: `within`, `over` or `error`. */
export type Verdict = CheckResult["verdict"];
/** Which side of the threshold is within. */
export type Direction = CheckResult["direction"];
/** How a budget's value is measured. */
export type Measure = ListedBudget["measure"];

/** The version of this package, which releases in lock step with the `onebudgetspec` binary. */
export const VERSION = "0.2.1";

export type { BudgetRun, ResultNote, UnavailableBudget } from "./generated/budget-run.ts";
export type { ComparisonInput } from "./generated/comparison-input.ts";
export type { ComparisonReport, ComparisonRow } from "./generated/comparison-report.ts";
export type { DeliveryResult } from "./generated/delivery-result.ts";
export type { HistoricalFlag, PublishRequest } from "./generated/publish-request.ts";
export type { HistoryEntry, ReportHistory } from "./generated/report-history.ts";
export type { RollingHistory } from "./generated/rolling-history.ts";
