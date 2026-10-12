#!/usr/bin/env bash
# The one command: collect every budgets document the e2e journeys write, then print every
# difference between serde_norway and serde-saphyr / saphyr in what is read, written and
# refused. Extra arguments are budgets.yaml files outside this repository, read in place.
#
#   spikes/saphyr-diff/run.sh [external budgets.yaml ...]
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
# Temporary files go under this dispatch's scratch directory when one is given, else under
# the clone's own gitignored target/; never the system temporary directory.
scratch="${ONEPIPELINE_NODE_SCRATCH_DIR:-$root/target}/saphyr-diff"
mkdir -p "$scratch/tmp" "$scratch/cargo-home"
# So do the tools': the journeys' and rustc's temporary files through TMPDIR, the registry
# and cargo-deny's advisory database through CARGO_HOME, and no rustc wrapper, since a
# ~/.cargo/config.toml above the clone may route rustc through a shared compile cache.
export TMPDIR="$scratch/tmp" CARGO_HOME="$scratch/cargo-home" RUSTC_WRAPPER=""
corpus="$(mktemp -d "$scratch/corpus.XXXXXX")"
trap 'rm -rf "$corpus"' EXIT
cd "$root"
cargo build --quiet -p onebudgetspec
if ! ONEBUDGETSPEC_SPIKE_CORPUS="$corpus" cargo test --quiet -p onebudgetspec-e2e >"$corpus/journeys.log" 2>&1; then
  cat "$corpus/journeys.log" >&2
  echo "error: the journeys failed while collecting the corpus; fix them, then re-run" >&2
  exit 1
fi
cargo run --quiet --release --manifest-path spikes/saphyr-diff/Cargo.toml \
  --target-dir "$root/target/saphyr-diff-harness" -- "$root" "$corpus" "$@"
