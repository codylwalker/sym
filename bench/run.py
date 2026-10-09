#!/usr/bin/env python3
"""sym benchmark: session cost with and without the plugin, cache reads billed.

Runs each task in bench/tasks.toml as a fresh headless Claude Code session,
in two arms:
  plain   — stock session (no plugin)
  plugin  — with --plugin-dir ../plugin (the hook, the skill, the MCP server)
N runs per cell, same checkout, same model. Collects total_cost_usd, the four
token counts, num_turns, and the tool mix from the session transcript. Writes
bench/results/<date>.json and a Markdown table beside it. Publishes losses too.

Parses as Python 3.10 (no nested f-string expressions, no tomllib).

    python3 bench/run.py --dry           # print the plan, run nothing
    python3 bench/run.py --runs 3        # the real thing (~N*tasks*2 sessions)
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
import time

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.join(os.path.dirname(HERE), "plugin")
# Checkouts live OUTSIDE the monorepo so no project CLAUDE.md, .mcp.json or
# hook from the tree around the bench leaks into either arm.
CHECKOUTS = os.environ.get("SYM_BENCH_CHECKOUTS", os.path.expanduser("~/sym-bench-checkouts"))
RESULTS = os.path.join(HERE, "results")


# ── tiny TOML reader (only the subset tasks.toml uses) ───────────────────────
def read_tasks(path: str) -> tuple[dict, list[dict]]:
    repo: dict = {}
    tasks: list[dict] = []
    cur: dict | None = None
    section = ""
    for raw in open(path, encoding="utf-8"):
        line = raw.split("#", 1)[0].strip() if not raw.strip().startswith("#") else ""
        if not line:
            continue
        if line == "[repo]":
            section = "repo"
            cur = repo
            continue
        if line == "[[task]]":
            section = "task"
            cur = {}
            tasks.append(cur)
            continue
        m = re.match(r'^(\w+)\s*=\s*(.*)$', line)
        if not m or cur is None:
            continue
        key, val = m.group(1), m.group(2).strip()
        if val.startswith('"'):
            cur[key] = json.loads(val)
        elif val.startswith("["):
            # inline table list: keep it as raw text; parsed lazily by --all
            cur[key] = val
        else:
            cur[key] = val
    return repo, tasks


def sh(cmd: list[str], cwd: str | None = None, timeout: int = 900) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout)


def ensure_checkout(url: str, ref: str) -> str:
    name = url.rstrip("/").rsplit("/", 1)[-1]
    dest = os.path.join(CHECKOUTS, "%s-%s" % (name, ref))
    if not os.path.isdir(dest):
        os.makedirs(CHECKOUTS, exist_ok=True)
        r = sh(["git", "clone", "--depth", "1", "--branch", ref, "--single-branch", url, dest])
        if r.returncode != 0:
            sys.exit("clone failed: " + r.stderr[-400:])
    return dest


def claude_bin() -> str:
    for c in (os.environ.get("CLAUDE_BIN"), "/Users/john_walker/.npm-global/bin/claude",
              shutil.which("claude")):
        if c and os.path.exists(c):
            return c
    sys.exit("no claude binary found; set CLAUDE_BIN")


def run_one(task: dict, arm: str, cwd: str, model: str) -> dict:
    # Headless sessions only get the tools they are allowed: both arms may read
    # and search; the plugin arm may also call sym (Bash or the MCP tools).
    # Without this the plugin arm is handed a hint it cannot act on.
    allowed = ["Read", "Grep", "Glob", "LS"]
    cmd = [claude_bin(), "-p", task["prompt"], "--output-format", "json", "--model", model,
           "--permission-mode", "default"]
    if arm == "plugin" or arm.startswith("mod"):
        cmd += ["--plugin-dir", PLUGIN]
        # Plugin-provided MCP servers are namespaced plugin_<plugin>_<server>;
        # the mod's registered tools are mcp__sym__<name>.
        allowed += ["Bash(sym:*)", "mcp__plugin_sym_sym__sym_ls", "mcp__plugin_sym_sym__sym_read",
                    "mcp__plugin_sym_sym__sym_map", "mcp__plugin_sym_sym__sym_find",
                    "mcp__sym__map", "mcp__sym__ls", "mcp__sym__read", "mcp__sym__find", "mcp__sym__where"]
    cmd += ["--allowedTools", ",".join(allowed)]
    env = dict(os.environ)
    env.pop("CLAUDECODE", None)  # a nested session must not inherit the parent's flags
    if arm == "plugin":
        # The hint alone measured as ignored (the agent Greps and does ranged
        # Reads). The product's teeth are the deny mode, so that is the arm.
        env["SYM_HOOK_MODE"] = os.environ.get("SYM_BENCH_HOOK_MODE", "deny")
    if arm.startswith("mod"):
        # The mod (Claude Code 2.1.287+): a whole-file Read of a big source
        # file is answered with its skeleton; the classic hook stays quiet.
        # Ablations: mod-sum adds Haiku file summaries to the map (cached per
        # commit, so only the first session pays), mod-nomap sends no map.
        env["SYM_HOOK_MODE"] = "off"
        if arm == "mod-sum":
            env["SYM_SUMMARIES"] = "haiku-wait"
        if arm == "mod-nomap":
            env["SYM_MAP_BUDGET"] = "0"
    t0 = time.time()
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=900, env=env)
    wall = time.time() - t0
    # An account at its limit answers every prompt with the same line and a
    # zero cost; a run that recorded those would publish a lie.
    lowered = (r.stdout + r.stderr).lower()
    if "weekly limit" in lowered or "rate limit" in lowered or "usage limit" in lowered:
        raise SystemExit("bench: the account is at its usage limit (%s); nothing recorded" % (
            [l for l in (r.stdout + r.stderr).splitlines() if "limit" in l.lower()] or ["?"])[0][:160])
    out: dict = {"task": task["id"], "arm": arm, "wall_s": round(wall, 1), "rc": r.returncode}
    try:
        j = json.loads(r.stdout)
    except ValueError:
        out["error"] = (r.stderr or r.stdout)[-400:]
        return out
    usage = j.get("usage") or {}
    out.update({
        "cost_usd": j.get("total_cost_usd"),
        "turns": j.get("num_turns"),
        "input": usage.get("input_tokens"),
        "output": usage.get("output_tokens"),
        "cache_create": usage.get("cache_creation_input_tokens"),
        "cache_read": usage.get("cache_read_input_tokens"),
        "session_id": j.get("session_id"),
        "answer": (j.get("result") or "")[:2000],
    })
    out["tools"] = tool_mix(j.get("session_id"))
    return out


def tool_mix(session_id: str | None) -> dict:
    """Count Read vs sym tool calls from the session transcript, when found."""
    # Only calls that RAN count: a permission-denied or failed call leaves a
    # tool_result with is_error, and must not be scored as a use of the tool.
    mix: dict = {"Read": 0, "sym_bash": 0, "sym_mcp": 0, "other": 0, "denied": 0, "skeleton": 0}
    if not session_id:
        return mix
    home = os.path.expanduser("~")
    for root, _dirs, files in os.walk(os.path.join(home, ".claude", "projects")):
        for f in files:
            if not (f.startswith(session_id) and f.endswith(".jsonl")):
                continue
            uses: dict = {}
            errored: set = set()
            for line in open(os.path.join(root, f), encoding="utf-8", errors="replace"):
                if '"tool_use"' not in line and '"tool_result"' not in line:
                    continue
                try:
                    ev = json.loads(line)
                except ValueError:
                    continue
                for block in (ev.get("message") or {}).get("content") or []:
                    if block.get("type") == "tool_use":
                        uses[block.get("id")] = block
                    elif block.get("type") == "tool_result":
                        if block.get("is_error"):
                            errored.add(block.get("tool_use_id"))
                        c = block.get("content")
                        text = c if isinstance(c, str) else json.dumps(c)
                        # The mod answers a big Read with the skeleton; that leaves no
                        # tool_use of its own, only this marker in the Read's result.
                        if "[sym] Whole file not loaded" in text:
                            mix["skeleton"] += 1
            for uid, block in uses.items():
                if uid in errored:
                    mix["denied"] += 1
                    continue
                name = block.get("name", "")
                cmd = (block.get("input") or {}).get("command", "")
                if name == "Read":
                    mix["Read"] += 1
                elif name.startswith("mcp__") and name.split("__")[1] in ("sym", "plugin_sym_sym") and name.rsplit("__", 1)[-1] in ("sym_ls", "sym_read", "sym_map", "sym_find", "ls", "read", "map", "find", "where"):
                    mix["sym_mcp"] += 1
                elif name == "Bash" and re.search(r"(^|\s)(rtk\s+)?(staros\s+)?sym\s", cmd):
                    mix["sym_bash"] += 1
                else:
                    mix["other"] += 1
            return mix
    return mix


def summarize(rows: list[dict]) -> dict:
    by: dict = {}
    for r in rows:
        if r.get("cost_usd") is None:
            continue
        by.setdefault(r["arm"], []).append(r)
    summary: dict = {}
    for arm, rs in by.items():
        summary[arm] = {
            "n": len(rs),
            "cost_median": statistics.median(r["cost_usd"] for r in rs),
            "cost_mean": statistics.fmean(r["cost_usd"] for r in rs),
            "cache_read_median": statistics.median(r["cache_read"] or 0 for r in rs),
            "input_median": statistics.median(r["input"] or 0 for r in rs),
            "turns_median": statistics.median(r["turns"] or 0 for r in rs),
            "reads": sum(r["tools"]["Read"] for r in rs),
            "sym_calls": sum(r["tools"]["sym_bash"] + r["tools"]["sym_mcp"] for r in rs),
            "skeletons": sum(r["tools"].get("skeleton", 0) for r in rs),
        }
    # Per arm against plain: per task, the median cost of each arm, then the
    # median of those paired deltas. Pooling tasks of different sizes would let
    # the expensive tasks move the number on their own.
    per_task: dict = {}
    for r in rows:
        if r.get("cost_usd") is None:
            continue
        per_task.setdefault(r["task"], {}).setdefault(r["arm"], []).append(r["cost_usd"])
    summary["arms"] = {}
    for other in [a for a in ("mod", "mod-sum", "mod-nomap", "plugin") if a in summary]:
        if "plain" not in summary:
            continue
        a, b = summary["plain"]["cost_median"], summary[other]["cost_median"]
        deltas: dict = {}
        for t, arms in sorted(per_task.items()):
            if "plain" in arms and other in arms:
                pa, pb = statistics.median(arms["plain"]), statistics.median(arms[other])
                if pa:
                    deltas[t] = round((pb - pa) / pa * 100.0, 1)
        summary["arms"][other] = {
            "delta_median_cost_pct": round((b - a) / a * 100.0, 1) if a else None,
            "paired_task_deltas_pct": deltas,
            "headline_pct": round(statistics.median(deltas.values()), 1) if deltas else None,
            "tasks_cheaper": sum(1 for d in deltas.values() if d < 0),
            "tasks_total": len(deltas),
        }
    # The headline is the mod's when it ran (the install on Claude Code
    # 2.1.287+), else the classic plugin's; the other arm is in `arms`.
    head = summary["arms"].get("mod") or summary["arms"].get("plugin")
    if head:
        summary["other_arm"] = "mod" if "mod" in summary["arms"] else "plugin"
        for k in ("delta_median_cost_pct", "paired_task_deltas_pct", "headline_pct", "tasks_cheaper", "tasks_total"):
            summary[k] = head[k]
    return summary


def table(rows: list[dict], summary: dict) -> str:
    lines = ["| task | arm | cost USD | input | cache read | turns | Read | sym |", "|---|---|---|---|---|---|---|---|"]
    for r in sorted(rows, key=lambda r: (r["task"], r["arm"])):
        if r.get("cost_usd") is None:
            lines.append("| %s | %s | error | | | | | |" % (r["task"], r["arm"]))
            continue
        lines.append("| %s | %s | %.4f | %s | %s | %s | %s | %s |" % (
            r["task"], r["arm"], r["cost_usd"], r["input"], r["cache_read"], r["turns"],
            r["tools"]["Read"], r["tools"]["sym_bash"] + r["tools"]["sym_mcp"]))
    lines.append("")
    for arm in ("plain", "plugin", "mod", "mod-sum", "mod-nomap"):
        s = summary.get(arm)
        if s:
            lines.append("**%s**: n=%d · median cost %.4f · mean %.4f · median cache read %s · median turns %s · Reads %d · sym calls %d · skeleton answers %d" % (
                arm, s["n"], s["cost_median"], s["cost_mean"], s["cache_read_median"], s["turns_median"], s["reads"], s["sym_calls"], s.get("skeletons", 0)))
    for arm, a in (summary.get("arms") or {}).items():
        if a.get("headline_pct") is None:
            continue
        lines.append("")
        lines.append("**Median of per-task paired cost deltas, %s vs plain: %+.1f%%; cheaper on %d of %d tasks** (negative = cheaper). Pooled median delta %+.1f%%. Cache reads are billed; losses are in the table." % (
            arm, a["headline_pct"], a["tasks_cheaper"], a["tasks_total"], a.get("delta_median_cost_pct") or 0.0))
        lines.append("per task: " + ", ".join("%s %+.0f%%" % (t, d) for t, d in sorted(a["paired_task_deltas_pct"].items(), key=lambda x: x[1])))
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--model", default="claude-sonnet-5")
    ap.add_argument("--dry", action="store_true")
    ap.add_argument("--only", help="comma-separated task ids")
    ap.add_argument("--arms", default="plain,plugin", help="comma-separated arms: plain, plugin, mod")
    ap.add_argument("--recount", help="recompute the summary and table of an existing results JSON (no sessions run)")
    args = ap.parse_args()
    if args.recount:
        j = json.load(open(args.recount, encoding="utf-8"))
        j["summary"] = summarize(j["rows"])
        with open(args.recount, "w", encoding="utf-8") as f:
            json.dump(j, f, indent=1)
        with open(os.path.join(RESULTS, j["stamp"] + ".md"), "w", encoding="utf-8") as f:
            f.write(table(j["rows"], j["summary"]))
        with open(os.path.join(RESULTS, "latest.json"), "w", encoding="utf-8") as f:
            json.dump(j, f, indent=1)
        print(table(j["rows"], j["summary"]).split("\n\n", 1)[-1])
        return 0
    repo, tasks = read_tasks(os.path.join(HERE, "tasks.toml"))
    if args.only:
        keep = set(args.only.split(","))
        tasks = [t for t in tasks if t["id"] in keep]
    plan = ["%s × %s × %d runs on %s@%s (model %s)" % (len(tasks), "{plain,plugin}", args.runs, repo["url"], repo["ref"], args.model)]
    print("\n".join(plan))
    if args.dry:
        for t in tasks:
            print(" ", t["id"], "—", t["prompt"][:70])
        return 0
    cwd = ensure_checkout(repo["url"], repo["ref"])
    rows: list[dict] = []
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H%MZ")
    os.makedirs(RESULTS, exist_ok=True)
    for i in range(args.runs):
        for t in tasks:
            for arm in [a.strip() for a in args.arms.split(",") if a.strip()]:
                r = run_one(t, arm, cwd, args.model)
                r["run"] = i
                rows.append(r)
                print("%-18s %-7s run %d  cost %s  turns %s  tools %s" % (
                    t["id"], arm, i, r.get("cost_usd"), r.get("turns"), r.get("tools")))
                with open(os.path.join(RESULTS, stamp + ".jsonl"), "a", encoding="utf-8") as f:
                    f.write(json.dumps(r) + "\n")
    summary = summarize(rows)
    out = {"stamp": stamp, "repo": repo, "model": args.model, "runs": args.runs, "summary": summary, "rows": rows}
    with open(os.path.join(RESULTS, stamp + ".json"), "w", encoding="utf-8") as f:
        json.dump(out, f, indent=1)
    with open(os.path.join(RESULTS, stamp + ".md"), "w", encoding="utf-8") as f:
        f.write(table(rows, summary))
    if args.only:
        print("partial run (--only): latest.json left alone")
    else:
        with open(os.path.join(RESULTS, "latest.json"), "w", encoding="utf-8") as f:
            json.dump(out, f, indent=1)
    print(table(rows, summary))
    return 0


if __name__ == "__main__":
    sys.exit(main())
