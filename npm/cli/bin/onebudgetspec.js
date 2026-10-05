#!/usr/bin/env node
// Resolve the platform carrier npm installed beside this launcher and exec its binary,
// passing the arguments, the streams and the exit status straight through.
const { spawnSync } = require("node:child_process");
const { realpathSync } = require("node:fs");
const { dirname, join } = require("node:path");

const platform = `${process.platform}-${process.arch}`;
const carriers = ["linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64"];
// llmlint: ignore[changed_behavior_has_e2e] reaching this branch needs a host outside the four platforms the release ships (Windows, say), which no runner here provides; the other refusals are driven by the packaging journeys.
if (!carriers.includes(platform)) {
  console.error(
    `onebudgetspec: no build for ${platform}; install it with 'cargo install onebudgetspec' instead`,
  );
  process.exit(64);
}
const carrier = `@onebudgetspec/cli-${platform}`;
let binary;
try {
  binary = realpathSync(
    join(dirname(require.resolve(`${carrier}/package.json`)), "bin", "onebudgetspec"),
  );
} catch (error) {
  console.error(
    `onebudgetspec: ${carrier} is not installed (${error.code || error.message}); reinstall @onebudgetspec/cli`,
  );
  process.exit(69);
}
const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(
    `onebudgetspec: cannot run ${binary}: ${result.error.message}; reinstall ${carrier}`,
  );
  process.exit(69);
}
if (result.status === null) {
  console.error(`onebudgetspec: the binary was terminated by ${result.signal}`);
  process.exit(70);
}
process.exit(result.status);
