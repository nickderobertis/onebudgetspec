// Consume recorded telemetry only; the SDK writes the result and the engine judges it.
import { readFileSync } from "node:fs";
import { report } from "@onebudgetspec/sdk";
import { assertTelemetry } from "./telemetry.mjs";
import { digest } from "./workflow-graph.mjs";

const args = process.argv.slice(2);
if (args.length !== 4 || args[0] !== "--metric" || args[2] !== "--telemetry")
  throw new Error("usage: --metric jobs-per-run|runs-per-change --telemetry JSON");
let telemetry;
try {
  telemetry = JSON.parse(readFileSync(args[3], "utf8"));
} catch (error) {
  throw new Error(`missing/unreadable Mac telemetry: ${String(error)}; run github-workflows:test`);
}
assertTelemetry(telemetry);
if (telemetry.sourceDigest !== digest())
  throw new Error("stale Mac telemetry; run github-workflows:test");
let figure, breakdown;
if (args[1] === "jobs-per-run") {
  figure = telemetry.jobsPerRun;
  breakdown = telemetry.scenarios.flatMap((s) =>
    s.runs.map((r) => ({
      event: s.label,
      diff: s.diff,
      fork: s.fork,
      publish: s.switches,
      workflow: r.workflow,
      jobs: r.rows.filter((row) => row.macos),
    })),
  );
} else if (args[1] === "runs-per-change") {
  figure = telemetry.runsPerChange;
  breakdown = telemetry.lifecycles.map((l) => ({
    fork: l.fork,
    publish: l.switches,
    runs: l.runs
      .filter((r) => r.rows.some((row) => row.macos))
      .map((r) => ({
        workflow: r.workflow,
        event: r.label,
        jobs: r.rows.filter((row) => row.macos),
      })),
  }));
} else throw new Error(`unknown metric: ${args[1]}`);
if (!Number.isFinite(figure) || !telemetry.scenarios?.length || !telemetry.lifecycles?.length)
  throw new Error("missing/incomplete Mac telemetry; run github-workflows:test");
report(figure, JSON.stringify(breakdown));
