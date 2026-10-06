// The launcher's decisions, over real files in a temporary directory and real processes.
const { expect, test } = require("bun:test");
const { spawnSync } = require("node:child_process");
const { mkdirSync, mkdtempSync, symlinkSync, writeFileSync } = require("node:fs");
const { tmpdir } = require("node:os");
const { join } = require("node:path");
const { CARRIERS, finish, locate } = require("../lib/launcher.js");

/**
 * A carrier package on disk holding `bin/<name>`. `locate` reads only the files, never runs
 * the binary, so its content is a placeholder.
 * @param {string} name the binary's file name
 */
function carrier(name) {
  const root = mkdtempSync(join(tmpdir(), "carrier-"));
  mkdirSync(join(root, "bin"));
  writeFileSync(join(root, "package.json"), "{}");
  writeFileSync(join(root, "bin", name), "a carrier's binary\n");
  return root;
}

/**
 * The refusal `locate` answered, failing the test when it located a binary instead.
 * @param {ReturnType<typeof locate>} found
 */
function refused(found) {
  if (!("status" in found)) throw new Error(`expected a refusal, located ${found.binary}`);
  return found;
}

/**
 * The binary `locate` found, failing the test when it refused instead.
 * @param {ReturnType<typeof locate>} found
 */
function located(found) {
  if ("status" in found) throw new Error(`expected a binary, refused: ${found.message}`);
  return found;
}

/**
 * This runtime run with the script `source`: a real process on every platform.
 * @param {string} source
 */
function runScript(source) {
  return spawnSync(process.execPath, ["-e", source]);
}

test("every published platform is a carrier, and nothing else is", () => {
  expect(CARRIERS).toEqual([
    "linux-x64",
    "linux-arm64",
    "darwin-x64",
    "darwin-arm64",
    "win32-x64",
    "win32-arm64",
  ]);
  for (const [platform, arch] of [
    ["win32", "ia32"],
    ["linux", "ia32"],
    ["freebsd", "x64"],
  ]) {
    const found = refused(
      locate(platform, arch, () => {
        throw new Error("an unsupported platform resolves nothing");
      }),
    );
    expect(found.status).toBe(64);
    expect(found.message).toContain(`no build for ${platform}-${arch}`);
    expect(found.message).toContain("cargo install onebudgetspec");
  }
});

test.each([
  ["linux", "x64", "onebudgetspec"],
  ["darwin", "arm64", "onebudgetspec"],
  ["win32", "x64", "onebudgetspec.exe"],
  ["win32", "arm64", "onebudgetspec.exe"],
])("the carrier for %s-%s is resolved to its %s", (platform, arch, name) => {
  const root = carrier(name);
  /** @type {string[]} */
  const requested = [];
  const found = located(
    locate(platform, arch, (request) => {
      requested.push(request);
      return join(root, "package.json");
    }),
  );
  expect(requested).toEqual([`@onebudgetspec/cli-${platform}-${arch}/package.json`]);
  expect(found.carrier).toBe(`@onebudgetspec/cli-${platform}-${arch}`);
  expect(found.binary.endsWith(join("bin", name))).toBe(true);
});

test("a carrier holding only the other platforms' binary name is refused", () => {
  const windowsOnly = carrier("onebudgetspec.exe");
  const unixOnly = carrier("onebudgetspec");
  for (const [platform, root] of [
    ["linux", windowsOnly],
    ["win32", unixOnly],
  ]) {
    const without = refused(locate(platform, "x64", () => join(root, "package.json")));
    expect(without.status).toBe(69);
    expect(without.message).toContain(`@onebudgetspec/cli-${platform}-x64 is not installed`);
  }
});

test("a missing carrier, or one without its binary, is refused with 69", () => {
  const missing = refused(
    locate("darwin", "arm64", (request) => {
      /** @type {NodeJS.ErrnoException} */
      const error = new Error(`Cannot find module '${request}'`);
      error.code = "MODULE_NOT_FOUND";
      throw error;
    }),
  );
  expect(missing.status).toBe(69);
  expect(missing.message).toContain(
    "@onebudgetspec/cli-darwin-arm64 is not installed (MODULE_NOT_FOUND)",
  );
  const empty = mkdtempSync(join(tmpdir(), "carrier-"));
  writeFileSync(join(empty, "package.json"), "{}");
  const without = refused(locate("linux", "arm64", () => join(empty, "package.json")));
  expect(without.status).toBe(69);
  expect(without.message).toContain("reinstall @onebudgetspec/cli");
});

test("a binary that resolves outside its package is refused", () => {
  const root = carrier("onebudgetspec");
  const elsewhere = carrier("onebudgetspec");
  const linked = mkdtempSync(join(tmpdir(), "carrier-"));
  writeFileSync(join(linked, "package.json"), "{}");
  // The package's bin directory is a link to another package's; a junction on Windows,
  // which needs no privilege there, and a directory symlink elsewhere.
  symlinkSync(join(elsewhere, "bin"), join(linked, "bin"), "junction");
  expect(located(locate("linux", "x64", () => join(root, "package.json"))).binary).toBeDefined();
  const escaped = refused(locate("linux", "x64", () => join(linked, "package.json")));
  expect(escaped.status).toBe(69);
  expect(escaped.message).toContain("outside the package");
});

test("the binary's exit status and failure to start become the launcher's", () => {
  expect(finish(runScript("process.exit(3)"), "b", "c")).toEqual({ status: 3 });
  const absent = join(carrier("onebudgetspec"), "bin", "not-there");
  const failed = finish(spawnSync(absent, []), absent, "@onebudgetspec/cli-linux-x64");
  expect(failed.status).toBe(69);
  expect(failed.message).toContain("reinstall @onebudgetspec/cli-linux-x64");
});

// Windows has no signals: a process ended there by TerminateProcess has an exit status,
// which the status case above covers, so no Windows run can end the binary this way.
test.skipIf(process.platform === "win32")(
  "a binary ended by a signal ends the launcher with 70",
  () => {
    const signalled = finish(runScript("process.kill(process.pid, 'SIGKILL')"), "b", "c");
    expect(signalled.status).toBe(70);
    expect(signalled.message).toContain("terminated by SIGKILL");
  },
);
