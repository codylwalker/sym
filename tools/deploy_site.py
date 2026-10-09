#!/usr/bin/env python3
"""Build and deploy the sym site (mirror of starlens/tools/deploy_site.py).

    python3 tools/deploy_site.py --build-only     # writes site/_deploy.html
    python3 tools/deploy_site.py                  # build + scp to the box

Fills /*__COST__*/ and /*__BENCH__*/ in site/index.html from
bench/results/latest.json (placeholders stay honest when there is no result:
"not yet measured"). Parses as Python 3.10."""
from __future__ import annotations

import html
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SITE = os.path.join(ROOT, "site")
HOST = os.environ.get("SYM_DEPLOY_HOST", "staros@10.42.0.5")
DESTDIR = os.environ.get("SYM_DEPLOY_DEST", "/srv/stardata/site/sym")
KEY = os.environ.get("SYM_DEPLOY_KEY", os.path.expanduser("~/.ssh/staros-work1"))
EXTRA = ["privacy.html", "terms.html", "llms.txt"]


def bench_bits() -> tuple[str, str]:
    path = os.path.join(ROOT, "bench", "results", "latest.json")
    if not os.path.exists(path):
        return "not yet measured", '<p class="note">No benchmark result has been published yet. The harness is in the repo; the first table lands here.</p>'
    j = json.load(open(path, encoding="utf-8"))
    s = j.get("summary") or {}
    d = s.get("headline_pct", s.get("delta_median_cost_pct"))
    cost = ("%+.0f%%" % d) if d is not None else "n/a"
    md = os.path.join(ROOT, "bench", "results", j["stamp"] + ".md")
    rows = []
    for r in sorted(j.get("rows") or [], key=lambda r: (r["task"], r["arm"], r.get("run", 0))):
        if r.get("cost_usd") is None:
            continue
        rows.append("<tr><td class=\"mono\">%s</td><td>%s</td><td class=\"mono\">%.4f</td><td class=\"mono\">%s</td><td class=\"mono\">%s</td><td class=\"mono\">%s</td></tr>" % (
            html.escape(r["task"]), r["arm"], r["cost_usd"], r.get("input"), r.get("cache_read"), r.get("turns")))
    arms = s.get("arms") or {}
    per_arm = " ".join("<b>%s</b> vs plain: %+.1f%% (median of per-task paired deltas; cheaper on %s of %s tasks; pooled %+.1f%%)." % (
        html.escape(arm), a.get("headline_pct") or 0.0, a.get("tasks_cheaper", "?"), a.get("tasks_total", "?"), a.get("delta_median_cost_pct") or 0.0)
        for arm, a in arms.items() if a.get("headline_pct") is not None) or (
        "Headline = median of per-task paired deltas (cheaper on %s of %s tasks); pooled median delta %s%%." % (
            s.get("tasks_cheaper", "?"), s.get("tasks_total", "?"), s.get("delta_median_cost_pct", "?")))
    # The certificate: did the cheaper arms answer the same? (bench/certify.py)
    cert = ""
    cpath = os.path.join(ROOT, "bench", "results", "latest-certify.json")
    if os.path.exists(cpath):
        c = json.load(open(cpath, encoding="utf-8"))
        if c.get("stamp") == j.get("stamp"):
            parts = []
            for arm, v in (c.get("summary") or {}).items():
                parts.append("<b>%s</b> agreed with plain on %d of %d tasks (partial %d, disagree %d)" % (
                    html.escape(arm), v.get("agree", 0), v.get("tasks", 0), v.get("partial", 0), v.get("disagree", 0)))
            if parts:
                cert = " Answers, judged by %s against the plain arm's: %s.%s" % (
                    html.escape(c.get("judge_model", "haiku")), "; ".join(parts), (" " + html.escape(c["note"])) if c.get("note") else "")
    # The ablation (a separate full run): what the map and the summaries are worth.
    abl = ""
    apath = os.path.join(ROOT, "bench", "results", "latest-ablation.json")
    if os.path.exists(apath):
        a = json.load(open(apath, encoding="utf-8"))
        arms_a = (a.get("summary") or {}).get("arms") or {}
        bits = []
        for arm, label in (("mod", "the mod"), ("mod-nomap", "the mod without its repo map"), ("mod-sum", "the mod with Haiku file summaries on the map")):
            v = arms_a.get(arm)
            if v and v.get("headline_pct") is not None:
                bits.append("%s %+.1f%% (cheaper on %s of %s)" % (label, v["headline_pct"], v.get("tasks_cheaper", "?"), v.get("tasks_total", "?")))
        cpath2 = os.path.join(ROOT, "bench", "results", a["stamp"] + "-certify.json")
        if os.path.exists(cpath2):
            c2 = json.load(open(cpath2, encoding="utf-8"))
            agree = ["%s %d/%d" % (html.escape(arm), v.get("agree", 0), v.get("tasks", 0)) for arm, v in (c2.get("summary") or {}).items()]
            if agree:
                bits.append("answers agreed with plain (full-length, %s judge): %s" % (html.escape(c2.get("judge_model", "haiku")), ", ".join(agree)))
        if bits:
            abl = " Ablation (run %s, same tasks, %d runs per cell): %s. The repo map with the first message is where the saving comes from; summaries cost more tokens per turn than they save here, so they stay opt-in." % (
                html.escape(a["stamp"]), a.get("runs", 0), "; ".join(bits))
    table = ("<table><tr><th>task</th><th>arm</th><th>cost USD</th><th>input</th><th>cache read</th><th>turns</th></tr>%s</table>"
             "<p class=\"note\">%s on %s@%s, %d runs per cell, model %s, Claude Code %s. %s The headline above is the mod arm (the install on Claude Code 2.1.287+).%s%s Full file: <code>bench/results/%s.json</code>.</p>") % (
        "".join(rows), html.escape(j["stamp"]), html.escape(j["repo"]["url"]), html.escape(j["repo"]["ref"]),
        j.get("runs", 0), html.escape(j.get("model", "")), html.escape(j.get("claude_code", "2.1.294")), per_arm, cert, abl, html.escape(j["stamp"]))
    _ = md
    return cost, table


def build() -> str:
    src = open(os.path.join(SITE, "index.html"), encoding="utf-8").read()
    cost, table = bench_bits()
    out = src.replace("/*__COST__*/", html.escape(cost)).replace("/*__BENCH__*/", table)
    dest = os.path.join(SITE, "_deploy.html")
    open(dest, "w", encoding="utf-8").write(out)
    return dest


def main() -> int:
    out = build()
    print("built", out)
    if "--build-only" in sys.argv:
        return 0
    subprocess.run(["ssh", "-i", KEY, HOST, "mkdir -p %s" % DESTDIR], check=True)
    subprocess.run(["scp", "-i", KEY, "-q", out, "%s:%s/index.html" % (HOST, DESTDIR)], check=True)
    for name in EXTRA:
        subprocess.run(["scp", "-i", KEY, "-q", os.path.join(SITE, name), "%s:%s/%s" % (HOST, DESTDIR, name)], check=True)
    # The root llms.txt for every s2ar tool, one level up from the sym page.
    root_llms = os.path.join(SITE, "root-llms.txt")
    if os.path.exists(root_llms):
        subprocess.run(["scp", "-i", KEY, "-q", root_llms, "%s:%s/llms.txt" % (HOST, os.path.dirname(DESTDIR.rstrip("/")))], check=True)
    print("deployed ->", HOST + ":" + DESTDIR)
    return 0


if __name__ == "__main__":
    sys.exit(main())
