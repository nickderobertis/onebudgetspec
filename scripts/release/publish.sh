#!/usr/bin/env bash
# Publish one release target from artifacts scripts/build-dist.sh built, skipping anything
# the registry already serves at this version so a re-run after a partial failure resumes
# rather than fails. Called by .github/workflows/release.yml, whose guards decide whether
# it runs at all; here a missing token is an error, because reaching this means publication
# was switched on. Each artifact is checked against the target it is published under and
# the workspace version before it is uploaded.
#
# Usage: scripts/release/publish.sh crate
#        scripts/release/publish.sh pypi|sdk-pypi <wheel-dir>
#        scripts/release/publish.sh npm <carrier-dir> <launcher-dir>
#        scripts/release/publish.sh sdk-npm <package-dir>
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
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
  fail "$1" "run 'scripts/release/publish.sh crate | pypi|sdk-pypi <dir> | npm <carriers> <launcher> | sdk-npm <dir>'"
}

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
readonly VERSION
[ -n "$VERSION" ] || fail "Cargo.toml has no [workspace.package] version line" "restore 'version = \"X.Y.Z\"' under [workspace.package] in Cargo.toml"

skipped=()

# The declaration of what each target publishes; ONEBUDGETSPEC_RELEASE_TARGETS points
# elsewhere, as the tests do.
readonly DECLARATION="${ONEBUDGETSPEC_RELEASE_TARGETS:-$ROOT/release-targets.toml}"

# The packages a target publishes, read from the declaration (its id and what it covers),
# so a stray artifact is refused rather than uploaded. Fails the run when the declaration
# cannot be read or names no packages for the target, rather than publishing nothing.
target_packages() {
  local packages
  packages="$(python3 - "$DECLARATION" "$1" <<'PY'
import re, sys, tomllib
registry = {"crate": "crate", "pypi": "pypi", "sdk-pypi": "pypi", "npm": "npm", "sdk-npm": "npm"}.get(sys.argv[2])
try:
    targets = tomllib.load(open(sys.argv[1], "rb"))["target"]
    for target in targets:
        if target["name"] == sys.argv[2]:
            covers = target.get("covers", [])
            if not isinstance(covers, list):
                sys.exit(f"target {sys.argv[2]}'s `covers` is not a list")
            ids = [target["id"], *covers]
            for identifier in ids:
                if not isinstance(identifier, str) or not re.fullmatch(rf"{registry}:\S+", identifier):
                    sys.exit(f"{identifier!r} is not a {registry}:<name> id")
            print(" ".join(i.partition(":")[2] for i in ids))
except (OSError, tomllib.TOMLDecodeError, KeyError, TypeError, AttributeError) as error:
    sys.exit(f"{type(error).__name__}: {error}")
PY
)" || fail "$DECLARATION cannot be read as a list of release targets (above)" "restore release-targets.toml from git, then re-run the release workflow"
  [ -n "$packages" ] || fail "$DECLARATION names no packages for target $1" "restore release-targets.toml from git, then re-run the release workflow"
  printf '%s\n' "$packages"
}

npm_publish() {
  local target="$1" tarball="$2" identity name version
  identity="$(tar -xOzf "$tarball" package/package.json | python3 -c 'import json, sys; m = json.load(sys.stdin); print(m["name"], m["version"])')" \
    || fail "$tarball holds no readable package/package.json" "rebuild it with scripts/build-dist.sh"
  name="${identity% *}"
  version="${identity#* }"
  case " $PACKAGES " in
    *" $name "*) ;;
    *) fail "$tarball is $name, which target $target does not publish" "publish only what scripts/build-dist.sh built for $target" ;;
  esac
  [ "$version" = "$VERSION" ] || fail "$tarball is $name@$version, not the workspace's $VERSION" "rebuild it from this commit with scripts/build-dist.sh"
  local served errors
  errors="$(mktemp)"
  if served="$(npm view "$name@$VERSION" version 2>"$errors")"; then
    rm -f "$errors"
    case "$served" in
      "$VERSION")
        skipped+=("$name")
        return
        ;;
      "") ;;
      *) fail "npm answered '$served' when asked for $name@$VERSION" "re-run the release workflow once npm answers plainly" ;;
    esac
  elif grep -q E404 "$errors"; then
    rm -f "$errors"
  else
    cat "$errors" >&2
    rm -f "$errors"
    fail "npm could not say whether $name@$VERSION is published" "re-run the release workflow once npm answers"
  fi
  npm publish "$tarball" --access public --userconfig "$NPMRC"
}

target="${1:-}"
[ $# -gt 0 ] && shift
case "$target" in
  crate)
    [ $# -eq 0 ] || usage "crate takes no directory"
    need CARGO_REGISTRY_TOKEN
    # The SDK before the binary crate that depends on it, as release-targets.toml covers it.
    # ONEBUDGETSPEC_CRATES_API points the lookup elsewhere, as the tests do.
    api="${ONEBUDGETSPEC_CRATES_API:-https://crates.io/api/v1}"
    PACKAGES="$(target_packages crate)" || exit 1
    for crate in $(printf '%s\n' "$PACKAGES" | awk '{for (i = NF; i > 0; i--) print $i}'); do
      status="$(curl -sS -o /dev/null -w '%{http_code}' \
        -A "onebudgetspec-release (+https://github.com/nickderobertis/onebudgetspec)" \
        "$api/crates/$crate/$VERSION")" \
        || fail "crates.io could not be reached to ask about $crate $VERSION" "re-run the release workflow once crates.io answers"
      case "$status" in
        200) skipped+=("$crate") ;;
        # llmlint: ignore[changed_behavior_has_e2e] uploading to crates.io needs its token, which this repository is not given until publishing is provisioned, and cargo publishes only there; the npm path above is driven against a local registry.
        404) cargo publish --locked -p "$crate" --manifest-path "$ROOT/Cargo.toml" ;;
        *) fail "crates.io answered HTTP $status for $crate $VERSION" "re-run the release workflow once crates.io answers" ;;
      esac
    done
    ;;
  pypi | sdk-pypi)
    if [ $# -ne 1 ] || [ ! -d "$1" ]; then
      usage "$target takes the directory holding its wheels"
    fi
    need PYPI_TOKEN
    name="$(target_packages "$target")" || exit 1
    for wheel in "$1"/*.whl; do
      # The identity the wheel's own metadata declares, not its file name.
      identity="$(python3 - "$wheel" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as wheel:
    metadata = next(n for n in wheel.namelist() if n.endswith(".dist-info/METADATA"))
    fields = dict(l.split(": ", 1) for l in wheel.read(metadata).decode().splitlines() if ": " in l)
print(fields["Name"], fields["Version"])
PY
)" || fail "$wheel is not a readable wheel" "rebuild it with scripts/build-dist.sh"
      [ "$identity" = "$name $VERSION" ] \
        || fail "$wheel is $identity, not $name $VERSION" "publish only what scripts/build-dist.sh built for $target at this commit"
    done
    # llmlint: ignore[changed_behavior_has_e2e] uploading to PyPI needs its token, which this repository is not given until publishing is provisioned; every wheel is checked against its metadata above, which scripts/release/tests/test_release_scripts.py drives.
    UV_PUBLISH_TOKEN="$PYPI_TOKEN" uv publish --quiet --check-url "https://pypi.org/simple/$name/" "$1"/*.whl
    ;;
  npm | sdk-npm)
    [ $# -gt 0 ] || usage "$target takes the directories holding its packed packages, carriers first"
    need NPM_TOKEN
    PACKAGES="$(target_packages "$target")" || exit 1
    NPMRC="$(mktemp)"
    trap 'rm -f "$NPMRC"' EXIT
    registry="$(npm config get registry)"
    printf '%s:_authToken=%s\n' "${registry#http*:}" "$NPM_TOKEN" >"$NPMRC"
    # In argument order: the carriers must exist before the launcher that pins them.
    for dir in "$@"; do
      for tarball in "$dir"/*.tgz; do npm_publish "$target" "$tarball"; done
    done
    ;;
  *) usage "unknown target '${target}'" ;;
esac

if [ ${#skipped[@]} -gt 0 ]; then
  echo "publish: already published at $VERSION, skipped: ${skipped[*]}"
fi
