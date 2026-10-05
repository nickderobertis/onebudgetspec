#!/usr/bin/env bash
# The Rust line-coverage floor: every crate's tests and the journeys, instrumented, in one
# report held to 95% lines. The journeys spawn the instrumented binary, so what they cover
# counts. The packaging journeys are left out: they build release artifacts, which carry
# no instrumentation. Quiet on success; on failure the uncovered lines and what to do.
set -euo pipefail

readonly MIN_LINES=95
cd "$(dirname "${BASH_SOURCE[0]}")/.."

for tool in llvm-cov nextest; do
  if ! cargo "$tool" --version >/dev/null 2>&1; then
    echo "rust-coverage: cargo-$tool is not installed; install it with 'cargo binstall cargo-$tool', then re-run." >&2
    exit 1
  fi
done

if ! report="$(cargo llvm-cov nextest --workspace --exclude onebudgetspec-packaging-e2e --locked \
  --summary-only --show-missing-lines --fail-under-lines "$MIN_LINES" 2>&1)"; then
  printf '%s\n' "$report" >&2
  echo "rust-coverage: a test failed, or the workspace is below ${MIN_LINES}% line coverage (above)." >&2
  echo "rust-coverage: fix the failure, or cover the listed lines with a test that drives the real behaviour." >&2
  exit 1
fi
