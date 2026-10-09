---
name: sym
description: Read source code at the symbol level instead of whole files. Use before reading any source file over ~200 lines (Rust, Lua, Python, TypeScript, JavaScript, Go, C, C++, Java, Ruby), when orienting in an unfamiliar directory, or when a task names a function, type or class.
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
| `sym map <dir> --budget N` | orienting in an unfamiliar directory | per-file top-level signatures, files ranked by PageRank over the import graph, cut at N tokens |

Add `--json` for structured output, `--est` for a token estimate line.

| `sym find <name> [dir]` | the task names a function, type or class and you do not know its file | every definition with that leaf name or qualified path, with file and line range |
| `sym where "<question>" --index <idx>` | you know what the code does but not what it is called | definitions ranked by meaning (needs an index: `sym index <dir> --out <idx> --embed-url <base>`) |

## The rule

1. Start with `sym ls`. Read the skeleton, pick the symbol.
2. `sym read` the symbol. The qualified path from `ls` disambiguates
   (`Widget::new`, `Runner.helper`, `Server.Serve`); `impl Trait for Type`,
   `Type as Trait`, `Type::method` and short trait paths are accepted too.
3. Only if you need a region no symbol covers, use Read with `offset`/`limit`
   from the ranges `ls` printed. Never a bare whole-file Read of a big file.

**A definition question is one call.** "Where is X defined / what does X's
`update` do" is `sym find X` (unknown file) or `sym read <file> X` (known
file), then the answer. Do not `ls` first, do not read the struct and then
each method: `read` the impl or the method you were asked about. When the
question is about *mentions* (callers, uses, every place a string appears),
Grep is the right tool and sym is not.

On Claude Code 2.1.287+ this plugin is a mod: a whole-file Read of a source
file over 200 lines comes back as its skeleton (no turn wasted; set
`read_mode` to `hint` or `off` in the plugin config to change that), the repo
map arrives with your first message, and `map`, `ls`, `read`, `find` (and
`where`, code by meaning, when an embeddings endpoint is configured) are
tools. `/sym-stats` shows what was kept out of context; `/sym-index` rebuilds
the semantic index; `summaries: haiku` annotates every file of the map with one
Haiku-written line. Older clients get the classic hint hook.

## Install

```
cargo install sym-cli          # the binary is `sym`
```

The plugin's hook speaks up when a Read is about to pull a big source file.
By default it adds a hint; set `SYM_HOOK_MODE=deny` in your environment to
have it refuse the whole-file Read instead (ranged Reads, `sym ls` and
`sym read` still work). It stays silent when `sym` is not installed.
