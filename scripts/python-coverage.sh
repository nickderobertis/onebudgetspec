#!/usr/bin/env bash
# The Python line-coverage floor, over target/python-coverage:
#
#   python-coverage.sh --clear            empty it, once, before the projects run
#   python-coverage.sh <project> <args>   pytest <args> in that project, data kept
#   python-coverage.sh --report           combine every project's data and hold it to 95%
#
# Quiet on success; on failure the report and what to do.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT
readonly DATA="$ROOT/target/python-coverage"
readonly MIN_LINES=95
readonly REQUEST="${1:?usage: scripts/python-coverage.sh --clear | <project-dir> <pytest args> | --report}"
shift

case "$REQUEST" in
  --clear)
    rm -rf "$DATA"
    mkdir -p "$DATA"
    ;;
  --report)
    # Nothing to hold to the floor when this invocation selected no Python suite.
    shopt -s nullglob
    data=("$DATA"/.coverage.*)
    [ ${#data[@]} -gt 0 ] || exit 0
    cd "$ROOT"
    if ! log="$(uv run --frozen --no-sync coverage combine --quiet --data-file="$DATA/.coverage" "$DATA" 2>&1 \
      && uv run --frozen --no-sync coverage report --data-file="$DATA/.coverage" --skip-covered --show-missing --fail-under="$MIN_LINES" 2>&1)"; then
      printf '%s\n' "$log" >&2
      echo "python-coverage: below ${MIN_LINES}% lines over every Python project's run, or no run to report on (above)." >&2
      echo "python-coverage: run 'just coverage'; cover listed lines with a test that drives the real behaviour." >&2
      exit 1
    fi
    ;;
  *)
    [ -d "$ROOT/$REQUEST" ] || {
      echo "python-coverage: $REQUEST is not a directory; pass a project directory relative to the repository root, such as sdks/python" >&2
      exit 64
    }
    mkdir -p "$DATA"
    cd "$ROOT/$REQUEST"
    name="$(echo "$REQUEST" | tr '/' '-')"
    if ! log="$(COVERAGE_FILE="$DATA/.coverage.$name" uv run --frozen --no-sync pytest -q -p no:cacheprovider --cov-report= "$@" 2>&1)"; then
      printf '%s\n' "$log" >&2
      echo "python-coverage: $REQUEST's tests failed under coverage; fix the failures above, then re-run." >&2
      exit 1
    fi
    ;;
esac
