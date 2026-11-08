// Build the SDK into dist/ without dist/ ever being absent or holding a partial file.
//
// `bun run build` (nx `sdk-typescript:build`, and scripts/build-dist.sh's `sdk-typescript`
// arm, which the packaging journeys drive) bundles src/index.ts and emits its declarations
// into a fresh staging directory, then renames each file over its dist/ counterpart and
// removes what the new build no longer has. A rename replaces a file in one step, so a task
// importing dist/index.js while another rebuilds it, as `github-workflows:budgets` does
// beside the packaging journeys, reads either the old build or the new one.
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readdirSync, renameSync, rmSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const DIST = join(ROOT, "dist");

function run(command: string[]): void {
  const result = spawnSync(command[0] ?? "", command.slice(1), { cwd: ROOT, stdio: "inherit" });
  if (result.status !== 0) {
    console.error(
      `build: \`${command.join(" ")}\` exited ${result.status ?? result.signal}; fix the error above and re-run \`bun run build\``,
    );
    process.exit(1);
  }
}

/** Every file under `dir`, as paths relative to it. */
function files(dir: string): string[] {
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => relative(dir, join(entry.parentPath, entry.name)));
}

// node_modules/ is ignored, never packed and on dist/'s filesystem, so a rename cannot cross
// devices; a directory per build keeps two concurrent builds apart.
const cache = join(ROOT, "node_modules", ".cache");
mkdirSync(cache, { recursive: true });
const stage = mkdtempSync(join(cache, "sdk-build-"));
try {
  run([process.execPath, "build", "src/index.ts", "--outdir", stage, "--target", "node"]);
  const tsc = join(dirname(require.resolve("typescript/package.json")), "bin", "tsc");
  run([process.execPath, tsc, "-p", "tsconfig.build.json", "--outDir", stage]);
  const built = files(stage);
  for (const file of built) {
    mkdirSync(dirname(join(DIST, file)), { recursive: true });
    renameSync(join(stage, file), join(DIST, file));
  }
  const kept = new Set(built);
  for (const file of files(DIST)) {
    if (!kept.has(file)) {
      rmSync(join(DIST, file), { force: true });
    }
  }
} finally {
  rmSync(stage, { recursive: true, force: true });
}
