//! Text renderings. These are byte-for-byte what `staros sym` printed, so
//! agents and docs that learned the old shape keep working.

use crate::ops::{LsOut, MapOut, ReadOut};
use std::fmt::Write as _;

pub fn ls_text(o: &LsOut) -> String {
    if o.symbols.is_empty() {
        return "(no symbols found — fall back to Read)\n".to_string();
    }
    let mut s = format!("{} — {} lines, {} symbols\n", o.path, o.lines, o.symbols.len());
    for sym in &o.symbols {
        // The signature already carries the kind keyword (`pub fn …`,
        // `struct …`) — printing the kind too would duplicate it.
        let _ = writeln!(
            s,
            "{:>5}-{:<5} {}{}",
            sym.start_line,
            sym.end_line,
            "  ".repeat(sym.depth),
            sym.signature
        );
    }
    s
}

pub fn read_text(o: &ReadOut) -> String {
    let mut s = format!(
        "{} {} ({}, lines {}-{})\n",
        o.kind, o.symbol, o.path, o.start_line, o.end_line
    );
    for (n, line) in &o.lines {
        let _ = writeln!(s, "{n:>5}\t{line}");
    }
    s
}

/// One file's block in the map; its length is what the budget counts.
pub fn map_block(rel: &str, top: &[String]) -> String {
    format!("{rel}:\n  {}\n", top.join("\n  "))
}

pub fn map_text(o: &MapOut) -> String {
    let mut s = String::new();
    for f in &o.files {
        s.push_str(&map_block(&f.path, &f.signatures));
    }
    if o.omitted > 0 {
        let _ = writeln!(
            s,
            "… budget reached — {} more files omitted (raise --budget or run `sym ls` per file)",
            o.omitted
        );
    }
    s
}

/// Append the token estimate line used by `--est`.
pub fn with_est(mut text: String) -> String {
    let n = crate::tokens_est(text.len());
    let _ = writeln!(text, "~{n} tokens");
    text
}
