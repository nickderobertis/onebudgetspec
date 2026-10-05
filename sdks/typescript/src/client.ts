// Run the `onebudgetspec` binary once per call and read the JSON report it prints.
//
// Nothing here measures, loads or selects a budget: the binary does all of it, and this
// module turns a call into its argv and its stdout into the generated report type,
// validated against the schema it was generated from.
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { Ajv2020, type ValidateFunction } from "ajv/dist/2020.js";
import type { CheckReport } from "./generated/check-report.ts";
import type { ListReport } from "./generated/list-report.ts";
import { reportSchemas } from "./generated/schemas.ts";

/** The environment variable naming the binary when no explicit one is passed. */
export const BINARY_ENV = "ONEBUDGETSPEC_BIN";
/** The package whose launcher runs the binary for the host it is installed on. */
export const CLI_PACKAGE = "@onebudgetspec/cli";
/** The statuses whose stdout is a report: within, over and error. A verdict is in the
 * report, so none of them rejects. */
const REPORTED = new Set([0, 1, 3]);

/** Where the budgets files are, how to find the binary, and where to run it. */
export type FileOptions = {
  /** Budgets files, or directories with `recursive`; `./budgets.yaml` when omitted. */
  paths?: readonly string[] | undefined;
  /** Search directories for files named `budgets.yaml`. */
  recursive?: boolean | undefined;
  /** The directory to run from; relative paths are read from it. */
  cwd?: string | undefined;
  /** The binary to run; see {@link resolveBinary}. */
  binary?: string | undefined;
};

/** Which budgets a call keeps; every filter applies. */
export type SelectionOptions = {
  /** Keep only these budgets; an id no file registers is refused. */
  ids?: readonly string[] | undefined;
  /** Keep budgets carrying at least one of these labels. */
  labels?: readonly string[] | undefined;
  /** Drop budgets carrying any of these labels. */
  excludeLabels?: readonly string[] | undefined;
};

export type CheckOptions = FileOptions & SelectionOptions;
export type ValidateOptions = FileOptions;
export type ListOptions = FileOptions & SelectionOptions;
export type SchemaOptions = { binary?: string | undefined };

/** The binary refused the call, could not run, or printed no report. */
export class OnebudgetspecError extends Error {
  /** The binary's exit status: `2` for an invalid invocation or budgets file, and `null`
   * when no binary ran or it was ended by a signal. */
  readonly exitCode: number | null;

  constructor(message: string, exitCode: number | null) {
    super(message);
    this.name = "OnebudgetspecError";
    this.exitCode = exitCode;
  }
}

/** The argv prefix that runs the binary: the program and any arguments before the call's. */
export type Program = readonly [string, ...string[]];

/** The `onebudgetspec` launcher a `@onebudgetspec/cli` manifest names, checked. */
function launcherOf(manifestPath: string): string {
  const reinstall = `reinstall ${CLI_PACKAGE}`;
  let manifest: unknown;
  try {
    manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  } catch (error) {
    throw new OnebudgetspecError(
      `onebudgetspec: cannot read ${manifestPath} (${error}); ${reinstall}`,
      null,
    );
  }
  const bin = isObject(manifest) ? manifest.bin : undefined;
  const launcher = isObject(bin) ? bin.onebudgetspec : undefined;
  if (typeof launcher !== "string" || launcher === "") {
    throw new OnebudgetspecError(
      `onebudgetspec: ${manifestPath} names no onebudgetspec launcher; ${reinstall}`,
      null,
    );
  }
  return launcher;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * The binary a call runs. In order: `binary` when given, then `ONEBUDGETSPEC_BIN` when set
 * and non-empty, then the launcher of the `@onebudgetspec/cli` package this package
 * resolves, run by the current runtime.
 */
export function resolveBinary(binary?: string): Program {
  if (binary !== undefined) return [binary];
  const fromEnvironment = process.env[BINARY_ENV];
  if (fromEnvironment) return [fromEnvironment];
  const require = createRequire(import.meta.url);
  let manifestPath: string;
  try {
    manifestPath = require.resolve(`${CLI_PACKAGE}/package.json`);
  } catch {
    throw new OnebudgetspecError(
      `onebudgetspec: no binary found; install ${CLI_PACKAGE}, set ${BINARY_ENV}, or pass binary`,
      null,
    );
  }
  const launcher = launcherOf(manifestPath);
  return [process.execPath, join(dirname(manifestPath), launcher)];
}

function strings(name: string, values: readonly string[] | undefined): string[] {
  if (values === undefined) return [];
  if (!Array.isArray(values) || !values.every((value) => typeof value === "string")) {
    throw new TypeError(`${name} must be an array of strings, not ${JSON.stringify(values)}`);
  }
  return [...values];
}

/** The flags and operands naming the budgets files, after `--` so none reads as a flag. */
function files(options: FileOptions): string[] {
  const { recursive } = options;
  if (recursive !== undefined && typeof recursive !== "boolean") {
    throw new TypeError(`recursive must be true or false, not ${JSON.stringify(recursive)}`);
  }
  return [...(recursive ? ["--recursive"] : []), "--", ...strings("paths", options.paths)];
}

/** `value`, which a JavaScript caller may pass as anything, when it is a string or absent. */
function optionalString(name: string, value: string | undefined): string | undefined {
  if (value !== undefined && typeof value !== "string") {
    throw new TypeError(`${name} must be a string, not ${JSON.stringify(value)}`);
  }
  return value;
}

/** The selection flags, each value joined to its flag so none can read as a flag. */
function selection(options: SelectionOptions): string[] {
  return [
    ...strings("ids", options.ids).map((value) => `--id=${value}`),
    ...strings("labels", options.labels).map((value) => `--label=${value}`),
    ...strings("excludeLabels", options.excludeLabels).map((value) => `--exclude-label=${value}`),
  ];
}

/** Run the binary with `args` and resolve its stdout, or reject with its message. */
function run(binary: string | undefined, args: string[], cwd: string | undefined): Promise<string> {
  const [program, ...prefix] = resolveBinary(optionalString("binary", binary));
  optionalString("cwd", cwd);
  return new Promise((resolve, reject) => {
    const child = spawn(program, [...prefix, ...args], {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
    child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));
    child.on("error", (error) => {
      reject(
        new OnebudgetspecError(`onebudgetspec: cannot run ${program}: ${error.message}`, null),
      );
    });
    child.on("close", (status, signal) => {
      if (status !== null && REPORTED.has(status)) {
        resolve(Buffer.concat(stdout).toString("utf8"));
        return;
      }
      const ended = signal === null ? `exited ${status}` : `was terminated by ${signal}`;
      const message =
        Buffer.concat(stderr).toString("utf8").trim() ||
        `onebudgetspec: ${program} ${ended} with no message`;
      reject(new OnebudgetspecError(message, status));
    });
  });
}

/** An RFC 3339 timestamp, as the reports' `date-time` fields carry. */
const RFC3339 = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(\.\d+)?(Z|[+-](\d{2}):(\d{2}))$/i;
/** Whether `value` is an RFC 3339 timestamp naming a real instant: a day its month has, and
 * an hour, minute, second and offset in range (a leap second allowed). */
function isDateTime(value: string): boolean {
  const parts = RFC3339.exec(value);
  if (parts === null) return false;
  // Groups 9 and 10 are the offset's hours and minutes, absent for `Z`.
  const field = (group: number) => Number(parts[group] ?? 0);
  const [year, month, day] = [field(1), field(2), field(3)];
  const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
  return (
    month >= 1 &&
    month <= 12 &&
    day >= 1 &&
    day <= daysInMonth &&
    field(4) <= 23 &&
    field(5) <= 59 &&
    field(6) <= 60 &&
    field(9) <= 23 &&
    field(10) <= 59
  );
}
const isUnsigned = (bits: number) => (value: number) =>
  Number.isInteger(value) && value >= 0 && value < 2 ** bits;
// Every format the schema names is checked: the timestamps, and the numeric widths the
// binary's Rust types carry.
const ajv = new Ajv2020({
  allErrors: true,
  formats: {
    "date-time": isDateTime,
    // A double is any JSON number: ajv's own `number` check already refuses a value that
    // overflows to Infinity.
    double: true,
    uint32: { type: "number", validate: isUnsigned(32) },
    uint64: { type: "number", validate: isUnsigned(64) },
  },
});
const checkReport = ajv.compile<CheckReport>(reportSchemas["check-report"]);
const listReport = ajv.compile<ListReport>(reportSchemas["list-report"]);

/** `stdout` as the report `validator` accepts, or the reason it is not one. */
function report<T>(validator: ValidateFunction<T>, root: string, stdout: string): T {
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch (error) {
    throw new OnebudgetspecError(`onebudgetspec: the binary printed no JSON: ${error}`, null);
  }
  if (!validator(parsed)) {
    throw new OnebudgetspecError(
      `onebudgetspec: the binary printed no valid ${root}: ${ajv.errorsText(validator.errors)}`,
      null,
    );
  }
  return parsed;
}

/**
 * Measure every selected budget once: `onebudgetspec check`. Resolves with the report
 * whatever its verdicts, since within, over and error are all in it; rejects with the
 * binary's own message when the invocation or a budgets file is invalid (exit status 2).
 */
export async function check(options: CheckOptions = {}): Promise<CheckReport> {
  const args = ["check", "--json", ...selection(options), ...files(options)];
  return report(checkReport, "check-report", await run(options.binary, args, options.cwd));
}

/**
 * Check the files' shape and id uniqueness, running no command: `onebudgetspec validate`.
 * Resolves with every budget the valid files register, as `list` reports them.
 */
export async function validate(options: ValidateOptions = {}): Promise<ListReport> {
  const args = ["validate", "--json", ...files(options)];
  return report(listReport, "list-report", await run(options.binary, args, options.cwd));
}

/** Report the selected budgets, running no command: `onebudgetspec list`. */
export async function listBudgets(options: ListOptions = {}): Promise<ListReport> {
  const args = ["list", "--json", ...selection(options), ...files(options)];
  return report(listReport, "list-report", await run(options.binary, args, options.cwd));
}

/** The JSON Schema bundle the binary prints: `onebudgetspec schema`. */
export async function schema(options: SchemaOptions = {}): Promise<Record<string, unknown>> {
  const stdout = await run(options.binary, ["schema"], undefined);
  let bundle: unknown;
  try {
    bundle = JSON.parse(stdout);
  } catch (error) {
    throw new OnebudgetspecError(`onebudgetspec: the schema is not JSON: ${error}`, null);
  }
  if (!isObject(bundle)) {
    throw new OnebudgetspecError("onebudgetspec: the schema is not a JSON object", null);
  }
  return bundle;
}
