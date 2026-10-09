#!/usr/bin/env python3
"""Certify a bench result: did the cheaper arms give the same answers?

    python3 bench/certify.py bench/results/latest.json [--model haiku]

For every task, the plain arm's answer (its median-cost run) is the reference;
each other arm's median-cost answer is judged against it by a small model:
agree / partial / disagree, with one line of reason. Writes
bench/results/<stamp>-certify.json and prints the agreement per arm. The
judge runs through `claude -p` from the same home as the bench (no plugin).
Parses as Python 3.10."""
from __future__ import annotations

import json
import os
import statistics
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(HERE, "results")

JUDGE = (
    "You are grading two answers to the same question about a codebase. Decide whether the SECOND "
    "answer agrees with the FIRST on substance: the same functions, fields, values and behaviour. "
    "Formatting, length and wording do not matter; a missing detail that the question asked for "
    "is 'partial'; a wrong function, value or behaviour is 'disagree'.\n\n"
    "Reply with exactly one line: agree|partial|disagree — <reason in at most 20 words>.\n\n"
    "QUESTION:\n%s\n\nFIRST (reference):\n%s\n\nSECOND:\n%s\n"
)


def claude_bin() -> str:
    for p in (os.path.expanduser("~/.npm-global/bin/claude"), "/opt/homebrew/bin/claude", "claude"):
        if os.path.exists(p) or p == "claude":
            return p
    return "claude"


def judge(question: str, ref: str, other: str, model: str) -> tuple[str, str]:
    cmd = [claude_bin(), "-p", JUDGE % (question, ref, other), "--model", model, "--output-format", "json",
           "--allowedTools", "", "--permission-mode", "default"]
    env = dict(os.environ)
    env.pop("CLAUDECODE", None)
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=300, env=env, stdin=subprocess.DEVNULL)
    out = r.stdout
    try:
        text = json.loads(out[out.index("{"):]).get("result", "")
    except (ValueError, AttributeError):
        text = out.strip()
    first = (text.strip().splitlines() or [""])[0].strip().lower()
    if "weekly limit" in first or "rate limit" in first or "usage limit" in first:
        raise SystemExit("certify: the judge account is at its usage limit; nothing written: " + first[:120])
    verdict = "agree" if first.startswith("agree") else "partial" if first.startswith("partial") else "disagree" if first.startswith("disagree") else "unclear"
    reason = first.split("—", 1)[1].strip() if "—" in first else first[:120]
    return verdict, reason


def median_row(rows: list[dict]) -> dict | None:
    ok = [r for r in rows if r.get("cost_usd") is not None and r.get("answer")]
    if not ok:
        return None
    ok.sort(key=lambda r: r["cost_usd"])
    return ok[len(ok) // 2]


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else os.path.join(RESULTS, "latest.json")
    model = sys.argv[sys.argv.index("--model") + 1] if "--model" in sys.argv else "haiku"
    j = json.load(open(path, encoding="utf-8"))
    tasks_path = os.path.join(HERE, "tasks.toml")
    import tomllib
    prompts = {t["id"]: t["prompt"] for t in tomllib.load(open(tasks_path, "rb"))["task"]}
    by: dict = {}
    for r in j["rows"]:
        by.setdefault(r["task"], {}).setdefault(r["arm"], []).append(r)
    arms = sorted({r["arm"] for r in j["rows"] if r["arm"] != "plain"})
    verdicts: dict = {a: {} for a in arms}
    for task, per_arm in sorted(by.items()):
        ref = median_row(per_arm.get("plain", []))
        if not ref:
            continue
        for arm in arms:
            row = median_row(per_arm.get(arm, []))
            if not row:
                continue
            v, why = judge(prompts.get(task, task), ref["answer"], row["answer"], model)
            verdicts[arm][task] = {"verdict": v, "reason": why, "plain_session": ref["session_id"], "session": row["session_id"]}
            print("%-18s %-9s %-8s %s" % (task, arm, v, why[:90]))
    summary = {}
    for arm in arms:
        vs = [x["verdict"] for x in verdicts[arm].values()]
        summary[arm] = {"tasks": len(vs), "agree": vs.count("agree"), "partial": vs.count("partial"), "disagree": vs.count("disagree"), "unclear": vs.count("unclear")}
        print("** %s: agree %d · partial %d · disagree %d of %d tasks" % (arm, summary[arm]["agree"], summary[arm]["partial"], summary[arm]["disagree"], len(vs)))
    out = {"stamp": j["stamp"], "judge_model": model, "reference": "plain (median-cost run per task)", "summary": summary, "verdicts": verdicts}
    out_path = os.path.join(RESULTS, j["stamp"] + "-certify.json")
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(out, f, indent=1)
    with open(os.path.join(RESULTS, "latest-certify.json"), "w", encoding="utf-8") as f:
        json.dump(out, f, indent=1)
    print("wrote", out_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
