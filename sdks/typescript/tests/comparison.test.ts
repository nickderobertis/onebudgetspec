// Saved observations drive the real CLI, with no measurement executable present.
import { afterAll, expect, test } from "bun:test";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  type ComparisonInput,
  compare,
  type RollingHistory,
  renderMarkdown,
} from "../src/index.ts";
import { builtBinary, cleanScratch, ROOT, scratch } from "./helpers.ts";

afterAll(cleanScratch);

test("saved rolling history roundtrips through comparison and Markdown rendering", async () => {
  const dir = scratch();
  const current = JSON.parse(
    readFileSync(join(ROOT, "crates/onebudgetspec-core/tests/fixtures/budget-run.json"), "utf8"),
  );
  const history: RollingHistory = {
    schema_version: 1,
    repository: current.repository,
    base_ref: current.base_ref,
    runs: [
      { ...current, head_sha: "base-0" },
      { ...current, head_sha: "base-1" },
      { ...current, head_sha: "base-2" },
      { ...current, head_sha: "base-3" },
      { ...current, head_sha: "base-4" },
    ],
  };
  const saved: ComparisonInput = {
    schema_version: 1,
    current,
    // This JSON is produced only from the typed local history above, with no external input.
    baseline: (JSON.parse(JSON.stringify(history)) as RollingHistory).runs,
  };
  const input = join(dir, "input.json");
  writeFileSync(input, JSON.stringify(saved));
  const options = { input, binary: builtBinary(), cwd: dir };
  const report = await compare(options);
  expect(report.rows[0]?.baseline_n).toBe(5);
  expect(report.rows[0]?.verdict).toBe("over");
  expect(report.rows[0]?.sigma_change).toBe(0);
  expect(report.rows[0]?.flag).toBe(false);
  const markdown = await renderMarkdown(options);
  expect(markdown).toContain(
    "| **latency** | 15.0 ms<br>+0.0% (+0.0 SD) | Over limit (report-only) |",
  );
  const failed = await compare({ ...options, reporter: join(dir, "missing") });
  expect(failed.rows).toEqual(report.rows);
  expect(failed.diagnostics.some((d) => d.includes("cannot start reporter"))).toBe(true);
  writeFileSync(input, "{}");
  await expect(compare(options)).rejects.toThrow("missing field");
  await expect(renderMarkdown(options)).rejects.toThrow("missing field");
});

test("saved input and reporter arguments reject non-string JavaScript values", async () => {
  // JavaScript can omit the required input; bypass TypeScript to prove runtime rejection.
  await expect(compare({ input: undefined as never })).rejects.toThrow("input must be");
  // JavaScript can supply a numeric path; bypass TypeScript to test that boundary.
  await expect(renderMarkdown({ input: 3 as never })).rejects.toThrow("input must be");
  // JavaScript can supply a numeric executable; bypass TypeScript to test that boundary.
  await expect(compare({ input: "input.json", reporter: 3 as never })).rejects.toThrow(
    "reporter must be",
  );
});
