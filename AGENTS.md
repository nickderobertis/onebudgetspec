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
  languages/python.md, languages/typescript.md, intersections/rust-cli.md,
  intersections/python-cli.md, ci.md, llmlint.md, releasing.md.
- **Excluded, and why:** asdf/direnv (CI pins tools through setup actions and
  `rust-toolchain.toml`); Windows (journeys and measurement are POSIX, so the release
  ships Linux and macOS); GitHub Release archives (every install surface is a registry).
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

## Journeys

`tools/tests/test_journeys.py` fails when this list and the journey files differ.

- `crates/onebudgetspec-e2e/tests/journeys/verdicts.rs` — within and over under both directions, equality within, the figures, exits 0, 1 and 3.
- `crates/onebudgetspec-e2e/tests/journeys/elapsed.rs` — `elapsed` times a sleep; a non-zero exit is an error.
- `crates/onebudgetspec-e2e/tests/journeys/errors.rs` — every failed measurement is an error, and an error wins over over-budget.
- `crates/onebudgetspec-e2e/tests/journeys/detail.rs` — `detail` is the command's, or null.
- `crates/onebudgetspec-e2e/tests/journeys/measurement_order.rs` — once each, one at a time, in file order; unselected never run.
- `crates/onebudgetspec-e2e/tests/journeys/conditions.rs` — declared conditions run once per file and reach only that file's results.
- `crates/onebudgetspec-e2e/tests/journeys/returned_conditions.rs` — returned conditions stay in their result; collisions and malformed ones are errors.
- `crates/onebudgetspec-e2e/tests/journeys/command_environment.rs` — working directory, inherited environment, empty result file, no shell.
- `crates/onebudgetspec-e2e/tests/journeys/output_streams.rs` — command output on stderr only; an unwritable report exits 3.
- `crates/onebudgetspec-e2e/tests/journeys/host.rs` — host values, `unknown` conditions, the file path and ordered times.
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
- `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_python.rs` — the Python SDK wheel installs and imports, typed.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_typescript.rs` — the TypeScript SDK tarball installs and imports, with types.

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
