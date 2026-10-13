import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

/** @type {RegExp | undefined} */
let versionFormat;
/** @param {string} value */
export function isReleaseVersion(value) {
  if (!versionFormat) {
    const pattern = execFileSync(
      process.platform === "win32" ? "python" : "python3",
      [resolve(import.meta.dirname, "version_contract.py")],
      { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] },
    ).trim();
    versionFormat = new RegExp(`^(?:${pattern})$`);
  }
  return versionFormat.exec(value)?.[0] === value;
}
