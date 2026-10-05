// Which binary a call runs, and how a call that gets no report rejects.
//
// Every candidate is a real executable that logs its name and then runs the built binary,
// so each test reads which one ran. The package candidate is a real install layout: this
// SDK's source under node_modules/@onebudgetspec/sdk beside the repository's own
// @onebudgetspec/cli launcher and a carrier for this host whose binary is a recording one,
// so the SDK resolves the package exactly as an installed copy does.
import { afterAll, afterEach, beforeEach, describe, expect, test } from "bun:test";
import { cpSync, mkdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import * as inTree from "../src/index.ts";
import { BINARY_ENV } from "../src/index.ts";
import {
  builtBinary,
  cleanScratch,
  ROOT,
  ran,
  recording,
  rejection,
  runCli,
  scratch,
} from "./helpers.ts";

type Sdk = typeof inTree;

afterAll(cleanScratch);
beforeEach(() => {
  delete process.env[BINARY_ENV];
});
afterEach(() => {
  delete process.env[BINARY_ENV];
});

/** The SDK's entry module at `path`, which is a copy of this one. */
async function importSdk(path: string): Promise<Sdk> {
  const imported: Sdk = await import(path);
  return imported;
}

/** A project with this SDK, the CLI launcher and a recording carrier installed, and every
 * other candidate beside it; returns the SDK as imported from that project. */
async function installed(): Promise<{
  sdk: Sdk;
  log: string;
  explicit: string;
  variable: string;
  dir: string;
}> {
  const dir = scratch();
  const log = join(dir, "ran.log");
  const body = `exec "${builtBinary()}" "$@"`;
  const modules = join(dir, "project", "node_modules");
  const sdk = join(modules, "@onebudgetspec", "sdk");
  mkdirSync(sdk, { recursive: true });
  cpSync(join(ROOT, "sdks", "typescript", "src"), join(sdk, "src"), { recursive: true });
  cpSync(join(ROOT, "sdks", "typescript", "package.json"), join(sdk, "package.json"));
  // The SDK's one runtime import outside itself, which its build bundles, linked as a
  // package manager links it.
  symlinkSync(
    realpathSync(join(ROOT, "sdks", "typescript", "node_modules", "ajv")),
    join(modules, "ajv"),
  );
  const cli = join(modules, "@onebudgetspec", "cli");
  for (const part of ["package.json", "bin", "lib"]) {
    cpSync(join(ROOT, "npm", "cli", part), join(cli, part), { recursive: true });
  }
  const carrier = join(modules, "@onebudgetspec", `cli-${process.platform}-${process.arch}`);
  mkdirSync(carrier, { recursive: true });
  writeFileSync(
    join(carrier, "package.json"),
    JSON.stringify({ name: `@onebudgetspec/cli-${process.platform}-${process.arch}` }),
  );
  recording(join(carrier, "bin"), "package", log, body);
  return {
    sdk: await importSdk(join(sdk, "src", "index.ts")),
    log,
    explicit: recording(join(dir, "explicit"), "explicit", log, body),
    variable: recording(join(dir, "variable"), "variable", log, body),
    dir,
  };
}

describe("the binary a call runs", () => {
  test("an explicit binary wins over the variable and the package", async () => {
    const { sdk, log, explicit, variable, dir } = await installed();
    process.env[BINARY_ENV] = variable;
    const bundle = await sdk.schema({ binary: explicit });
    expect(ran(log)).toEqual(["explicit"]);
    expect(bundle).toEqual(JSON.parse(runCli(["schema"], dir).stdout));
  });

  test("the variable wins over the package", async () => {
    const { sdk, log, variable } = await installed();
    process.env[BINARY_ENV] = variable;
    expect(sdk.resolveBinary()).toEqual([variable]);
    await sdk.schema();
    expect(ran(log)).toEqual(["variable"]);
  });

  test("with neither, the resolved @onebudgetspec/cli package runs", async () => {
    const { sdk, log, dir } = await installed();
    const [runtime, launcher] = sdk.resolveBinary();
    expect(runtime).toBe(process.execPath);
    expect(launcher).toBe(
      join(dir, "project", "node_modules", "@onebudgetspec", "cli", "bin", "onebudgetspec.js"),
    );
    const work = join(dir, "work");
    mkdirSync(work);
    writeFileSync(
      join(work, "budgets.yaml"),
      "schema_version: 1\nbudgets:\n  - id: quick\n    measure: elapsed\n" +
        '    command: ["/bin/sh", "-c", "exit 0"]\n    unit: seconds\n' +
        "    direction: max\n    threshold: 60\n",
    );
    const report = await sdk.check({ cwd: work });
    expect(report.results.map((result) => result.verdict)).toEqual(["within"]);
    expect(ran(log)).toEqual(["package"]);
  });

  test("an empty variable is not a binary", async () => {
    const { sdk, log } = await installed();
    process.env[BINARY_ENV] = "";
    await sdk.schema();
    expect(ran(log)).toEqual(["package"]);
  });

  test("a launcher whose carrier is missing rejects with the launcher's reason", async () => {
    const { sdk, dir } = await installed();
    rmSync(
      join(
        dir,
        "project",
        "node_modules",
        "@onebudgetspec",
        `cli-${process.platform}-${process.arch}`,
      ),
      {
        recursive: true,
      },
    );
    const refused = await rejection(sdk.schema(), sdk.OnebudgetspecError);
    expect(refused.exitCode).toBe(69);
    expect(refused.message).toContain("is not installed");
  });

  test.each([
    [JSON.stringify({ name: "@onebudgetspec/cli" }), "names no onebudgetspec launcher"],
    [JSON.stringify({ bin: { onebudgetspec: 3 } }), "names no onebudgetspec launcher"],
    [JSON.stringify({ bin: "bin/onebudgetspec.js" }), "names no onebudgetspec launcher"],
    ["not json", "cannot read"],
  ])("a package manifest %p rejects", async (manifest, reason) => {
    const { sdk, dir } = await installed();
    writeFileSync(
      join(dir, "project", "node_modules", "@onebudgetspec", "cli", "package.json"),
      manifest,
    );
    const refused = await rejection(sdk.schema(), sdk.OnebudgetspecError);
    expect(refused.exitCode).toBeNull();
    expect(refused.message).toContain(reason);
    expect(refused.message).toContain("reinstall @onebudgetspec/cli");
  });

  test("no package and no variable rejects naming the ways to provide a binary", async () => {
    const { sdk, dir } = await installed();
    rmSync(join(dir, "project", "node_modules", "@onebudgetspec", "cli"), { recursive: true });
    const refused = await rejection(sdk.schema(), sdk.OnebudgetspecError);
    expect(refused.exitCode).toBeNull();
    for (const way of ["@onebudgetspec/cli", BINARY_ENV, "binary"]) {
      expect(refused.message).toContain(way);
    }
  });

  test("in this workspace the package resolves to the repository's own launcher", () => {
    const [runtime, launcher] = inTree.resolveBinary();
    expect(runtime).toBe(process.execPath);
    expect(launcher).toBe(join(ROOT, "npm", "cli", "bin", "onebudgetspec.js"));
  });
});
