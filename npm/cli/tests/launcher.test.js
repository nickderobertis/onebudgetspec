// The launcher's decisions, over real files in a temporary directory and real processes.
const { expect, test } = require("bun:test");
const { spawnSync } = require("node:child_process");
const { chmodSync, mkdirSync, mkdtempSync, symlinkSync, writeFileSync } = require("node:fs");
const { tmpdir } = require("node:os");
const { join } = require("node:path");
const { CARRIERS, finish, locate } = require("../lib/launcher.js");

/**
 * A carrier package on disk whose binary is the shell script `body`.
 * @param {string} body
 */
function carrier(body) {
  const root = mkdtempSync(join(tmpdir(), "carrier-"));
  mkdirSync(join(root, "bin"));
  writeFileSync(join(root, "package.json"), "{}");
  writeFileSync(join(root, "bin", "onebudgetspec"), `#!/bin/sh\n${body}\n`);
  chmodSync(join(root, "bin", "onebudgetspec"), 0o755);
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

test("every published platform is a carrier, and nothing else is", () => {
  expect(CARRIERS).toEqual(["linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64"]);
  for (const [platform, arch] of [
    ["win32", "x64"],
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

test("the carrier for the platform is resolved to its binary", () => {
  const root = carrier("exit 0");
  /** @type {string[]} */
  const requested = [];
  const found = located(
    locate("linux", "x64", (request) => {
      requested.push(request);
      return join(root, "package.json");
    }),
  );
  expect(requested).toEqual(["@onebudgetspec/cli-linux-x64/package.json"]);
  expect(found.carrier).toBe("@onebudgetspec/cli-linux-x64");
  expect(found.binary.endsWith(join("bin", "onebudgetspec"))).toBe(true);
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
  const root = carrier("exit 0");
  const elsewhere = carrier("exit 0");
  const linked = mkdtempSync(join(tmpdir(), "carrier-"));
  mkdirSync(join(linked, "bin"));
  writeFileSync(join(linked, "package.json"), "{}");
  symlinkSync(join(elsewhere, "bin", "onebudgetspec"), join(linked, "bin", "onebudgetspec"));
  expect(located(locate("linux", "x64", () => join(root, "package.json"))).binary).toBeDefined();
  const escaped = refused(locate("linux", "x64", () => join(linked, "package.json")));
  expect(escaped.status).toBe(69);
  expect(escaped.message).toContain("outside the package");
});

test("the binary's exit status, failure to start and signal become the launcher's", () => {
  const exits = carrier("exit 3");
  const binary = join(exits, "bin", "onebudgetspec");
  expect(finish(spawnSync(binary, []), binary, "c")).toEqual({ status: 3 });
  const killed = join(carrier("kill -9 $$"), "bin", "onebudgetspec");
  const signalled = finish(spawnSync(killed, []), killed, "c");
  expect(signalled.status).toBe(70);
  expect(signalled.message).toContain("terminated by SIGKILL");
  const absent = join(exits, "bin", "not-there");
  const failed = finish(spawnSync(absent, []), absent, "@onebudgetspec/cli-linux-x64");
  expect(failed.status).toBe(69);
  expect(failed.message).toContain("reinstall @onebudgetspec/cli-linux-x64");
});
