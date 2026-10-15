// Adapt GitHub's event file to the agreed classifier CLI and emit its routing outputs.
import { execFileSync } from "node:child_process";
import { appendFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { isRecord } from "./workflow-input.mjs";

let event = {};
try {
  const input = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH ?? "", "utf8"));
  if (isRecord(input)) event = input;
} catch {
  // Missing event data never authorizes a PR omission; the classifier sees only its name.
}
const dir = mkdtempSync(
  resolve(
    process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? process.env.RUNNER_TEMP ?? tmpdir(),
    "macos-event-",
  ),
);
try {
  writeFileSync(
    resolve(dir, "event.json"),
    JSON.stringify({ ...event, event_name: process.env.EVENT_NAME }),
  );
  const policy = JSON.parse(
    execFileSync(
      "node",
      [
        resolve(import.meta.dirname, "macos-policy.mjs"),
        "classify",
        "--event",
        resolve(dir, "event.json"),
        "--base",
        process.env.BASE_SHA ?? "",
        "--head",
        process.env.HEAD_SHA ?? "",
      ],
      { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] },
    ),
  );
  if (
    !isRecord(policy) ||
    typeof policy.macosRequired !== "boolean" ||
    typeof policy.isReleaseOnly !== "boolean" ||
    typeof policy.reason !== "string"
  )
    throw new Error("classifier returned invalid routing; fix macos-policy.mjs");
  if (!process.env.GITHUB_OUTPUT)
    throw new Error("GITHUB_OUTPUT is required; run this adapter in a workflow");
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    `macosRequired=${policy.macosRequired}\nisReleaseOnly=${policy.isReleaseOnly}\ndiagnostic=${process.env.DIAGNOSTIC === "true"}\n`,
  );
  console.log(policy.reason);
} finally {
  rmSync(dir, { recursive: true, force: true });
}
