import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { VERSION } from "../src/index.ts";

test("VERSION is the version the package manifest releases", () => {
  const manifest = JSON.parse(readFileSync(join(import.meta.dir, "..", "package.json"), "utf8"));
  expect(VERSION).toBe(manifest.version);
  expect(VERSION).toMatch(/^\d+\.\d+\.\d+/);
});
