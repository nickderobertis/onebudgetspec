// scripts/generate.ts, run as `just generate` and `just lint` run it, over a copy of the
// package: the generator, the committed generated files and the formatter configuration
// laid out as in the workspace, with the installed tools linked in, so nothing in the tree
// is written.
import { afterAll, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  cpSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { builtBinary, cleanScratch, ROOT, scratch } from "./helpers.ts";

afterAll(cleanScratch);

const PACKAGE = join(ROOT, "sdks", "typescript");

/** A copy of the workspace holding just what the generator reads; returns its package. */
function workspace(): string {
  const root = join(scratch(), "workspace");
  const copy = join(root, "sdks", "typescript");
  mkdirSync(join(copy, "src"), { recursive: true });
  cpSync(join(PACKAGE, "scripts"), join(copy, "scripts"), { recursive: true });
  cpSync(join(PACKAGE, "src", "generated"), join(copy, "src", "generated"), { recursive: true });
  for (const file of ["biome.json", ".gitignore"]) cpSync(join(ROOT, file), join(root, file));
  symlinkSync(join(ROOT, "node_modules"), join(root, "node_modules"));
  symlinkSync(join(PACKAGE, "node_modules"), join(copy, "node_modules"));
  return copy;
}

function generate(copy: string, binary: string, ...args: string[]) {
  const ran = spawnSync(process.execPath, [join(copy, "scripts", "generate.ts"), ...args], {
    cwd: copy,
    encoding: "utf8",
    env: { ...process.env, ONEBUDGETSPEC_BIN: binary },
  });
  return { status: ran.status, stderr: ran.stderr };
}

function contents(copy: string): Record<string, string> {
  const directory = join(copy, "src", "generated");
  return Object.fromEntries(
    readdirSync(directory).map((name) => [name, readFileSync(join(directory, name), "utf8")]),
  );
}

test("--check passes on the committed files and writes nothing", () => {
  const copy = workspace();
  const checked = generate(copy, builtBinary(), "--check");
  expect(checked).toEqual({ status: 0, stderr: "" });
  expect(contents(copy)).toEqual(contents(PACKAGE));
});

test("--check names stale, missing and extra files; generating repairs them", () => {
  const copy = workspace();
  const committed = contents(copy);
  const generated = join(copy, "src", "generated");
  writeFileSync(join(generated, "check-report.ts"), "// edited by hand\n");
  rmSync(join(generated, "list-report.ts"));
  writeFileSync(join(generated, "stray.ts"), "");
  const drifted = contents(copy);

  const checked = generate(copy, builtBinary(), "--check");
  expect(checked.status).toBe(1);
  for (const name of ["check-report.ts", "list-report.ts", "stray.ts"]) {
    expect(checked.stderr).toContain(`src/generated/${name} differs`);
  }
  expect(checked.stderr).toContain("run 'just generate'");
  expect(contents(copy)).toEqual(drifted);

  expect(generate(copy, builtBinary()).status).toBe(0);
  expect(contents(copy)).toEqual(committed);
  expect(generate(copy, builtBinary(), "--check").status).toBe(0);
});

function fakeBinary(printed: string): string {
  const program = join(scratch(), "onebudgetspec");
  writeFileSync(program, `#!/bin/sh\nprintf '%s' '${printed}'\n`);
  chmodSync(program, 0o755);
  return program;
}

test.each([
  ["not json", "the binary's schema is not JSON"],
  ['{"version": 1, "roots": {"check-report": {}}}', 'has no "list-report" root'],
  ['{"version": "1", "roots": {}}', "lacks an integer `version`"],
])("a bundle printed as %p is refused with a next step", (printed, reason) => {
  const copy = workspace();
  const before = contents(copy);
  const refused = generate(copy, fakeBinary(printed));
  expect(refused.status).toBe(1);
  expect(refused.stderr).toContain(reason);
  expect(refused.stderr).toContain("run 'just generate'");
  expect(contents(copy)).toEqual(before);
});

test("a missing binary is refused with how to build it", () => {
  const copy = workspace();
  const refused = generate(copy, join(copy, "absent"), "--check");
  expect(refused.status).toBe(1);
  expect(refused.stderr).toContain("absent is missing");
  expect(refused.stderr).toContain("run 'just generate', which builds the binary first");
});

test("a binary that fails is refused with its own message", () => {
  const copy = workspace();
  const program = join(scratch(), "onebudgetspec");
  writeFileSync(program, "#!/bin/sh\necho 'schema: broken' >&2\nexit 3\n");
  chmodSync(program, 0o755);
  const refused = generate(copy, program);
  expect(refused.status).toBe(1);
  expect(refused.stderr).toContain("schema: broken");
});

test("an unknown argument is refused", () => {
  const refused = generate(workspace(), builtBinary(), "--force");
  expect(refused.status).toBe(1);
  expect(refused.stderr).toContain("unknown argument");
});

test("a generated file it cannot write is refused with a next step", () => {
  const copy = workspace();
  const stale = join(copy, "src", "generated", "check-report.ts");
  writeFileSync(stale, "// stale\n");
  chmodSync(stale, 0o444);
  const refused = generate(copy, builtBinary());
  expect(refused.status).toBe(1);
  expect(refused.stderr).toContain("make it writable");
});

test("a binary that cannot execute is refused with a next step", () => {
  const program = join(scratch(), "onebudgetspec");
  writeFileSync(program, "not a program\n");
  chmodSync(program, 0o644);
  const refused = generate(workspace(), program);
  expect(refused.status).toBe(1);
  expect(refused.stderr).toContain(`cannot run ${program}`);
  expect(refused.stderr).toContain("run 'just generate'");
});

test("a root the compiler cannot turn into types is refused, writing nothing", () => {
  const copy = workspace();
  const before = contents(copy);
  const unresolvable = '{"$ref": "#/$defs/Missing"}';
  const refused = generate(
    copy,
    fakeBinary(`{"version": 1, "roots": {"check-report": ${unresolvable}, "list-report": {}}}`),
  );
  expect(refused.status).toBe(1);
  expect(refused.stderr).toContain('cannot compile the "check-report" root');
  expect(refused.stderr).toContain("run 'just bootstrap'");
  expect(contents(copy)).toEqual(before);
});
