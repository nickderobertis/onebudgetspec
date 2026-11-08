#!/usr/bin/env bash
# Throwaway visual spike: the one command that renders the mock comments and re-captures every
# image from github.com.
#
#   spike/budget-report-visual/run.sh OUT_DIR            # render + re-capture comments.json's URLs
#   POST_PR=<n> spike/budget-report-visual/run.sh OUT_DIR  # also post the after comments on demo PR n
#                                                        # (onebudgetspec) and rewrite comments.json
#
# Inputs are real saved results: onepipeline-ui run 42384341444 (PR #676, no flag) and run
# 42421538096 (PR #686, two `over` rows, live), plus each PR's live standing comment.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?usage: run.sh OUT_DIR}"
mkdir -p "$out"
cols="${COLUMNS_SET:-5}"
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
	post() { gh api "repos/nickderobertis/onebudgetspec/issues/$POST_PR/comments" -F "body=@$1" -q .html_url; }
	python3 -I - "$here/comments.json" \
		"$(post "$out/trial-flagged-5.md")" "$(post "$out/trial-flagged-4.md")" "$(post "$out/trial-flagged-3.md")" \
		"$(post "$out/after-noflag.md")" "$(post "$out/after-flagged.md")" <<'EOF'
import json, sys
p, t5, t4, t3, an, af = sys.argv[1:]
c = json.load(open(p))
c.update(trial_flagged_5=t5, trial_flagged_4=t4, trial_flagged_3=t3, after_noflag=an, after_flagged=af)
json.dump(c, open(p, "w"), indent=2)
open(p, "a").write("\n")
EOF
fi

python3 -I - "$here/comments.json" "$out/shots.json" <<'EOF'
import json, sys
c = json.load(open(sys.argv[1]))
shots = []
for key, url in c.items():
    if not url:
        continue
    for w, tag in ((390, "phone"), (1280, "desktop")):
        if key.startswith("trial_") and w == 1280:
            continue
        shots.append({"name": f"{key}-{tag}.png", "url": url, "width": w, "expand": False})
if c.get("after_noflag"):
    shots.append({"name": "after_noflag-phone-expanded.png", "url": c["after_noflag"], "width": 390, "expand": True})
json.dump(shots, open(sys.argv[2], "w"), indent=2)
EOF
PLAYWRIGHT_MODULE="${PLAYWRIGHT_MODULE:-/home/whoop/ai-orchestrator/node_modules/playwright}" \
	node "$here/capture.mjs" "$out/shots.json" "$out"
