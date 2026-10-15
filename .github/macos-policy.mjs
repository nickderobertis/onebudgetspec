// The workflow and subprocess journeys share this conservative classification boundary.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { parse } from "yaml";
import { isReleaseVersion } from "./version-format.mjs";
import { isRecord } from "./workflow-input.mjs";

const knownNonPrEvents = new Set([
  "push",
  "release",
  "workflow_run",
  "repository_dispatch",
  "schedule",
  "workflow_dispatch",
]);
const normalizer = fileURLToPath(new URL("./semantic-version.py", import.meta.url));
/** @param {string[]} args */
const git = (...args) =>
  execFileSync("git", args, { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] });

/** @param {string} path @param {string} text @returns {unknown} */
function normalized(path, text) {
  if (
    path.endsWith("Cargo.toml") ||
    path.endsWith("pyproject.toml") ||
    ["Cargo.lock", "uv.lock"].includes(path)
  ) {
    return JSON.parse(
      execFileSync(process.platform === "win32" ? "python" : "python3", [normalizer], {
        input: JSON.stringify({ path, text }),
        encoding: "utf8",
        stdio: ["pipe", "pipe", "pipe"],
      }),
    );
  }
  const metadata = new Map([
    ["sdks/python/src/onebudgetspec_sdk/__init__.py", /^(__version__ = ")[^"\n]+(")$/m],
    ["sdks/typescript/src/index.ts", /^(export const VERSION = ")[^"\n]+(";)$/m],
  ]);
  const pattern = metadata.get(path);
  if (pattern) {
    const captured = pattern.exec(text);
    if (!captured) throw new Error("missing generated version");
    const version = captured[0].slice(captured[1].length, -captured[2].length);
    if (!isReleaseVersion(version)) throw new Error("invalid generated version");
    return text.replace(pattern, "$1<local-version>$2");
  }
  if (path === "bun.lock" || path.endsWith("package.json")) {
    const doc = path === "bun.lock" ? parse(text) : JSON.parse(text);
    if (!isRecord(doc)) throw new Error("package/lock document must be an object");
    /** @type {unknown[]} */
    let records = [doc];
    if (path === "bun.lock") {
      const workspaces = doc.workspaces;
      if (!isRecord(workspaces)) throw new Error("bun workspaces must be an object");
      records = Object.values(workspaces);
    }
    for (const record of records) {
      if (!isRecord(record) || typeof record.name !== "string")
        throw new Error("workspace/package record needs a string name");
      if ("version" in record) {
        if (typeof record.version !== "string") throw new Error("version must be a string");
        if (!isReleaseVersion(record.version)) throw new Error("invalid version");
        record.version = "<local-version>";
      }
    }
    return doc;
  }
  throw new Error(`unrecognized version surface: ${path}`);
}

/** @param {import("./contracts.js").Event} event @param {string} base @param {string} head */
export function classify(event, base, head) {
  const eventName = event?.event_name;
  if (knownNonPrEvents.has(eventName))
    return {
      isReleaseOnly: false,
      macosRequired: false,
      reason: `known non-PR event: ${eventName}`,
    };
  if (eventName !== "pull_request")
    return {
      isReleaseOnly: false,
      macosRequired: true,
      reason: `unknown CI event: ${String(eventName)}`,
    };
  /** @param {string} reason */
  const ordinary = (reason) => ({ isReleaseOnly: false, macosRequired: true, reason });
  const pr = event.pull_request;
  if (
    !isRecord(pr) ||
    !isRecord(pr.head) ||
    typeof pr.head.ref !== "string" ||
    !isRecord(pr.head.repo) ||
    typeof pr.head.repo.full_name !== "string" ||
    !isRecord(event.repository) ||
    typeof event.repository.full_name !== "string" ||
    !pr.head.ref.startsWith("release-plz-") ||
    !pr.head.repo.full_name ||
    pr.head.repo.full_name !== event.repository.full_name
  ) {
    return ordinary("ordinary or fork-headed PR; release branch identity not established");
  }
  try {
    base = git("rev-parse", "--verify", "--end-of-options", `${base}^{commit}`).trim();
    head = git("rev-parse", "--verify", "--end-of-options", `${head}^{commit}`).trim();
    const paths = git("diff", "--name-status", "--no-renames", base, head)
      .trim()
      .split("\n")
      .filter(Boolean);
    if (!paths.length) return ordinary("empty diff is not a verified release");
    for (const line of paths) {
      const [status, path] = line.split("\t");
      if (status !== "M") return ordinary(`release diff adds/deletes a file: ${path}`);
      if (/^(?:crates\/[^/]+\/)?CHANGELOG\.md$/.test(path)) continue;
      const allowed =
        /^(?:Cargo\.(?:toml|lock)|uv\.lock|bun\.lock|pyproject\.toml|crates\/[^/]+\/Cargo\.toml|sdks\/python\/pyproject\.toml|(?:sdks\/typescript|npm\/cli|npm\/platforms\/[^/]+)\/package\.json|sdks\/python\/src\/onebudgetspec_sdk\/__init__\.py|sdks\/typescript\/src\/index\.ts)$/;
      if (!allowed.test(path)) return ordinary(`non-release file: ${path}`);
      const before = normalized(path, git("show", `${base}:${path}`));
      const after = normalized(path, git("show", `${head}:${path}`));
      if (!isDeepStrictEqual(before, after))
        return ordinary(`non-version content changed: ${path}`);
    }
    return {
      isReleaseOnly: true,
      macosRequired: false,
      reason: "same-repository release-plz branch; parsed version/changelog-only diff",
    };
  } catch (error) {
    return ordinary(`unreadable release classification: ${String(error)}`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (
    args.length !== 7 ||
    args[0] !== "classify" ||
    args[1] !== "--event" ||
    args[3] !== "--base" ||
    args[5] !== "--head"
  )
    throw new Error("usage: classify --event JSON --base REF --head REF");
  let result;
  try {
    result = classify(JSON.parse(readFileSync(args[2], "utf8")), args[4], args[6]);
  } catch (error) {
    result = {
      isReleaseOnly: false,
      macosRequired: true,
      reason: `unreadable PR event: ${String(error)}`,
    };
  }
  console.log(JSON.stringify(result));
}
