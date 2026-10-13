// Resolve this repository's scheduling graph with GitHub's published expression engine.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { data, Evaluator, Lexer, Parser } from "@actions/expressions";
import { parse } from "yaml";
import { guardOutputs } from "./guard-outputs.mjs";
import { assertMatrix, assertWorkflow } from "./workflow-input.mjs";

export const root = resolve(import.meta.dirname, "..");
/** @param {unknown} input @returns {import("@actions/expressions").data.ExpressionData} */
function value(input) {
  if (input === null || input === undefined) return new data.Null();
  if (typeof input === "boolean") return new data.BooleanData(input);
  if (typeof input === "number") return new data.NumberData(input);
  if (typeof input === "string") return new data.StringData(input);
  if (Array.isArray(input)) return new data.Array(...input.map(value));
  return new data.Dictionary(
    ...Object.entries(input).map(([key, val]) => ({ key, value: value(val) })),
  );
}
/** @param {string | boolean} expression @param {import("./contracts.js").Context} context @returns {import("./contracts.js").Json} */
export function evaluate(expression, context) {
  if (typeof expression !== "string") return expression;
  const source = expression
    .replace(/^\s*\$\{\{/, "")
    .replace(/\}\}\s*$/, "")
    .trim();
  const needs = Object.values(context.needs ?? {});
  const funcs = new Map(
    Object.entries({
      always: /** @returns {boolean} */ () => true,
      success: () => needs.every((n) => n.result === "success"),
      failure: () => needs.some((n) => n.result === "failure"),
      cancelled: () => needs.some((n) => n.result === "cancelled"),
    }).map(([name, call]) => [name, { name, minArgs: 0, maxArgs: 0, call: () => value(call()) }]),
  );
  const ast = new Parser(new Lexer(source).lex().tokens, Object.keys(context), [
    ...funcs.values(),
  ]).parse();
  const dictionary = value(context);
  if (!(dictionary instanceof data.Dictionary))
    throw new Error("expression context must be an object");
  return JSON.parse(
    JSON.stringify(new Evaluator(ast, dictionary, funcs).evaluate(), data.replacer),
  );
}
/** @param {import("./contracts.js").Json | undefined} input @param {import("./contracts.js").Context} context @returns {import("./contracts.js").Json} */
function render(input, context) {
  if (typeof input !== "string") return input ?? null;
  if (/^\s*\$\{\{[\s\S]*\}\}\s*$/.test(input) && (input.match(/\$\{\{/g) ?? []).length === 1)
    return evaluate(input, context);
  return input.replace(/\$\{\{(.*?)\}\}/g, (_, expression) =>
    String(evaluate(expression, context)),
  );
}
/** @param {import("./contracts.js").Matrix | string} matrix @param {import("./contracts.js").Context} context */
export function expand(matrix = {}, context = {}) {
  if (typeof matrix === "string") {
    const resolved = evaluate(matrix, context);
    assertMatrix(resolved);
    matrix = resolved;
  }
  assertMatrix(matrix);
  /** @type {Record<string, import("./contracts.js").Json>[]} */
  let rows = [{}];
  for (const [key, definition] of Object.entries(matrix)) {
    if (["include", "exclude"].includes(key)) continue;
    const options = render(definition, context);
    if (!Array.isArray(options)) throw new Error(`unresolved matrix dimension ${key}`);
    rows = rows.flatMap((row) => options.map((option) => ({ ...row, [key]: option })));
  }
  rows = rows.filter(
    (row) =>
      !(matrix.exclude ?? []).some((excluded) =>
        Object.entries(excluded).every(([key, val]) => row[key] === val),
      ),
  );
  if (matrix.include && Object.keys(matrix).every((k) => ["include", "exclude"].includes(k)))
    return matrix.include.map((row) => ({ ...row }));
  // Includes merge only into original combinations; appended rows stay independent.
  // https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations
  const originalRows = [...rows];
  for (const included of matrix.include ?? []) {
    let merged = false;
    for (const row of originalRows) {
      if (
        Object.keys(matrix)
          .filter((k) => !["include", "exclude"].includes(k))
          .every((k) => !(k in included) || row[k] === included[k])
      ) {
        Object.assign(row, included);
        merged = true;
      }
    }
    if (!merged) rows.push(included);
  }
  return rows;
}
/** @returns {import("./contracts.js").Workflow[]} */
export function workflows(directory = resolve(root, ".github/workflows")) {
  return readdirSync(directory)
    .filter((f) => /\.ya?ml$/.test(f))
    .map((file) => {
      const document = parse(readFileSync(resolve(directory, file), "utf8"));
      assertWorkflow(document);
      return { file: resolve(directory, file), document };
    });
}
/** @param {import("./contracts.js").Workflow["document"]} document @param {import("./contracts.js").Event} scenario */
export function triggered(document, scenario) {
  let events = document.on;
  if (typeof events === "string") events = { [events]: {} };
  if (Array.isArray(events)) events = Object.fromEntries(events.map((e) => [e, {}]));
  const routing = events?.[scenario.event_name];
  if (routing === undefined) return false;
  if (Array.isArray(routing)) return scenario.event_name === "schedule";
  if (routing?.types && !routing.types.includes(scenario.action ?? "")) return false;
  if (scenario.event_name === "push") {
    if (scenario.ref?.startsWith("refs/tags/")) {
      if (routing?.tags) return routing.tags.includes(scenario.ref.slice("refs/tags/".length));
      return !routing?.branches;
    }
    if (
      routing?.branches &&
      !routing.branches.includes(scenario.ref?.replace("refs/heads/", "") ?? "")
    )
      return false;
    if (routing?.tags && !routing?.branches) return false;
  }
  if (
    scenario.event_name === "workflow_run" &&
    routing?.workflows &&
    !routing.workflows.includes(scenario.workflow_run?.name ?? "")
  )
    return false;
  return true;
}
/** @param {import("./contracts.js").Workflow} workflow @param {import("./contracts.js").Event} scenario @param {import("./contracts.js").Policy} policy @param {import("./contracts.js").Needs} overrides @param {string[]} stack @returns {import("./contracts.js").Run} */
export function resolveRun(workflow, scenario, policy, overrides = {}, stack = []) {
  assertWorkflow(workflow.document);
  if (stack.includes(workflow.file))
    throw new Error(`recursive reusable workflow: ${workflow.file}`);
  const jobs = workflow.document.jobs;
  if (!jobs || typeof jobs !== "object") throw new Error(`unresolved jobs: ${workflow.file}`);
  /** @type {import("./contracts.js").Needs} */
  const results = {};
  /** @type {import("./contracts.js").Row[]} */
  const rows = [];
  const pending = new Set(Object.keys(jobs));
  while (pending.size) {
    let progress = false;
    for (const id of [...pending]) {
      const job = jobs[id];
      const required = typeof job.needs === "string" ? [job.needs] : (job.needs ?? []);
      if (required.some((n) => !(n in jobs))) throw new Error(`unresolved needs in ${id}`);
      if (required.some((n) => !(n in results))) continue;
      progress = true;
      pending.delete(id);
      const needs = Object.fromEntries(required.map((n) => [n, results[n]]));
      /** @type {import("./contracts.js").Context} */
      const context = {
        github: {
          event_name: scenario.event_name,
          event: scenario,
          repository: scenario.repository?.full_name,
          head_ref: scenario.pull_request?.head?.ref ?? "",
          ref: scenario.ref,
          workflow: workflow.document.name,
        },
        needs,
        inputs: scenario.inputs ?? {},
        vars: scenario.vars ?? {},
      };
      let condition = job.if ?? "success()";
      if (
        typeof condition === "string" &&
        !/\b(always|success|failure|cancelled)\(/.test(condition)
      )
        condition = `success() && (${condition.replace(/^\s*\$\{\{/, "").replace(/\}\}\s*$/, "")})`;
      const enabled = Boolean(evaluate(condition, context));
      results[id] = overrides[id] ?? {
        result: enabled ? "success" : "skipped",
        outputs:
          id === "policy"
            ? Object.fromEntries(Object.entries(policy).map(([k, v]) => [k, String(v)]))
            : enabled && job.steps?.some((step) => step.run?.startsWith("bash scripts/ci-guard.sh"))
              ? guardOutputs(job, scenario.provisioned !== false)
              : {},
      };
      if (!enabled) {
        rows.push({ job: id, name: job.name ?? id, skipped: true, runner: null });
        continue;
      }
      if (job.uses) {
        if (!job.uses.startsWith("./.github/workflows/") || job.uses.includes("${{"))
          throw new Error(`unresolved reusable workflow: ${job.uses}`);
        const file = resolve(dirname(workflow.file), "../..", job.uses);
        const called = { file, document: parse(readFileSync(file, "utf8")) };
        const nested = resolveRun(
          called,
          {
            ...scenario,
            inputs: Object.fromEntries(
              Object.entries(job.with ?? {}).map(([k, v]) => [k, render(v, context)]),
            ),
          },
          policy,
          {},
          [...stack, workflow.file],
        );
        rows.push(...nested.rows.map((row) => ({ ...row, job: `${id}/${row.job}` })));
        continue;
      }
      for (const matrix of expand(job.strategy?.matrix, context)) {
        const lane = { ...context, matrix };
        const runner = render(job["runs-on"], lane);
        const labels = (Array.isArray(runner) ? runner : [runner]).map((label) => {
          if (typeof label !== "string") throw new Error(`unresolved runner: ${id}`);
          return label;
        });
        if (labels.some((label) => typeof label !== "string" || !label || label.includes("${{")))
          throw new Error(`unresolved runner: ${id}`);
        if (!labels.every((label) => /^(ubuntu|windows|macos)-/.test(label)))
          throw new Error(`unsupported runner labels: ${labels}`);
        rows.push({
          job: id,
          name: render(job.name ?? id, lane),
          runner,
          matrix,
          targets:
            typeof matrix.target === "string"
              ? [matrix.target]
              : (job.steps ?? []).flatMap((step) =>
                  [
                    ...String(step.run ?? "").matchAll(
                      /scripts\/release\/native\/native-products\.sh (\S+)/g,
                    ),
                  ].map((match) => match[1]),
                ),
          macos: labels.some((label) => label.startsWith("macos-")),
          skipped: false,
        });
      }
    }
    if (!progress) throw new Error("unresolved/cyclic workflow prerequisites");
  }
  return { workflow: relative(root, workflow.file), event: scenario.event_name, rows, results };
}
export function digest() {
  const files = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard"], {
    cwd: root,
    encoding: "utf8",
  })
    .trim()
    .split("\n")
    .filter(Boolean)
    .sort();
  const hash = createHash("sha256");
  for (const file of files) hash.update(file).update(readFileSync(resolve(root, file)));
  return hash.digest("hex");
}
