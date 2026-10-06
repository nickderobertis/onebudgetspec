// Write a `reported` budget's result from inside the command that measures it.
//
// Nothing here reads a budgets file or compares a value with a threshold: `onebudgetspec
// check` reads the result this writes and is the only judge.
import { writeFileSync } from "node:fs";

/** The environment variable naming the file a `reported` budget's command writes to. */
const RESULT_ENV = "ONEBUDGETSPEC_RESULT";

/**
 * Report `value`, and `detail` when given, as the running budget's result.
 *
 * When `ONEBUDGETSPEC_RESULT` is set and non-empty, the file it names is replaced by one
 * JSON object, `{"value": value}` with `"detail": detail` when a detail is given, and this
 * returns `true`. When it is unset or empty, nothing is written and this returns `false`,
 * so a test that measures behaves the same outside a check.
 *
 * @throws {TypeError} `value` is not a number or `detail` not a string; nothing is written.
 * @throws {RangeError} `value` is not finite; nothing is written.
 * @throws the error writing the file, when it cannot be written.
 */
export function report(value: number, detail?: string): boolean {
  // A JavaScript caller is held to the signature too: anything else writes a result the
  // check would refuse.
  if (typeof value !== "number") {
    throw new TypeError(`a reported value must be a number, not ${typeof value}`);
  }
  if (detail !== undefined && typeof detail !== "string") {
    throw new TypeError(`a reported detail must be a string, not ${typeof detail}`);
  }
  if (!Number.isFinite(value)) {
    throw new RangeError(`a reported value must be a finite number, not ${value}`);
  }
  const path = process.env[RESULT_ENV];
  if (!path) {
    return false;
  }
  const result = detail === undefined ? { value } : { value, detail };
  writeFileSync(path, JSON.stringify(result), "utf8");
  return true;
}
