#!/usr/bin/env python3
"""The s2ar plugin bench: does the plugin raise the pass rate of final outputs, and what does it cost?

    python3 bench-s2ar/run.py --dry
    python3 bench-s2ar/run.py --runs 2 --arms plain,plugin

Each task hands the agent a short source and a schema and asks for a JSON answer with one verbatim quotation
and the source URL. Each session is `claude -p … --output-format json --model <m>`; the plugin arm adds
`--plugin-dir` of a SCRATCH COPY of plugin-s2ar whose server header carries the house key (so the arm is not
throttled by free samples; the copy is never committed). The harness then extracts the final JSON and asserts
it through /v1/assert with the same checks for both arms — api-response (the schema), quotes-source (the
source), cited-answer (the URL's domain) — under the house key. Reported: the pass rate per arm, the paired
session cost with cache reads billed, turns, and whether the plugin arm asserted before returning. Every run
lands in results/ with its stamp; losses included. Requirements as sym/bench: CLAUDE_BIN at
~/.npm-global/bin/claude (never the PATH claude on a box that routes accounts), a working directory outside
any repo, the key in STARLENS_API_KEY or the keep's starlens-admin-key.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import tomllib
import urllib.request
from urllib.parse import urlparse

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN_SRC = os.path.join(HERE, "..", "plugin-s2ar")
RESULTS = os.path.join(HERE, "results")
API = "https://api.s2ar.dev"


def claude_bin() -> str:
    for c in (os.environ.get("CLAUDE_BIN"), "/Users/john_walker/.npm-global/bin/claude", os.path.expanduser("~/.npm-global/bin/claude")):
        if c and os.path.exists(c):
            return c
    sys.exit("no claude binary found; set CLAUDE_BIN")


def house_key() -> str:
    k = os.environ.get("STARLENS_API_KEY", "")
    if not k:
        env = {**os.environ, "STAROS_KEEP_DIR": os.environ.get("STAROS_KEEP_DIR", "/Users/john_walker/.staros/keep")}
        r = subprocess.run(["staros", "keep", "get", "starlens-admin-key"], capture_output=True, text=True, env=env)
        k = r.stdout.strip()
    if not k:
        sys.exit("no key: STARLENS_API_KEY or the keep's starlens-admin-key")
    return k


def plugin_copy(key: str) -> str:
    """A scratch copy of the plugin with the house key in the server header; deleted by the caller."""
    d = tempfile.mkdtemp(prefix="s2ar-plugin-")
    dst = os.path.join(d, "plugin-s2ar")
    shutil.copytree(PLUGIN_SRC, dst, ignore=shutil.ignore_patterns("tests", "node_modules", ".claude-plugin/types"))
    mp = os.path.join(dst, ".claude-plugin", "plugin.json")
    j = json.load(open(mp, encoding="utf-8"))
    j["mcpServers"]["api"]["headers"]["Authorization"] = "Bearer " + key
    json.dump(j, open(mp, "w", encoding="utf-8"), indent=2)
    return dst


def read_tasks() -> list[dict]:
    d = tomllib.load(open(os.path.join(HERE, "tasks.toml"), "rb"))
    return d["task"]


def prompt_for(t: dict) -> str:
    return ("%s\n\nAnswer ONLY with a JSON object matching this schema (no prose around it): %s\n"
            "The object must include one verbatim quotation from the source as `quote` and the source's URL as `source_url`.\n\n"
            "Source URL: %s\nSource:\n%s" % (t["prompt"], t["schema"], t["url"], t["source"]))


def run_one(t: dict, arm: str, cwd: str, model: str, plugin_dir: str | None) -> dict:
    cmd = [claude_bin(), "-p", prompt_for(t), "--output-format", "json", "--model", model, "--permission-mode", "default", "--max-turns", "6"]
    allowed: list[str] = []
    if arm == "plugin":
        cmd += ["--plugin-dir", plugin_dir]
        allowed += ["mcp__s2ar_api__assert_output", "mcp__plugin_s2ar_api__assert_output", "mcp__s2ar_api__inspect_x402"]
    if allowed:
        cmd += ["--allowedTools", ",".join(allowed)]
    env = dict(os.environ)
    env.pop("CLAUDECODE", None)
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=900, env=env, stdin=subprocess.DEVNULL)
    lowered = (r.stdout + r.stderr).lower()
    if "weekly limit" in lowered or "usage limit" in lowered:
        raise SystemExit("bench: the account is at its usage limit; nothing recorded")
    out: dict = {"task": t["id"], "arm": arm, "rc": r.returncode}
    try:
        j = json.loads(r.stdout)
    except ValueError:
        out["error"] = (r.stderr or r.stdout)[-400:]
        return out
    usage = j.get("usage") or {}
    answer = j.get("result") or ""
    out.update({"cost_usd": j.get("total_cost_usd"), "turns": j.get("num_turns"), "cache_read": usage.get("cache_read_input_tokens"),
                "session_id": j.get("session_id"), "answer": answer[:3000], "asserted_in_session": tool_calls(j.get("session_id"))})
    return out


def tool_calls(session_id: str | None) -> int:
    """How many s2ar assert_output calls the session made (from the transcript, when findable)."""
    if not session_id:
        return 0
    home = os.path.expanduser("~")
    n = 0
    for root, _dirs, files in os.walk(os.path.join(home, ".claude", "projects")):
        for f in files:
            if f == session_id + ".jsonl":
                try:
                    for line in open(os.path.join(root, f), encoding="utf-8"):
                        if "assert_output" in line and '"tool_use"' in line:
                            n += 1
                except OSError:
                    pass
    return n


def extract_json(answer: str):
    m = re.search(r"```json\s*(\{[\s\S]*?\})\s*```", answer)
    cand = m.group(1) if m else None
    if cand is None:
        m = re.search(r"\{[\s\S]*\}", answer)
        cand = m.group(0) if m else None
    if cand is None:
        return None
    try:
        return json.loads(cand)
    except ValueError:
        return None


def assert_final(key: str, t: dict, answer: str) -> dict:
    obj = extract_json(answer)
    if obj is None:
        return {"accepted": False, "verdict": "no-json", "gates_failed": ["no JSON object in the answer"]}
    body = {"json": obj, "source": t["source"],
            "checks": [{"check": "json_schema", "schema": json.loads(t["schema"])}, {"check": "quotes_in_source", "path": "quote"},
                       {"check": "citations_present", "min": 1}, {"check": "urls_allowed", "domains": [urlparse(t["url"]).hostname]}]}
    req = urllib.request.Request(API + "/v1/assert", data=json.dumps(body).encode(), method="POST",
                                 headers={"Content-Type": "application/json", "Authorization": "Bearer " + key})
    with urllib.request.urlopen(req, timeout=30) as r:
        j = json.load(r)
    return {"accepted": j["verdict"] == "pass", "verdict": j["verdict"], "gates_failed": [x["check"] for x in j["results"] if not x["ok"]],
            "record": j["record_sha256"]}


def summarize(rows: list[dict]) -> dict:
    by: dict = {}
    for r in rows:
        if r.get("cost_usd") is None:
            continue
        by.setdefault(r["arm"], []).append(r)
    s: dict = {}
    for arm, rs in by.items():
        s[arm] = {"n": len(rs), "pass_rate": round(sum(1 for r in rs if r["final"]["accepted"]) / len(rs), 3),
                  "cost_median": statistics.median(r["cost_usd"] for r in rs), "cost_mean": statistics.fmean(r["cost_usd"] for r in rs),
                  "turns_median": statistics.median(r["turns"] or 0 for r in rs),
                  "asserted_in_session": sum(1 for r in rs if r.get("asserted_in_session", 0) > 0)}
    per_task: dict = {}
    for r in rows:
        if r.get("cost_usd") is None:
            continue
        per_task.setdefault(r["task"], {}).setdefault(r["arm"], []).append(r["cost_usd"])
    if "plain" in s and "plugin" in s:
        deltas = {}
        for t, arms in sorted(per_task.items()):
            if "plain" in arms and "plugin" in arms:
                pa, pb = statistics.median(arms["plain"]), statistics.median(arms["plugin"])
                if pa:
                    deltas[t] = round((pb - pa) / pa * 100.0, 1)
        s["paired"] = {"headline_cost_pct": round(statistics.median(deltas.values()), 1) if deltas else None,
                       "per_task": deltas, "tasks_cheaper": sum(1 for d in deltas.values() if d < 0), "tasks_total": len(deltas),
                       "pass_rate_delta": round(s["plugin"]["pass_rate"] - s["plain"]["pass_rate"], 3)}
    return s


def table(rows: list[dict], s: dict) -> str:
    lines = ["| task | arm | run | final | gates failed | cost USD | turns | asserted in session |", "|---|---|---|---|---|---|---|---|"]
    for r in sorted(rows, key=lambda r: (r["task"], r["arm"], r.get("run", 0))):
        if r.get("cost_usd") is None:
            lines.append("| %s | %s | %s | error | | | | |" % (r["task"], r["arm"], r.get("run")))
            continue
        lines.append("| %s | %s | %s | %s | %s | %.4f | %s | %s |" % (r["task"], r["arm"], r.get("run"), "pass" if r["final"]["accepted"] else "FAIL",
                                                               ", ".join(r["final"]["gates_failed"]) or "", r["cost_usd"], r["turns"], r.get("asserted_in_session", 0)))
    lines.append("")
    for arm in ("plain", "plugin"):
        a = s.get(arm)
        if a:
            lines.append("**%s**: n=%d · final outputs pass %.0f%% · median cost %.4f · mean %.4f · median turns %s · asserted before returning in %d session(s)" % (
                arm, a["n"], a["pass_rate"] * 100, a["cost_median"], a["cost_mean"], a["turns_median"], a["asserted_in_session"]))
    p = s.get("paired")
    if p and p["headline_cost_pct"] is not None:
        lines.append("")
        lines.append("**Pass rate plugin − plain: %+.0f points. Median of per-task paired cost deltas, plugin vs plain: %+.1f%% (cheaper on %d of %d).**" % (
            p["pass_rate_delta"] * 100, p["headline_cost_pct"], p["tasks_cheaper"], p["tasks_total"]))
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=2)
    ap.add_argument("--model", default="claude-sonnet-5")
    ap.add_argument("--arms", default="plain,plugin")
    ap.add_argument("--only")
    ap.add_argument("--dry", action="store_true")
    ap.add_argument("--regrade", help="re-assert the recorded answers of an existing results JSON (no sessions run); rewrites it, its .md and latest.json")
    args = ap.parse_args()
    if args.regrade:
        key = house_key()
        j = json.load(open(args.regrade, encoding="utf-8"))
        by_id = {t["id"]: t for t in read_tasks()}
        for r in j["rows"]:
            if r.get("cost_usd") is not None and r["task"] in by_id:
                r["final"] = assert_final(key, by_id[r["task"]], r.get("answer") or "")
        j["summary"] = summarize(j["rows"])
        j["regraded"] = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H%MZ")
        with open(args.regrade, "w", encoding="utf-8") as f:
            json.dump(j, f, indent=1)
        with open(os.path.join(RESULTS, j["stamp"] + ".md"), "w", encoding="utf-8") as f:
            f.write(table(j["rows"], j["summary"]))
        with open(os.path.join(RESULTS, "latest.json"), "w", encoding="utf-8") as f:
            json.dump(j, f, indent=1)
        print(table(j["rows"], j["summary"]).split("\n\n", 1)[-1])
        return 0
    tasks = read_tasks()
    if args.only:
        keep = set(args.only.split(","))
        tasks = [t for t in tasks if t["id"] in keep]
    arms = [a.strip() for a in args.arms.split(",") if a.strip()]
    print("%d tasks × %s × %d runs (model %s)" % (len(tasks), "{" + ",".join(arms) + "}", args.runs, args.model))
    if args.dry:
        for t in tasks:
            print(" ", t["id"], "—", t["prompt"][:70])
        return 0
    key = house_key()
    plugin_dir = plugin_copy(key) if "plugin" in arms else None
    cwd = tempfile.mkdtemp(prefix="s2ar-bench-cwd-")
    rows: list[dict] = []
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H%MZ")
    os.makedirs(RESULTS, exist_ok=True)
    try:
        for i in range(args.runs):
            for t in tasks:
                for arm in arms:
                    r = run_one(t, arm, cwd, args.model, plugin_dir)
                    r["run"] = i
                    if r.get("cost_usd") is not None:
                        r["final"] = assert_final(key, t, r["answer"])
                    rows.append(r)
                    print("%-20s %-7s run %d  cost %s  turns %s  final %s %s" % (
                        t["id"], arm, i, r.get("cost_usd"), r.get("turns"), (r.get("final") or {}).get("verdict"), (r.get("final") or {}).get("gates_failed")))
                    with open(os.path.join(RESULTS, stamp + ".jsonl"), "a", encoding="utf-8") as f:
                        f.write(json.dumps(r) + "\n")
    finally:
        if plugin_dir:
            shutil.rmtree(os.path.dirname(plugin_dir), ignore_errors=True)
        shutil.rmtree(cwd, ignore_errors=True)
    s = summarize(rows)
    out = {"stamp": stamp, "model": args.model, "runs": args.runs, "summary": s, "rows": rows}
    with open(os.path.join(RESULTS, stamp + ".json"), "w", encoding="utf-8") as f:
        json.dump(out, f, indent=1)
    with open(os.path.join(RESULTS, stamp + ".md"), "w", encoding="utf-8") as f:
        f.write(table(rows, s))
    if not args.only:
        with open(os.path.join(RESULTS, "latest.json"), "w", encoding="utf-8") as f:
            json.dump(out, f, indent=1)
    print(table(rows, s))
    return 0


if __name__ == "__main__":
    sys.exit(main())
