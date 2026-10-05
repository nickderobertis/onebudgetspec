#!/usr/bin/env bash
# Decide whether a credentialed CI job runs: writes `enabled=true` to $GITHUB_OUTPUT when
# the named environment variables are set (all of them, or with --any at least one), and
# `enabled=false` with a notice naming what is missing otherwise. It always exits 0 on a
# well-formed call, so a repository that has not been provisioned yet skips the job that
# reads this output instead of failing it. The secrets reach this step as environment
# variables, because a job-level `if:` cannot read `secrets` itself.
#
# Usage: scripts/ci-guard.sh [--any] NAME...
set -euo pipefail

mode=all
if [ "${1:-}" = "--any" ]; then
  mode=any
  shift
fi
if [ $# -eq 0 ]; then
  echo "ci-guard: name at least one environment variable, e.g. 'scripts/ci-guard.sh NPM_TOKEN'" >&2
  exit 64
fi
if [ -z "${GITHUB_OUTPUT:-}" ]; then
  echo "ci-guard: GITHUB_OUTPUT is not set; run this as a GitHub Actions step, or set it to a file" >&2
  exit 64
fi

for name in "$@"; do
  if ! [[ "$name" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
    echo "ci-guard: '$name' is not an environment variable name; pass names such as NPM_TOKEN" >&2
    exit 64
  fi
done

present=()
missing=()
for name in "$@"; do
  if [ -n "${!name:-}" ]; then present+=("$name"); else missing+=("$name"); fi
done

if { [ "$mode" = all ] && [ ${#missing[@]} -eq 0 ]; } || { [ "$mode" = any ] && [ ${#present[@]} -gt 0 ]; }; then
  echo "enabled=true" >>"$GITHUB_OUTPUT"
else
  echo "enabled=false" >>"$GITHUB_OUTPUT"
  echo "::notice::skipping: ${missing[*]} not provisioned for this repository"
fi
