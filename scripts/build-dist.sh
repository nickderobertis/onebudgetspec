#!/usr/bin/env bash
# Build one distributable artifact into a directory, and print its path on stdout.
#
# The one build of every artifact this repository releases: .github/workflows/release.yml
# calls it to produce what it publishes, and the packaging journeys
# (crates/onebudgetspec-packaging-e2e) call it to produce what they install, so the build a
# journey proves is the build the release ships.
#
# Usage: scripts/build-dist.sh <artifact> <out-dir> [<rust-target>]
#   cli-wheel       the `onebudgetspec-cli` wheel carrying the binary (maturin, bindings = "bin")
#   npm-carrier     the `@onebudgetspec/cli-<platform>` package carrying the binary
#   npm-launcher    the `@onebudgetspec/cli` launcher package
#   sdk-python      the `onebudgetspec-sdk` wheel
#   sdk-typescript  the `@onebudgetspec/sdk` package
# <rust-target> picks the platform for cli-wheel and npm-carrier; the host's when omitted.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT

usage() {
  echo "build-dist: $1" >&2
  echo "build-dist: usage: scripts/build-dist.sh <cli-wheel|npm-carrier|npm-launcher|sdk-python|sdk-typescript> <out-dir> [<rust-target>]" >&2
  exit 64
}

[ $# -ge 2 ] && [ $# -le 3 ] || usage "takes an artifact, an output directory and an optional Rust target"
readonly ARTIFACT="$1"
TARGET="${3:-}"
mkdir -p "$2"
OUT="$(cd "$2" && pwd)"
readonly OUT

# Build output is noise unless a step fails, which is when all of it matters.
quietly() {
  local log
  if ! log="$("$@" 2>&1)"; then
    printf '%s\n' "$log" >&2
    echo "build-dist: '$*' failed (output above); fix it, then re-run 'scripts/build-dist.sh $ARTIFACT $OUT${TARGET:+ $TARGET}'" >&2
    exit 1
  fi
}

host_target() {
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) echo x86_64-unknown-linux-gnu ;;
    Linux-aarch64 | Linux-arm64) echo aarch64-unknown-linux-gnu ;;
    Darwin-x86_64) echo x86_64-apple-darwin ;;
    Darwin-arm64) echo aarch64-apple-darwin ;;
    *) usage "this host ($(uname -s)-$(uname -m)) is not a platform the release ships" ;;
  esac
}

npm_platform() {
  case "$1" in
    x86_64-unknown-linux-gnu) echo linux-x64 ;;
    aarch64-unknown-linux-gnu) echo linux-arm64 ;;
    x86_64-apple-darwin) echo darwin-x64 ;;
    aarch64-apple-darwin) echo darwin-arm64 ;;
    *) usage "$1 is not a Rust target the release ships" ;;
  esac
}

# The output directory may hold earlier builds, so the artifact is the file newer than
# this run's start stamp.
written_since_start() {
  find "$1" -maxdepth 1 -type f -name "$2" -newer "$STAMP" | head -n 1
}

STAMP="$(mktemp)"
trap 'rm -f "$STAMP"' EXIT
sleep 1

case "$ARTIFACT" in
  cli-wheel)
    [ -n "$TARGET" ] || TARGET="$(host_target)"
    npm_platform "$TARGET" >/dev/null # refuses a target the release does not ship
    quietly uvx --from 'maturin>=1.9,<2' maturin build --release --locked \
      --manifest-path "$ROOT/crates/onebudgetspec/Cargo.toml" --target "$TARGET" --out "$OUT"
    written_since_start "$OUT" '*.whl'
    ;;
  npm-carrier)
    [ -n "$TARGET" ] || TARGET="$(host_target)"
    platform="$(npm_platform "$TARGET")"
    quietly cargo build --release --locked --quiet -p onebudgetspec --target "$TARGET" \
      --manifest-path "$ROOT/Cargo.toml"
    stage="$(mktemp -d)"
    mkdir -p "$stage/bin"
    cp "$ROOT/npm/platforms/$platform/package.json" "$stage/package.json"
    cp "$ROOT/target/$TARGET/release/onebudgetspec" "$stage/bin/onebudgetspec"
    quietly npm pack "$stage" --silent --pack-destination "$OUT"
    rm -rf "$stage"
    written_since_start "$OUT" '*.tgz'
    ;;
  npm-launcher)
    # bun's pack, because it writes the carriers' `workspace:*` as the release version.
    quietly bun install --frozen-lockfile --cwd "$ROOT"
    (cd "$ROOT/npm/cli" && quietly bun pm pack --quiet --destination "$OUT")
    written_since_start "$OUT" '*.tgz'
    ;;
  sdk-python)
    quietly uv build --quiet --package onebudgetspec-sdk --wheel --out-dir "$OUT" --directory "$ROOT"
    written_since_start "$OUT" '*.whl'
    ;;
  sdk-typescript)
    quietly bun install --frozen-lockfile --cwd "$ROOT"
    quietly bun run --cwd "$ROOT/sdks/typescript" build
    quietly npm pack "$ROOT/sdks/typescript" --silent --pack-destination "$OUT"
    written_since_start "$OUT" '*.tgz'
    ;;
  *) usage "unknown artifact '$ARTIFACT'" ;;
esac
