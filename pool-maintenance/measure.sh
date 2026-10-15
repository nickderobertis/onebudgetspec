#!/usr/bin/env bash
# Measures onebudgetspec-pool-maintenance-seconds: one steady-state pass of the declared
# pool maintenance (`just pool-maintenance`), the pass after one ordinary run's worth of new
# superseded builds, as the pruner itself reports it.
#
#   1. The ordinary build, then one declared pass, untimed, which clears any backlog,
#      including whatever that build superseded; its seconds go in the detail only.
#   2. The workspace's test targets, built as the gate's uninstrumented lane builds them
#      (`cargo test --no-run`, the build `cargo nextest run` makes) but with every workspace
#      crate's overflow checks off, which gives this repository's own crates (and nothing
#      else) new metadata hashes. Exactly the entries that build created are backdated past
#      the 2-day window, standing in for a run's builds that have since been superseded.
#   3. The ordinary build again, so each unit's newest build is the ordinary one.
#   4. One declared pass, timed: the pruner reports its own seconds and breakdown to
#      ONEBUDGETSPEC_RESULT, and that pass alone is the figure.
#
# MEASURE_EVIDENCE_DIR, when set, receives the entry lists it compares (by name and metadata
# hash): before, alternative, built, ordinary and removed.
#
# It fails, rather than reporting, unless that pass removed some of step 2's entries and
# nothing else of target/debug, so the figure always measures the workload it states.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

readonly PROFILE=target/debug
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

fail() {
  echo "measure: $1" >&2
  exit 1
}

# Every entry of the profile's build containers, one `<container>/<name>` per line, sorted.
entries() {
  local container
  for container in deps .fingerprint build incremental; do
    [ ! -d "$PROFILE/$container" ] || (cd "$PROFILE/$container" && ls -A) | sed "s|^|$container/|"
  done | LC_ALL=C sort
}

build_tests() {
  cargo test --workspace --locked --no-run --quiet "$@" >&2 \
    || fail "the workspace's test targets did not build (above); fix the build, then re-run."
}

build_tests
ONEBUDGETSPEC_RESULT="$scratch/backlog.json" just pool-maintenance >&2 \
  || fail "the untimed backlog pass failed (above); run 'just bootstrap' if the pruner is missing."

alternative=()
for crate in $(cargo metadata --no-deps --locked --format-version 1 \
  | node -e 'for (const p of JSON.parse(require("fs").readFileSync(0, "utf8")).packages) console.log(p.name)'); do
  alternative+=(--config "profile.dev.package.$crate.overflow-checks=false")
done
entries >"$scratch/before"
build_tests "${alternative[@]}"
entries | LC_ALL=C comm -13 "$scratch/before" - >"$scratch/alternative"
[ -s "$scratch/alternative" ] || fail "the alternative build created no entries in $PROFILE."
while IFS= read -r entry; do
  find "$PROFILE/$entry" -exec touch -h -t 200101010000 {} +
done <"$scratch/alternative"

build_tests
entries >"$scratch/built"
LC_ALL=C comm -23 "$scratch/built" "$scratch/alternative" >"$scratch/ordinary"

timed="${ONEBUDGETSPEC_RESULT:-$scratch/timed.json}"
ONEBUDGETSPEC_RESULT="$timed" just pool-maintenance >&2 \
  || fail "the timed pass failed (above)."

entries | LC_ALL=C comm -23 "$scratch/built" - >"$scratch/removed"
[ -z "${MEASURE_EVIDENCE_DIR:-}" ] \
  || cp "$scratch"/{before,alternative,built,ordinary,removed} "$MEASURE_EVIDENCE_DIR"/
stray="$(LC_ALL=C comm -23 "$scratch/removed" "$scratch/alternative")"
[ -z "$stray" ] || fail "the timed pass removed entries the alternative build did not create: $stray"
[ -s "$scratch/removed" ] || fail "the timed pass removed none of the alternative build's entries."
echo "measure: removed $(wc -l <"$scratch/removed" | tr -d ' ') of the alternative build's $(wc -l <"$scratch/alternative" | tr -d ' ') entries; all $(wc -l <"$scratch/ordinary" | tr -d ' ') entries of the ordinary build kept" >&2

# The backlog pass's seconds join the detail as separate evidence; the value is untouched.
node - "$timed" "$scratch/backlog.json" <<'EOF'
const fs = require("fs");
const [timed, backlog] = process.argv.slice(2);
const result = JSON.parse(fs.readFileSync(timed, "utf8"));
const before = JSON.parse(fs.readFileSync(backlog, "utf8"));
result.detail = `${result.detail}; untimed backlog pass seconds (not in the figure): ${before.value}`;
fs.writeFileSync(timed, JSON.stringify(result));
EOF
[ -n "${ONEBUDGETSPEC_RESULT:-}" ] || cat "$timed"
