#!/usr/bin/env bash
# On the open release-plz pull request, bring every manifest release-plz does not write
# (the wheel, the npm launcher and carriers, both SDKs) to the version it chose, relock,
# and push. Nothing to do when no release pull request is open or it already agrees.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
trap 'echo "sync-release-pr: \"$BASH_COMMAND\" failed; the release PR is unchanged. Fix the cause above, then re-run the release-plz workflow." >&2' ERR

branch="$(gh pr list --json headRefName --jq 'map(select(.headRefName | startswith("release-plz-"))) | .[0].headRefName // empty')"
[ -n "$branch" ] || exit 0

git fetch --quiet origin "$branch"
git checkout --quiet -B "$branch" "origin/$branch"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
just set-version "$version"
uv lock --quiet
bun install --silent --lockfile-only
if git diff --quiet; then
  exit 0
fi
git -c user.name="github-actions[bot]" -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
  commit --quiet -am "chore: bring every manifest to v$version"
git push --quiet origin "$branch"
