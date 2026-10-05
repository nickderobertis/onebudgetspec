# The one command surface. Every gate recipe delegates to Nx: `affected` for what a change
# can reach (against NX_BASE, nx.json's defaultBase origin/main unless CI derives one),
# `run-many` for a full sweep. scripts/nx installs the locked Node toolchain first in a
# clone that has none, so every recipe works from a clean clone.

set shell := ["bash", "-uc"]
# Recipe arguments reach the shell as "$1", "$@", never spliced into its source.
set positional-arguments


# List available recipes.
default:
    @just --list

# Set up from a clean clone: the locked Node, Python and Rust dependencies.
bootstrap:
    @./scripts/nx run workspace:bootstrap

# The full gate over the affected projects: formatting, lint, types, tests (unit, journeys,
# conformance, packaging journeys) and the coverage floors over the tests it ran. Fails on
# any issue.
check: format-check lint typecheck test

# The broader tier: the same gate over every project. CI runs it on the release pull request.
check-all:
    @./scripts/nx run-many -t format-check lint typecheck --all
    @./scripts/nx run-many -t test coverage --all

# Tests for the affected projects, and the coverage floors over them, in one run: every
# instrumented test runs once, after the coverage data is cleared, and each floor (95% lines
# for Rust and for Python, over the union of those runs) reports after its tests. Each
# TypeScript project's test holds its own 95%.
test:
    @./scripts/nx affected -t test coverage

# The end-to-end tier on its own: the journeys and the packaging journeys, whatever changed.
test-e2e:
    @./scripts/nx run-many -t test -p onebudgetspec-e2e onebudgetspec-packaging-e2e

# The same run as `test`: the floors are only meaningful over the tests they measure.
coverage: test

# Lint for the affected projects (clippy -D warnings, ruff, biome, actionlint).
lint:
    @./scripts/nx affected -t lint

# Type check the affected projects.
typecheck:
    @./scripts/nx affected -t typecheck

# Check formatting without changing anything, for the affected projects.
format-check:
    @./scripts/nx affected -t format-check

# Format every project in place.
format:
    @./scripts/nx run-many -t format --all

# Supply chain: licences, bans, advisories and sources (cargo-deny), and unused crates.
deny:
    @./scripts/nx run workspace:deny

# Upgrade every ecosystem's dependencies, then re-run the whole gate on the result.
upgrade:
    @./scripts/nx run workspace:upgrade
    @just check-all

# Print the JSON Schema bundle the contract is emitted as.
# llmlint: ignore[tool_output_is_signal] stdout is the schema bundle itself, consumed by generators.
schema:
    @cargo run --quiet -p onebudgetspec -- schema

# Regenerate both SDKs' models from the binary's `onebudgetspec schema` (building it first).
# `just lint` fails while they and the schema part.
generate:
    @./scripts/nx run-many -t generate -p sdk-python sdk-typescript

# Every version agrees with the workspace's; `just set-version X` writes X everywhere.
versions:
    @uv run --quiet --frozen --package onebudgetspec-repo-checks python -m repo_checks.versions check

set-version version:
    @uv run --quiet --frozen --package onebudgetspec-repo-checks python -m repo_checks.versions set "$1"

# Branch protection on main requiring exactly `check` and `pr-title`, with the squash-only
# merge model. llmlint joins the required set when publishing is provisioned (AGENTS.md).
# Extra flags pass through: `--verify` reads the live state back; `--dry-run --repo
# nickderobertis/onebudgetspec --branch main` prints what it would apply, offline.
# llmlint: ignore[tool_output_is_signal] the governance script's report is the answer the caller asked for.
# The script is create-repo's (dero-skills), read from the user-scope skill install or
# CREATE_REPO_SKILL_DIR.
governance *flags:
    @script="${CREATE_REPO_SKILL_DIR:-$HOME/.claude/skills/create-repo}/scripts/setup_github_governance.py"; \
        test -f "$script" || { echo "governance: $script not found; install the create-repo skill or set CREATE_REPO_SKILL_DIR" >&2; exit 1; }; \
        uv run --quiet --script "$script" check pr-title --allow-missing-llmlint "$@"

# Show the project graph Nx selects against.
graph:
    @./scripts/nx graph --file=.nx/graph.html

# llmlint: ignore[tool_output_is_signal] a session-startup installer logs each step and continues rather than blocking startup.
# Provision the dev toolchain for a session (runs from the SessionStart hook; idempotent).
session-setup:
    ./scripts/session-setup.sh

# llmlint: ignore[tool_output_is_signal] a session-startup installer logs each step and continues rather than blocking startup.
# Install or refresh the judged-lint toolchain (oneharness + llmlint). Idempotent.
setup-llmlint:
    ./scripts/setup-llmlint.sh

# Out of the deterministic gate: these need an authenticated harness and the network.

# LLM-judge lint over the configured set (or the paths given).
lint-llm *paths:
    @command -v llmlint >/dev/null 2>&1 || { echo "llmlint not installed — run 'just setup-llmlint'" >&2; exit 1; }
    @llmlint "$@"

# Model-free llmlint gate (config, suppressions, versions). CI runs it before the judged lint.
lint-llm-validate *args:
    @command -v llmlint >/dev/null 2>&1 || { echo "llmlint not installed — run 'just setup-llmlint'" >&2; exit 1; }
    @llmlint validate "$@"

# The judged lint over what this branch changed since it forked from the base.
lint-llm-diff base="origin/main" *args:
    @command -v llmlint >/dev/null 2>&1 || { echo "llmlint not installed — run 'just setup-llmlint'" >&2; exit 1; }
    @llmlint --diff --diff-base "$1" "${@:2}"
