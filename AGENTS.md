<!-- llmlint: ignore-file[agents_md_durable_and_terse] three sections here are required content rather than prose to trim: "Stack and composition" is read by create-repo's baseline checker, "Journeys" is held to the journey files by tools/tests/test_journeys.py, and the required-check statement is what the publishing provisioning step re-applies. -->
# AGENTS.md

Durable constraints for humans and agents working here. `CLAUDE.md` is a symlink to this
file; edit only this one.

## What this repo is

`onebudgetspec` registers measurable budgets in `budgets.yaml` files and gates on them:
a Rust SDK (`onebudgetspec-core`) and CLI (`onebudgetspec`), shipped on crates.io, PyPI
and npm, with Python and TypeScript SDKs, all released at one version.

## Two standing goals on every task

1. **Engineer the context for next time:** realistic journeys for what a user sees,
   scripts that shrink repetitive steps to signal, terse notes here.
2. **Engineer the codebase and environment:** a strict gate, `just bootstrap` from a
   clean clone, and the same pinned checks locally and in CI.

## Stack and composition

- **Product shape:** cli, built on a library published as the Rust SDK.
- **Language(s):** rust, python, typescript.
- **References composed:** base.md, project-graph.md, shapes/cli.md, languages/rust.md,
  languages/python.md, languages/typescript.md, intersections/rust-cli.md, ci.md,
  llmlint.md, releasing.md.
- **Excluded, and why:** the python-cli intersection (the CLI is Rust; its PyPI
  distribution is the compiled binary in a maturin `bin` wheel, with no Python console
  entry point, and the `cli_wheel.rs` journey installs and drives it); asdf/direnv (CI pins tools through setup actions and
  `rust-toolchain.toml`); GitHub Release archives (every install surface is a registry).
- **Platforms:** Linux, macOS and Windows, each on x86_64 and aarch64. A timeout ends a
  command's process group on Unix and its Job Object on Windows. Every measuring command in
  the journeys and conformance cases is Node (`node` on `PATH`), never a POSIX shell, so
  they run on all three; a test gated to Unix states why beside its gate. Windows checks
  out LF (`.gitattributes`), `just` exports `PYTHONUTF8=1`, and its gate lane holds no Rust
  coverage floor (`scripts/rust-coverage.sh` says why).
- **Modelled on onetaskgraph:** the crate split, the maturin `bin` wheel, the npm launcher
  with carriers, the schema emitted from the Rust types and the release flow.

## The contract

The Rust types in `crates/onebudgetspec-core` are the one source of the budgets file and
both reports; `onebudgetspec schema` emits them. Never hand-write a copy: generate it or
reconcile it with a test. A schema change bumps `SCHEMA_BUNDLE_VERSION` and re-records
`schema.golden` in the same change. Other repositories build against this contract, so
changing it is its owner's decision. The CLI parses, calls the SDK and renders; behaviour
lives in the SDK.

What ships names none of the sibling libraries it is used beside, depends on none, and
mentions no plans, design documents or approvals; `tools/src/repo_checks/boundary.py`
enforces it. A judged-lint suppression directive is the one exempt occurrence.

## SDKs

<!-- llmlint: ignore-block[instruction_layer_localized] The three SDKs are documented here side by side by requirement: one place states each SDK's package, install, calls, binary order and owed tests so they can be compared, and nesting them would split that comparison across three files. -->
Three, side by side; the Python and TypeScript SDKs run the binary and parse its JSON, and
never reimplement or link the engine. Their models are generated from `onebudgetspec
schema` (`sdks/python/generate.py`, `sdks/typescript/scripts/generate.ts`): `just generate`
regenerates them, and each generator's `--check` runs under its project's `lint`. Statuses
0, 1 and 3 return the report; 2 raises or rejects `OnebudgetspecError` with the CLI's message.

- **Rust** — `onebudgetspec-core` (`cargo add onebudgetspec-core`): `load`, `select`,
  `check`, `list_report`, `schema_bundle`, in process. Owes `crates/onebudgetspec-core/tests/`
  and, through the binary, every journey and conformance case.
- **Python** — `onebudgetspec-sdk` (`pip install onebudgetspec-sdk`, requiring
  `onebudgetspec-cli==` the workspace version): `check`, `validate`, `list_budgets`,
  `schema`. Binary: `binary=`, `ONEBUDGETSPEC_BIN`, the cli wheel's (`bin/onebudgetspec`,
  or `Scripts/onebudgetspec.exe` on Windows), `PATH`. Owes
  `sdks/python/tests/test_conformance.py` (every case), `test_binary.py` (resolution,
  refusals), `test_generate.py` (the generator) and the `sdk_python.rs` journey.
- **TypeScript** — `@onebudgetspec/sdk` (`npm install @onebudgetspec/sdk`, with
  `@onebudgetspec/cli` optional, `workspace:*` packed as the workspace version): `check`,
  `validate`, `listBudgets`, `schema`. Binary: `binary`, `ONEBUDGETSPEC_BIN`, the resolved
  `@onebudgetspec/cli` launcher. Owes `sdks/typescript/tests/conformance.test.ts`,
  `binary.test.ts`, `errors.test.ts`, `generator.test.ts` and the `sdk_typescript.rs` journey.

A new conformance case must use only flags both SDK runners parse; each fails on any other.
<!-- llmlint: ignore-end[instruction_layer_localized] -->

## Journeys

`tools/tests/test_journeys.py` fails when this list and the journey files differ.

- `crates/onebudgetspec-e2e/tests/journeys/verdicts.rs` — within and over under both directions, equality within, the figures, exits 0, 1 and 3.
- `crates/onebudgetspec-e2e/tests/journeys/elapsed.rs` — `elapsed` times a short wait; a non-zero exit is an error.
- `crates/onebudgetspec-e2e/tests/journeys/errors.rs` — every failed measurement is an error, and an error wins over over-budget; a timeout ends the command's whole process tree; a failure names its exit status (an NTSTATUS in hex on Windows).
- `crates/onebudgetspec-e2e/tests/journeys/detail.rs` — `detail` is the command's, or null.
- `crates/onebudgetspec-e2e/tests/journeys/measurement_order.rs` — once each, one at a time, in file order; unselected never run.
- `crates/onebudgetspec-e2e/tests/journeys/conditions.rs` — declared conditions run once per file and reach only that file's results.
- `crates/onebudgetspec-e2e/tests/journeys/returned_conditions.rs` — returned conditions stay in their result; collisions and malformed ones are errors.
- `crates/onebudgetspec-e2e/tests/journeys/budget_id.rs` — `ONEBUDGETSPEC_BUDGET_ID` reaches every budget's command and no condition's; one generic runner serves two budgets.
- `crates/onebudgetspec-e2e/tests/journeys/command_environment.rs` — working directory, a relative program found from the file's directory, inherited environment, empty result file, no shell.
- `crates/onebudgetspec-e2e/tests/journeys/command_stderr.rs` — a failed or unreadable measurement's `error` keeps its exit status and bounded stderr tail, in JSON and text; a failing condition's diagnostic line too; success unchanged.
- `crates/onebudgetspec-e2e/tests/journeys/rust_report.rs` — a Rust measurement's `report` under a check gives `actual` and `detail`; outside one it writes nothing; a non-finite value or a failed write is an error.
- `crates/onebudgetspec-e2e/tests/journeys/output_streams.rs` — command output on stderr only; an unwritable report exits 3.
- `crates/onebudgetspec-e2e/tests/journeys/host.rs` — host values each platform supplies (load on Linux and macOS, available memory on Linux and Windows), `unknown` conditions, the file path and ordered times.
- `crates/onebudgetspec-e2e/tests/journeys/files.rs` — the default file, several files, a missing file refused.
- `crates/onebudgetspec-e2e/tests/journeys/selection.rs` — id, label and excluded-label filters together; unknown ids refused.
- `crates/onebudgetspec-e2e/tests/journeys/discovery.rs` — recursive discovery in path order, `.gitignore`, cross-file ids, invalid nested files.
- `crates/onebudgetspec-e2e/tests/journeys/invalid_files.rs` — every invalid shape refused with 2, naming file and key, running nothing.
- `crates/onebudgetspec-e2e/tests/journeys/text_output.rs` — the text line for each verdict, matching the JSON.
- `crates/onebudgetspec-e2e/tests/journeys/json_flag.rs` — `--json` equals `--output json` on every verb.
- `crates/onebudgetspec-e2e/tests/journeys/list.rs` — `list` under every selection, running nothing.
- `crates/onebudgetspec-e2e/tests/journeys/validate.rs` — `validate` accepts a good file, running nothing.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/cli_wheel.rs` — the wheel's binary checks a case exactly as the cargo build does.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/npm_launcher.rs` — the launcher runs its carrier like the cargo build, and refuses a missing, broken or killed one.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_python.rs` — the SDK wheel pins `onebudgetspec-cli` at the workspace version, and installed beside it answers a conformance case through every call, typed.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_typescript.rs` — the SDK tarball takes `@onebudgetspec/cli` as an optional dependency at the workspace version, answers a conformance case through every call beside the launcher, and type-checks a consumer.

## Commits, releases, and merging

- **Squash-merge only, via PR, with auto-merge**; head branches auto-delete; admins may
  override. `just governance` applies it; `just governance --verify` reads it back.
- **Required checks: exactly `check` and `pr-title`.** llmlint becomes required when
  publishing is provisioned; until then there is no harness credential and its job is
  skipped. Provisioning re-runs `just governance` with `llmlint` added and
  `--allow-missing-llmlint` dropped.
- **Tiers:** pull requests and merges run `just check` (affected); the release pull
  request release-plz opens runs `just check-all` once, since merging it releases; the
  release workflows only build, publish and then install what they published.
- **Releases:** Conventional Commits drive release-plz (pre-1.0: `feat` bumps minor,
  `fix`/`perf` patch). Merging its release PR tags and cuts the GitHub Release that
  publishes. The five target names in `release-targets.toml` are a contract with other
  repositories; never rename one.
- **Secrets and variables** are the user's to provision; never create one, or a registry
  package, from here. Jobs needing one are skipped by a guard until it exists.

## Invariants

- No warnings-only mode; a suppression carries its reason at its site.
- Journeys drive the built binary and installed packages over real files and commands;
  never double the layer under test.
- A budgets file is untrusted input, validated at the boundary.
- Scripts are quiet on success and name the error and a next step on failure.

## Keeping the allowlist current

Add a routine command to `.claude/settings.json` rather than re-approving it; keep it narrow.
