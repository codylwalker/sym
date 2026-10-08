//! `sym` — read the function, not the file.
//!
//! Symbol-level code reads for AI coding agents, via tree-sitter. Three verbs
//! (`ls`, `read`, `map`) share one core (`ops`) and three fronts: the CLI
//! (`cli`), a stdio MCP server (`mcp`) and a loopback HTTP server (`serve`).
//! `hook` is a Claude Code PreToolUse helper that nudges whole-file Reads
//! toward the skeleton. Nothing here rewrites context after the fact; the
//! point is to keep whole files out of the model's context in the first
//! place, which is what keeps provider prompt caching intact.

pub mod cli;
pub mod extract;
pub mod hook;
pub mod lang;
pub mod mcp;
pub mod ops;
pub mod render;
pub mod serve;

pub use extract::Symbol;
pub use lang::Lang;
pub use ops::{LsOut, MapFile, MapOut, ReadOut};

/// Token estimate used everywhere: chars/4, rounded up.
pub fn tokens_est(bytes: usize) -> usize {
    bytes.div_ceil(4)
}
