# s2ar — the hosted tools as a Claude Code plugin

```
claude plugin marketplace add https://github.com/codylwalker/sym
claude plugin install s2ar@starlab
```

Nine MCP tools from https://api.s2ar.dev/mcp: `assert_output` (an output checked against a schema, terms,
citations, quotations found in a source; a signed record), `inspect_x402` (an x402 endpoint probed before anyone
pays it), `score_text` (a text against a form), `certify_image` (an image compressed and proven still readable),
`sym_map_repo` / `sym_ls_repo` / `sym_read_repo` / `sym_find_repo` / `sym_where_repo` (code from any public repo),
`buy_credits`. Three skills: `assert`, `inspect`, and `/s2ar` (the rules). Hooks add one line of context and, under
an answer, a reminder when JSON went out unasserted or an x402 endpoint was named and not inspected; **a hook never
makes a paid call**.

Free samples answer a few calls a day without a key. A key (a $5 pack, or $0.50 from signing in) goes in the
plugin's `api_key` setting; or pay USDC per call with any tool's `x402_payment` argument. Every paid answer carries
a receipt; assert, inspect, score and certify answers are signed records, verifiable free. Failed calls are never
charged. The rules: https://api.s2ar.dev/charter.

Measured: see `bench-s2ar/` in this repository (the numbers on https://s2ar.dev/mcp.html carry their run stamp).

Tests: `claude plugin test plugin-s2ar/`. Validate: `claude plugin validate plugin-s2ar/`.
