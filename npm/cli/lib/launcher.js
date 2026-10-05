// How the launcher finds its platform carrier's binary and turns the binary's ending into
// the launcher's own. bin/onebudgetspec.js calls these with the real platform, module
// resolution and process; the tests call them with real files and processes too.
const { realpathSync } = require("node:fs");
const { dirname, join, sep } = require("node:path");

/** The platforms a carrier is published for, as `${process.platform}-${process.arch}`. */
const CARRIERS = ["linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64"];

/**
 * The binary of the carrier for this platform, or the status and reason to exit with.
 * @param {string} platform `process.platform`
 * @param {string} arch `process.arch`
 * @param {(request: string) => string} resolve `require.resolve`, from the launcher
 * @returns {{binary: string, carrier: string} | {status: number, message: string}}
 */
function locate(platform, arch, resolve) {
  const key = `${platform}-${arch}`;
  if (!CARRIERS.includes(key)) {
    return {
      status: 64,
      message: `onebudgetspec: no build for ${key}; install it with 'cargo install onebudgetspec' instead`,
    };
  }
  const carrier = `@onebudgetspec/cli-${key}`;
  try {
    const root = realpathSync(dirname(resolve(`${carrier}/package.json`)));
    const binary = realpathSync(join(root, "bin", "onebudgetspec"));
    if (!binary.startsWith(root + sep)) throw new Error("its binary is outside the package");
    return { binary, carrier };
  } catch (error) {
    const reason = /** @type {NodeJS.ErrnoException} */ (error);
    return {
      status: 69,
      message: `onebudgetspec: ${carrier} is not installed (${reason.code || reason.message}); reinstall @onebudgetspec/cli`,
    };
  }
}

/**
 * The status to exit with once the binary has run, and what to say first, if anything.
 * @param {{error?: Error, status: number | null, signal: string | null}} result `spawnSync`'s
 * @param {string} binary the binary that ran
 * @param {string} carrier the package it came from
 * @returns {{status: number, message?: string}}
 */
function finish(result, binary, carrier) {
  if (result.error) {
    return {
      status: 69,
      message: `onebudgetspec: cannot run ${binary}: ${result.error.message}; reinstall ${carrier}`,
    };
  }
  if (result.status === null) {
    return { status: 70, message: `onebudgetspec: the binary was terminated by ${result.signal}` };
  }
  return { status: result.status };
}

module.exports = { CARRIERS, finish, locate };
