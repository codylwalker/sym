# sym — read the function, not the file

Symbol-level code reads for AI coding agents. `sym` parses a source file with
tree-sitter and gives an agent the three things it needs to avoid pulling a
whole file into context: the file's skeleton, one symbol's source, and a
budgeted map of a directory.

Why that matters: once a whole file is in context it is replayed on every turn
of the session. Tools that compress or rewrite context after the fact break
the provider's prompt cache and often cost more than they save. `sym` keeps the
file out in the first place.

Languages: Rust, Lua, Python, TypeScript/TSX, JavaScript, Go.

## Install

```
cargo install sym-cli        # the binary is `sym`
```

## The three verbs

```
sym ls <file>                    # every symbol: line range + one-line signature
sym read <file> <symbol>         # one symbol's source with its doc block
sym map <dir> [--budget 1000]    # per-file signatures, ranked by import fan-in
```

Add `--json` for structured output and `--est` for a token estimate.

```
$ sym ls src/ops.rs
src/ops.rs — 247 lines, 12 symbols
   10-15    pub struct LsOut
   18-28    pub struct ReadOut
   ...
$ sym read src/ops.rs read
fn read (src/ops.rs, lines 58-104)
   58	pub fn read(file: &Path, symbol: &str) -> Result<ReadOut, String> {
   ...
```

Nested symbols are addressed by leaf name or qualified path
(`Widget::new`, `Runner.helper`, `Server.Serve`); an ambiguous leaf lists the
candidates.

## For agents

**Claude Code plugin** (skill + MCP server + a hook that nudges big Reads):

```
claude plugin marketplace add https://github.com/codylwalker/sym
claude plugin install sym@sym
```

**MCP** (any client): `{"command": "sym", "args": ["mcp"]}` exposes
`sym_ls`, `sym_read`, `sym_map` over stdio.

**HTTP** (for a gateway): `sym serve --port 8431 --root <dir>` answers
`POST /ls`, `/read`, `/map` and `GET /healthz` on loopback, every path jailed
under `--root`.

**Hook**: `sym hook pre` reads a Claude Code PreToolUse payload on stdin and,
for a `Read` of a supported file over 200 lines, returns `additionalContext`
pointing at the skeleton. It never blocks.

## Hosted

`s2ar.dev/sym` runs the same verbs over any public git URL for agents that
cannot install a binary, paid per call (credits, x402, or Stripe's Machine
Payments Protocol). See the site for the 402 flow.

## Measurement

`bench/` runs a fixed task list with and without the plugin and reports the
session cost with cache reads billed. The number on the site comes from there,
losses included.

## License

PolyForm Shield 1.0.0 (source-available, non-compete). See `LICENSE.md`.
