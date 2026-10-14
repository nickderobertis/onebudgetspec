import { events } from "./scenarios.mjs";

const supportedEvents = new Set([...events.map(([, name]) => name), "workflow_call"]);
// Validate the scheduling subset this repository's resolver supports. actionlint owns
// GitHub's full workflow grammar; unsupported routing is an error, never an empty run.
/** @param {unknown} value @returns {value is Record<string, unknown>} */
export function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
/** @param {Record<string, unknown>} record @param {string[]} allowed @param {string} site */
function keys(record, allowed, site) {
  for (const key of Object.keys(record))
    if (!allowed.includes(key)) throw new Error(`unsupported ${site} key: ${key}`);
}
/** @param {unknown} input @returns {asserts input is import('./contracts.js').Matrix} */
export function assertMatrix(input) {
  if (!isRecord(input)) throw new Error("unresolved matrix: expected an object");
  for (const [key, value] of Object.entries(input)) {
    if (["include", "exclude"].includes(key)) {
      if (!Array.isArray(value) || !value.every(isRecord))
        throw new Error(`unresolved matrix ${key}`);
    } else if (!Array.isArray(value) && typeof value !== "string")
      throw new Error(`unresolved matrix dimension ${key}`);
  }
}
/** @param {unknown} input @returns {asserts input is import('./contracts.js').Workflow['document']} */
export function assertWorkflow(input) {
  if (!isRecord(input) || !isRecord(input.jobs))
    throw new Error("unresolved workflow: jobs must be an object");
  keys(
    input,
    ["name", "run-name", "on", "jobs", "permissions", "concurrency", "env", "defaults"],
    "workflow",
  );
  if (input.name !== undefined && typeof input.name !== "string")
    throw new Error("unresolved workflow name");
  const on = input.on;
  if (
    on !== undefined &&
    typeof on !== "string" &&
    !isRecord(on) &&
    !(Array.isArray(on) && on.every((e) => typeof e === "string"))
  )
    throw new Error("unresolved workflow events");
  const declared =
    typeof on === "string" ? [on] : Array.isArray(on) ? on : isRecord(on) ? Object.keys(on) : [];
  for (const event of declared)
    if (!supportedEvents.has(event))
      throw new Error(
        `unsupported workflow event: ${event}; add its workload scenario before routing it`,
      );
  if (isRecord(on)) {
    for (const [event, routing] of Object.entries(on)) {
      if (event === "schedule") {
        if (
          !Array.isArray(routing) ||
          !routing.every((row) => isRecord(row) && typeof row.cron === "string")
        )
          throw new Error("unresolved schedule");
        continue;
      }
      if (routing === null) continue;
      if (!isRecord(routing)) throw new Error(`unresolved ${event} routing`);
      keys(routing, ["types", "branches", "tags", "inputs", "workflows"], `${event} routing`);
      for (const key of ["types", "branches", "tags", "workflows"]) {
        if (
          routing[key] !== undefined &&
          !(Array.isArray(routing[key]) && routing[key].every((value) => typeof value === "string"))
        )
          throw new Error(`unresolved ${event}.${key}`);
      }
      for (const key of ["branches", "tags", "workflows"]) {
        const patterns = routing[key];
        if (Array.isArray(patterns) && patterns.some((pattern) => /[!*?[\]]/.test(pattern)))
          throw new Error(
            `unsupported ${event}.${key} pattern; add resolver support before routing it`,
          );
        if (patterns !== undefined && key !== "workflows" && event !== "push")
          throw new Error(`unsupported ${event}.${key} filter`);
      }
      if (routing.inputs !== undefined && !isRecord(routing.inputs))
        throw new Error("unresolved diagnostic inputs");
    }
  }
  for (const [id, job] of Object.entries(input.jobs)) {
    if (!isRecord(job)) throw new Error(`unresolved job: ${id}`);
    keys(
      job,
      [
        "name",
        "if",
        "needs",
        "uses",
        "with",
        "runs-on",
        "strategy",
        "steps",
        "defaults",
        "env",
        "outputs",
        "permissions",
        "concurrency",
        "timeout-minutes",
        "environment",
      ],
      `job ${id}`,
    );
    if (job.if !== undefined && !["string", "boolean"].includes(typeof job.if))
      throw new Error(`unresolved ${id}.if`);
    for (const key of ["name", "uses"])
      if (job[key] !== undefined && typeof job[key] !== "string")
        throw new Error(`unresolved ${id}.${key}`);
    if (
      job.needs !== undefined &&
      typeof job.needs !== "string" &&
      !(Array.isArray(job.needs) && job.needs.every((n) => typeof n === "string"))
    )
      throw new Error(`unresolved ${id}.needs`);
    if (job.with !== undefined && !isRecord(job.with)) throw new Error(`unresolved ${id}.with`);
    if (
      !job.uses &&
      typeof job["runs-on"] !== "string" &&
      !(Array.isArray(job["runs-on"]) && job["runs-on"].every((label) => typeof label === "string"))
    )
      throw new Error(`unresolved ${id} runner`);
    if (job.strategy !== undefined) {
      if (!isRecord(job.strategy)) throw new Error(`unresolved ${id}.strategy`);
      keys(job.strategy, ["matrix", "fail-fast", "max-parallel"], `strategy ${id}`);
      if (job.strategy.matrix !== undefined && typeof job.strategy.matrix !== "string")
        assertMatrix(job.strategy.matrix);
    }
    if (
      job.outputs !== undefined &&
      (!isRecord(job.outputs) ||
        !Object.values(job.outputs).every((value) => typeof value === "string"))
    )
      throw new Error(`unresolved ${id}.outputs`);
    if (job.steps !== undefined && !(Array.isArray(job.steps) && job.steps.every(isRecord)))
      throw new Error(`unresolved ${id}.steps`);
    if (Array.isArray(job.steps))
      for (const step of job.steps) {
        for (const key of ["id", "run", "uses"])
          if (step[key] !== undefined && typeof step[key] !== "string")
            throw new Error(`unresolved step ${id}.${key}`);
        if (
          step.env !== undefined &&
          (!isRecord(step.env) ||
            !Object.values(step.env).every((value) => typeof value === "string"))
        )
          throw new Error(`unresolved ${id} step env`);
        if (
          step.with !== undefined &&
          (!isRecord(step.with) ||
            !Object.values(step.with).every((value) =>
              ["string", "number", "boolean"].includes(typeof value),
            ))
        )
          throw new Error(`unresolved ${id} step inputs`);
      }
  }
}
