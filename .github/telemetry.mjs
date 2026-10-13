import { relative } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { assertNeeds } from "./job-results.mjs";
import { diffs, events, lifecycle as lifecycleOrder } from "./scenarios.mjs";
import { root, workflows } from "./workflow-graph.mjs";
import { isRecord } from "./workflow-input.mjs";

const declaredJobs = new Map(
  workflows().map((workflow) => [
    relative(root, workflow.file),
    Object.keys(workflow.document.jobs),
  ]),
);

// llmlint: ignore-block[boundary_inputs_validated] The reporter contract forbids re-evaluating scenarios: exact matrix, condition and trigger correctness is proved by github-workflows:test against real workflows. Here the cached JSON boundary rejects missing/stale/empty per-job evidence; it deliberately does not repeat routing to attest every recorded row.
/** @param {unknown} run @returns {run is import('./contracts.js').Run} */
function validRun(run) {
  if (
    !isRecord(run) ||
    typeof run.workflow !== "string" ||
    typeof run.event !== "string" ||
    !Array.isArray(run.rows)
  )
    return false;
  try {
    assertNeeds(run.results);
  } catch {
    return false;
  }
  const declared = declaredJobs.get(run.workflow);
  if (
    !declared?.length ||
    !run.rows.length ||
    !isDeepStrictEqual(Object.keys(run.results).sort(), [...declared].sort())
  )
    return false;
  for (const job of declared)
    if (
      !run.rows.some(
        (row) =>
          isRecord(row) &&
          typeof row.job === "string" &&
          (row.job === job || row.job.startsWith(`${job}/`)),
      )
    )
      return false;
  return run.rows.every((row) => {
    if (!isRecord(row) || typeof row.job !== "string" || typeof row.skipped !== "boolean")
      return false;
    if (row.skipped) return row.runner === null && !row.macos;
    const labels = Array.isArray(row.runner) ? row.runner : [row.runner];
    if (
      !labels.length ||
      !labels.every((label) => typeof label === "string" && /^(macos|ubuntu|windows)-/.test(label))
    )
      return false;
    return row.macos === labels.some((label) => label.startsWith("macos-"));
  });
}
// llmlint: ignore-end[boundary_inputs_validated]
/** @param {unknown} input @returns {asserts input is import('./contracts.js').Telemetry} */
export function assertTelemetry(input) {
  const fail = () => {
    throw new Error("invalid/inconsistent Mac telemetry; run github-workflows:test");
  };
  if (
    !isRecord(input) ||
    typeof input.sourceDigest !== "string" ||
    !Array.isArray(input.scenarios) ||
    !input.scenarios.length ||
    !Array.isArray(input.lifecycles) ||
    !input.lifecycles.length
  )
    return fail();
  const coverage = new Map();
  for (const scenario of input.scenarios) {
    if (!isRecord(scenario)) return fail();
    const key = JSON.stringify([scenario.label, scenario.diff, scenario.fork, scenario.switches]);
    if (coverage.has(key)) return fail();
    coverage.set(key, scenario);
  }
  const expectedKeys = new Set();
  for (const [label] of events)
    for (const diff of diffs)
      for (const fork of [false, true])
        for (let switches = 0; switches < 8; switches++) {
          const key = JSON.stringify([label, diff, fork, switches]);
          expectedKeys.add(key);
          const scenario = coverage.get(key);
          if (!scenario || !Array.isArray(scenario.runs)) return fail();
          const required =
            label.startsWith("pr-") ||
            label.startsWith("manual-") ||
            ["base-push", "release-merge"].includes(label)
              ? ".github/workflows/ci.yml"
              : label === "release-published"
                ? ".github/workflows/release.yml"
                : undefined;
          if (
            required &&
            !scenario.runs.some(
              /** @param {unknown} run */ (run) => isRecord(run) && run.workflow === required,
            )
          )
            return fail();
        }
  if (coverage.size !== expectedKeys.size || input.lifecycles.length !== 16) return fail();
  const lifecycleCoverage = new Set();
  let jobs = 0;
  let runs = 0;
  for (const scenario of input.scenarios) {
    if (
      !isRecord(scenario) ||
      typeof scenario.label !== "string" ||
      typeof scenario.diff !== "string" ||
      typeof scenario.fork !== "boolean" ||
      typeof scenario.automatic !== "boolean" ||
      scenario.automatic !== !scenario.label.startsWith("manual-") ||
      !Number.isInteger(scenario.switches) ||
      !Array.isArray(scenario.runs) ||
      !scenario.runs.every(validRun)
    )
      return fail();
    jobs = Math.max(
      jobs,
      ...scenario.runs
        .filter(validRun)
        .map(
          /** @param {import("./contracts.js").Run} run */ (run) =>
            run.rows.filter((row) => row.macos).length,
        ),
    );
  }
  for (const lifecycle of input.lifecycles) {
    if (
      !isRecord(lifecycle) ||
      typeof lifecycle.fork !== "boolean" ||
      typeof lifecycle.switches !== "number" ||
      !Number.isInteger(lifecycle.switches) ||
      !Array.isArray(lifecycle.runs) ||
      !lifecycle.runs.every(validRun)
    )
      return fail();
    const key = JSON.stringify([lifecycle.fork, lifecycle.switches]);
    if (
      lifecycleCoverage.has(key) ||
      ![false, true].includes(lifecycle.fork) ||
      lifecycle.switches < 0 ||
      lifecycle.switches >= 8
    )
      return fail();
    lifecycleCoverage.add(key);
    const expectedRuns = lifecycleOrder.flatMap(([label, diff, owner]) => {
      const scenario = coverage.get(
        JSON.stringify([
          label,
          diff,
          owner === "original" ? lifecycle.fork : false,
          lifecycle.switches,
        ]),
      );
      if (!scenario || !Array.isArray(scenario.runs) || !scenario.runs.every(validRun))
        return fail();
      return scenario.runs
        .filter(validRun)
        .map(/** @param {import("./contracts.js").Run} run */ (run) => ({ ...run, label }));
    });
    if (!isDeepStrictEqual(expectedRuns, lifecycle.runs)) return fail();
    if (
      lifecycle.macosRuns !==
      lifecycle.runs.filter((run) => run.rows.some((row) => row.macos)).length
    )
      return fail();
    runs = Math.max(
      runs,
      lifecycle.runs.filter(validRun).filter((run) => run.rows.some((row) => row.macos)).length,
    );
  }
  if (
    !Number.isInteger(input.jobsPerRun) ||
    input.jobsPerRun !== jobs ||
    !Number.isInteger(input.runsPerChange) ||
    input.runsPerChange !== runs
  )
    return fail();
}
