#!/usr/bin/env bash
# Drive the installed CLIs over the release's real SDK-reporting case.
set -euo pipefail
verify_report() {
  node - "$1" <<'JS'
const fs=require('node:fs');
try {
  const report=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
  const record=value=>typeof value==='object' && value!==null && !Array.isArray(value);
  if(!record(report) || !Array.isArray(report.results) || !report.results.length || !record(report.results[0])) throw Error('invalid staged report shape');
  const result=report.results[0];
  if(typeof result.actual!=='number' || !Number.isFinite(result.actual) || typeof result.verdict!=='string') throw Error('invalid staged report fields');
  if(result.actual!==2 || result.verdict!=='over') throw Error('staged check disagrees: '+JSON.stringify(report));
} catch(error) {
  console.error('verify-installed: '+String(error)+'; rebuild the staged products and rerun native-products.sh');
  process.exit(1);
}
JS
}
if [ $# -eq 2 ] && [ "$1" = --report ]; then
  verify_report "$2"
  exit
fi
if [ $# -lt 4 ]; then
  echo 'verify-installed: usage: <expected version line> <case directory> <report JSON> <binary>...; pass the staged products' >&2
  exit 64
fi
expected="$1" case_dir="$2" report="$3"
shift 3
fail() {
  echo "verify-installed: $1; rebuild the staged products and rerun native-products.sh" >&2
  exit 1
}
trap 'echo "verify-installed: command failed at line $LINENO; fix its diagnostic and rebuild staged products" >&2' ERR
for binary in "$@"; do
  actual="$("$binary" --version)"
  [ "$actual" = "$expected" ] || fail "installation $binary reports '$actual', expected '$expected'"
  status=0
  (cd "$case_dir" && "$binary" check --json) > "$report" || status=$?
  if [ "$status" -ne 1 ]; then
    cat "$report" >&2
    fail "$binary check exited $status, expected an over-budget exit of 1"
  fi
  verify_report "$report"
done
