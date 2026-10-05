#!/usr/bin/env bash
# Publish one release target from artifacts scripts/build-dist.sh built, skipping anything
# the registry already serves at this version so a re-run after a partial failure resumes
# rather than fails. Called by .github/workflows/release.yml, whose guards decide whether
# it runs at all; here a missing token is an error, because reaching this means publication
# was switched on.
#
# Usage: scripts/publish.sh crate
#        scripts/publish.sh pypi|sdk-pypi <wheel-dir>
#        scripts/publish.sh npm <carrier-dir> <launcher-dir>
#        scripts/publish.sh sdk-npm <package-dir>
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT

fail() {
  echo "publish: $1" >&2
  exit 1
}
need() {
  [ -n "${!1:-}" ] || fail "$1 is not set; provision it as a repository secret (AGENTS.md, Releasing)"
}

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
readonly VERSION
[ -n "$VERSION" ] || fail "no [workspace.package] version in Cargo.toml"

npm_publish() {
  local tarball="$1" name
  name="$(tar -xOzf "$tarball" package/package.json | python3 -c 'import json, sys; print(json.load(sys.stdin)["name"])')"
  if [ -n "$(npm view "$name@$VERSION" version 2>/dev/null)" ]; then
    echo "publish: $name@$VERSION is already on npm; skipping"
    return
  fi
  npm publish "$tarball" --access public --userconfig "$NPMRC"
}

case "${1:-}" in
  crate)
    need CARGO_REGISTRY_TOKEN
    for crate in onebudgetspec-core onebudgetspec; do
      if curl -fsS -o /dev/null -A "onebudgetspec-release (+https://github.com/nickderobertis/onebudgetspec)" \
        "https://crates.io/api/v1/crates/$crate/$VERSION"; then
        echo "publish: $crate $VERSION is already on crates.io; skipping"
      else
        cargo publish --locked -p "$crate" --manifest-path "$ROOT/Cargo.toml"
      fi
    done
    ;;
  pypi | sdk-pypi)
    need PYPI_TOKEN
    [ -d "${2:-}" ] || fail "name the directory holding the wheels"
    name=onebudgetspec-cli
    [ "$1" = pypi ] || name=onebudgetspec-sdk
    UV_PUBLISH_TOKEN="$PYPI_TOKEN" uv publish --check-url "https://pypi.org/simple/$name/" "$2"/*.whl
    ;;
  npm | sdk-npm)
    need NPM_TOKEN
    NPMRC="$(mktemp)"
    trap 'rm -f "$NPMRC"' EXIT
    printf '//registry.npmjs.org/:_authToken=%s\n' "$NPM_TOKEN" >"$NPMRC"
    shift
    [ $# -gt 0 ] || fail "name the directories holding the packed packages, carriers first"
    # In argument order: the carriers must exist before the launcher that pins them.
    for dir in "$@"; do
      for tarball in "$dir"/*.tgz; do npm_publish "$tarball"; done
    done
    ;;
  *) fail "usage: scripts/publish.sh crate | pypi|sdk-pypi <dir> | npm <carriers> <launcher> | sdk-npm <dir>" ;;
esac
