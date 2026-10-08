//! The three operations as data. Rendering (text) lives in `render`; the
//! CLI, MCP and HTTP fronts all call these and choose a rendering.

use crate::extract::{extract_symbols, Symbol};
use crate::lang::{is_doc_line, lang_of, Lang, EXTENSIONS};
use std::collections::HashMap;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct LsOut {
    pub path: String,
    pub lang: Lang,
    pub lines: usize,
    pub symbols: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadOut {
    pub path: String,
    pub lang: Lang,
    pub kind: &'static str,
    pub symbol: String,
    /// 1-based, inclusive; `start_line` includes the doc block above.
    pub start_line: usize,
    pub end_line: usize,
    pub lines: Vec<(usize, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapFile {
    pub path: String,
    pub fan_in: usize,
    pub signatures: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapOut {
    pub dir: String,
    pub budget: usize,
    pub files: Vec<MapFile>,
    /// Files not shown once the budget ran out (0 when everything fit).
    pub omitted: usize,
    pub total_files: usize,
}

/// Display form of a path: as given, with `/` separators.
pub fn display(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

pub fn load(file: &Path) -> Result<(String, Lang), String> {
    let lang = lang_of(file)
        .ok_or_else(|| format!("unsupported extension: {} ({EXTENSIONS} only)", display(file)))?;
    let src = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", display(file)))?;
    Ok((src, lang))
}

pub fn ls(file: &Path) -> Result<LsOut, String> {
    let (src, lang) = load(file)?;
    let symbols = extract_symbols(&src, lang)?;
    Ok(LsOut {
        path: display(file),
        lang,
        lines: src.lines().count(),
        symbols,
    })
}

pub fn read(file: &Path, symbol: &str) -> Result<ReadOut, String> {
    let (src, lang) = load(file)?;
    let symbols = extract_symbols(&src, lang)?;
    let matches: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| s.name == symbol || s.path == symbol)
        .collect();
    let sym = match matches.len() {
        0 => {
            let names: Vec<&str> = symbols.iter().map(|s| s.path.as_str()).collect();
            return Err(format!(
                "symbol {symbol:?} not found in {}. Available: {}",
                display(file),
                names.join(", ")
            ));
        }
        1 => matches[0],
        _ => {
            // Ambiguous leaf name across containers: report choices.
            let paths: Vec<&str> = matches.iter().map(|s| s.path.as_str()).collect();
            return Err(format!(
                "symbol {symbol:?} is ambiguous: {}. Use the qualified path.",
                paths.join(", ")
            ));
        }
    };

    let lines: Vec<&str> = src.lines().collect();
    // Include contiguous doc comments / attributes directly above.
    let mut start = sym.start_line - 1; // 0-based
    while start > 0 && is_doc_line(lang, lines[start - 1].trim_start()) {
        start -= 1;
    }
    let end = sym.end_line.min(lines.len());
    Ok(ReadOut {
        path: display(file),
        lang,
        kind: sym.kind,
        symbol: sym.path.clone(),
        start_line: start + 1,
        end_line: sym.end_line,
        lines: lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, l)| (start + 1 + i, (*l).to_string()))
            .collect(),
    })
}

fn files_sorted_at(sorted: &[PathBuf], order: &[usize], original: usize) -> PathBuf {
    let pos = order.iter().position(|&i| i == original).unwrap_or(0);
    sorted[pos].clone()
}

#[derive(Debug, Clone, Serialize)]
pub struct FindHit {
    pub path: String,
    pub symbol: Symbol,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindOut {
    pub dir: String,
    pub name: String,
    pub prefix: bool,
    pub hits: Vec<FindHit>,
    pub files_scanned: usize,
}

/// Definitions by name across a tree: every symbol whose leaf name or
/// qualified path equals `name` (or starts with it under `prefix`). What
/// grep cannot do: definitions only, with kind and qualified path.
pub fn find(dir: &Path, name: &str, prefix: bool) -> Result<FindOut, String> {
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", display(dir)));
    }
    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        })
        .filter_map(Result::ok)
    {
        let p = entry.path();
        if !entry.file_type().is_file() || lang_of(p).is_none() {
            continue;
        }
        let Ok((src, lang)) = load(p) else { continue };
        let Ok(symbols) = extract_symbols(&src, lang) else { continue };
        scanned += 1;
        let rel = display(p.strip_prefix(dir).unwrap_or(p));
        for s in symbols {
            let hit = if prefix {
                s.name.starts_with(name) || s.path.starts_with(name)
            } else {
                s.name == name || s.path == name
            };
            if hit {
                hits.push(FindHit { path: rel.clone(), symbol: s });
            }
        }
    }
    hits.sort_by(|a, b| a.path.cmp(&b.path).then(a.symbol.start_line.cmp(&b.symbol.start_line)));
    Ok(FindOut { dir: display(dir), name: name.to_string(), prefix, hits, files_scanned: scanned })
}

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "shared-target",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    "vendor",
];

/// Top-level signatures shown per file in the map before "… +N more".
pub const MAP_FILE_CAP: usize = 12;

/// Repo map: per-file top-level signatures, files ranked by import fan-in,
/// cut at `budget` tokens (chars/4) the way the text rendering counts them.
pub fn map(dir: &Path, budget: usize) -> Result<MapOut, String> {
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", display(dir)));
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        })
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() && lang_of(entry.path()).is_some() {
            files.push(entry.path().to_path_buf());
        }
    }
    if files.is_empty() {
        return Err(format!("no {EXTENSIONS} files found"));
    }

    // Rank: resolve imports to files, PageRank the graph (see `rank`).
    let scores = crate::rank::rank(dir, &files);
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| {
        scores[b].0.partial_cmp(&scores[a].0).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| scores[b].1.cmp(&scores[a].1))
            .then_with(|| files[a].cmp(&files[b]))
    });
    let files: Vec<PathBuf> = order.iter().map(|&i| files[i].clone()).collect();
    let fan_in: HashMap<PathBuf, usize> = order.iter().map(|&i| (files_sorted_at(&files, &order, i), scores[i].1)).collect();

    // Emit per-file skeletons until the budget is exhausted. The budget is
    // measured on the text rendering so `--json` and text agree.
    let char_budget = budget * 4;
    let mut used = 0usize;
    let mut shown = Vec::new();
    let mut omitted = 0usize;
    for f in &files {
        let Ok((src, lang)) = load(f) else { continue };
        let Ok(symbols) = extract_symbols(&src, lang) else {
            continue;
        };
        let mut top: Vec<String> = symbols
            .iter()
            .filter(|s| s.depth == 0)
            .map(|s| s.signature.clone())
            .collect();
        if top.is_empty() {
            continue;
        }
        // One file must not eat the whole budget: cap its lines and say so.
        if top.len() > MAP_FILE_CAP {
            let more = top.len() - MAP_FILE_CAP;
            top.truncate(MAP_FILE_CAP);
            top.push(format!("… +{more} more (sym ls for all)"));
        }
        let rel = display(f.strip_prefix(dir).unwrap_or(f));
        let block_len = crate::render::map_block(&rel, &top).len();
        if used + block_len > char_budget && !shown.is_empty() {
            omitted = files.len() - shown.len();
            break;
        }
        used += block_len;
        shown.push(MapFile {
            path: rel,
            fan_in: fan_in.get(f).copied().unwrap_or(0),
            signatures: top,
        });
    }
    Ok(MapOut {
        dir: display(dir),
        budget,
        files: shown,
        omitted,
        total_files: files.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_extension_is_clear_error() {
        let err = load(Path::new("x.toml")).unwrap_err();
        assert!(err.contains("unsupported extension"));
    }

    #[test]
    fn read_includes_doc_block_and_errors_list_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("m.rs");
        std::fs::write(&f, "/// Doc.\n#[inline]\npub fn a() {}\n\nimpl X { fn a(&self) {} }\n").unwrap();
        let err = read(&f, "a").unwrap_err();
        assert!(err.contains("ambiguous"), "{err}");
        let out = read(&f, "X::a").unwrap();
        assert_eq!(out.start_line, 5);
        let err = read(&f, "zz").unwrap_err();
        assert!(err.contains("Available: a, X, X::a"), "{err}");
    }

    #[test]
    fn find_returns_definitions_by_leaf_or_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "pub fn run() {}\nimpl X { fn run(&self) {} }\n").unwrap();
        std::fs::write(dir.path().join("b.py"), "def run():\n    pass\ndef runner():\n    pass\n").unwrap();
        let out = find(dir.path(), "run", false).unwrap();
        assert_eq!(out.hits.len(), 3, "{:?}", out.hits.iter().map(|h| &h.symbol.path).collect::<Vec<_>>());
        let out = find(dir.path(), "X::run", false).unwrap();
        assert_eq!(out.hits.len(), 1);
        let out = find(dir.path(), "run", true).unwrap();
        assert_eq!(out.hits.len(), 4);
        assert_eq!(out.files_scanned, 2);
    }

    #[test]
    fn map_ranks_by_fan_in_and_respects_budget() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("core.py"), "def core():\n    pass\n").unwrap();
        std::fs::write(dir.path().join("a.py"), "from core import core\ndef a():\n    pass\n").unwrap();
        std::fs::write(dir.path().join("b.py"), "import core\ndef b():\n    pass\n").unwrap();
        let out = map(dir.path(), 1000).unwrap();
        assert_eq!(out.files[0].path, "core.py");
        assert_eq!(out.files[0].fan_in, 2);
        assert_eq!(out.omitted, 0);
        let small = map(dir.path(), 5).unwrap();
        assert_eq!(small.files.len(), 1);
        assert_eq!(small.omitted, 2);
    }
}
