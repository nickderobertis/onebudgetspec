// What the SDK's tests share: the built binary, the conformance cases, and stand-in
// binaries, such as recording ones that log their name before running the real one.
import assert from "node:assert";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  linkSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

export const ROOT = resolve(import.meta.dir, "..", "..", "..");
export const CASES = join(ROOT, "conformance", "cases");
/** Whether this host is Windows, which runs a program only by its extension. */
export const WINDOWS = process.platform === "win32";
/** The suffix this host's executables carry: cargo builds `onebudgetspec.exe` on Windows. */
export const EXE = WINDOWS ? ".exe" : "";

/** The `onebudgetspec` cargo built, which Nx's sdk-typescript:test builds first. */
export function builtBinary(): string {
  const binary = join(ROOT, "target", "debug", `onebudgetspec${EXE}`);
  assert(existsSync(binary), `${binary} is missing; build it with 'just test', which builds it`);
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
  compiled = undefined;
  for (const dir of scratches.splice(0)) rmSync(dir, { recursive: true, force: true });
}

/** The compiled stand-in, built once per test file into a scratch directory. */
let compiled: string | undefined;

/**
 * A real executable that runs the script beside it: the one named as it is, without `.exe`,
 * plus `.stand-in.cjs`. A script cannot stand in for the binary itself: Windows runs no file
 * without an executable's extension, and Node spawns no `.cmd` without a shell, which the SDK
 * never asks for. So the stand-in is this runtime compiled into an executable, which each
 * {@link standIn} links under its own name.
 */
function standInExecutable(): string {
  if (compiled === undefined) {
    const dir = scratch();
    const source = join(dir, "stand-in.cjs");
    writeFileSync(source, 'require(process.execPath.replace(/\\.exe$/i, "") + ".stand-in.cjs");\n');
    const output = join(dir, `stand-in${EXE}`);
    const built = spawnSync(process.execPath, ["build", "--compile", source, "--outfile", output], {
      encoding: "utf8",
    });
    assert(
      built.status === 0 && existsSync(output),
      `cannot compile the stand-in: ${built.stderr}`,
    );
    compiled = output;
  }
  return compiled;
}

/** An executable `onebudgetspec` in `directory` that runs the CommonJS `source`, with the
 * executable's arguments from `process.argv[2]` on, and exits as it does. */
export function standIn(directory: string, source: string): string {
  mkdirSync(directory, { recursive: true });
  const program = join(directory, `onebudgetspec${EXE}`);
  const executable = standInExecutable();
  try {
    linkSync(executable, program);
  } catch {
    // Another volume than the scratch directory's holds no link to it.
    copyFileSync(executable, program);
  }
  writeFileSync(join(directory, "onebudgetspec.stand-in.cjs"), source);
  return program;
}

/** Stand-in source that runs `binary` with the stand-in's arguments and exits as it did. */
export function delegating(binary: string): string {
  return (
    `const ran = require("node:child_process").spawnSync(${JSON.stringify(binary)}, ` +
    'process.argv.slice(2), { stdio: "inherit" });\nprocess.exit(ran.status ?? 1);\n'
  );
}

/** An executable `onebudgetspec` in `directory` that logs `name` to `log`, then runs `source`. */
export function recording(directory: string, name: string, log: string, source: string): string {
  const logged = `require("node:fs").appendFileSync(${JSON.stringify(log)}, ${JSON.stringify(`${name}\n`)});\n`;
  return standIn(directory, logged + source);
}

/** An executable `onebudgetspec` in `directory` that prints `stdout` verbatim and exits 0. */
export function printing(directory: string, stdout: string): string {
  return standIn(directory, `process.stdout.write(${JSON.stringify(stdout)});\n`);
}

/**
 * Whether this host can make a symlink to a file. Linux and macOS always can; Windows lets
 * only an elevated process, or one with Developer Mode on, make one. A test that needs one
 * is skipped where it cannot; what it checks of a link is the same on every platform, and
 * every Linux and macOS run checks it.
 */
export const SYMLINKS = (() => {
  const dir = mkdtempSync(join(tmpdir(), "onebudgetspec-symlink-"));
  try {
    writeFileSync(join(dir, "target"), "");
    symlinkSync(join(dir, "target"), join(dir, "link"));
    return true;
  } catch {
    return false;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
})();

/** The names the recording binaries logged, in order. */
export function ran(log: string): string[] {
  return existsSync(log) ? readFileSync(log, "utf8").split(/\s+/).filter(Boolean) : [];
}

/**
 * What `call` rejects with, which must be an instance of `kind`: the SDK's error class,
 * passed in because an installed copy of the SDK is its own module with its own class.
 */
export async function rejection<E extends Error>(
  call: Promise<unknown>,
  kind: abstract new (...args: never[]) => E,
): Promise<E> {
  let outcome: unknown = "a resolved call";
  try {
    await call;
  } catch (error) {
    outcome = error;
  }
  assert(outcome instanceof kind, `expected a ${kind.name} rejection, got ${String(outcome)}`);
  return outcome;
}
