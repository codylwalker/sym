# The s2ar plugin bench: does asserting raise the pass rate of final outputs, and at what cost?

The harness behind the numbers on https://s2ar.dev/mcp.html. Eight tasks each hand the agent a short source (our
own published text) and a JSON schema and ask for a JSON answer with one verbatim quotation and the source URL.
Two arms: `plain` (Claude Code alone) and `plugin` (`--plugin-dir` of a scratch copy of `plugin-s2ar` with the house
key in the server header, so the arm is not throttled by free samples; the copy is never committed). Every session
is `claude -p … --output-format json --model claude-sonnet-5 --max-turns 6`. The harness then extracts the final JSON
and asserts it through `/v1/assert` with the same checks for both arms — `json_schema` (the task's schema),
`quotes_in_source` with `path: quote`, `citations_present`, `urls_allowed` — and reports the pass rate per arm,
the paired session cost with cache reads billed, turns, and whether the plugin arm asserted before returning.

```
python3 bench-s2ar/run.py --dry
python3 bench-s2ar/run.py --runs 2 --arms plain,plugin      # the published shape
python3 bench-s2ar/run.py --regrade bench-s2ar/results/<stamp>.json   # re-assert recorded answers, no sessions
```

Requirements as `bench/`: `CLAUDE_BIN` at `~/.npm-global/bin/claude` (never the PATH `claude` on a box that routes
accounts); a home where the plugin is not installed at user scope; the key in `STARLENS_API_KEY` or the keep's
`starlens-admin-key`.

## Reading the numbers

- The headline is the pass-rate difference and the median of per-task paired cost deltas; n=16 per arm moves by a
  few points between clean runs, so quote the stamp and n, never a number alone.
- The first run (2026-10-10T1447Z) found a harness bug before it found anything about the plugin: `quotes_in_source`
  had read every JSON string as a quotation, so both arms scored 0 %. The check gained a `path` (the quotation lives
  in one field) and the recorded answers were re-graded, no sessions re-spent. Both arms pass most of the time on
  these tasks: the plugin's value, if any, is a few points of pass rate for most of a session's cost in extra turns.
