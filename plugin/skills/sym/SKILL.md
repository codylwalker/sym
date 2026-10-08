---
name: sym
description: Read source code at the symbol level instead of whole files. Use before reading any source file over ~200 lines (Rust, Lua, Python, TypeScript, JavaScript, Go), when orienting in an unfamiliar directory, or when a task names a function, type or class.
---

# sym: read the function, not the file

Whole-file reads are the biggest avoidable cost in a coding session, and they are
the one cost that later compression cannot undo: once the file is in context it
is replayed on every turn. `sym` keeps the file out of context in the first place.

## The three verbs

| Verb | Use it when | Returns |
|---|---|---|
| `sym ls <file>` | before any Read of a source file > ~200 lines | every symbol with its line range and one-line signature |
| `sym read <file> <symbol>` | you need one function, type or class | that symbol's source, line-numbered, with its doc block |
| `sym map <dir> --budget N` | orienting in an unfamiliar directory | per-file top-level signatures, files ranked by import fan-in, cut at N tokens |

Add `--json` for structured output, `--est` for a token estimate line.

## The rule

1. Start with `sym ls`. Read the skeleton, pick the symbol.
2. `sym read` the symbol. The qualified path from `ls` disambiguates
   (`Widget::new`, `Runner.helper`, `Server.Serve`).
3. Only if you need a region no symbol covers, use Read with `offset`/`limit`
   from the ranges `ls` printed. Never a bare whole-file Read of a big file.

When this plugin's MCP server is connected the same verbs are tools:
`sym_ls`, `sym_read`, `sym_map` (same arguments; `json: true` for structured).

## Install

```
cargo install sym-cli          # the binary is `sym`
```

The plugin's hook will remind you when a Read is about to pull a big source
file; it stays silent when `sym` is not installed.
