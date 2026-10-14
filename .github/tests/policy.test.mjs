import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { test } from "node:test";
import { root } from "../workflow-graph.mjs";

import { event, history, runClassifier } from "./fixtures.mjs";

test("real git histories authorize only same-repository release branches and semantic release diffs", () => {
  const h = history();
  try {
    assert.equal(runClassifier(h, event()).isReleaseOnly, true);
    const human = event();
    human.pull_request.user.login = "human";
    assert.equal(runClassifier(h, human).isReleaseOnly, true);
    for (const payload of [event("feature"), event("release-plz-forged", true)]) {
      assert.equal(runClassifier(h, payload).macosRequired, true);
    }
    for (const head of [h.source, h.dependency])
      assert.equal(runClassifier(h, event(), head).macosRequired, true);
    assert.equal(runClassifier(h, "{broken").macosRequired, true);
    assert.equal(runClassifier(h, event(), "missing-ref").macosRequired, true);
    const savedBase = h.base;
    const injected = resolve(h.dir, "injected-output");
    h.base = `--output=${injected}`;
    assert.equal(runClassifier(h, event()).macosRequired, true);
    assert.equal(existsSync(injected), false);
    h.base = savedBase;
    assert.match(runClassifier(h, { event_name: "surprise" }).reason ?? "", /surprise/);
    for (const event_name of [
      "push",
      "release",
      "workflow_run",
      "repository_dispatch",
      "schedule",
      "workflow_dispatch",
    ])
      assert.equal(runClassifier(h, { event_name }).macosRequired, false);
    // A registry package with the same name as a local crate must retain its version.
    h.git("checkout", "-q", h.base);
    writeFileSync(
      resolve(h.dir, "Cargo.lock"),
      'version = 4\n[[package]]\nname = "onebudgetspec-core"\nversion = "0.1.4"\n[[package]]\nname = "onebudgetspec-core"\nversion = "0.1.5"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n',
    );
    assert.equal(runClassifier(h, event(), h.commit()).macosRequired, true);
    h.git("checkout", "-q", h.base);
    writeFileSync(
      resolve(h.dir, "Cargo.lock"),
      'version = 4\n[[package]]\nname = "onebudgetspec-core"\nversion = "0.1.5"\n[[package]]\nname = "onebudgetspec-core"\nversion = "0.1.4"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n',
    );
    assert.equal(runClassifier(h, event(), h.commit()).isReleaseOnly, true);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("carriers exercise the real command with success, cancellation, missing work and authorized skips", () => {
  /** @param {import("../contracts.js").Needs} needs @param {string[]} args */
  const invoke = (needs, args = []) =>
    spawnSync("node", [resolve(root, ".github/job-results.mjs"), ...args], {
      env: { ...process.env, NEEDS: JSON.stringify(needs) },
      encoding: "utf8",
    });
  const policy = { result: "success", outputs: { macosRequired: "false" } };
  const completed = invoke({ policy, "mac-proof": { result: "success" } });
  assert.equal(completed.status, 0);
  assert.doesNotMatch(completed.stdout, /no Mac tests ran/);
  const omitted = invoke({ policy, "mac-proof": { result: "skipped" } });
  assert.equal(omitted.status, 0);
  assert.match(omitted.stdout, /no Mac tests ran/);
  for (const result of ["failure", "cancelled", undefined])
    assert.notEqual(invoke({ policy, "mac-proof": { result } }).status, 0);
  assert.notEqual(
    invoke({
      policy: { ...policy, outputs: { macosRequired: "true" } },
      "mac-proof": { result: "skipped" },
    }).status,
    0,
  );
  for (const result of ["failure", "cancelled", "skipped", undefined])
    assert.notEqual(
      invoke({ policy: { ...policy, result }, "mac-proof": { result: "skipped" } }).status,
      0,
    );
  for (const result of ["success", "failure", "cancelled", "skipped", undefined]) {
    const outcome = invoke({ gate: { result }, deny: { result: "success" } });
    assert.equal(outcome.status === 0, result === "success");
  }
  // Job names restated as arguments could drift from the needs list, so they are refused.
  const restated = invoke({ policy, "mac-proof": { result: "success" } }, ["policy"]);
  assert.notEqual(restated.status, 0);
  assert.match(restated.stderr, /every job in NEEDS is required/);
});

test("each semantic version surface admits only version content over real git diffs", () => {
  const cases = [
    [
      "Cargo.toml",
      '[workspace.package]\nversion="0.1.4"\n[workspace.dependencies]\nlocal={path="crates/local",version="=0.1.4"}\nexternal="1.0.0"\n',
      'external="1.0.0"',
      'external="2.0.0"',
    ],
    [
      "crates/example/Cargo.toml",
      '[package]\nname="example"\nversion="0.1.4"\nedition="2021"\n[dev-dependencies]\nlocal={path="../local",version="0.1.4"}\n',
      'edition="2021"',
      'edition="2024"',
    ],
    [
      "sdks/python/pyproject.toml",
      '[project]\nname="onebudgetspec-sdk"\nversion="0.1.4"\ndependencies=["onebudgetspec-cli==0.1.4","registry==1.0.0"]\n[tool.uv.sources]\nonebudgetspec-cli={workspace=true}\n',
      "registry==1.0.0",
      "registry==2.0.0",
    ],
    [
      "uv.lock",
      'version=1\n[[package]]\nname="onebudgetspec-sdk"\nversion="0.1.4"\nsource={editable="sdks/python"}\n[[package]]\nname="onebudgetspec-sdk"\nversion="1.0.0"\nsource={registry="https://pypi.org/simple"}\n',
      'version="1.0.0"',
      'version="2.0.0"',
    ],
    [
      "bun.lock",
      '{"workspaces":{"npm/cli":{"name":"@onebudgetspec/cli","version":"0.1.4"}},"packages":{"@onebudgetspec/cli":["@onebudgetspec/cli@1.0.0","registry",{},"hash"]}}',
      "@onebudgetspec/cli@1.0.0",
      "@onebudgetspec/cli@2.0.0",
    ],
    [
      "sdks/python/src/onebudgetspec_sdk/__init__.py",
      '__version__ = "0.1.4"\nPUBLIC = "old"\n',
      'PUBLIC = "old"',
      'PUBLIC = "new"',
    ],
    [
      "sdks/typescript/src/index.ts",
      'export const VERSION = "0.1.4";\nexport const PUBLIC = "old";\n',
      'PUBLIC = "old"',
      'PUBLIC = "new"',
    ],
  ];
  for (const [path, before, content, changed] of cases) {
    const h = history();
    try {
      h.git("checkout", "-q", h.base);
      mkdirSync(dirname(resolve(h.dir, path)), { recursive: true });
      writeFileSync(resolve(h.dir, path), before);
      h.base = h.commit();
      assert.equal(
        runClassifier(h, event(), h.base).macosRequired,
        true,
        "empty diff requires coverage",
      );
      writeFileSync(resolve(h.dir, path), before.replaceAll("0.1.4", "0.1.5"));
      assert.equal(runClassifier(h, event(), h.commit()).isReleaseOnly, true, path);
      writeFileSync(resolve(h.dir, path), before.replace(content, changed));
      assert.equal(runClassifier(h, event(), h.commit()).macosRequired, true, `${path} content`);
      writeFileSync(resolve(h.dir, path), before.replaceAll("0.1.4", "invalid"));
      assert.equal(
        runClassifier(h, event(), h.commit()).macosRequired,
        true,
        `${path} invalid version`,
      );
    } finally {
      rmSync(h.dir, { recursive: true, force: true });
    }
  }
});

test("changelog content, added/deleted files, diagnostic authorization and malformed commands", () => {
  const h = history();
  try {
    h.git("checkout", "-q", h.base);
    writeFileSync(resolve(h.dir, "CHANGELOG.md"), "# New release\n");
    assert.equal(runClassifier(h, event(), h.commit()).isReleaseOnly, true);
    writeFileSync(resolve(h.dir, "new-file"), "extra source");
    assert.equal(runClassifier(h, event(), h.commit()).macosRequired, true);
    h.git("checkout", "-q", h.base);
    h.git("rm", "source.rs");
    assert.equal(runClassifier(h, event(), h.commit()).macosRequired, true);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
  /** @param {import("../contracts.js").Needs} needs @param {string[]} args */
  const invoke = (needs, args = []) =>
    spawnSync("node", [resolve(root, ".github/job-results.mjs"), ...args], {
      env: { ...process.env, NEEDS: JSON.stringify(needs) },
      encoding: "utf8",
    });
  assert.notEqual(invoke({}).status, 0);
  assert.notEqual(
    invoke({
      policy: { result: "success", outputs: { macosRequired: "false", diagnostic: "true" } },
      "mac-proof": { result: "skipped" },
    }).status,
    0,
  );
});

test("production event adapter emits classifier outputs and unreadable PR inputs require the Mac lane", () => {
  const h = history();
  try {
    /** @type {[string,boolean][]} */
    const cases = [
      [JSON.stringify(event()), false],
      ["{broken", true],
    ];
    for (const [payload, required] of cases) {
      const path = resolve(h.dir, ".git/ci-event.json");
      const output = resolve(h.dir, ".git/github-output");
      writeFileSync(path, payload);
      writeFileSync(output, "");
      const invoked = spawnSync("node", [resolve(root, ".github/ci-policy.mjs")], {
        cwd: h.dir,
        env: {
          ...process.env,
          EVENT_NAME: "pull_request",
          GITHUB_EVENT_PATH: path,
          GITHUB_OUTPUT: output,
          BASE_SHA: h.base,
          HEAD_SHA: h.version,
          DIAGNOSTIC: "false",
        },
        encoding: "utf8",
      });
      assert.equal(invoked.status, 0, invoked.stderr);
      assert.match(readFileSync(output, "utf8"), new RegExp(`macosRequired=${required}\\n`));
      assert.match(readFileSync(output, "utf8"), new RegExp(`isReleaseOnly=${!required}\\n`));
    }
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("result commands reject malformed roots and outputs before deriving skip permissions", () => {
  for (const needs of [
    null,
    [],
    { policy: { result: "success", outputs: { macosRequired: false } } },
    { policy: { result: ["success"] } },
  ]) {
    const refused = spawnSync("node", [resolve(root, ".github/job-results.mjs")], {
      env: { ...process.env, NEEDS: JSON.stringify(needs) },
      encoding: "utf8",
    });
    assert.notEqual(refused.status, 0);
    assert.match(refused.stderr, /NEEDS must be|invalid outputs|invalid outcome/);
    assert.doesNotMatch(refused.stderr, /TypeError/);
  }
});

test("release carriers reject every non-successful Darwin proof using the actual result command", () => {
  for (const result of ["success", "failure", "cancelled", "skipped", undefined]) {
    const run = spawnSync("node", [resolve(root, ".github/job-results.mjs")], {
      env: { ...process.env, NEEDS: JSON.stringify({ darwin: { result } }) },
      encoding: "utf8",
    });
    assert.equal(run.status === 0, result === "success");
  }
});

test("target-specific Cargo and path-sourced Python dependencies preserve their non-version content", () => {
  const cases = [
    [
      "Cargo.toml",
      '[workspace.package]\nversion="0.1.4"\n[target.\'cfg(unix)\'.dependencies]\nlocal={path="crates/local",version="=0.1.4",features=["old"]}\n',
      'features=["old"]',
      'features=["new"]',
    ],
    [
      "sdks/python/pyproject.toml",
      '[project]\nname="example"\nversion="0.1.4"\ndependencies=["local==0.1.4"]\n[tool.uv.sources]\nlocal={path="../local",editable=true}\n',
      "editable=true",
      "editable=false",
    ],
  ];
  for (const [path, before, content, changed] of cases) {
    const h = history();
    try {
      h.git("checkout", "-q", h.base);
      mkdirSync(dirname(resolve(h.dir, path)), { recursive: true });
      writeFileSync(resolve(h.dir, path), before);
      h.base = h.commit();
      writeFileSync(resolve(h.dir, path), before.replaceAll("0.1.4", "0.1.5"));
      assert.equal(runClassifier(h, event(), h.commit()).isReleaseOnly, true, path);
      writeFileSync(resolve(h.dir, path), before.replace(content, changed));
      assert.equal(runClassifier(h, event(), h.commit()).macosRequired, true, path);
      if (path === "Cargo.toml") {
        writeFileSync(resolve(h.dir, path), before.replace('version="=0.1.4"', 'version="0.1.5"'));
        assert.equal(
          runClassifier(h, event(), h.commit()).macosRequired,
          true,
          "dependency operator changes require Mac",
        );
      }
    } finally {
      rmSync(h.dir, { recursive: true, force: true });
    }
  }
});

test("the production adapter emits true diagnostic authorization for an explicit manual run", () => {
  const h = history();
  try {
    const path = resolve(h.dir, ".git/manual.json");
    const output = resolve(h.dir, ".git/manual-output");
    writeFileSync(path, "{}");
    writeFileSync(output, "");
    const invoked = spawnSync("node", [resolve(root, ".github/ci-policy.mjs")], {
      cwd: h.dir,
      env: {
        ...process.env,
        EVENT_NAME: "workflow_dispatch",
        GITHUB_EVENT_PATH: path,
        GITHUB_OUTPUT: output,
        DIAGNOSTIC: "true",
      },
      encoding: "utf8",
    });
    assert.equal(invoked.status, 0, invoked.stderr);
    assert.match(readFileSync(output, "utf8"), /diagnostic=true\n/);
    assert.match(readFileSync(output, "utf8"), /macosRequired=false\n/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("malformed admitted manifest structures fail closed at the actual classifier boundary", () => {
  const cases = [
    ["npm/cli/package.json", '{"name":"@onebudgetspec/cli","version":"0.1.4"}', "[]"],
    [
      "npm/cli/package.json",
      '{"name":"@onebudgetspec/cli","version":"0.1.4"}',
      '{"name":42,"version":"0.1.5"}',
    ],
    [
      "bun.lock",
      '{"workspaces":{"npm/cli":{"name":"@onebudgetspec/cli","version":"0.1.4"}}}',
      '{"workspaces":[]}',
    ],
    [
      "bun.lock",
      '{"workspaces":{"npm/cli":{"name":"@onebudgetspec/cli","version":"0.1.4"}}}',
      '{"workspaces":{"npm/cli":42}}',
    ],
    ["Cargo.toml", '[workspace.package]\nversion="0.1.4"\n', '[workspace]\npackage="bad"\n'],
    ["Cargo.toml", '[workspace.package]\nversion="0.1.4"\n', "target=[]\n"],
    ["Cargo.toml", '[workspace.package]\nversion="0.1.4"\n', "dependencies=[]\n"],
    [
      "sdks/python/pyproject.toml",
      '[project]\nname="example"\nversion="0.1.4"\n',
      "[project]\ndependencies=[42]\n",
    ],
    [
      "sdks/python/pyproject.toml",
      '[project]\nname="example"\nversion="0.1.4"\n',
      "[tool.uv]\nsources=[]\n",
    ],
    [
      "Cargo.lock",
      'version=4\n[[package]]\nname="local"\nversion="0.1.4"\n',
      "version=4\npackage=[42]\n",
    ],
    // A version-only edit whose local-source claim is not a path is not admitted as local.
    ...[0, 42, ""].map((path) => {
      const manifest = (/** @type {string} */ v) =>
        `[project]\nname="example"\nversion="${v}"\ndependencies=["dep==${v}"]\n[tool.uv.sources]\ndep={path=${JSON.stringify(path)}}\n`;
      return ["sdks/python/pyproject.toml", manifest("0.1.4"), manifest("0.1.5")];
    }),
    ...["editable", "virtual", "directory"].map((key) => {
      const lock = (/** @type {string} */ v) =>
        `version=1\n[[package]]\nname="local"\nversion="${v}"\nsource={${key}=42}\n`;
      return ["uv.lock", lock("0.1.4"), lock("0.1.5")];
    }),
  ];
  for (const [path, before, after] of cases) {
    const h = history();
    try {
      h.git("checkout", "-q", h.base);
      mkdirSync(dirname(resolve(h.dir, path)), { recursive: true });
      writeFileSync(resolve(h.dir, path), before);
      h.base = h.commit();
      writeFileSync(resolve(h.dir, path), after);
      assert.equal(runClassifier(h, event(), h.commit()).macosRequired, true, path);
    } finally {
      rmSync(h.dir, { recursive: true, force: true });
    }
  }
  const h = history();
  try {
    const malformed = event();
    Reflect.set(malformed.pull_request.head, "ref", 42);
    const result = runClassifier(h, malformed);
    assert.equal(result.macosRequired, true);
    assert.match(result.reason ?? "", /identity not established/);
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});

test("a release written by the repository's own version tool is release-only at every place it owns", () => {
  /** @param {string[]} args @param {string} code */
  const tool = (code, ...args) =>
    execFileSync(
      "uv",
      [
        ...["run", "--quiet", "--frozen", "--no-sync", "--package", "onebudgetspec-repo-checks"],
        ...["python", "-c", code, ...args],
      ],
      { cwd: root, encoding: "utf8" },
    );
  // The places come from versions.py, so a place added there that this classifier does not
  // admit fails here rather than turning every later release pull request into an ordinary one.
  /** @type {string[]} */
  const owned = JSON.parse(
    tool(
      "import json; from repo_checks.versions import WORKSPACE, places; " +
        "print(json.dumps(sorted({p.path for p in [WORKSPACE, *places()]})))",
    ),
  );
  assert.ok(owned.length > 5 && owned.includes("bun.lock"));
  const h = history();
  try {
    for (const path of owned) {
      mkdirSync(dirname(resolve(h.dir, path)), { recursive: true });
      writeFileSync(resolve(h.dir, path), readFileSync(resolve(root, path)));
    }
    h.base = h.commit();
    tool(
      "import sys; from pathlib import Path; from repo_checks.versions import main; " +
        "sys.exit(main(['set', '987.65.4'], Path(sys.argv[1])))",
      h.dir,
    );
    const written = h.git("diff", "--name-only", "HEAD").split("\n").sort();
    assert.deepEqual(written, owned);
    const policy = runClassifier(h, event(), h.commit());
    assert.equal(policy.isReleaseOnly, true, policy.reason ?? "no reason");
  } finally {
    rmSync(h.dir, { recursive: true, force: true });
  }
});
