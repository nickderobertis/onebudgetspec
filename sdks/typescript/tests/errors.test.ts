// How a call that gets no report rejects, and what it refuses before anything runs.
import { afterAll, expect, test } from "bun:test";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { check, listBudgets, OnebudgetspecError, schema, validate } from "../src/index.ts";
import { builtBinary, cleanScratch, recording, runCli, scratch } from "./helpers.ts";

afterAll(cleanScratch);

async function rejection(call: Promise<unknown>): Promise<OnebudgetspecError> {
  try {
    await call;
  } catch (error) {
    expect(error).toBeInstanceOf(OnebudgetspecError);
    return error as OnebudgetspecError;
  }
  throw new Error("the call resolved; it should have rejected");
}

test("an invalid file rejects every call with the CLI's own message", async () => {
  const dir = scratch();
  writeFileSync(join(dir, "budgets.yaml"), "schema_version: 1\nbudgets: nope\n");
  const cli = runCli(["validate"], dir);
  expect(cli.status).toBe(2);
  for (const call of [check, validate, listBudgets]) {
    const refused = await rejection(call({ cwd: dir, binary: builtBinary() }));
    expect(refused.exitCode).toBe(2);
    expect(refused.message).toBe(cli.stderr.trim());
    expect(refused.message).toContain("budgets.yaml");
  }
});

test("a binary that cannot run rejects", async () => {
  const dir = scratch();
  const refused = await rejection(check({ cwd: dir, binary: join(dir, "absent") }));
  expect(refused.exitCode).toBeNull();
  expect(refused.message).toContain("cannot run");
});

test("a status that is no report rejects with what the binary said", async () => {
  const dir = scratch();
  const log = join(dir, "ran.log");
  const refusing = recording(join(dir, "a"), "a", log, 'echo "launcher: no carrier" >&2; exit 69');
  const refused = await rejection(check({ cwd: dir, binary: refusing }));
  expect([refused.exitCode, refused.message]).toEqual([69, "launcher: no carrier"]);

  const silent = recording(join(dir, "b"), "b", log, "exit 70");
  expect((await rejection(check({ cwd: dir, binary: silent }))).message).toContain(
    "exited 70 with no message",
  );

  const killed = recording(join(dir, "c"), "c", log, "kill -9 $$");
  const ended = await rejection(validate({ cwd: dir, binary: killed }));
  expect(ended.exitCode).toBeNull();
  expect(ended.message).toContain("terminated by SIGKILL");
});

test.each([
  ["not json", "check", "printed no JSON"],
  ['{"schema_version": 1, "budgets": [], "extra": 1}', "list", "no valid list-report"],
  ['{"schema_version": 1}', "check", "no valid check-report"],
  ["not json", "schema", "not JSON"],
  ["[]", "schema", "not a JSON object"],
])("stdout %p from %s rejects: %s", async (printed, call, reason) => {
  const dir = scratch();
  const liar = recording(join(dir, "bin"), "liar", join(dir, "ran.log"), `echo '${printed}'`);
  const calls = {
    check: () => check({ binary: liar }),
    list: () => listBudgets({ binary: liar }),
    schema: () => schema({ binary: liar }),
  } as const;
  const refused = await rejection(calls[call as keyof typeof calls]());
  expect(refused.message).toContain(reason);
});

test.each([
  [{ ids: "api" }],
  [{ labels: ["api", 3] }],
  [{ excludeLabels: [null] }],
  [{ paths: "budgets.yaml" }],
])("%p is refused before anything runs", async (options) => {
  const dir = scratch();
  const never = join(dir, "never-run");
  // A JavaScript caller can pass what the types forbid; the SDK refuses it at the boundary.
  const call = () => check({ ...(options as object), cwd: dir, binary: never });
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
  );
  expect(refused.message).toContain("--version");
});
