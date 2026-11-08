import { expect, test } from "bun:test";
import { spawn } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = join(import.meta.dir, "..");
const DIST = join(ROOT, "dist");
const ENTRY = join(DIST, "index.js");

// `github-workflows:budgets` imports dist/index.js while the packaging journeys rebuild the
// SDK through scripts/build-dist.sh; a rebuild that removes dist/ first fails that import.
test("a rebuild never leaves dist/index.js missing or partial, and drops stale files", async () => {
  const build = (): Promise<number | null> =>
    new Promise((done) => {
      spawn(process.execPath, ["run", "--silent", "build"], { cwd: ROOT, stdio: "ignore" }).on(
        "exit",
        done,
      );
    });
  expect(await build()).toBe(0);
  const size = statSync(ENTRY).size;
  const stale = join(DIST, "stale.js");
  writeFileSync(stale, "export {};\n");

  let finished = false;
  const rebuilt = build().then((status) => {
    finished = true;
    return status;
  });
  const missed: string[] = [];
  let reads = 0;
  while (!finished) {
    try {
      const read = readFileSync(ENTRY).length;
      if (read !== size) missed.push(`read ${read} bytes of ${size}`);
    } catch (error) {
      missed.push(String(error));
    }
    reads += 1;
    await new Promise((tick) => setTimeout(tick, 1));
  }

  expect(await rebuilt).toBe(0);
  expect(reads).toBeGreaterThan(10);
  expect(missed).toEqual([]);
  expect(existsSync(stale)).toBe(false);
  expect(readdirSync(DIST).sort()).toEqual([
    "client.d.ts",
    "generated",
    "index.d.ts",
    "index.js",
    "report.d.ts",
  ]);
}, 60_000);
