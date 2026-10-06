// `report` writes a `reported` budget's result, and `onebudgetspec check` reads it.
import { afterAll, afterEach, beforeEach, expect, test } from "bun:test";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { check, report } from "../src/index.ts";
import { builtBinary, cleanScratch, ROOT, scratch } from "./helpers.ts";

const RESULT_ENV = "ONEBUDGETSPEC_RESULT";
let caller: string | undefined;

beforeEach(() => {
  caller = process.env[RESULT_ENV];
});
afterEach(() => {
  if (caller === undefined) delete process.env[RESULT_ENV];
  else process.env[RESULT_ENV] = caller;
});
afterAll(cleanScratch);

test("with the variable set, the file is replaced by the value and detail", () => {
  const result = join(scratch(), "result.json");
  writeFileSync(result, "what an earlier write left behind");
  process.env[RESULT_ENV] = result;
  expect(report(2.5, "two and a half")).toBe(true);
  expect(JSON.parse(readFileSync(result, "utf8"))).toEqual({
    value: 2.5,
    detail: "two and a half",
  });
  expect(report(3)).toBe(true);
  expect(JSON.parse(readFileSync(result, "utf8"))).toEqual({ value: 3 });
});

test("outside a check nothing is written and report returns false", () => {
  const dir = scratch();
  const cwd = process.cwd();
  process.chdir(dir);
  try {
    delete process.env[RESULT_ENV];
    expect(report(7, "detail")).toBe(false);
    process.env[RESULT_ENV] = "";
    expect(report(7, "detail")).toBe(false);
  } finally {
    process.chdir(cwd);
  }
  expect(readdirSync(dir)).toEqual([]);
});

test("a non-finite value is refused without writing", () => {
  const result = join(scratch(), "result.json");
  writeFileSync(result, "untouched");
  process.env[RESULT_ENV] = result;
  for (const value of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
    expect(() => report(value)).toThrow(RangeError);
    expect(() => report(value)).toThrow("finite");
    expect(readFileSync(result, "utf8")).toBe("untouched");
  }
});

test("a failed write throws", () => {
  const result = join(scratch(), "no-such-directory", "result.json");
  process.env[RESULT_ENV] = result;
  expect(() => report(1)).toThrow();
  expect(existsSync(result)).toBe(false);
});

test("under the binary, what report wrote is the result's actual and detail", async () => {
  const dir = scratch();
  const sdk = JSON.stringify(join(ROOT, "sdks", "typescript", "src", "index.ts"));
  writeFileSync(
    join(dir, "measure.ts"),
    `import { report } from ${sdk};\nconst [value, detail] = process.argv.slice(2);\nreport(Number(value), detail);\n`,
  );
  const runner = JSON.stringify(process.execPath);
  const budget = (id: string, args: string) =>
    `  - id: ${id}\n    measure: reported\n    command: [${runner}, measure.ts, ${args}]\n` +
    "    unit: ms\n    direction: max\n    threshold: 1500\n";
  writeFileSync(
    join(dir, "budgets.yaml"),
    `schema_version: 1\nbudgets:\n${budget("p95", "'1395.5', p95 of 200 requests")}${budget("bare", "'1600'")}`,
  );
  const checked = await check({ cwd: dir, binary: builtBinary() });
  const byId = Object.fromEntries(checked.results.map((result) => [result.id, result]));
  expect(byId.p95).toMatchObject({
    actual: 1395.5,
    detail: "p95 of 200 requests",
    verdict: "within",
  });
  expect(byId.bare).toMatchObject({ actual: 1600, detail: null, verdict: "over" });
});
