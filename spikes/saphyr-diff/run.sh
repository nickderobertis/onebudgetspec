#!/usr/bin/env bash
# The one command: collect every budgets document the e2e journeys write, then print every
# difference between serde_norway and serde-saphyr / saphyr in what is read, written and
# refused. Extra arguments are budgets.yaml files outside this repository, read in place.
#
#   spikes/saphyr-diff/run.sh [external budgets.yaml ...]
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
corpus="$(mktemp -d)"
trap 'rm -rf "$corpus"' EXIT
cd "$root"
cargo build --quiet -p onebudgetspec
if ! ONEBUDGETSPEC_SPIKE_CORPUS="$corpus" cargo test --quiet -p onebudgetspec-e2e >"$corpus/journeys.log" 2>&1; then
  cat "$corpus/journeys.log" >&2
  echo "error: the journeys failed while collecting the corpus; fix them, then re-run" >&2
  exit 1
fi
cargo run --quiet --release --manifest-path spikes/saphyr-diff/Cargo.toml -- "$root" "$corpus" "$@"
