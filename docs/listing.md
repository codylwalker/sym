# Listing copy (paste-ready)

## Claude directory — https://claude.ai/directory/manage (paid plan, public GitHub repo)

- **Plugin repo:** https://github.com/codylwalker/sym (marketplace at `.claude-plugin/marketplace.json`, plugin at `plugin/`)
- **Name:** sym
- **Tagline:** Read the function, not the file.
- **Description:** Symbol-level code reads for coding agents. `sym ls` gives a file's skeleton with line ranges, `sym read` one symbol with its docs, `sym map` a budgeted map of a directory ranked by import fan-in. Whole files stay out of context, so prompt caching stays intact. Ships a skill, an MCP server (`sym_ls`, `sym_read`, `sym_map`) and a Read hook (`SYM_HOOK_MODE=deny` refuses whole-file Reads of big source files). Rust, Lua, Python, TypeScript/TSX, JavaScript, Go. Needs the `sym` binary: `cargo install sym-cli`.
- **Category:** productivity / developer tools
- **Homepage:** https://sym.s2ar.dev · **Docs:** https://sym.s2ar.dev/#docs · **Support:** https://github.com/codylwalker/sym/issues
- **Privacy:** https://sym.s2ar.dev/privacy.html · **Terms:** https://sym.s2ar.dev/terms.html
- **What the plugin accesses:** reads source files the user names, locally; the hook reads the file about to be Read. No network. (The hosted tier at sym.s2ar.dev is separate and opt-in.)

## Stripe Directory — email machine-payments@stripe.com

Subject: Directory listing: sym (Stardata)

Business name: Stardata (Cody Lee Walker, Canada)
Stripe account id: <acct_… from the dashboard>
Stripe profile id: <profile_… from Settings → Profiles>
llms.txt: https://sym.s2ar.dev/llms.txt
Agent skill: the Claude Code plugin at https://github.com/codylwalker/sym (skill `sym`)

Example prompts:
1. "Map https://github.com/BurntSushi/ripgrep at 14.1.1 through sym.s2ar.dev with a 1200-token budget and tell me which file has the most importers."
2. "Using sym.s2ar.dev, read the function that parses the --glob flag in ripgrep's crates/core/flags/defs.rs and explain what it does with a leading '!'."
3. "Before reading any source file over 200 lines, run sym ls on it and pick the symbol."

What we sell to agents: `POST /v1/sym/map` ($0.01) and `POST /v1/sym/read` ($0.005) over any public git URL, paid per call over MPP (SPT cards or Tempo USDC), x402 (USDC on Base), or prepaid credits. Failed requests are never charged.

## crates.io

`cargo login` (token from https://crates.io/settings/tokens, scope publish-new + publish-update), then in `sym/`: `cargo publish`. The dry run passes (25 files, 76 KB).
