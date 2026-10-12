#!/usr/bin/env bash
# Shows where one harness script writes: runs it in a sandbox where everything but the clone
# and the scratch directory is read-only, tracing every file operation (programs run among
# them). Quiet about the script's own output; prints its exit status, every write the
# sandbox refused, and every run of a compile cache, whose daemon would write outside the
# sandbox unseen.
#
#   spikes/saphyr-diff/confined.sh spikes/saphyr-diff/run.sh [args ...]
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="${ONEPIPELINE_NODE_SCRATCH_DIR:-$root/target}/saphyr-diff"
mkdir -p "$scratch"
for tool in bwrap strace; do
  command -v "$tool" >/dev/null || { echo "error: $tool is not installed; install it, then re-run" >&2; exit 1; }
done
name="$(basename "$1" .sh)"
log="$scratch/confined-$name.log"
trace="$scratch/confined-$name.strace"
status=0
# A read-only root, a fresh /dev and /proc, and only the clone and scratch writable; the
# network stays shared, since the scripts fetch crates and the advisory database.
strace -f -qq -e trace=%file -o "$trace" \
  bwrap --ro-bind / / --dev /dev --proc /proc \
    --bind "$root" "$root" --bind "$scratch" "$scratch" \
    --chdir "$root" -- "$@" >"$log" 2>&1 || status=$?
echo "$1 exit status: $status (output in $log)"
refused="$(grep -c ' EROFS ' "$trace" || true)"
echo "writes refused outside the clone and $scratch: $refused, to these paths:"
grep ' EROFS ' "$trace" | grep -o '"[^"]*"' | sort | uniq -c | sed 's/^/   /' || true
cached="$(grep -E 'execve\("[^"]*/(sccache|ccache)"' "$trace" | grep -vc ' = -1 ' || true)"
echo "compile-cache runs (sccache or ccache): $cached, of $(grep -c 'execve(' "$trace" || true) programs run"
exit $(( status != 0 || refused != 0 || cached != 0 ))
