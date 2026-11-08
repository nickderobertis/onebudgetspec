#!/usr/bin/env bash
# Throwaway visual spike: the one command that renders the mock comments and re-captures every
# image from github.com.
#
#   spike/budget-report-visual/run.sh OUT_DIR            # render + re-capture comments.json's URLs
#   CAPTURE_ONLY=after_ ...                            # re-capture only keys with that prefix
#   POST_PR=<n> spike/budget-report-visual/run.sh OUT_DIR  # also post the after comments on demo PR n
#                                                        # (onebudgetspec) and rewrite comments.json
#
# Inputs are real saved results: onepipeline-ui run 42384341444 (PR #676, no flag) and run
# 42421538096 (PR #686, two `over` rows, live), plus each PR's live standing comment.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?usage: run.sh OUT_DIR}"
mkdir -p "$out"
cols="${COLUMNS_SET:-3}" # chosen for phone width; trials 4 and 5 overflow or wrap at 390 px
ui=nickderobertis/onepipeline-ui
declare -A run=([noflag]=42384341444 [flagged]=42421538096)
declare -A live=([noflag]=6707640908 [flagged]=6714331765)

for case in noflag flagged; do
	d="$out/data-$case"
	if [ ! -f "$d/comparison-report.json" ]; then
		mkdir -p "$d"
		gh run download "${run[$case]}" -R "$ui" -n budget-results -D "$d" ||
			{ echo "run.sh: cannot download budget-results of run ${run[$case]} (expired?)" >&2; exit 1; }
	fi
	gh api "repos/$ui/issues/comments/${live[$case]}" -q .body >"$d/live-comment.md"
	python3 -I "$here/render.py" "$d/comparison-report.json" --standing "$d/live-comment.md" \
		--columns "$cols" >"$out/after-$case.md"
	for trial in 3 4 5; do
		python3 -I "$here/render.py" "$d/comparison-report.json" --standing "$d/live-comment.md" \
			--columns "$trial" >"$out/trial-$case-$trial.md"
	done
done

if [ -n "${POST_PR:-}" ]; then
	python3 -I - "$here/comments.json" "$POST_PR" "$out" <<'PY'
import json, subprocess, sys
p, pr, out = sys.argv[1:]
c = json.load(open(p))
def post(path):
    return subprocess.run(["gh", "api", f"repos/nickderobertis/onebudgetspec/issues/{pr}/comments",
        "-F", f"body=@{path}", "-q", ".html_url"], check=True, capture_output=True, text=True).stdout.strip()
for n in (5, 4, 3):  # trials are posted once, on the first run against a PR
    if f"trial_flagged_{n}" not in c:
        c[f"trial_flagged_{n}"] = post(f"{out}/trial-flagged-{n}.md")
c["after_noflag"] = post(f"{out}/after-noflag.md")
c["after_flagged"] = post(f"{out}/after-flagged.md")
json.dump(c, open(p, "w"), indent=2)
open(p, "a").write("\n")
PY
fi

python3 -I - "$here/comments.json" "$out/shots.json" "${CAPTURE_ONLY:-}" <<'EOF'
import json, sys
c = json.load(open(sys.argv[1]))
only = sys.argv[3]  # CAPTURE_ONLY=<prefix> re-captures just the matching keys, e.g. after_
shots = []
for key, url in c.items():
    if not url or key.startswith("superseded"):
        continue
    for w, tag in ((390, "phone"), (1280, "desktop")):
        if key.startswith("trial_") and w == 1280:
            continue
        shots.append({"name": f"{key}-{tag}.png", "url": url, "width": w, "expand": False})
if c.get("after_noflag"):
    shots.append({"name": "after_noflag-phone-expanded.png", "url": c["after_noflag"], "width": 390, "expand": True})
shots = [x for x in shots if x["name"].startswith(only)]
json.dump(shots, open(sys.argv[2], "w"), indent=2)
EOF
PLAYWRIGHT_MODULE="${PLAYWRIGHT_MODULE:-/home/whoop/ai-orchestrator/node_modules/playwright}" \
	node "$here/capture.mjs" "$out/shots.json" "$out"
