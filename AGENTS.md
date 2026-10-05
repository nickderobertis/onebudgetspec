# AGENTS.md

Durable constraints for humans and agents working here. Terse on purpose: this is
always-loaded context. `CLAUDE.md` is a symlink to this file; edit only this one.

## What this repo is

`onebudgetspec` registers measurable budgets in `budgets.yaml` files and gates on them.
It ships a Rust SDK (`onebudgetspec-core`), a CLI (`onebudgetspec`) on crates.io, PyPI
(`onebudgetspec-cli`, a maturin `bin` wheel) and npm (`@onebudgetspec/cli` with
per-platform carriers), and Python and TypeScript SDKs (`onebudgetspec-sdk`,
`@onebudgetspec/sdk`), all at one version. The SDK packages are scaffolds today; their
API lands on top of them.

## Two standing goals on every task

1. **Engineer the context for next time:** realistic journeys for what a user sees,
   scripts that shrink repetitive steps to signal, terse notes here for what the code
   does not make obvious.
2. **Engineer the codebase and environment:** keep the gate strict, `just bootstrap`
   working from a clean clone, and local and CI running the same pinned checks.

Fold either into a task when it is the lowest-error path; otherwise propose it after.

## Stack and composition

- **Product shape:** cli (with the library it is built on as a published Rust SDK).
- **Language(s):** rust, python, typescript.
- **References composed:** base.md, project-graph.md, shapes/cli.md, languages/rust.md,
  languages/python.md, languages/typescript.md, intersections/rust-cli.md,
  intersections/python-cli.md, ci.md, llmlint.md, releasing.md.
- **Projects in the graph:** `onebudgetspec-core` (the SDK and the contract, depending on
  no other project), `onebudgetspec` (the binary and the conformance runner),
  `onebudgetspec-e2e` (journeys), `onebudgetspec-packaging-e2e` (install-and-drive
  journeys, the slowest suite, behind its own edge), `npm-cli`, `sdk-python`,
  `sdk-typescript`, `repo-checks` (`tools/`: the checks below) and `workspace` (the Rust
  coverage floor, supply chain and bootstrap).
- **Excluded, and why:** asdf/direnv (CI installs pinned tools with setup actions;
  `rust-toolchain.toml` pins Rust); Windows (journeys and measurement are POSIX; the
  release ships Linux and macOS only); GitHub Release binary archives (every install
  surface is a registry). Nothing non-negotiable is excluded.
- **Modelled on onetaskgraph:** the crate split, the maturin `bin` wheel, the npm
  launcher with carriers, the schema emitted from the Rust types and the release flow.

## The contract

The Rust types in `crates/onebudgetspec-core` are the one source of the budgets file and
the check and list reports; `onebudgetspec schema` emits them (roots `budgets-file`,
`check-report`, `list-report`). Never hand-write a copy: generate it from that output or
reconcile it with a test. Changing the schema means bumping `SCHEMA_BUNDLE_VERSION` and
re-recording `schema.golden` in the same change. Other repositories build against this
contract, so a change to it is a decision for its owner, never a side effect.

The CLI parses, calls `onebudgetspec-core` and renders; behaviour lives in the SDK.

The library knows nothing of the stack it is used beside: what ships (both crates, the
wheel, the npm packages, both SDKs, the README, the schema) names none of the sibling
libraries and depends on none, nor mentions plans, design documents or approvals.
`tools/src/repo_checks/boundary.py` enforces this, including over resolved dependencies.

## Command surface

`just --list` describes every recipe. `just check` is the gate over the projects a change
can reach (`nx affected` against `NX_BASE`, default `origin/main`); `just check-all` is
the same over every project. `just test-e2e` runs both journey suites on their own.
Coverage floors are 95% lines: one report over every Rust crate's run (journeys
included), and per project for each Python project and the TypeScript SDK.
`just lint-llm*` is the judged lint, outside the deterministic gate.

## Journeys

Every journey file under the end-to-end tests, with what it proves.
`tools/tests/test_journeys.py` fails when this list and the files differ either way.

- `crates/onebudgetspec-e2e/tests/journeys/verdicts.rs` — within and over under `max` and `min`, equality is within, the computed figures, and exit statuses 0, 1 and 3.
- `crates/onebudgetspec-e2e/tests/journeys/elapsed.rs` — `elapsed` times a sleeping command at least its interval; a non-zero exit is an error.
- `crates/onebudgetspec-e2e/tests/journeys/errors.rs` — failing, timed-out, empty, non-JSON and non-numeric results are errors that win over an over-budget value.
- `crates/onebudgetspec-e2e/tests/journeys/detail.rs` — a command's `detail` is its result's, and null when it writes none.
- `crates/onebudgetspec-e2e/tests/journeys/measurement_order.rs` — each selected budget runs once, one at a time, in file order; unselected ones never run.
- `crates/onebudgetspec-e2e/tests/journeys/conditions.rs` — declared conditions run once per file per check and reach only that file's results.
- `crates/onebudgetspec-e2e/tests/journeys/returned_conditions.rs` — returned conditions sit in their own result; colliding or malformed ones make it an error.
- `crates/onebudgetspec-e2e/tests/journeys/command_environment.rs` — commands run from their file's directory, inherit the environment, get an empty result file and no shell.
- `crates/onebudgetspec-e2e/tests/journeys/output_streams.rs` — command output goes to stderr; stdout is only the report, in JSON and text.
- `crates/onebudgetspec-e2e/tests/journeys/host.rs` — host values of the contract's types, a failing condition as `unknown`, the file path and ordered RFC 3339 times.
- `crates/onebudgetspec-e2e/tests/journeys/files.rs` — `./budgets.yaml` by default, several named files, and a missing file refused with 2.
- `crates/onebudgetspec-e2e/tests/journeys/selection.rs` — `--id`, `--label` and `--exclude-label` together, unknown ids refused, an empty selection exits 0.
- `crates/onebudgetspec-e2e/tests/journeys/discovery.rs` — `--recursive` in path order, gitignored files skipped, ids unique across files, invalid nested files refused.
- `crates/onebudgetspec-e2e/tests/journeys/invalid_files.rs` — every invalid shape refused with 2 by `validate` and `check`, naming file and key, running nothing.
- `crates/onebudgetspec-e2e/tests/journeys/text_output.rs` — the text line for within, over and error, `unknown` values, and the same figures as the JSON.
- `crates/onebudgetspec-e2e/tests/journeys/json_flag.rs` — `--json` equals `--output json` on every verb.
- `crates/onebudgetspec-e2e/tests/journeys/list.rs` — `list` reports every field under each selection and `--recursive`, running nothing.
- `crates/onebudgetspec-e2e/tests/journeys/validate.rs` — `validate` accepts a well-formed file and runs nothing.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/cli_wheel.rs` — the `onebudgetspec-cli` wheel installs a binary that checks a case exactly as the cargo build does.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/npm_launcher.rs` — the launcher runs the host carrier like the cargo build, and refuses with 69 without it.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_python.rs` — the `onebudgetspec-sdk` wheel installs and imports at the release version, typed.
- `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_typescript.rs` — the packed `@onebudgetspec/sdk` installs and imports at the release version, with types.

`conformance/` holds data-only cases any implementation must pass; `crates/onebudgetspec/tests/conformance.rs`
runs them against the command named by `ONEBUDGETSPEC_CONFORMANCE_COMMAND` (default: the binary).

## Commits, releases, and merging

- **Squash-merge only, via PR, with auto-merge**; head branches auto-delete; admins may
  override. `just governance` applies this with create-repo's governance script, and
  `just governance --verify` reads it back.
- **Required checks today: exactly `check` (the aggregate of every gate lane and the
  supply chain) and `pr-title`.** llmlint becomes required when publishing is provisioned:
  until then the repository has no harness credential and the `llmlint` job is skipped.
  Provisioning re-runs `just governance` with `llmlint` added to its checks and
  `--allow-missing-llmlint` dropped.
- **Tiers:** pull requests run `just check` (affected); each merge to main runs
  `just check-all` once. The release workflows build and publish only.
- **Releases:** Conventional Commits drive release-plz (pre-1.0: `feat` bumps minor,
  `fix`/`perf` patch). On main it opens a release PR, bringing every non-Cargo manifest to
  the same version (`scripts/sync-release-pr.sh`); merging it tags `v<version>` and cuts
  the GitHub Release, which fires `release.yml`. `release-targets.toml` names the five
  targets (`crate`, `pypi`, `npm`, `sdk-pypi`, `sdk-npm`) and `scripts/release-probe.py`
  answers what each registry serves. Every artifact is built by `scripts/build-dist.sh`,
  the script the packaging journeys install from.
- **Secrets:** `CARGO_REGISTRY_TOKEN`, `PYPI_TOKEN`, `NPM_TOKEN`, `RELEASE_PLZ_TOKEN`,
  `CLAUDE_CODE_OAUTH_TOKEN`, `OPENAI_API_KEY`; variables `CARGO_PUBLISH`, `PYPI_PUBLISH`,
  `NPM_PUBLISH`. Every job needing one is skipped by a guard job (`scripts/ci-guard.sh`)
  until it exists, and each registry publishes only when its variable is `true`.
  `tools/tests/test_workflows.py` runs those guards locally. Never create a secret,
  variable or registry package from here; the user provisions them.
- **PRs follow the template**, `.github/pull_request_template.md`.

## Invariants

- No warnings-only mode; a suppression carries its reason at its site.
- Journeys drive the built binary and the installed packages over real files and real
  commands. Never double the layer under test.
- Validate inputs at trust boundaries; a budgets file is untrusted input.
- No secrets in the tree; grants stay least-privilege.
- Scripts are quiet on success and print the error and a next step on failure.

## Keeping the allowlist current

`.claude/settings.json` holds the agent allowlist. Add a routine command there rather
than re-approving it; keep it narrow.
