#!/usr/bin/env bash
# Build a wheel once, reuse its binary (stripped by the release profile) in the carrier and the
# Cargo install, and prove local installs.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$root"
if [ $# -ne 3 ]; then
  echo 'native-products: usage: <Rust target> <output directory> <launcher tarball>' >&2
  exit 64
fi
target="$1"
fail() {
  echo "native-products: $1; rebuild $target with scripts/build-dist.sh, then rerun native-products.sh" >&2
  exit 1
}
trap 'echo "native-products: command failed at line $LINENO for $target; fix the diagnostic above, rebuild the target and rerun native-products.sh" >&2' ERR
mkdir -p "$2"
out="$(cd "$2" && pwd)"
launcher="$3"
node_cpu="$(node -p process.arch)"
case "$target" in
  x86_64-*) expected_cpu=x64 ;;
  aarch64-*) expected_cpu=arm64 ;;
  *) expected_cpu="$node_cpu" ;;
esac
[ "$node_cpu" = "$expected_cpu" ] || fail "Node architecture $node_cpu != $expected_cpu; select the Node build for the target's architecture"
python="${ONEBUDGETSPEC_PYTHON:-python3}"
"$python" scripts/release/native/verify-interpreter.py "$expected_cpu"
wheel="$(bash scripts/build-dist.sh cli-wheel "$out/wheels" "$target")"
mkdir -p "$out/staged"
# Wheel scripts are the compiled CLI, not Python console entry points.
python3 - "$wheel" "$out/staged/onebudgetspec" <<'PY'
import pathlib
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1]) as wheel:
    scripts = [name for name in wheel.namelist() if name.endswith('/scripts/onebudgetspec')]
    if len(scripts) != 1:
        raise ValueError('wheel must contain exactly one CLI script')
    pathlib.Path(sys.argv[2]).write_bytes(wheel.read(scripts[0]))
PY
chmod +x "$out/staged/onebudgetspec"
carrier="$(ONEBUDGETSPEC_BINARY="$out/staged/onebudgetspec" bash scripts/build-dist.sh npm-carrier "$out/carriers" "$target")"
# All installed products must answer the same release version and a real failing budget.
expected="$("$out/staged/onebudgetspec" --version)"
if [ -n "${TAG:-}" ] && [ "$expected" != "onebudgetspec ${TAG#v}" ]; then
  fail "tag $TAG disagrees with compiled version '$expected'; reconcile the tag and workspace version"
fi
uv venv --quiet --python "$python" "$out/venv"
uv pip install --quiet --offline --no-deps --python "$out/venv/bin/python" "$wheel"
mkdir -p "$out/npm"
printf '{"name":"release-consumer","private":true}\n' > "$out/npm/package.json"
(cd "$out/npm" && npm install --offline --ignore-scripts --no-audit --no-fund --silent "$launcher" "$carrier")
CARGO_TARGET_DIR="$root/target" cargo install --quiet --message-format json-render-diagnostics --path crates/onebudgetspec --target "$target" --root "$out/cargo" --locked --offline > "$out/cargo-build.jsonl"
python3 scripts/release/native/verify-fresh.py "$out/cargo-build.jsonl"
mkdir -p "$out/case"
./scripts/nx run sdk-typescript:build > "$out/sdk-build.log" 2>&1 \
  || { cat "$out/sdk-build.log" >&2; fail "sdk-typescript:build failed (output above)"; }
node --input-type=module - "$root/sdks/typescript/dist/index.js" "$out/case/budgets.yaml" <<'JS'
import {writeFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
const reporter = pathToFileURL(process.argv[2]).href;
writeFileSync(process.argv[3], JSON.stringify({schema_version:1,budgets:[{
  id:'staged-proof',measure:'reported',command:['node','--input-type=module','-e',`import(${JSON.stringify(reporter)}).then(({report})=>report(2))`],
  unit:'requests',direction:'max',threshold:1,
}]}));
JS
bash scripts/release/native/verify-installed.sh "$expected" "$out/case" "$out/report.json" "$out/venv/bin/onebudgetspec" "$out/npm/node_modules/.bin/onebudgetspec" "$out/cargo/bin/onebudgetspec"
python3 scripts/release/native/validate-products.py "$wheel" "$carrier" "$out/staged/onebudgetspec" "$out/payload-hashes.json" "$out/cargo/bin/onebudgetspec" "$target" "${expected#onebudgetspec }"
