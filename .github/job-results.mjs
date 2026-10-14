// Only GitHub job outcomes are supplied; omitted or unauthorized work fails closed.

import { fileURLToPath } from "node:url";
import { isRecord } from "./workflow-input.mjs";

/** @param {unknown} input @returns {asserts input is import('./contracts.js').Needs} */
export function assertNeeds(input) {
  if (!isRecord(input)) throw new Error("NEEDS must be a job outcome map; pass toJSON(needs)");
  for (const [name, outcome] of Object.entries(input)) {
    if (
      !isRecord(outcome) ||
      typeof outcome.result !== "string" ||
      !["success", "failure", "cancelled", "skipped"].includes(outcome.result)
    )
      throw new Error(`invalid outcome for ${name}; pass GitHub job results`);
    if (
      outcome.outputs !== undefined &&
      (!isRecord(outcome.outputs) ||
        !Object.values(outcome.outputs).every((value) => typeof value === "string"))
    )
      throw new Error(`invalid outputs for ${name}; pass GitHub job outputs`);
  }
}
/** @param {import("./contracts.js").Needs} needs @param {string[]} required @param {Record<string, boolean>} optional */
export function assertResults(needs, required, optional = {}) {
  assertNeeds(needs);
  if (!required.length)
    throw new Error("name at least one required job; use the workflow needs list");
  for (const name of required) {
    const result = needs[name]?.result;
    if (result === "success") continue;
    if (result === "skipped" && optional[name] === true) continue;
    throw new Error(`${name} finished ${result ?? "missing"}; inspect its prerequisite/proof job`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const needs = JSON.parse(process.env.NEEDS ?? "{}");
  assertNeeds(needs);
  // The job's own needs list is the required set, so a need added there is enforced here.
  if (process.argv.length > 2) throw new Error("pass no job names; every job in NEEDS is required");
  const required = Object.keys(needs);
  /** @type {Record<string, boolean>} */
  const optional = {};
  if (required.includes("mac-proof")) {
    optional["mac-proof"] =
      needs.policy?.result === "success" &&
      needs.policy.outputs?.macosRequired === "false" &&
      needs.policy.outputs?.diagnostic !== "true";
  }
  assertResults(needs, required, optional);
  if (optional["mac-proof"] && needs["mac-proof"]?.result === "skipped")
    console.log("Mac proof intentionally omitted: verified routing policy; no Mac tests ran.");
}
