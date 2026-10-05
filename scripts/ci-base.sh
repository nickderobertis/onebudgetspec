#!/usr/bin/env bash
# Print the commit affected selection compares against, derived explicitly: an implicit
# base is how selection quietly compares against the wrong commit.
#
#   pull request   (BASE_REF and BASE_SHA set)  the merge base with the base branch
#   push           (BEFORE_SHA set, readable)   the commit the push replaced
#   otherwise                                   HEAD's parent
#
# Writes `base=<sha>` to $GITHUB_OUTPUT when it is set, and the sha to stdout.
set -euo pipefail

if [ -n "${BASE_SHA:-}" ]; then
  [ -n "${BASE_REF:-}" ] || { echo "ci-base: BASE_SHA is set without BASE_REF; set both for a pull request" >&2; exit 64; }
  git fetch --quiet --no-tags origin "$BASE_REF" || {
    echo "ci-base: could not fetch $BASE_REF from origin (above); check that it is the pull request's base branch and that the checkout can reach origin, then re-run" >&2
    exit 1
  }
  base="$(git merge-base "$BASE_SHA" HEAD)" || {
    echo "ci-base: $BASE_SHA has no common ancestor with HEAD here; check out with fetch-depth: 0 so the history holds both, then re-run" >&2
    exit 1
  }
elif [ -n "${BEFORE_SHA:-}" ] && git rev-parse --verify --quiet "${BEFORE_SHA}^{commit}" >/dev/null; then
  base="$BEFORE_SHA"
elif base="$(git rev-parse --verify --quiet 'HEAD~1^{commit}')"; then
  :
else
  echo "ci-base: no base can be derived (no pull request base, no readable push predecessor, no parent); fetch with fetch-depth: 0" >&2
  exit 1
fi
[ -z "${GITHUB_OUTPUT:-}" ] || echo "base=$base" >>"$GITHUB_OUTPUT"
echo "$base"
