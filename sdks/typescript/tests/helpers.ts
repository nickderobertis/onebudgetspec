// What the SDK's tests share: the built binary, the conformance cases, and recording
// binaries that log their name before running the real one.
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

export const ROOT = resolve(import.meta.dir, "..", "..", "..");
export const CASES = join(ROOT, "conformance", "cases");

/** The `onebudgetspec` cargo built, which Nx's sdk-typescript:test builds first. */
export function builtBinary(): string {
  const binary = join(ROOT, "target", "debug", "onebudgetspec");
  if (!existsSync(binary)) {
    throw new Error(`${binary} is missing; build it with 'cargo build -p onebudgetspec'`);
  }
  return binary;
}

/** Run the binary directly, as a person would, to compare the SDK with. */
export function runCli(args: string[], cwd: string) {
  const ran = spawnSync(builtBinary(), args, { cwd, encoding: "utf8", stdio: "pipe" });
  return { status: ran.status, stdout: ran.stdout, stderr: ran.stderr };
}

const scratches: string[] = [];

/** A fresh temporary directory, removed by {@link cleanScratch}. */
export function scratch(): string {
  const dir = mkdtempSync(join(tmpdir(), "onebudgetspec-sdk-"));
  scratches.push(dir);
  return dir;
}

/** Remove every directory {@link scratch} made; each test file runs it after its tests. */
export function cleanScratch(): void {
  for (const dir of scratches.splice(0)) rmSync(dir, { recursive: true, force: true });
}

/** An executable `onebudgetspec` in `directory` that logs `name` to `log`, then runs `body`. */
export function recording(directory: string, name: string, log: string, body: string): string {
  mkdirSync(directory, { recursive: true });
  const program = join(directory, "onebudgetspec");
  writeFileSync(program, `#!/bin/sh\necho ${name} >> "${log}"\n${body}\n`);
  chmodSync(program, 0o755);
  return program;
}

/** The names the recording binaries logged, in order. */
export function ran(log: string): string[] {
  return existsSync(log) ? readFileSync(log, "utf8").split(/\s+/).filter(Boolean) : [];
}
