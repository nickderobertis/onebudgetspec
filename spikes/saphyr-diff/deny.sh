#!/usr/bin/env bash
# What this repository's deny.toml says once serde_norway is replaced: a throwaway copy of
# the committed workspace, serde_norway swapped for serde-saphyr and saphyr in the
# workspace and in both crates that use it, then `cargo deny check` (as `just deny` runs
# it) and a search of the resolved graph for every serde_yaml-family crate. The copy is
# never compiled, so the swap needs no code change to be judged.
#
#   spikes/saphyr-diff/deny.sh
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
copy="$(mktemp -d)"
trap 'rm -rf "$copy"' EXIT
git -C "$root" archive HEAD | tar -x -C "$copy"
cd "$copy"
sed -i 's/^serde_norway = .*/serde-saphyr = "=1.3.0"\nsaphyr = "=0.2.1"/' Cargo.toml
for manifest in crates/onebudgetspec-core/Cargo.toml crates/onebudgetspec-e2e/Cargo.toml; do
  sed -i 's/^serde_norway.workspace = true/serde-saphyr.workspace = true\nsaphyr.workspace = true/' "$manifest"
done
if grep -rn "serde_norway" Cargo.toml crates/*/Cargo.toml; then
  echo "error: serde_norway is still named above; extend the swap" >&2
  exit 1
fi
cargo update --quiet --workspace 2>/dev/null || cargo generate-lockfile --quiet
echo "== cargo deny check (this repository's deny.toml), serde_norway replaced =="
status=0
cargo deny --all-features check 2>&1 | grep -v "^\s*$" | tail -40 || true
cargo deny --all-features check >/dev/null 2>&1 || status=$?
echo "cargo deny exit status: $status"
echo
echo "== serde_yaml-family crates in the resolved graph =="
banned=0
for name in serde_yaml serde_yaml_ng serde_norway serde_yml unsafe-libyaml unsafe-libyaml-norway libyml; do
  if cargo tree --workspace --all-features --target all -e all -i "$name" >/dev/null 2>&1; then
    echo "PRESENT: $name"; banned=1
  else
    echo "absent:  $name"
  fi
done
echo
echo "== what serde-saphyr and saphyr add (normal edges, all targets) =="
cargo tree --workspace --target all -e normal -p onebudgetspec-core --prefix none 2>/dev/null \
  | sort -u > all.txt
for top in serde-saphyr saphyr; do
  echo "-- $top"; cargo tree --target all -e normal -p onebudgetspec-core -i "$top" --prefix none 2>/dev/null | head -1
  cargo tree --target all -e normal --prefix none -p "$top" 2>/dev/null | sort -u | sed 's/ (\*)//' | sort -u | sed 's/^/   /'
done
echo
echo "== licences of those crates =="
cargo metadata --format-version 1 --all-features 2>/dev/null | python3 -c '
import json, sys
meta = json.load(sys.stdin)
pkgs = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
seen, stack = set(), [i for i, p in pkgs.items() if p["name"] in ("serde-saphyr", "saphyr")]
while stack:
    i = stack.pop()
    if i in seen: continue
    seen.add(i)
    stack += [d["pkg"] for d in nodes[i]["deps"] if any(k["kind"] is None for k in d["dep_kinds"])]
for i in sorted(seen, key=lambda i: pkgs[i]["name"]):
    p = pkgs[i]
    print("   %s %s: %s" % (p["name"], p["version"], p["license"]))
'
exit $(( status != 0 || banned != 0 ))
