#!/usr/bin/env bash
# Set up a clean clone: the locked Node, Python and Rust dependencies. Idempotent and quiet.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

run() {
  local log
  if ! log="$("$@" 2>&1)"; then
    printf '%s\n' "$log" >&2
    echo "bootstrap: '$*' failed (above); fix it, then re-run 'just bootstrap'." >&2
    exit 1
  fi
}

for tool in cargo uv bun; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "bootstrap: $tool is not installed; install it (cargo: https://rustup.rs, uv: https://docs.astral.sh/uv/, bun: https://bun.sh), then re-run 'just bootstrap'." >&2
    exit 1
  }
done
run bun install --frozen-lockfile
run uv sync --frozen --all-packages
run cargo fetch --locked

# The pruner `just pool-maintenance` runs, at the one version pinned here; skipped when that
# version is already the one on PATH.
readonly PRUNE_SUPERSEDED_VERSION=0.1.1
if [ "$(cargo-prune-superseded --version 2>/dev/null)" != "cargo-prune-superseded $PRUNE_SUPERSEDED_VERSION" ]; then
  run cargo install --locked --quiet cargo-prune-superseded --version "$PRUNE_SUPERSEDED_VERSION"
fi
