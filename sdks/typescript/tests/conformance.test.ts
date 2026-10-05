// Every conformance case, run through the SDK's own calls against the built binary.
//
// Each case's `args` become a `check` call, and the same files and selection a `listBudgets`
// and a `validate` call. The check report, normalized as conformance/README.md defines,
// must equal the case's expected.json; a case the CLI refuses must reject with the CLI's own
// message; and `listBudgets` and `validate` must answer exactly what the binary answers
// when run directly.
import { afterAll, afterEach, beforeEach, describe, expect, test } from "bun:test";
import { cpSync, existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import {
  BINARY_ENV,
  type CheckReport,
  check,
  listBudgets,
  OnebudgetspecError,
  schema,
  validate,
} from "../src/index.ts";
import { builtBinary, CASES, cleanScratch, runCli, scratch } from "./helpers.ts";

type Case = {
  description: string;
  args: string[];
  exit: number;
  timed?: string[];
  error_contains?: Record<string, string>;
};

type Invocation = {
  verb: string;
  paths: string[];
  ids: string[];
  labels: string[];
  excludeLabels: string[];
  recursive: boolean;
};

/** Read `args` with the grammar the cases use; anything else throws, so a case using a flag
 * this runner cannot pass to the SDK is never silently dropped. */
function parse(args: string[]): Invocation {
  const [verb = "", ...rest] = args;
  const invocation: Invocation = {
    verb,
    paths: [],
    ids: [],
    labels: [],
    excludeLabels: [],
    recursive: false,
  };
  for (let index = 0; index < rest.length; index++) {
    const word = rest[index] ?? "";
    const value = () => rest[++index] ?? "";
    if (word === "--json") continue;
    if (word === "--recursive") invocation.recursive = true;
    else if (word === "--id") invocation.ids.push(value());
    else if (word === "--label") invocation.labels.push(value());
    else if (word === "--exclude-label") invocation.excludeLabels.push(value());
    else if (word.startsWith("-")) throw new Error(`the SDK runner cannot pass ${word}`);
    else invocation.paths.push(word);
  }
  return invocation;
}

function filesArgs(invocation: Invocation): string[] {
  return [...(invocation.recursive ? ["--recursive"] : []), ...invocation.paths];
}

function selectionArgs(invocation: Invocation): string[] {
  return [
    ...invocation.ids.flatMap((id) => ["--id", id]),
    ...invocation.labels.flatMap((label) => ["--label", label]),
    ...invocation.excludeLabels.flatMap((label) => ["--exclude-label", label]),
  ];
}

const EPOCH = "1970-01-01T00:00:00Z";

/** The normalization conformance/README.md defines, applied to a copy of `report`. */
function normalize(report: CheckReport, spec: Case, name: string): unknown {
  const copy = structuredClone(report);
  for (const result of copy.results) {
    result.started_at = EPOCH;
    result.ended_at = EPOCH;
    result.host.load1 = 0.0;
    result.host.cpus = 1;
    result.host.mem_available_mib = 0;
    if (result.error !== null) {
      const expected = spec.error_contains?.[result.id];
      if (expected !== undefined && !result.error.includes(expected)) {
        throw new Error(`${name}: ${result.id}'s error ${result.error} lacks ${expected}`);
      }
      result.error = "<error>";
    }
    if (spec.timed?.includes(result.id)) {
      if (result.actual !== null) result.actual = 0.0;
      if (result.headroom !== null) result.headroom = 0.0;
      if (result.headroom_percent !== null) result.headroom_percent = 0.0;
    }
  }
  return copy;
}

/** The status the CLI exits with for `report`: 3 on an error, 1 when over, else 0. */
function exitStatus(report: CheckReport): number {
  const verdicts = new Set(report.results.map((result) => result.verdict));
  return verdicts.has("error") ? 3 : verdicts.has("over") ? 1 : 0;
}

async function rejection(call: Promise<unknown>): Promise<OnebudgetspecError> {
  try {
    await call;
  } catch (error) {
    expect(error).toBeInstanceOf(OnebudgetspecError);
    return error as OnebudgetspecError;
  }
  throw new Error("the call resolved; it should have rejected");
}

const caseNames = readdirSync(CASES)
  .filter((name) => statSync(join(CASES, name)).isDirectory())
  .sort();

function prepare(name: string): { spec: Case; work: string; invocation: Invocation } {
  const work = join(scratch(), name);
  cpSync(join(CASES, name), work, { recursive: true });
  const spec = JSON.parse(readFileSync(join(work, "case.json"), "utf8")) as Case;
  return { spec, work, invocation: parse(spec.args) };
}

afterAll(cleanScratch);
beforeEach(() => {
  process.env[BINARY_ENV] = builtBinary();
});
afterEach(() => {
  delete process.env[BINARY_ENV];
});

describe("check returns the report each case expects", () => {
  test("there are cases", () => {
    expect(caseNames.length).toBeGreaterThan(0);
  });
  for (const name of caseNames) {
    test(name, async () => {
      const { spec, work, invocation } = prepare(name);
      expect(invocation.verb).toBe("check");
      const call = () =>
        check({
          paths: invocation.paths.length > 0 ? invocation.paths : undefined,
          ids: invocation.ids,
          labels: invocation.labels,
          excludeLabels: invocation.excludeLabels,
          recursive: invocation.recursive,
          cwd: work,
        });
      if (spec.exit === 2) {
        const refused = await rejection(call());
        const cli = runCli(spec.args, work);
        expect(cli.status).toBe(2);
        expect(refused.exitCode).toBe(2);
        expect(refused.message).toBe(cli.stderr.trim());
        expect(existsSync(join(work, "expected.json"))).toBe(false);
        return;
      }
      const report = await call();
      expect(exitStatus(report)).toBe(spec.exit);
      const expected = JSON.parse(readFileSync(join(work, "expected.json"), "utf8"));
      expect(normalize(report, spec, name)).toEqual(expected);
    });
  }
});

describe("listBudgets and validate answer as the binary does", () => {
  for (const name of caseNames) {
    test(name, async () => {
      const { spec, work, invocation } = prepare(name);
      const listing = runCli(
        ["list", "--json", ...selectionArgs(invocation), ...filesArgs(invocation)],
        work,
      );
      const listCall = () =>
        listBudgets({
          paths: invocation.paths,
          ids: invocation.ids,
          labels: invocation.labels,
          excludeLabels: invocation.excludeLabels,
          recursive: invocation.recursive,
          cwd: work,
        });
      if (listing.status === 2) {
        const refused = await rejection(listCall());
        expect([refused.exitCode, refused.message]).toEqual([2, listing.stderr.trim()]);
        expect(spec.exit).toBe(2);
      } else {
        const listed = await listCall();
        expect(listed).toEqual(JSON.parse(listing.stdout));
        // What list selects is what check measured, in the same order.
        const expected = JSON.parse(readFileSync(join(work, "expected.json"), "utf8")) as {
          results: Record<string, unknown>[];
        };
        const keys = ["id", "file", "labels", "unit", "direction", "threshold"] as const;
        const pick = (entry: object) =>
          Object.fromEntries(keys.map((key) => [key, (entry as Record<string, unknown>)[key]]));
        expect(listed.budgets.map(pick)).toEqual(expected.results.map(pick));
      }

      const validation = runCli(["validate", "--json", ...filesArgs(invocation)], work);
      const validateCall = () =>
        validate({ paths: invocation.paths, recursive: invocation.recursive, cwd: work });
      if (validation.status === 2) {
        const refused = await rejection(validateCall());
        expect([refused.exitCode, refused.message]).toEqual([2, validation.stderr.trim()]);
      } else {
        expect(validation.status).toBe(0);
        expect(await validateCall()).toEqual(JSON.parse(validation.stdout));
      }
    });
  }
});

function caseReport(name: string): Promise<CheckReport> {
  return check({ cwd: prepare(name).work });
}

test("returned conditions are read beside the declared ones", async () => {
  const [gate, plain] = (await caseReport("returned-conditions")).results;
  expect(gate?.verdict).toBe("within");
  expect(gate?.host.conditions).toEqual({
    dispatches: "3",
    dispatches_max: "6",
    gate_load: "2.1",
  });
  expect(gate?.host.conditions.gate_load).toBe("2.1");
  expect(plain?.host.conditions).toEqual({ dispatches: "3" });
});

test("colliding and malformed returned conditions are error results", async () => {
  const [collides, hostValue, fine] = (await caseReport("returned-condition-collides")).results;
  for (const result of [collides, hostValue]) {
    expect([result?.verdict, result?.actual]).toEqual(["error", null]);
    expect(result?.host.conditions).toEqual({ dispatches: "3" });
  }
  expect(collides?.error).toContain("dispatches");
  expect(hostValue?.error).toContain("load1");
  expect(fine?.verdict).toBe("within");

  const malformed = (await caseReport("returned-condition-malformed")).results;
  expect(malformed.map((result) => result.verdict)).toEqual(["error", "error", "error"]);
  expect(malformed.map((result) => result.host.conditions)).toEqual([{}, {}, {}]);
});

test("schema() is the bundle the binary prints", async () => {
  const printed = runCli(["schema"], scratch());
  expect(printed.status).toBe(0);
  expect(await schema()).toEqual(JSON.parse(printed.stdout));
});
