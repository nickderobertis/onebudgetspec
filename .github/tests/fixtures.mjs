import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { root } from "../workflow-graph.mjs";

export function history() {
  const dir = mkdtempSync(
    resolve(process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(), "macos-history-"),
  );
  /** @param {string[]} args */
  const git = (...args) => execFileSync("git", args, { cwd: dir, encoding: "utf8" }).trim();
  git("init", "-q");
  git("config", "user.email", "journey@example.test");
  git("config", "user.name", "Journey");
  mkdirSync(resolve(dir, "npm/cli"), { recursive: true });
  writeFileSync(
    resolve(dir, "npm/cli/package.json"),
    '{"name":"@onebudgetspec/cli","version":"0.1.4","dependencies":{"external":"1.0.0"}}',
  );
  writeFileSync(
    resolve(dir, "Cargo.lock"),
    'version = 4\n[[package]]\nname = "onebudgetspec-core"\nversion = "0.1.4"\n[[package]]\nname = "onebudgetspec-core"\nversion = "0.1.4"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n',
  );
  writeFileSync(resolve(dir, "CHANGELOG.md"), "# Changelog\n");
  writeFileSync(resolve(dir, "source.rs"), "fn old() {}\n");
  const commit = () => {
    git("add", "-A");
    git("commit", "-qm", "fixture");
    return git("rev-parse", "HEAD");
  };
  const base = commit();
  writeFileSync(
    resolve(dir, "npm/cli/package.json"),
    '{"name":"@onebudgetspec/cli","version":"0.1.5","dependencies":{"external":"1.0.0"}}',
  );
  const version = commit();
  writeFileSync(resolve(dir, "source.rs"), "fn changed() {}\n");
  const source = commit();
  git("checkout", "-q", version);
  writeFileSync(
    resolve(dir, "npm/cli/package.json"),
    '{"name":"@onebudgetspec/cli","version":"0.1.5","dependencies":{"external":"2.0.0"}}',
  );
  const dependency = commit();
  return { dir, git, base, version, source, dependency, commit };
}
export function event(head = "release-plz-2026", fork = false, event_name = "pull_request") {
  return {
    event_name,
    action: "synchronize",
    repository: { full_name: "nickderobertis/onebudgetspec" },
    pull_request: {
      title: "chore: release",
      user: { login: "github-actions[bot]" },
      head: {
        ref: head,
        repo: { full_name: fork ? "fork/onebudgetspec" : "nickderobertis/onebudgetspec" },
      },
    },
  };
}
/** @param {ReturnType<typeof history>} h @param {import("../contracts.js").Event | string} payload @param {string} head @returns {import("../contracts.js").Policy} */
export function runClassifier(h, payload, head = h.version) {
  const path = resolve(h.dir, ".git/event.json");
  writeFileSync(path, typeof payload === "string" ? payload : JSON.stringify(payload));
  return JSON.parse(
    execFileSync(
      "node",
      [
        resolve(root, ".github/macos-policy.mjs"),
        "classify",
        "--event",
        path,
        "--base",
        h.base,
        "--head",
        head,
      ],
      { cwd: h.dir, encoding: "utf8" },
    ),
  );
}
