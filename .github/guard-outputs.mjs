// Run the workflow's admitted guard producer; credentials alone are doubled here.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

const cache = new Map();
/** @param {import('./contracts.js').Job} job @param {boolean} provisioned @returns {Record<string,string>} */
export function guardOutputs(job, provisioned) {
  const key = JSON.stringify([job, provisioned]);
  if (cache.has(key)) return cache.get(key);
  /** @type {Record<string,string>} */
  const outputs = {};
  const dir = mkdtempSync(
    resolve(process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(), "macos-guard-"),
  );
  try {
    for (const [name, expression] of Object.entries(job.outputs ?? {})) {
      if (!/^[A-Za-z_][A-Za-z0-9_-]*$/.test(name))
        throw new Error(`unresolved guard output name: ${name}`);
      const match = /^\$\{\{\s*steps\.([\w-]+)\.outputs\.enabled\s*\}\}$/.exec(expression);
      if (!match) throw new Error(`unresolved guard output ${name}: ${expression}`);
      const step = job.steps?.find((candidate) => candidate.id === match[1]);
      if (step?.if !== undefined)
        throw new Error(
          "unresolved conditional guard producer; add its condition model before routing",
        );
      const producer = /^bash scripts\/ci-guard\.sh((?: --any)?(?: [A-Z_][A-Z0-9_]*)+)$/.exec(
        step?.run ?? "",
      );
      if (!producer) throw new Error(`unresolved guard producer for ${name}`);
      const args = producer[1].trim().split(" ");
      /** @type {NodeJS.ProcessEnv & {GITHUB_OUTPUT:string}} */
      const env = { ...process.env, GITHUB_OUTPUT: resolve(dir, name) };
      for (const variable of args.filter((arg) => arg !== "--any")) {
        const source = step?.env?.[variable];
        if (source !== undefined && !/^\$\{\{\s*secrets\.[A-Z_][A-Z0-9_]*\s*\}\}$/.test(source))
          throw new Error(`unresolved guard credential source: ${source}`);
        env[variable] = source !== undefined && provisioned ? "fixture-credential" : "";
      }
      const bash =
        process.platform === "win32"
          ? resolve(
              execFileSync("git", ["--exec-path"], { encoding: "utf8" }).trim(),
              "../../..",
              "bin/bash.exe",
            )
          : "bash";
      execFileSync(bash, ["scripts/ci-guard.sh", ...args], {
        cwd: resolve(import.meta.dirname, ".."),
        env,
        stdio: ["pipe", "pipe", "pipe"],
      });
      const result = /^enabled=(true|false)\r?\n$/.exec(readFileSync(env.GITHUB_OUTPUT, "utf8"));
      if (!result)
        throw new Error("guard produced no enabled boolean; inspect scripts/ci-guard.sh");
      outputs[name] = result[1];
    }
    cache.set(key, outputs);
    return outputs;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
