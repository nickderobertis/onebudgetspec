import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";
import { events } from "../scenarios.mjs";
import { digest, expand, resolveRun, root, triggered, workflows } from "../workflow-graph.mjs";
import { event, history, runClassifier } from "./fixtures.mjs";

test("actual workflow graph records event/diff/ownership/publish workloads and required names", () => {
  const h = history();
  const documents = workflows();
  /** @type {import("../contracts.js").Scenario[]} */
  const scenarios = [];
  const classifications = new Map();
  const eventPolicies = new Map();
  try {
    for (const [diff, head] of [
      ["source", h.source],
      ["version", h.version],
      ["dependency", h.dependency],
    ]) {
      for (const fork of [false, true]) {
        const policy = runClassifier(h, event("release-plz-2026", fork), head);
        classifications.set(`${diff}/${fork}`, policy);
        for (const [label, event_name, action] of events) {
          for (let switches = 0; switches < 8; switches++) {
            const scenario = {
              ...event("release-plz-2026", fork, event_name),
              action,
              ref:
                label === "release-tag" || event_name === "release"
                  ? "refs/tags/v0.1.4"
                  : "refs/heads/main",
              workflow_run: { name: "release", head_sha: head },
              inputs: { diagnostic: label === "manual-diagnostic" },
              vars: {
                CARGO_PUBLISH: String(Boolean(switches & 1)),
                PYPI_PUBLISH: String(Boolean(switches & 2)),
                NPM_PUBLISH: String(Boolean(switches & 4)),
              },
            };
            const key = JSON.stringify([label, diff, fork]);
            if (!eventPolicies.has(key))
              eventPolicies.set(
                key,
                event_name === "pull_request" ? policy : runClassifier(h, scenario, head),
              );
            const classified = { ...eventPolicies.get(key) };
            classified.diagnostic = label === "manual-diagnostic";
            const runs = documents
              .filter((w) => triggered(w.document, scenario))
              .map((w) => resolveRun(w, scenario, classified));
            if (event_name === "pull_request") {
              const names = runs.flatMap((r) => r.rows.map((row) => row.name));
              for (const name of ["check", "llmlint", "pr-title"])
                assert.equal(
                  names.filter((n) => n === name).length,
                  1,
                  `${label}: required ${name}`,
                );
              const ci = runs.find((r) => r.workflow.endsWith("/ci.yml"));
              assert.ok(ci);
              assert.equal(
                ci.rows.some((r) => r.macos),
                policy.macosRequired,
              );
              assert.ok(
                ci.rows.some((r) => r.runner === "windows-latest"),
                "Windows PR proof retained",
              );
            }
            if (
              event_name !== "pull_request" &&
              label !== "manual-diagnostic" &&
              event_name !== "release"
            )
              assert.equal(
                runs.some((r) => r.rows.some((row) => row.macos)),
                false,
                label,
              );
            if (event_name === "release") {
              const published = runs.find((r) => r.workflow.endsWith("/release.yml"));
              assert.ok(published);
              for (const [id, bit] of Object.entries({
                "publish-crates": 1,
                "publish-pypi": 2,
                "publish-sdk-pypi": 2,
                "publish-npm": 4,
                "publish-sdk-npm": 4,
              })) {
                assert.equal(
                  published.results[id].result,
                  switches & bit ? "success" : "skipped",
                  id,
                );
              }
            }
            if (label === "manual-diagnostic") {
              const ci = runs.find((run) => run.workflow.endsWith("/ci.yml"));
              assert.ok(
                ci?.rows.some((row) => row.job === "mac-proof" && row.macos),
                "manual opt-in requests the Mac proof",
              );
            }
            scenarios.push({
              label,
              diff,
              fork,
              switches,
              automatic: event_name !== "workflow_dispatch",
              runs,
            });
          }
        }
      }
    }
    const lifecycles = [];
    for (let switches = 0; switches < 8; switches++) {
      for (const fork of [false, true]) {
        /** @param {string} label @param {string} diff @param {boolean} owner */
        const select = (label, diff, owner = fork) =>
          scenarios.find(
            (s) =>
              s.label === label && s.diff === diff && s.fork === owner && s.switches === switches,
          ) ?? assert.fail("missing lifecycle scenario");
        // One source push: PR, base merge, verified same-repository release PR/merge,
        // release tag/publication, and any automatic downstream event fan-out.
        const chain = [
          select("pr-synchronize", "source"),
          select("base-push", "source"),
          select("pr-synchronize", "version", false),
          select("release-merge", "version", false),
          select("release-tag", "version", false),
          select("release-published", "version", false),
          select("downstream", "version", false),
          select("dispatch", "version", false),
          select("scheduled", "version", false),
        ];
        lifecycles.push({
          fork,
          switches,
          runs: chain.flatMap((s) => s.runs.map((run) => ({ ...run, label: s.label }))),
          macosRuns: chain.flatMap((s) => s.runs).filter((run) => run.rows.some((row) => row.macos))
            .length,
        });
      }
    }
    const telemetry = {
      sourceDigest: digest(),
      events: events.map(([label, event]) => ({ label, event })),
      scenarios,
      lifecycles,
      jobsPerRun: Math.max(
        0,
        ...scenarios.flatMap((s) => s.runs.map((r) => r.rows.filter((row) => row.macos).length)),
      ),
      runsPerChange: Math.max(0, ...lifecycles.map((l) => l.macosRuns)),
    };
    mkdirSync(resolve(root, ".github/.telemetry"), { recursive: true });
    writeFileSync(
      resolve(root, ".github/.telemetry/macos-policy.json"),
      JSON.stringify(telemetry, null, 2),
    );
    for (const [metric, key] of [
      ["jobs-per-run", "jobsPerRun"],
      ["runs-per-change", "runsPerChange"],
    ]) {
      const resultFile = resolve(h.dir, ".git/result.json");
      execFileSync(
        "node",
        [
          resolve(root, ".github/macos-budget.mjs"),
          "--metric",
          metric,
          "--telemetry",
          resolve(root, ".github/.telemetry/macos-policy.json"),
        ],
        { env: { ...process.env, ONEBUDGETSPEC_RESULT: resultFile } },
      );
      const result = JSON.parse(readFileSync(resultFile, "utf8"));
      assert.equal(result.value, telemetry[key === "jobsPerRun" ? "jobsPerRun" : "runsPerChange"]);
      assert.match(result.detail, /release.yml/);
    }
    const stale = resolve(h.dir, ".git/stale.json");
    writeFileSync(stale, JSON.stringify({ ...telemetry, sourceDigest: "old" }));
    const corruptions = [];
    for (const kind of [
      "outcomes",
      "runner",
      "skipped",
      "macos",
      "duplicate-scenario",
      "automatic",
      "duplicate-lifecycle",
      "order",
      "macosRuns",
      "empty-evidence",
    ]) {
      const malformed = structuredClone(telemetry);
      const first = malformed.scenarios[0].runs[0];
      switch (kind) {
        case "empty-evidence":
          for (const scenario of malformed.scenarios)
            for (const run of scenario.runs) {
              run.rows = [];
              run.results = {};
            }
          for (const lifecycle of malformed.lifecycles)
            for (const run of lifecycle.runs) {
              run.rows = [];
              run.results = {};
            }
          malformed.jobsPerRun = 0;
          malformed.runsPerChange = 0;
          for (const lifecycle of malformed.lifecycles) lifecycle.macosRuns = 0;
          break;
        case "outcomes":
          Reflect.set(first, "results", { policy: { result: 42 } });
          break;
        case "runner":
          first.rows[0].runner = "unresolved-runner";
          break;
        case "skipped":
          first.rows[0].skipped = true;
          break;
        case "macos":
          first.rows[0].macos = !first.rows[0].macos;
          break;
        case "duplicate-scenario":
          malformed.scenarios.push(malformed.scenarios[0]);
          break;
        case "automatic":
          malformed.scenarios[0].automatic = false;
          break;
        case "duplicate-lifecycle":
          malformed.lifecycles[1] = malformed.lifecycles[0];
          break;
        case "order":
          malformed.lifecycles[0].runs.reverse();
          break;
        case "macosRuns":
          malformed.lifecycles[0].macosRuns++;
          break;
      }
      corruptions.push(malformed);
    }
    for (const malformed of [
      ...corruptions,
      { ...telemetry, jobsPerRun: -1 },
      { ...telemetry, runsPerChange: telemetry.runsPerChange + 1 },
      { ...telemetry, scenarios: [{ runs: null }] },
      { ...telemetry, scenarios: telemetry.scenarios.slice(1) },
      { ...telemetry, lifecycles: telemetry.lifecycles.slice(1) },
    ]) {
      writeFileSync(stale, JSON.stringify(malformed));
      const invalid = spawnSync(
        "node",
        [
          resolve(root, ".github/macos-budget.mjs"),
          "--metric",
          "jobs-per-run",
          "--telemetry",
          stale,
        ],
        { encoding: "utf8" },
      );
      assert.notEqual(invalid.status, 0);
      assert.match(invalid.stderr, /invalid\/inconsistent/);
    }
    writeFileSync(stale, JSON.stringify({ ...telemetry, sourceDigest: "old" }));
    const refused = spawnSync(
      "node",
      [resolve(root, ".github/macos-budget.mjs"), "--metric", "jobs-per-run", "--telemetry", stale],
      { encoding: "utf8" },
    );
    assert.notEqual(refused.status, 0);
    assert.match(refused.stderr, /stale/);
    console.log(
      `recorded jobs/run=${telemetry.jobsPerRun}; runs/change=${telemetry.runsPerChange} (no test cost thresholds)`,
    );
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("matrix and reusable-call resolution fails clearly instead of silently spending zero", () => {
  assert.deepEqual(
    expand({
      os: ["ubuntu-latest", "macos-latest"],
      include: [{ os: "macos-latest", target: "darwin" }],
      exclude: [{ os: "ubuntu-latest" }],
    }),
    [{ os: "macos-latest", target: "darwin" }],
  );
  assert.deepEqual(expand({ os: ["ubuntu-latest"], include: [{ os: "macos-latest" }] }), [
    { os: "ubuntu-latest" },
    { os: "macos-latest" },
  ]);
  assert.deepEqual(
    expand({
      include: [
        { os: "ubuntu-latest", target: "linux" },
        { os: "macos-latest", target: "darwin" },
        { os: "windows-latest", target: "windows" },
      ],
    }),
    [
      { os: "ubuntu-latest", target: "linux" },
      { os: "macos-latest", target: "darwin" },
      { os: "windows-latest", target: "windows" },
    ],
  );
  const scenario = { event_name: "push" };
  /** @param {string} source */
  const expression = (source) => `\${{ ${source} }}`;
  for (const job of [
    { "runs-on": expression("mystery.runner") },
    { uses: "organization/workflow/.github/workflows/build.yml@main" },
    { "runs-on": expression("matrix.os"), strategy: { matrix: { os: expression("mystery.os") } } },
  ]) {
    assert.throws(
      () => resolveRun({ file: "synthetic", document: { jobs: { job } } }, scenario, {}),
      /context|Unrecognized|unresolved|named-value/,
    );
  }
});

test("release graph retains every target, artifact, pre-publication Darwin install command and publish switch", () => {
  const release = workflows().find((w) => w.document.name === "release");
  assert.ok(release);
  const declared = readFileSync(resolve(root, "rust-toolchain.toml"), "utf8")
    .match(/"(?:aarch64|x86_64)-[^"]+"/g)
    ?.map((s) => JSON.parse(s));
  assert.ok(declared);
  const matrix = release.document.jobs.native.strategy?.matrix;
  assert.ok(matrix && typeof matrix !== "string" && matrix.include);
  const native = matrix.include.map((row) => row.target);
  const darwin = release.document.jobs.darwin.steps?.flatMap((s) =>
    [...String(s.run ?? "").matchAll(/scripts\/release\/native\/native-products\.sh (\S+)/g)].map(
      (m) => m[1],
    ),
  );
  assert.ok(darwin);
  assert.deepEqual([...native, ...darwin].sort(), declared.sort());
  // Apple Silicon only: one native target, no Intel target, cross-compile or Rosetta step.
  assert.deepEqual(darwin, ["aarch64-apple-darwin"]);
  assert.ok(!declared.includes("x86_64-apple-darwin"), "no Intel macOS target is declared");
  const releaseText = readFileSync(resolve(root, ".github/workflows/release.yml"), "utf8");
  assert.doesNotMatch(releaseText, /x86_64-apple-darwin|darwin-x64|rosetta/i);
  const rustTargets = release.document.jobs.darwin.steps?.find((s) =>
    s.uses?.startsWith("actions-rust-lang/setup-rust-toolchain@"),
  )?.with?.target;
  assert.equal(rustTargets, "aarch64-apple-darwin");
  let nodeArchitecture;
  for (const step of release.document.jobs.darwin.steps ?? []) {
    if (step.uses?.startsWith("actions/setup-node@")) nodeArchitecture = step.with?.architecture;
    const target = /native-products\.sh (\S+)/.exec(step.run ?? "")?.[1];
    if (target) {
      assert.equal(nodeArchitecture, "arm64");
      assert.ok(step.run?.includes('uv python install "$python_request"'));
      assert.ok(step.run?.includes('ONEBUDGETSPEC_PYTHON="$(uv python find "$python_request")"'));
      assert.ok(step.run?.includes("macos-aarch64-none"));
      assert.ok(step.run?.includes("cat .python-version"));
    }
  }
  for (const target of darwin) {
    for (const artifact of ["wheels", "carrier"])
      assert.ok(
        release.document.jobs.darwin.steps?.some((s) => s.with?.name === `${artifact}-${target}`),
      );
  }
  for (const name of [
    "publish-crates",
    "publish-pypi",
    "publish-npm",
    "publish-sdk-pypi",
    "publish-sdk-npm",
  ])
    assert.ok(
      release.document.jobs[name].needs?.includes("darwin"),
      `${name} waits for install proofs`,
    );
  for (const job of Object.entries(release.document.jobs).filter(([id]) =>
    id.startsWith("publish-"),
  ))
    assert.equal(job[1]["runs-on"], "ubuntu-latest");
  for (const [id, name] of Object.entries({
    "darwin-arm-carrier": "native (aarch64-apple-darwin)",
    "darwin-install-carrier": "install path (macos-latest)",
  })) {
    /** @type {import('../contracts.js').Job} */
    const carrier = release.document.jobs[id];
    assert.equal(carrier.name, name);
    assert.equal(carrier["runs-on"], "ubuntu-latest");
    assert.match(String(carrier.if), /always\(\)/);
    assert.deepEqual(carrier.needs, ["darwin"]);
  }
  const installs = readFileSync(resolve(root, "scripts/release/native/native-products.sh"), "utf8");
  const verification = readFileSync(
    resolve(root, "scripts/release/native/verify-installed.sh"),
    "utf8",
  );
  for (const command of [
    "uv pip install",
    "npm install",
    "cargo install",
    "check --json",
    "payload-hashes.json",
  ])
    assert.ok((installs + verification).includes(command), command);
});

test("local reusable workflows expand their actual matrices and automatic callers cannot opt in a diagnostic", () => {
  const h = history();
  try {
    const directory = resolve(h.dir, ".github/workflows");
    mkdirSync(directory, { recursive: true });
    writeFileSync(
      resolve(directory, "called.yml"),
      `on: workflow_call
jobs:
  darwin:
    runs-on: "\${{ matrix.os }}"
    strategy:
      matrix:
        os: [macos-latest, ubuntu-latest]
`,
    );
    const caller = {
      file: resolve(directory, "caller.yml"),
      document: { jobs: { call: { uses: "./.github/workflows/called.yml" } } },
    };
    const resolved = resolveRun(caller, { event_name: "push" }, {});
    assert.deepEqual(
      resolved.rows.map((row) => [row.job, row.runner]),
      [
        ["call/darwin", "macos-latest"],
        ["call/darwin", "ubuntu-latest"],
      ],
    );
    const ci = workflows().find((w) => w.document.name === "ci");
    assert.ok(ci);
    const automatic = resolveRun(
      ci,
      { event_name: "push", inputs: { diagnostic: true } },
      { macosRequired: false },
    );
    assert.equal(
      automatic.rows.some((row) => row.macos),
      false,
    );
    const failed = resolveRun(
      ci,
      event("feature"),
      { macosRequired: true },
      { "mac-proof": { result: "failure" } },
    );
    assert.equal(
      failed.results["mac-carrier"].result,
      "success",
      "always() schedules the carrier to run its failure-checking command",
    );
    assert.equal(
      failed.results.check.result,
      "success",
      "always() schedules aggregation even after prerequisite failure",
    );
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("job status expressions and unsupported routing are observable at the real graph boundary", () => {
  const workflow = {
    file: "status-case",
    document: {
      jobs: {
        proof: { "runs-on": "ubuntu-latest" },
        failed: { needs: ["proof"], if: "failure()", "runs-on": "ubuntu-latest" },
        cancelled: { needs: ["proof"], if: "cancelled()", "runs-on": "ubuntu-latest" },
        aggregate: { needs: ["proof"], if: "always()", "runs-on": "ubuntu-latest" },
      },
    },
  };
  for (const result of ["success", "failure", "cancelled", "skipped"]) {
    const run = resolveRun(workflow, { event_name: "push" }, {}, { proof: { result } });
    assert.equal(run.rows.find((row) => row.job === "failed")?.skipped, result !== "failure");
    assert.equal(run.rows.find((row) => row.job === "cancelled")?.skipped, result !== "cancelled");
    assert.equal(run.rows.find((row) => row.job === "aggregate")?.skipped, false);
  }
  const h = history();
  try {
    const directory = resolve(h.dir, "workflows");
    mkdirSync(directory);
    writeFileSync(
      resolve(directory, "invalid.yml"),
      "on:\n  push:\n    paths: [source/**]\njobs:\n  proof:\n    runs-on: ubuntu-latest\n",
    );
    assert.throws(() => workflows(directory), /unsupported push routing key: paths/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("declared guard outputs use the real producer with credentials present or absent", () => {
  const ci = workflows().find((workflow) => workflow.document.name === "ci");
  const release = workflows().find((workflow) => workflow.document.name === "release");
  assert.ok(ci && release);
  for (const provisioned of [false, true]) {
    const run = resolveRun(ci, { ...event("feature"), provisioned }, { macosRequired: true });
    assert.equal(run.results["llmlint-guard"].outputs?.enabled, String(provisioned));
    assert.equal(run.rows.find((row) => row.job === "llmlint")?.skipped, !provisioned);
    const built = resolveRun(
      release,
      {
        event_name: "release",
        provisioned,
        vars: { CARGO_PUBLISH: "true", PYPI_PUBLISH: "true", NPM_PUBLISH: "true" },
      },
      {},
    );
    assert.equal(built.results.guard.outputs?.cargo, String(provisioned));
    assert.equal(built.rows.find((row) => row.job === "publish-crates")?.skipped, !provisioned);
  }
  const broken = structuredClone(release);
  assert.ok(broken.document.jobs.guard.outputs);
  broken.document.jobs.guard.outputs.cargo = "$" + "{{ steps.cargo.outputs.renamed }}";
  assert.throws(() => resolveRun(broken, { event_name: "release" }, {}), /unresolved guard output/);
});

test("whole-matrix expressions and reusable inputs schedule actual rows", () => {
  const h = history();
  try {
    const directory = resolve(h.dir, ".github/workflows");
    mkdirSync(directory, { recursive: true });
    writeFileSync(
      resolve(directory, "matrix.yml"),
      `on: push
jobs:
  lane:
    runs-on: "\${{ matrix.os }}"
    strategy:
      matrix: "\${{ fromJSON('{\\"os\\":[\\"ubuntu-latest\\",\\"macos-latest\\"]}') }}"
`,
    );
    const matrix = workflows(directory)[0];
    const run = resolveRun(matrix, { event_name: "push" }, {});
    assert.deepEqual(
      run.rows.map((row) => row.runner),
      ["ubuntu-latest", "macos-latest"],
    );
    writeFileSync(
      resolve(directory, "called.yml"),
      `on: workflow_call
jobs:
  conditional:
    if: inputs.enabled
    runs-on: macos-latest
`,
    );
    const caller = {
      file: resolve(directory, "caller.yml"),
      document: {
        jobs: {
          call: {
            uses: "./.github/workflows/called.yml",
            with: { enabled: "$" + "{{ github.event_name == 'push' }}" },
          },
        },
      },
    };
    assert.equal(resolveRun(caller, { event_name: "push" }, {}).rows[0].macos, true);
    assert.equal(resolveRun(caller, { event_name: "pull_request" }, {}).rows[0].skipped, true);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("actual trigger declarations route strings, arrays, schedules, tags and filters without silent gaps", () => {
  const h = history();
  try {
    const directory = resolve(h.dir, "routing");
    mkdirSync(directory);
    /** @param {string} on @param {import('../contracts.js').Event} scenario */
    const route = (on, scenario) => {
      writeFileSync(
        resolve(directory, "route.yml"),
        `${on}\njobs:\n  proof:\n    runs-on: ubuntu-latest\n`,
      );
      const workflow = workflows(directory)[0];
      return triggered(workflow.document, scenario);
    };
    assert.equal(route("on: push", { event_name: "push" }), true);
    assert.equal(route("on: push", { event_name: "push", ref: "refs/tags/v0.1.4" }), true);
    assert.equal(route("on: [push, release]", { event_name: "schedule" }), false);
    assert.equal(route("on: [push, release]", { event_name: "release" }), true);
    assert.equal(
      route('on:\n  schedule:\n    - cron: "0 0 * * *"', { event_name: "schedule" }),
      true,
    );
    const tags = "on:\n  push:\n    tags: [v0.1.4]";
    assert.equal(route(tags, { event_name: "push", ref: "refs/tags/v0.1.4" }), true);
    assert.equal(route(tags, { event_name: "push", ref: "refs/tags/v0.1.5" }), false);
    assert.equal(route(tags, { event_name: "push", ref: "refs/heads/main" }), false);
    const branches = "on:\n  push:\n    branches: [main]";
    assert.equal(route(branches, { event_name: "push", ref: "refs/heads/main" }), true);
    assert.equal(route(branches, { event_name: "push", ref: "refs/heads/other" }), false);
    const request = "on:\n  pull_request:\n    types: [opened]";
    assert.equal(route(request, { event_name: "pull_request", action: "opened" }), true);
    assert.equal(route(request, { event_name: "pull_request", action: "reopened" }), false);
    const downstream = "on:\n  workflow_run:\n    workflows: [release]\n    types: [completed]";
    assert.equal(
      route(downstream, {
        event_name: "workflow_run",
        action: "completed",
        workflow_run: { name: "release" },
      }),
      true,
    );
    assert.equal(
      route(downstream, {
        event_name: "workflow_run",
        action: "completed",
        workflow_run: { name: "other" },
      }),
      false,
    );
    assert.throws(
      () => route("on: pull_request_target", { event_name: "pull_request_target" }),
      /unsupported workflow event/,
    );
    assert.throws(
      () => route("on: [push, deployment]", { event_name: "push" }),
      /unsupported workflow event/,
    );
    assert.throws(
      () => route("on:\n  deployment: {}", { event_name: "deployment" }),
      /unsupported workflow event/,
    );
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("matrix includes conform to GitHub published original-combination examples", () => {
  // Expected combinations are GitHub's specification example, not scheduler cost fixtures.
  // https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations
  /** @type {import('../contracts.js').Matrix} */
  const matrix = {
    fruit: ["apple", "pear"],
    animal: ["cat", "dog"],
    include: [
      { color: "green" },
      { color: "pink", animal: "cat" },
      { fruit: "apple", shape: "circle" },
      { fruit: "banana" },
      { fruit: "banana", animal: "cat" },
    ],
  };
  assert.deepEqual(expand(matrix), [
    { fruit: "apple", animal: "cat", color: "pink", shape: "circle" },
    { fruit: "apple", animal: "dog", color: "green", shape: "circle" },
    { fruit: "pear", animal: "cat", color: "pink" },
    { fruit: "pear", animal: "dog", color: "green" },
    { fruit: "banana" },
    { fruit: "banana", animal: "cat" },
  ]);
});

test("conditional guard producers are explicitly refused without executing them", () => {
  const release = structuredClone(
    workflows().find((workflow) => workflow.document.name === "release"),
  );
  assert.ok(release);
  assert.ok(release.document.jobs.guard.steps);
  const producer = release.document.jobs.guard.steps.find((step) =>
    step.run?.includes("ci-guard.sh"),
  );
  assert.ok(producer);
  producer.if = "false";
  assert.throws(
    () => resolveRun(release, { event_name: "release" }, {}),
    /unresolved conditional guard producer/,
  );
});
