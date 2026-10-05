// How a call that gets no report rejects, and what it refuses before anything runs.
import { afterAll, expect, test } from "bun:test";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { check, listBudgets, OnebudgetspecError, schema, validate } from "../src/index.ts";
import { builtBinary, cleanScratch, recording, rejection, runCli, scratch } from "./helpers.ts";

afterAll(cleanScratch);

test("an invalid file rejects every call with the CLI's own message", async () => {
  const dir = scratch();
  writeFileSync(join(dir, "budgets.yaml"), "schema_version: 1\nbudgets: nope\n");
  const cli = runCli(["validate"], dir);
  expect(cli.status).toBe(2);
  for (const call of [check, validate, listBudgets]) {
    const refused = await rejection(call({ cwd: dir, binary: builtBinary() }), OnebudgetspecError);
    expect(refused.exitCode).toBe(2);
    expect(refused.message).toBe(cli.stderr.trim());
    expect(refused.message).toContain("budgets.yaml");
  }
});

test("a binary that cannot run rejects", async () => {
  const dir = scratch();
  const refused = await rejection(
    check({ cwd: dir, binary: join(dir, "absent") }),
    OnebudgetspecError,
  );
  expect(refused.exitCode).toBeNull();
  expect(refused.message).toContain("cannot run");
});

test("a status that is no report rejects with what the binary said", async () => {
  const dir = scratch();
  const log = join(dir, "ran.log");
  const refusing = recording(join(dir, "a"), "a", log, 'echo "launcher: no carrier" >&2; exit 69');
  const refused = await rejection(check({ cwd: dir, binary: refusing }), OnebudgetspecError);
  expect([refused.exitCode, refused.message]).toEqual([69, "launcher: no carrier"]);

  const silent = recording(join(dir, "b"), "b", log, "exit 70");
  expect(
    (await rejection(check({ cwd: dir, binary: silent }), OnebudgetspecError)).message,
  ).toContain("exited 70 with no message");

  const killed = recording(join(dir, "c"), "c", log, "kill -9 $$");
  const ended = await rejection(validate({ cwd: dir, binary: killed }), OnebudgetspecError);
  expect(ended.exitCode).toBeNull();
  expect(ended.message).toContain("terminated by SIGKILL");
});

type Call = (binary: string) => Promise<unknown>;
const checkWith: Call = (binary) => check({ binary });
const listWith: Call = (binary) => listBudgets({ binary });
const schemaWith: Call = (binary) => schema({ binary });

test.each<[string, string, Call, string]>([
  ["not json", "check", checkWith, "printed no JSON"],
  ['{"schema_version": 1, "budgets": [], "extra": 1}', "list", listWith, "no valid list-report"],
  ['{"schema_version": 1}', "check", checkWith, "no valid check-report"],
  [
    JSON.stringify({
      schema_version: 1,
      results: [
        {
          id: "late",
          file: "budgets.yaml",
          labels: [],
          unit: "seconds",
          direction: "max",
          threshold: 1,
          verdict: "within",
          actual: 0.5,
          headroom: 0.5,
          headroom_percent: 50,
          detail: null,
          error: null,
          started_at: "yesterday",
          ended_at: "1970-01-01T00:00:00Z",
          host: { load1: null, cpus: 1, mem_available_mib: null, conditions: {} },
        },
      ],
    }),
    "check",
    checkWith,
    'must match format "date-time"',
  ],

  ["not json", "schema", schemaWith, "not JSON"],
  ["[]", "schema", schemaWith, "not a JSON object"],
])("stdout %p from %s rejects", async (printed, _name, call, reason) => {
  const dir = scratch();
  const liar = recording(join(dir, "bin"), "liar", join(dir, "ran.log"), `echo '${printed}'`);
  const refused = await rejection(call(liar), OnebudgetspecError);
  expect(refused.message).toContain(reason);
});

// A JavaScript caller can pass what the types forbid, so each row is typed as the open
// object such a caller holds; the SDK refuses it at the boundary, before anything runs.
test.each<[Record<string, unknown>]>([
  [{ ids: "api" }],
  [{ labels: ["api", 3] }],
  [{ excludeLabels: [null] }],
  [{ paths: "budgets.yaml" }],
])("%p is refused before anything runs", async (options) => {
  const dir = scratch();
  const call = () => check({ ...options, cwd: dir, binary: join(dir, "never-run") });
  await expect(call()).rejects.toBeInstanceOf(TypeError);
});

test("a value shaped like a flag is passed as a value", async () => {
  const dir = scratch();
  writeFileSync(
    join(dir, "budgets.yaml"),
    "schema_version: 1\nbudgets:\n  - id: quick\n    labels: [api]\n    measure: elapsed\n" +
      '    command: ["/bin/sh", "-c", "exit 0"]\n    unit: seconds\n' +
      "    direction: max\n    threshold: 60\n",
  );
  const listed = await listBudgets({ labels: ["--recursive"], cwd: dir, binary: builtBinary() });
  expect(listed.budgets).toEqual([]);
  const refused = await rejection(
    listBudgets({ paths: ["--version"], cwd: dir, binary: builtBinary() }),
    OnebudgetspecError,
  );
  expect(refused.message).toContain("--version");
});
