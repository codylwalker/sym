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

/// The spellings a qualified path answers to, with trait paths cut short:
/// `X as std::str::FromStr::from_str` yields `X as from_str` and
/// `X as FromStr::from_str`; `impl Trait for Type` / `impl Type` (what `ls`
/// prints as the signature) fold to the path `ls` prints first.
fn canon_set(q: &str) -> Vec<String> {
    let q = q.trim();
    let q = q.strip_prefix("impl ").map(|r| {
        let r = r.trim_end_matches('{').trim();
        match r.split_once(" for ") {
            Some((tr, ty)) => format!("{} as {}", ty.trim(), tr.trim()),
            None => r.to_string(),
        }
    }).unwrap_or_else(|| q.to_string());
    match q.split_once(" as ") {
        Some((ty, rest)) => {
            let segs: Vec<&str> = rest.split("::").collect();
            let mut out = vec![format!("{ty} as {}", segs[segs.len() - 1])];
            if segs.len() >= 2 {
                out.push(format!("{ty} as {}::{}", segs[segs.len() - 2], segs[segs.len() - 1]));
            }
            out
        }
        None => vec![q],
    }
}

pub fn read(file: &Path, symbol: &str) -> Result<ReadOut, String> {
    let (src, lang) = load(file)?;
    let symbols = extract_symbols(&src, lang)?;
    let mut matches: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| s.name == symbol || s.path == symbol)
        .collect();
    if matches.is_empty() {
        // Tolerant forms: the signature as `ls` prints it, `impl T for X`,
        // trait paths cut to their last segment, `Type::method` for a method
        // under any impl of Type.
        let want = canon_set(symbol);
        matches = symbols
            .iter()
            .filter(|s| s.signature.trim_end_matches('{').trim() == symbol.trim() || canon_set(&s.path).iter().any(|c| want.contains(c)))
            .collect();
        if matches.is_empty() {
            if let Some((ty, m)) = symbol.rsplit_once("::") {
                matches = symbols
                    .iter()
                    .filter(|s| s.name == m && (s.path.starts_with(&format!("{ty} as ")) || s.path.starts_with(&format!("{ty}::"))))
                    .collect();
            }
        }
    }
    let sym = match matches.len() {
        0 => {
            // A short, relevant list: the symbols sharing a token with the
            // query, then top-level names, never the whole file.
            let toks: Vec<String> = symbol.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|t| t.len() > 2).map(|t| t.to_lowercase()).collect();
            let mut near: Vec<&str> = symbols.iter().filter(|s| { let p = s.path.to_lowercase(); toks.iter().any(|t| p.contains(t.as_str())) }).map(|s| s.path.as_str()).collect();
            if near.is_empty() {
                near = symbols.iter().filter(|s| s.depth == 0).map(|s| s.path.as_str()).collect();
            }
            near.dedup();
            let total = near.len();
            near.truncate(12);
            let more = if total > 12 { format!(" (+{} more; `sym ls` lists every symbol)", total - 12) } else { String::new() };
            return Err(format!(
                "symbol {symbol:?} not found in {}. Near: {}{more}",
                display(file),
                near.join(", ")
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
    for p in source_files(dir) {
        let p = p.as_path();
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

/// Every supported source file under `dir`. Inside a git checkout this is
/// `git ls-files` (tracked + untracked, ignored files left out), so a
/// generated tree beside the code (a types dump, a build dir) never lands in
/// the map or the index; elsewhere a walk that skips the usual build dirs.
pub fn source_files(dir: &Path) -> Vec<PathBuf> {
    if let Some(files) = git_files(dir) {
        return files;
    }
    walkdir::WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref())))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && lang_of(e.path()).is_some())
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn git_files(dir: &Path) -> Option<Vec<PathBuf>> {
    let out = std::process::Command::new("git")
        .args(["-C"])
        .arg(dir)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut files: Vec<PathBuf> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| dir.join(String::from_utf8_lossy(p).as_ref()))
        .filter(|p| lang_of(p).is_some() && p.is_file())
        .collect();
    files.sort();
    Some(files)
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
    let files: Vec<PathBuf> = source_files(dir);
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
    fn source_files_respect_gitignore_inside_a_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::create_dir_all(d.join("gen")).unwrap();
        std::fs::write(d.join("gen/types.ts"), "export type T = 1\n").unwrap();
        // No checkout: the walk sees both.
        let names = |v: Vec<PathBuf>| v.iter().map(|p| display(p.strip_prefix(d).unwrap())).collect::<Vec<_>>();
        assert_eq!(names(source_files(d)), vec!["a.rs", "gen/types.ts"]);
        let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(d).args(args).output().map(|o| o.status.success()).unwrap_or(false);
        if !git(&["init", "-q"]) {
            eprintln!("NOT VERIFIED: git missing");
            return;
        }
        std::fs::write(d.join(".gitignore"), "gen/\n").unwrap();
        assert_eq!(names(source_files(d)), vec!["a.rs"], "ignored dirs stay out, untracked files stay in");
    }

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
        assert!(err.contains("Near: a, X"), "{err}");
    }

    #[test]
    fn read_accepts_impl_spellings_and_short_trait_paths_and_keeps_errors_short() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("t.rs");
        let mut src = String::from("struct Sort;\nimpl Flag for Sort { fn update(&self) {} fn name(&self) {} }\nimpl std::str::FromStr for Sort { type Err = (); fn from_str(s: &str) -> Result<Self, ()> { Ok(Sort) } }\nimpl Sort { fn new() -> Sort { Sort } }\n");
        for i in 0..40 { src.push_str(&format!("fn filler_{i}() {{}}\n")); }
        std::fs::write(&f, src).unwrap();
        assert_eq!(read(&f, "impl Flag for Sort").unwrap().symbol, "Sort as Flag");
        assert_eq!(read(&f, "Sort as Flag").unwrap().symbol, "Sort as Flag");
        assert_eq!(read(&f, "Sort as FromStr::from_str").unwrap().symbol, "Sort as std::str::FromStr::from_str");
        assert_eq!(read(&f, "impl FromStr for Sort").unwrap().symbol, "Sort as std::str::FromStr");
        assert_eq!(read(&f, "Sort::update").unwrap().symbol, "Sort as Flag::update");
        assert_eq!(read(&f, "Sort::new").unwrap().symbol, "Sort::new");
        let err = read(&f, "impl Flag for Nope").unwrap_err();
        assert!(err.contains("Near:") && err.contains("Sort as Flag") && !err.contains("filler_13"), "{err}");
        assert!(err.len() < 600, "{}", err.len());
        let err = read(&f, "zzz_nothing").unwrap_err();
        assert!(err.contains("+") && err.contains("more"), "{err}");
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
