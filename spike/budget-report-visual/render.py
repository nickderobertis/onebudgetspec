"""Throwaway visual spike: render a saved comparison report in the proposed readable layout.

Not production code. It mocks what `render_markdown` + `merge_standing_report` would emit,
from a saved `comparison-report.json` (and optionally the live standing comment, for its
per-head flag history), so the layout can be posted on github.com and captured.

    python3 -I render.py COMPARISON_REPORT_JSON [--standing COMMENT_MD] [--columns 5|4|3]
"""

from __future__ import annotations

import argparse
import json
import re
from collections import OrderedDict

UNITS = {"seconds": "s", "gigabytes": "GB", "jobs": "jobs", "runs": "runs"}
MIN_SAMPLES = 5


def fmt(v: float) -> str:
    a = abs(v)
    if a >= 100:
        return f"{v:,.0f}"
    if a >= 10:
        return f"{v:.1f}"
    return f"{v:.2f}"


def unit(u: str) -> str:
    return UNITS.get(u, u)


def short_id(i: str, prefix: str) -> str:
    return i[len(prefix) :] if prefix and i.startswith(prefix) else i


def report_only(report: dict, row: dict) -> bool:
    excl = set(report["current"].get("enforcement_exclude_labels", []))
    for b in report["current"]["catalog"]:
        if b["file"] == row["file"] and b["id"] == row["id"]:
            return bool(excl & set(b.get("labels", [])))
    return False


def status(report: dict, row: dict) -> tuple[bool, str]:
    """(needs attention, one short phrase)."""
    ro = " (report-only)" if report_only(report, row) else ""
    v = row.get("verdict")
    if v == "over":
        return True, f"❌ Over limit{ro}"
    if v == "error":
        return True, f"❌ Error{ro}"
    if row.get("flag"):
        if row.get("sd") == 0:
            return True, "⚠️ Flagged: changed from constant"
        return True, f"⚠️ Flagged: +{row['sigma_change']:.1f} SD"
    if v is None:  # unmeasured: listed in Details, never a warning on its own
        return False, f"No result this run{ro}"
    if row.get("baseline_n", 0) < MIN_SAMPLES:
        return False, "✅ Within · baseline filling"
    return False, "✅ Within"


def now(row: dict, with_limit: bool) -> str:
    if row.get("current") is None:
        return "—"
    s = f"{fmt(row['current'])} {unit(row['unit'])}"
    if with_limit and row.get("threshold") is not None:
        op = "≤" if row["direction"] == "max" else "≥"
        s += f" ({op} {fmt(row['threshold'])})"
    return s


def base(row: dict) -> str:
    n = row.get("baseline_n", 0)
    if n < MIN_SAMPLES or row.get("mean") is None:
        return f"{n}/{MIN_SAMPLES}"
    return f"{fmt(row['mean'])} ± {fmt(row['sd'])}"


def change(row: dict) -> str:
    if row.get("percent_change") is None:
        return "—"
    return f"{row['percent_change']:+.1f}% ({row['sigma_change']:+.1f} SD)"


def table(report: dict, rows: list[dict], cols: int, prefix: str) -> str:
    if cols == 5:
        head = "| Budget | Now | Base (mean ± SD) | Change | Status |\n| --- | ---: | ---: | ---: | --- |\n"
        cells = lambda r: [short_id(r["id"], prefix), now(r, False), base(r), change(r), status(report, r)[1]]
    elif cols == 4:
        head = "| Budget | Now (limit) | Change vs base | Status |\n| --- | ---: | ---: | --- |\n"
        cells = lambda r: [
            short_id(r["id"], prefix),
            now(r, True),
            change(r) if r.get("percent_change") is not None else f"baseline {base(r)}",
            status(report, r)[1],
        ]
    else:
        head = "| Budget | Now · vs base | Status |\n| --- | --- | --- |\n"

        def cells(r):
            vs = change(r) if r.get("percent_change") is not None else f"baseline {base(r)}"
            return [f"**{short_id(r['id'], prefix)}**", f"{now(r, False)}<br>{vs}", status(report, r)[1]]

    return head + "".join("| " + " | ".join(c.replace("|", "\\|") for c in cells(r)) + " |\n" for r in rows)


SHA_PREFIX = re.compile(r"^[0-9a-f]{40}: ")


def grouped_reasons(rows: list[dict], prefix: str) -> list[str]:
    out = []
    for r in rows:
        if not r.get("unavailable"):
            continue
        groups: OrderedDict[str, int] = OrderedDict()
        for part in re.split(r"; (?=[0-9a-f]{40}: |baseline unavailable: )", r["unavailable"]):
            msg = SHA_PREFIX.sub("", part)
            if msg.startswith("baseline unavailable: "):
                continue
            groups[msg] = groups.get(msg, 0) + (1 if SHA_PREFIX.match(part) else 0)
        for msg, heads in groups.items():
            where = f" — on {heads} earlier head(s)" if heads else ""
            out.append(f"- `{short_id(r['id'], prefix)}`: {msg}{where}")
    return out


def details(report: dict, prefix: str, standing: str | None, unmeasured: list[dict]) -> str:
    diags = report["diagnostics"]
    prov = [d for d in diags if d.startswith("baseline provenance: ")]
    lost = [d for d in diags if d.startswith("history entry lost: ")]
    left = [d for d in diags if "left out" in d]
    artifact = [d for d in diags if d.startswith("complete results are in artifact")]
    other = [d for d in diags if d not in prov + lost + left + artifact]
    lines = ["<details><summary>Details</summary>", ""]
    if unmeasured:
        lines += [f"**Not measured on this run ({len(unmeasured)})**", ""]
        lines += [f"- `{short_id(r['id'], prefix)}`: {status(report, r)[1]}, baseline {base(r)}" for r in unmeasured]
        lines.append("")
    reasons = grouped_reasons(report["rows"], prefix)
    if reasons:
        lines += ["**Why some figures are missing**", "", *reasons, ""]
    if prov:
        lines += [f"**Baseline ({len(prov)} saved runs)**", ""]
        for d in prov:
            m = re.search(r"head (\w{7})\w* run (\d+) .*URL (\S+)", d)
            lines.append(f"- [{m.group(1)}]({m.group(3)}) run {m.group(2)}" if m else f"- {d}")
        lines.append("")
    if lost:
        shas = ", ".join(re.search(r"at (\w{7})", d).group(1) for d in lost)
        lines += [f"**History entries lost ({len(lost)})**: base commits not in the selected history: {shas}", ""]
    for d in other:
        lines += [f"- {d}", ""]
    for d in left:
        lines += [d[0].upper() + d[1:], ""]
    for d in artifact:
        lines += [d[0].upper() + d[1:], ""]
    if standing is not None:
        m = re.search(r"<!-- onebudgetspec-history\n(.*?)\n-->", standing, re.S)
        flagged = []
        if m:
            for e in json.loads(m.group(1))["entries"]:
                if e["flags"]:
                    flagged.append(f"- [{e['head_sha'][:7]}]({e['run_url']}) attempt {e['attempt']}: {len(e['flags'])} adverse flag(s)")
        lines += ["**Earlier heads**: " + ("" if flagged else "none was flagged."), *flagged, ""]
    lines.append("</details>")
    return "\n".join(lines)


def render(report: dict, cols: int, standing: str | None) -> str:
    cur = report["current"]
    prefix = cur["repository"].split("/")[-1] + "-"
    attn, ok, unmeasured = [], [], []
    for r in report["rows"]:
        if status(report, r)[0]:
            attn.append(r)
        elif r.get("verdict") is None:
            unmeasured.append(r)
        else:
            ok.append(r)
    head = cur["head_sha"][:7]
    summary = (
        f"**{'⚠️ ' + str(len(attn)) + ' need attention' if attn else '✅ Nothing needs attention'}** · "
        f"{len(ok)} within budget" + (f" · {len(unmeasured)} not measured" if unmeasured else "") + f" · head `{head}` · [run {cur['run_id']}]({cur['run_url']})"
    )
    out = [
        "## Budget report",
        "",
        summary,
        "",
        "<details open><summary><b>Needs attention ({})</b></summary>".format(len(attn)),
        "",
        table(report, attn, cols, prefix) if attn else "Nothing failed or was flagged.\n",
        "</details>",
        "",
        f"<details><summary><b>Within budget ({len(ok)})</b></summary>",
        "",
        table(report, ok, cols, prefix) if ok else "No budget is within its limit.\n",
        "</details>",
        "",
        details(report, prefix, standing, unmeasured),
    ]
    if standing is not None:
        m = re.search(r"<!-- onebudgetspec-history\n.*?\n-->", standing, re.S)
        if m:
            out += ["", m.group(0)]
    return "\n".join(out) + "\n"


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("report")
    p.add_argument("--standing")
    p.add_argument("--columns", type=int, choices=(3, 4, 5), default=5)
    a = p.parse_args()
    with open(a.report, encoding="utf-8") as f:
        report = json.load(f)
    standing = open(a.standing, encoding="utf-8").read() if a.standing else None
    print(render(report, a.columns, standing), end="")


if __name__ == "__main__":
    main()
