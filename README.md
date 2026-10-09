# sym — read the function, not the file

Symbol-level code reads for AI coding agents. `sym` parses a source file with
tree-sitter and gives an agent the three things it needs to avoid pulling a
whole file into context: the file's skeleton, one symbol's source, and a
budgeted map of a directory.

Why that matters: once a whole file is in context it is replayed on every turn
of the session. Tools that compress or rewrite context after the fact break
the provider's prompt cache and often cost more than they save. `sym` keeps the
file out in the first place.

Languages: Rust, Lua, Python, TypeScript/TSX, JavaScript, Go, C, C++, Java, Ruby.

## Install

```
cargo install sym-cli        # the binary is `sym`
```

## The three verbs

```
sym ls <file>                    # every symbol: line range + one-line signature
sym read <file> <symbol>         # one symbol's source with its doc block
sym map <dir> [--budget 1000]    # per-file signatures, files ranked by PageRank over the import graph
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

**Claude Code plugin**: on 2.1.287+ it is a mod. A whole-file Read of a big
source file comes back as the skeleton (no wasted turn), the repo map arrives
with the first message, `map`/`ls`/`read`/`find` are tools, and a line under
each answer says what stayed out of context. Older clients get a hint hook.
Tested with Claude Code 2.1.294.

```
claude plugin marketplace add https://github.com/codylwalker/sym
claude plugin install sym@sym
```

**Code by meaning, locally.** `sym index <dir> --out <idx> --embed-url <base>`
embeds every definition (long ones in windows) through any OpenAI-shaped
`/v1/embeddings` endpoint and `sym where "<question>" --index <idx>` ranks
them by cosine plus a name match; test code is demoted. We run a small ONNX
service for it (Qwen3-Embedding-0.6B); any endpoint works. The plugin's
`index_url` / `index_model` config (or `SYM_INDEX_URL`, `SYM_INDEX_MODEL`,
`SYM_INDEX_DIR` in the environment) turns on the `where` tool and the
`/sym-index` command; the index is rebuilt in the background per HEAD.

**File summaries** (opt-in, `summaries: haiku` or `haiku-wait` in the plugin
config, or `SYM_SUMMARIES` in the environment): one line per file of the repo
map, written by Haiku on your own plan and cached per commit, so the map reads
`src/lang.rs:  — maps extensions to languages …`. `haiku` writes them after the
session starts (the next session on that commit has them); `haiku-wait` makes
the first session wait (about two seconds for a small repo).

**MCP** (any client): `{"command": "sym", "args": ["mcp"]}` exposes
`sym_ls`, `sym_read`, `sym_map` over stdio.

**HTTP** (for a gateway): `sym serve --port 8431 --root <dir>` answers
`POST /ls`, `/read`, `/find`, `/map`, `/where` and `GET /healthz` on loopback,
every path jailed under `--root`.

**Hook**: `sym hook pre` reads a Claude Code PreToolUse payload on stdin and,
for a `Read` of a supported file over 200 lines, returns `additionalContext`
pointing at the skeleton. It never blocks.

## Hosted

`api.s2ar.dev` runs the same verbs over any public git URL for agents that
cannot install a binary, paid per call (credits, x402, or Stripe's Machine
Payments Protocol). It is also an MCP server over HTTP:

```
claude mcp add --transport http sym-hosted https://api.s2ar.dev/mcp \
  --header "Authorization: Bearer <key>"
```

Tools: `sym_map_repo`, `sym_ls_repo`, `sym_read_repo`, `sym_find_repo`,
`sym_where_repo` (code by meaning: the first call on a repository starts its
semantic index on our GPU and answers "indexing"; call again in a moment),
and `buy_credits`, which returns the payment link an agent with a Link wallet
can pay (Stripe's "monetize your MCP server" pattern). See the site for the
402 flow.

## Measurement

`bench/` runs a fixed task list with and without the plugin and reports the
session cost with cache reads billed. The number on the site comes from there,
losses included. `bench/certify.py` then has a small model judge whether each
arm's answers agree with the plain arm's (agree / partial / disagree, with a
reason per task), so "cheaper" is printed next to "and the same answers", not
instead of it.

## License

PolyForm Shield 1.0.0 (source-available, non-compete). See `LICENSE.md`.
