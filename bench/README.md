# The sym bench: session cost, paired per task, with an answer certificate

This is the harness behind every number on https://s2ar.dev/sym/. It runs a fixed list of questions
against a pinned public repository through headless Claude Code, once per *arm* (plain Claude Code,
Claude Code with the classic plugin, Claude Code with the mod, and ablations), and reports **session cost
with cache reads billed**, paired per task, plus a judge's verdict on whether each arm's answer agrees
with the plain arm's. Every run is published in `results/`, losses included. There is no token-reduction
headline here: the field learned that per-call rewriting can cut tokens and raise cost, so cost is what we
measure.

## Run it

```
python3 bench/run.py --dry                                  # the plan: tasks × arms × runs
python3 bench/run.py --runs 3 --arms plain,plugin,mod       # the published shape (12 tasks × 3 arms × 3)
python3 bench/run.py --runs 1 --only json-begin --arms mod  # one cell; leaves latest.json alone
python3 bench/run.py --recount bench/results/<stamp>.json   # recompute summary + table, no sessions
python3 bench/certify.py bench/results/latest.json          # the judge; writes <stamp>-certify.json
```

Requirements: `claude` 2.1.287+ (the mod arm) at `~/.npm-global/bin/claude` or `CLAUDE_BIN` (**never the
`claude` on PATH on a box that routes accounts**); `sym` on PATH for the plugin/mod arms; the checkout lands
in `~/sym-bench-checkouts/<repo>-<ref>` so the repository's own CLAUDE.md, hooks and `.mcp.json` do not leak
into the arms. Run from a home where the plugin is **not** installed at user scope, or the plain arm is
contaminated (we published one such run by mistake; it is kept in `results/` and marked).

Each cell is `claude -p <prompt> --output-format json --model <model> --permission-mode default` with a
per-arm `--allowedTools` list and, for plugin/mod arms, `--plugin-dir plugin/`. A session's cost is the
engine's `total_cost_usd` (cache reads and writes billed at the provider's rates). The runner aborts on an
account usage-limit message rather than recording zero-cost junk, and for mod arms it reads the mod's own
meter back from the plugin store and fails the run if the two disagree by more than 1%.

## Arms

| arm | what runs | env |
|---|---|---|
| `plain` | Claude Code alone | |
| `plugin` | the classic PreToolUse hook in deny mode + the stdio MCP server | `SYM_HOOK_MODE=deny` |
| `mod` | the hooks module: skeleton-for-Read, the repo map with the first message, the tools | `SYM_HOOK_MODE=off` |
| `mod-sum` | the mod with Haiku file summaries on the map | `SYM_SUMMARIES=haiku-wait` |
| `mod-nomap` | the mod without the repo map | `SYM_MAP_BUDGET=0` |

Add an arm in `run_one` (an env switch the mod reads) and in `summarize`'s arm list.

## Tasks

`tasks.toml` pins the repository (`[repo] url, ref`) and lists tasks, each `id`, `prompt`, optional `family`.
The published families: **file-anchored** (a big file and a symbol in it, where a whole-file Read is the
tempting move), **orientation** (an unfamiliar area, no file named), **find** (a symbol, no file: one
definition lookup is the move). Add a family by adding tasks; keep prompts grep-friendly and specific
enough that the judge can compare answers.

## Reading the numbers

- **Headline** = the median of per-task paired deltas: for each task, the median cost of the arm over its
  runs against the median cost of plain, then the median of those ratios. Pooling all sessions would let the
  expensive tasks move the number on their own; the pooled median is printed beside it for honesty.
- **"Cheaper on k of n"** moves by about ±2 between clean runs of the same configuration; the plain arm's own
  median moved 0.0676 → 0.0571 between two runs an hour apart. Quote the paired median with its run stamp and
  n; never one number alone.
- **The certificate** (`certify.py`): for each task the plain arm's median-cost answer is the reference; a
  small model judges each other arm's median-cost answer *agree / partial / disagree* with one line of reason.
  Read the reasons: a "disagree" can be a more complete answer (the mod listed a fourth printer the plain
  arm missed). Answers are stored to 2,000 characters; older runs stored 600, and their partials mostly cite
  the cut.
- **`tools`** per row counts only calls that ran (`denied` separately); `skeleton` counts Read answers the
  mod replaced with a skeleton (0 in every published run: once the Read description says big files come back
  as skeletons, the agent plans ranged reads).

## Files

`run.py` (the runner), `certify.py` (the judge), `tasks.toml`, `results/<stamp>.{json,md,jsonl}` (every run;
`latest.json` is the headline run, `latest-ablation.json` the ablation, `latest-certify.json` the headline's
certificate), `tools/deploy_site.py` reads them onto the page.
