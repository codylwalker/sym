//! The three operations as data. Rendering (text) lives in `render`; the
//! CLI, MCP and HTTP fronts all call these and choose a rendering.

use crate::extract::{extract_symbols, Symbol};
use crate::lang::{is_doc_line, is_import_line, lang_of, Lang, EXTENSIONS};
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

    // Fan-in rank: count how often each file's stem appears in other files'
    // import lines (`use`, `require`, `import`/`from`).
    let mut fan_in: std::collections::HashMap<PathBuf, usize> = Default::default();
    let stems: Vec<Option<String>> = files
        .iter()
        .map(|f| {
            f.file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| *s != "mod" && *s != "init" && *s != "index")
                .map(str::to_string)
        })
        .collect();
    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        for line in src.lines() {
            let t = line.trim_start();
            if !is_import_line(t) {
                continue;
            }
            for (other, stem) in files.iter().zip(&stems) {
                if other == f {
                    continue;
                }
                if let Some(stem) = stem {
                    if t.contains(stem.as_str()) {
                        *fan_in.entry(other.clone()).or_default() += 1;
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| {
        let fa = fan_in.get(a).copied().unwrap_or(0);
        let fb = fan_in.get(b).copied().unwrap_or(0);
        fb.cmp(&fa).then_with(|| a.cmp(b))
    });

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
        let top: Vec<String> = symbols
            .iter()
            .filter(|s| s.depth == 0)
            .map(|s| s.signature.clone())
            .collect();
        if top.is_empty() {
            continue;
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
