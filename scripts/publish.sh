#!/usr/bin/env bash
# Publish one release target from artifacts scripts/build-dist.sh built, skipping anything
# the registry already serves at this version so a re-run after a partial failure resumes
# rather than fails. Called by .github/workflows/release.yml, whose guards decide whether
# it runs at all; here a missing token is an error, because reaching this means publication
# was switched on. Every artifact is checked against the target it is published under and
# the workspace version before anything is uploaded.
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
  echo "publish: next: $2" >&2
  exit 1
}
need() {
  [ -n "${!1:-}" ] || fail "$1 is not set" "provision the $1 repository secret (AGENTS.md, Commits, releases, and merging), then re-run the release workflow"
}
usage() {
  fail "$1" "run 'scripts/publish.sh crate | pypi|sdk-pypi <dir> | npm <carriers> <launcher> | sdk-npm <dir>'"
}

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
readonly VERSION
[ -n "$VERSION" ] || fail "Cargo.toml has no [workspace.package] version line" "restore 'version = \"X.Y.Z\"' under [workspace.package] in Cargo.toml"

skipped=()

# The packages an npm target may publish, so a stray tarball is refused, not uploaded.
npm_expected() {
  case "$1" in
    npm) echo "@onebudgetspec/cli-linux-x64 @onebudgetspec/cli-linux-arm64 @onebudgetspec/cli-darwin-x64 @onebudgetspec/cli-darwin-arm64 @onebudgetspec/cli" ;;
    sdk-npm) echo "@onebudgetspec/sdk" ;;
  esac
}

npm_publish() {
  local target="$1" tarball="$2" identity name version
  identity="$(tar -xOzf "$tarball" package/package.json | python3 -c 'import json, sys; m = json.load(sys.stdin); print(m["name"], m["version"])')" \
    || fail "$tarball holds no readable package/package.json" "rebuild it with scripts/build-dist.sh"
  name="${identity% *}"
  version="${identity#* }"
  case " $(npm_expected "$target") " in
    *" $name "*) ;;
    *) fail "$tarball is $name, which target $target does not publish" "publish only what scripts/build-dist.sh built for $target" ;;
  esac
  [ "$version" = "$VERSION" ] || fail "$tarball is $name@$version, not the workspace's $VERSION" "rebuild it from this commit with scripts/build-dist.sh"
  if [ -n "$(npm view "$name@$VERSION" version 2>/dev/null)" ]; then
    skipped+=("$name")
    return
  fi
  npm publish "$tarball" --access public --userconfig "$NPMRC"
}

# llmlint: ignore-block[changed_behavior_has_e2e] the uploads below reach crates.io, PyPI and npm with tokens this repository is not given until publishing is provisioned; every refusal before an upload is driven by tools/tests/test_scripts.py.
target="${1:-}"
[ $# -gt 0 ] && shift
case "$target" in
  crate)
    [ $# -eq 0 ] || usage "crate takes no directory"
    need CARGO_REGISTRY_TOKEN
    for crate in onebudgetspec-core onebudgetspec; do
      if curl -fsS -o /dev/null -A "onebudgetspec-release (+https://github.com/nickderobertis/onebudgetspec)" \
        "https://crates.io/api/v1/crates/$crate/$VERSION" 2>/dev/null; then
        skipped+=("$crate")
      else
        cargo publish --locked -p "$crate" --manifest-path "$ROOT/Cargo.toml"
      fi
    done
    ;;
  pypi | sdk-pypi)
    [ $# -eq 1 ] && [ -d "$1" ] || usage "$target takes the directory holding its wheels"
    need PYPI_TOKEN
    prefix=onebudgetspec_cli
    [ "$target" = pypi ] || prefix=onebudgetspec_sdk
    for wheel in "$1"/*.whl; do
      case "$(basename "$wheel")" in
        "$prefix-$VERSION-"*) ;;
        *) fail "$wheel is not a $prefix $VERSION wheel" "publish only what scripts/build-dist.sh built for $target at this commit" ;;
      esac
    done
    name="${prefix//_/-}"
    UV_PUBLISH_TOKEN="$PYPI_TOKEN" uv publish --quiet --check-url "https://pypi.org/simple/$name/" "$1"/*.whl
    ;;
  npm | sdk-npm)
    [ $# -gt 0 ] || usage "$target takes the directories holding its packed packages, carriers first"
    need NPM_TOKEN
    NPMRC="$(mktemp)"
    trap 'rm -f "$NPMRC"' EXIT
    printf '//registry.npmjs.org/:_authToken=%s\n' "$NPM_TOKEN" >"$NPMRC"
    # In argument order: the carriers must exist before the launcher that pins them.
    for dir in "$@"; do
      for tarball in "$dir"/*.tgz; do npm_publish "$target" "$tarball"; done
    done
    ;;
  *) usage "unknown target '${target}'" ;;
esac

# llmlint: ignore-end[changed_behavior_has_e2e]

if [ ${#skipped[@]} -gt 0 ]; then
  echo "publish: already published at $VERSION, skipped: ${skipped[*]}"
fi
