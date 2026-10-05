#!/usr/bin/env bash
# The Rust line-coverage floor, in three steps over cargo-llvm-cov's one profile directory:
#
#   rust-coverage.sh --clear     empty it, once, before the crates run
#   rust-coverage.sh <crate>     that crate's tests, instrumented, profiles kept
#   rust-coverage.sh --report    one report over every crate's run, held to 95% lines
#
# Each crate's Nx `test` target is its instrumented run and depends on the clear, so one
# invocation's report (rust-coverage:coverage) covers exactly the suites it ran.
#
# The journey and conformance crates spawn the binary, so their runs build the instrumented
# onebudgetspec beside them and what they cover counts. Quiet on success; on failure the
# reason and what to do.
set -euo pipefail

readonly REQUEST="${1:?usage: scripts/rust-coverage.sh --clear | <crate> | --report}"
readonly MIN_LINES=95
cd "$(dirname "${BASH_SOURCE[0]}")/.."

for tool in llvm-cov nextest; do
  if ! probe="$(cargo "$tool" --version 2>&1)"; then
    printf '%s\n' "$probe" >&2
    echo "rust-coverage: 'cargo $tool' did not run (above); install it with 'cargo binstall cargo-$tool', or fix the error shown, then re-run." >&2
    exit 1
  fi
done

quietly() {
  local log
  if ! log="$("$@" 2>&1)"; then
    printf '%s\n' "$log" >&2
    echo "rust-coverage: $2" >&2
    return 1
  fi
}

case "$REQUEST" in
  --clear)
    quietly cargo llvm-cov clean --workspace \
      || { echo "rust-coverage: could not clear target/llvm-cov-target; fix the error above, then re-run." >&2; exit 1; }
    ;;
  --report)
    # Nothing to hold to the floor when this invocation selected no Rust suite.
    shopt -s nullglob
    profiles=(target/llvm-cov-target/*.profraw)
    [ ${#profiles[@]} -gt 0 ] || exit 0
    if ! report="$(cargo llvm-cov report --summary-only --show-missing-lines --fail-under-lines "$MIN_LINES" 2>&1)"; then
      printf '%s\n' "$report" >&2
      echo "rust-coverage: below ${MIN_LINES}% lines over every crate's run, or no run to report on (above)." >&2
      echo "rust-coverage: run 'just coverage'; cover listed lines with a test that drives the real behaviour." >&2
      exit 1
    fi
    ;;
  onebudgetspec-core | onebudgetspec | onebudgetspec-e2e | onebudgetspec-conformance)
    # The binary the journeys and the conformance cases spawn, built instrumented where
    # they look for it (beside their own test executables).
    if [ "$REQUEST" != onebudgetspec-core ] \
      && ! build="$(cargo llvm-cov --no-report run --locked --quiet -p onebudgetspec -- --version 2>&1)"; then
      printf '%s\n' "$build" >&2
      echo "rust-coverage: the instrumented onebudgetspec did not build; fix the error above, then re-run." >&2
      exit 1
    fi
    if ! run="$(cargo llvm-cov --no-report nextest --locked --no-tests=pass --package "$REQUEST" 2>&1)"; then
      printf '%s\n' "$run" >&2
      echo "rust-coverage: $REQUEST's tests failed under instrumentation; fix the failures above, then re-run." >&2
      exit 1
    fi
    ;;
  *)
    echo "rust-coverage: '$REQUEST' is not a crate this floor measures; pass one of onebudgetspec-core, onebudgetspec, onebudgetspec-e2e, onebudgetspec-conformance, or --clear or --report." >&2
    exit 64
    ;;
esac
