// `checkSubjects` against the built binary, over real directories and real commands. Each
// subject's budgets file is read from its own directory, and its commands run there with its
// own environment: one reads `SUBJECT_MINUTES` from the environment the subject gives, the
// other a number from `./measure.txt`. The typed answer must hold what the binary prints when
// run directly.
import { afterAll, expect, test } from "bun:test";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  checkSubjects,
  OnebudgetspecError,
  type Subjects,
  type SubjectsReport,
} from "../src/index.ts";
import { builtBinary, cleanScratch, runCli, scratch } from "./helpers.ts";

afterAll(cleanScratch);

const DELIVERY =
  'require("fs").writeFileSync(process.env.ONEBUDGETSPEC_RESULT, ' +
  "JSON.stringify({ value: Number(process.env.SUBJECT_MINUTES) }));";
const GATE_RUNS =
  'const fs = require("fs"); fs.writeFileSync("gate-runs.ran", ""); ' +
  "fs.writeFileSync(process.env.ONEBUDGETSPEC_RESULT, " +
  'JSON.stringify({ value: Number(fs.readFileSync("./measure.txt", "utf8")) }));';

function budget(id: string, label: string, script: string, unit: string, threshold: number) {
  return (
    `  - id: ${id}\n    labels: [${label}]\n    measure: reported\n` +
    `    command: ${JSON.stringify(["node", "-e", script])}\n` +
    `    unit: ${unit}\n    direction: max\n    threshold: ${threshold}\n`
  );
}

const DELIVERY_TIME = budget("delivery-time", "delivery", DELIVERY, "minutes", 10);
const GATE_RUNS_BUDGET = budget("gate-runs", "gates", GATE_RUNS, "runs", 5);

/** Two subjects with both budgets and their own `measure.txt`, one with no budgets file,
 * and one holding only the `gates` budget. */
function subjectsDir(): string {
  const dir = scratch();
  for (const [name, runs] of [
    ["change-a", "2"],
    ["change-b", "7"],
  ] as const) {
    mkdirSync(join(dir, name));
    writeFileSync(
      join(dir, name, "budgets.yaml"),
      `schema_version: 1\nbudgets:\n${DELIVERY_TIME}${GATE_RUNS_BUDGET}`,
    );
    writeFileSync(join(dir, name, "measure.txt"), runs);
  }
  mkdirSync(join(dir, "change-c"));
  mkdirSync(join(dir, "change-d"));
  writeFileSync(
    join(dir, "change-d", "budgets.yaml"),
    `schema_version: 1\nbudgets:\n${GATE_RUNS_BUDGET}`,
  );
  writeFileSync(join(dir, "change-d", "measure.txt"), "1");
  return dir;
}

function document(withD = false): Subjects {
  const subjects: Subjects["subjects"] = [
    {
      id: "change-a",
      directory: "change-a",
      env: { SUBJECT_MINUTES: "30" },
      context: { node: 1, title: "first" },
    },
    {
      id: "change-b",
      directory: "change-b",
      env: { SUBJECT_MINUTES: "4" },
      files: ["budgets.yaml"],
      context: ["second", 2],
    },
    { id: "change-c", directory: "change-c", context: "third" },
  ];
  if (withD) subjects.push({ id: "change-d", directory: "change-d" });
  return { schema_version: 1, subjects };
}

/** Each checked subject's `[budget, verdict, actual]` triples, by subject id. */
function verdicts(answer: SubjectsReport) {
  return Object.fromEntries(
    answer.subjects.map((subject) => [
      subject.id,
      subject.report.results.map((result) => [result.id, result.verdict, result.actual]),
    ]),
  );
}

/** What the binary prints for `subjects`, run directly. */
function cliAnswer(dir: string, subjects: Subjects, ...args: string[]): SubjectsReport {
  writeFileSync(join(dir, "subjects.json"), JSON.stringify(subjects));
  const ran = runCli(["check", "--subjects", "subjects.json", "--json", ...args], dir);
  expect(ran.status).toBe(1);
  return JSON.parse(ran.stdout);
}

test("each subject is answered as the binary answers it", async () => {
  const dir = subjectsDir();
  const answer = await checkSubjects({ subjects: document(), cwd: dir, binary: builtBinary() });
  expect(verdicts(answer)).toEqual({
    "change-a": [
      ["delivery-time", "over", 30],
      ["gate-runs", "within", 2],
    ],
    "change-b": [
      ["delivery-time", "within", 4],
      ["gate-runs", "over", 7],
    ],
  });
  expect(answer.subjects.map((subject) => [subject.id, subject.context])).toEqual([
    ["change-a", { node: 1, title: "first" }],
    ["change-b", ["second", 2]],
  ]);
  expect(answer.unanswered.map((entry) => [entry.id, entry.context])).toEqual([
    ["change-c", "third"],
  ]);
  expect(answer.unanswered[0]?.reason).toContain("no such file");

  const cli = cliAnswer(dir, document());
  expect(verdicts(answer)).toEqual(verdicts(cli));
  expect(answer.subjects.map((s) => [s.id, s.context])).toEqual(
    cli.subjects.map((s) => [s.id, s.context]),
  );
  expect(answer.unanswered).toEqual(cli.unanswered);
});

test("a label keeps one budget per subject, and a subject it keeps none of is unanswered", async () => {
  const dir = subjectsDir();
  const answer = await checkSubjects({
    subjects: document(true),
    labels: ["delivery"],
    cwd: dir,
    binary: builtBinary(),
  });
  expect(verdicts(answer)).toEqual({
    "change-a": [["delivery-time", "over", 30]],
    "change-b": [["delivery-time", "within", 4]],
  });
  for (const name of ["change-a", "change-b", "change-d"]) {
    expect(existsSync(join(dir, name, "gate-runs.ran"))).toBe(false);
  }
  expect(answer.unanswered.map((entry) => entry.id)).toEqual(["change-c", "change-d"]);
  expect(answer.unanswered[1]?.context).toBeNull();
  expect(answer.unanswered[1]?.reason).toContain("selection is empty");
  const cli = cliAnswer(dir, document(true), "--label", "delivery");
  expect(verdicts(answer)).toEqual(verdicts(cli));
  expect(answer.unanswered).toEqual(cli.unanswered);
});

test("overOnly keeps only the over results", async () => {
  const dir = subjectsDir();
  const answer = await checkSubjects({
    subjects: document(),
    overOnly: true,
    cwd: dir,
    binary: builtBinary(),
  });
  expect(verdicts(answer)).toEqual({
    "change-a": [["delivery-time", "over", 30]],
    "change-b": [["gate-runs", "over", 7]],
  });
});

test("an invalid document rejects with the binary's message and status 2", async () => {
  const dir = subjectsDir();
  const duplicated: Subjects = {
    schema_version: 1,
    subjects: [
      { id: "x", directory: "change-a" },
      { id: "x", directory: "change-b" },
    ],
  };
  const rejected = checkSubjects({ subjects: duplicated, cwd: dir, binary: builtBinary() });
  await expect(rejected).rejects.toThrow(OnebudgetspecError);
  await expect(rejected).rejects.toMatchObject({ exitCode: 2 });
  await expect(rejected).rejects.toThrow("subjects[1].id");
  expect(existsSync(join(dir, "change-a", "gate-runs.ran"))).toBe(false);
});

test("arguments of the wrong type are refused before anything runs", async () => {
  const wrong = [
    { subjects: "subjects.json" },
    { subjects: { schema_version: 1, subjects: [] }, overOnly: "yes" },
    { subjects: { schema_version: 1, subjects: [] }, recursive: 1 },
  ];
  for (const options of wrong) {
    // A JavaScript caller may pass anything; the call must refuse what is not its shape.
    await expect(
      checkSubjects({ ...(options as unknown as { subjects: Subjects }), binary: "never-run" }),
    ).rejects.toThrow(TypeError);
  }
});
