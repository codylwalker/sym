//! Ranking files for the repo map: resolve each file's imports to files in
//! the same tree, then PageRank the graph. The old heuristic counted how
//! often a file's stem appeared in import lines, which crowned `util.rs`,
//! `path.rs` and `error.rs` in every repository.

use crate::lang::{lang_of, Lang};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Import specs found in one file, as resolver candidates: each entry is a
/// list of relative path suffixes any of which names the target.
pub fn import_candidates(src: &str, lang: Lang, file: &Path) -> Vec<Vec<PathBuf>> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let t = raw.trim();
        match lang {
            Lang::Rust => {
                if let Some(rest) = t.strip_prefix("mod ").or_else(|| t.strip_prefix("pub mod ")) {
                    if let Some(name) = rest.strip_suffix(';') {
                        let n = name.trim();
                        out.push(vec![PathBuf::from(format!("{n}.rs")), PathBuf::from(format!("{n}/mod.rs"))]);
                    }
                } else if let Some(rest) = t.strip_prefix("use ").or_else(|| t.strip_prefix("pub use ")).or_else(|| t.strip_prefix("pub(crate) use ")) {
                    let spec = rest.trim_end_matches(';');
                    let spec = spec.split("::{").next().unwrap_or(spec);
                    let segs: Vec<&str> = spec.split("::").map(str::trim).filter(|s| !s.is_empty() && *s != "self" && *s != "*").collect();
                    if segs.is_empty() { continue; }
                    // `crate::`/`super::` are local; a bare first segment may be a
                    // local module too (2018 paths), so try it and let the resolver
                    // say no for external crates.
                    let rest: &[&str] = match segs[0] {
                        "crate" | "super" => &segs[1..],
                        "std" | "core" if segs.len() > 1 && segs[1] == "fmt" => continue,
                        _ => &segs[..],
                    };
                    // Try progressively shorter prefixes: a::b::c → a/b/c.rs, a/b.rs, a.rs (+ mod.rs forms).
                    let mut cands = Vec::new();
                    for n in (1..=rest.len().min(4)).rev() {
                        let p = rest[..n].join("/");
                        cands.push(PathBuf::from(format!("{p}.rs")));
                        cands.push(PathBuf::from(format!("{p}/mod.rs")));
                    }
                    if !cands.is_empty() { out.push(cands); }
                }
            }
            Lang::Python => {
                let spec = if let Some(r) = t.strip_prefix("from ") {
                    r.split(" import ").next().map(str::trim)
                } else if let Some(r) = t.strip_prefix("import ") {
                    r.split(',').next().map(|s| s.trim().split(" as ").next().unwrap_or("").trim())
                } else { None };
                let Some(spec) = spec else { continue };
                let dots = spec.chars().take_while(|c| *c == '.').count();
                let body = &spec[dots..];
                let segs: Vec<&str> = body.split('.').filter(|s| !s.is_empty()).collect();
                let mut cands = Vec::new();
                for n in (1..=segs.len().min(4)).rev() {
                    let p = segs[..n].join("/");
                    cands.push(PathBuf::from(format!("{p}.py")));
                    cands.push(PathBuf::from(format!("{p}/__init__.py")));
                }
                if dots > 0 && segs.is_empty() { cands.push(PathBuf::from("__init__.py")); }
                if !cands.is_empty() { out.push(cands); }
            }
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
                let spec = js_spec(t);
                let Some(spec) = spec else { continue };
                if !spec.starts_with('.') && !spec.starts_with('/') { continue; }
                let p = spec.trim_start_matches("./");
                let mut cands = Vec::new();
                for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs"] {
                    cands.push(PathBuf::from(format!("{p}.{ext}")));
                    cands.push(PathBuf::from(format!("{p}/index.{ext}")));
                }
                cands.push(PathBuf::from(p));
                out.push(cands);
            }
            Lang::Go => {
                // `import "mod/pkg"` or a block line `"mod/pkg"`; match the trailing dir.
                let q = t.strip_prefix("import ").unwrap_or(t).trim();
                let q = q.split_whitespace().last().unwrap_or("");
                if q.len() > 2 && q.starts_with('"') && q.ends_with('"') {
                    let path = &q[1..q.len() - 1];
                    if path.contains('/') {
                        out.push(vec![PathBuf::from(path), PathBuf::from(path.rsplit('/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/"))]);
                    }
                }
            }
            Lang::Lua => {
                if let Some(i) = t.find("require") {
                    let rest = &t[i + 7..];
                    let name: String = rest.chars().skip_while(|c| *c == '(' || *c == ' ' || *c == '"' || *c == '\'').take_while(|c| *c != '"' && *c != '\'' && *c != ')').collect();
                    if !name.is_empty() {
                        let p = name.replace('.', "/");
                        out.push(vec![PathBuf::from(format!("{p}.lua")), PathBuf::from(format!("{p}/init.lua"))]);
                    }
                }
            }
            Lang::C | Lang::Cpp => {
                if let Some(rest) = t.strip_prefix("#include") {
                    let rest = rest.trim();
                    if let Some(inner) = rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
                        out.push(vec![PathBuf::from(inner)]);
                    }
                }
            }
            Lang::Java => {
                if let Some(rest) = t.strip_prefix("import ") {
                    let spec = rest.trim_start_matches("static ").trim_end_matches(';').trim();
                    let segs: Vec<&str> = spec.split('.').filter(|s| *s != "*").collect();
                    if segs.len() >= 2 {
                        out.push(vec![PathBuf::from(format!("{}.java", segs.join("/"))), PathBuf::from(format!("{}.java", segs[..segs.len() - 1].join("/")))]);
                    }
                }
            }
            Lang::Ruby => {
                for key in ["require_relative ", "require "] {
                    if let Some(rest) = t.strip_prefix(key) {
                        let name = rest.trim().trim_matches(|c| c == '"' || c == '\'');
                        if !name.is_empty() && !name.contains(' ') {
                            out.push(vec![PathBuf::from(format!("{name}.rb")), PathBuf::from(format!("lib/{name}.rb"))]);
                        }
                        break;
                    }
                }
            }
        }
        let _ = file;
    }
    out
}

fn js_spec(t: &str) -> Option<String> {
    let from = t.find(" from ").map(|i| &t[i + 6..]).or_else(|| t.strip_prefix("import "));
    let s = if let Some(f) = from { f } else if let Some(i) = t.find("require(") { &t[i + 8..] } else { return None };
    let s = s.trim_start_matches(|c| c == '(' || c == ' ');
    let q = s.chars().next()?;
    if q != '"' && q != '\'' { return None; }
    Some(s[1..].split(q).next()?.to_string())
}

/// Resolve a candidate list against the file set: nearest match first
/// (the importing file's dir, its ancestors, the tree root), then a unique
/// suffix match anywhere.
fn resolve(from: &Path, cands: &[PathBuf], root: &Path, index: &HashMap<PathBuf, usize>, by_suffix: &HashMap<String, Vec<usize>>) -> Option<usize> {
    let mut dir = from.parent().map(Path::to_path_buf);
    let mut dirs = Vec::new();
    while let Some(d) = dir {
        dirs.push(d.clone());
        if d == root { break; }
        dir = d.parent().map(Path::to_path_buf);
    }
    if !dirs.iter().any(|d| d == root) { dirs.push(root.to_path_buf()); }
    for c in cands {
        for d in &dirs {
            if let Some(i) = index.get(&d.join(c)) { return Some(*i); }
        }
        // src/ is the usual Rust/TS root one level down from the tree root.
        for d in &dirs {
            if let Some(i) = index.get(&d.join("src").join(c)) { return Some(*i); }
        }
    }
    for c in cands {
        let key = c.file_name()?.to_string_lossy().to_string();
        if let Some(v) = by_suffix.get(&key) {
            let want = c.to_string_lossy().replace('\\', "/");
            let hits: Vec<usize> = v.iter().copied().filter(|i| {
                let p = index.iter().find(|(_, j)| **j == *i).map(|(p, _)| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                p.ends_with(&want)
            }).collect();
            if hits.len() == 1 { return Some(hits[0]); }
        }
    }
    None
}

/// Test code by path convention, across the languages we read.
pub fn is_test_path(p: &Path) -> bool {
    let s = p.to_string_lossy().replace('\\', "/");
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    s.contains("/tests/") || s.contains("/test/") || s.contains("/__tests__/") || s.contains("/spec/")
        || name.ends_with("_test.go") || name.ends_with("_test.rs") || name.ends_with("_test.py") || name.starts_with("test_")
        || name.contains(".test.") || name.contains(".spec.") || name.ends_with("Test.java") || name.ends_with("_spec.rb")
}

/// PageRank over the resolved import graph. Returns (score, in-degree) per file.
pub fn rank(root: &Path, files: &[PathBuf]) -> Vec<(f64, usize)> {
    let n = files.len();
    let index: HashMap<PathBuf, usize> = files.iter().cloned().zip(0..).collect();
    let mut by_suffix: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, f) in files.iter().enumerate() {
        if let Some(name) = f.file_name() {
            by_suffix.entry(name.to_string_lossy().to_string()).or_default().push(i);
        }
    }
    let mut out_edges: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    // A test file's imports say less about what matters than a source
    // file's; they vote at a discount so `tests/util.rs` does not top the map.
    let weight: Vec<f64> = files.iter().map(|f| if is_test_path(f) { 0.3 } else { 1.0 }).collect();
    for (i, f) in files.iter().enumerate() {
        let Some(lang) = lang_of(f) else { continue };
        let Ok(src) = std::fs::read_to_string(f) else { continue };
        for cands in import_candidates(&src, lang, f) {
            if let Some(j) = resolve(f, &cands, root, &index, &by_suffix) {
                if j != i { out_edges[i].insert(j); }
            }
        }
    }
    let mut in_degree = vec![0usize; n];
    for edges in &out_edges {
        for &j in edges { in_degree[j] += 1; }
    }
    let d = 0.85;
    let mut score = vec![1.0 / n.max(1) as f64; n];
    for _ in 0..40 {
        let mut next = vec![(1.0 - d) / n.max(1) as f64; n];
        let mut dangling = 0.0;
        for (i, edges) in out_edges.iter().enumerate() {
            if edges.is_empty() { dangling += score[i]; continue; }
            let share = d * weight[i] * score[i] / edges.len() as f64;
            dangling += d * (1.0 - weight[i]) * score[i] / d; // the discounted part spreads evenly
            for &j in edges { next[j] += share; }
        }
        let spread = d * dangling / n.max(1) as f64;
        for v in next.iter_mut() { *v += spread; }
        score = next;
    }
    score.into_iter().zip(in_degree).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_resolve_and_the_hub_ranks_first() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("src/core")).unwrap();
        std::fs::write(r.join("src/lib.rs"), "mod core;\nmod util;\nmod error;\npub use core::engine::run;\n").unwrap();
        std::fs::write(r.join("src/core/mod.rs"), "pub mod engine;\n").unwrap();
        std::fs::write(r.join("src/core/engine.rs"), "use crate::util::helper;\nuse crate::error::E;\npub fn run() {}\n").unwrap();
        std::fs::write(r.join("src/util.rs"), "use crate::error::E;\npub fn helper() {}\n").unwrap();
        std::fs::write(r.join("src/error.rs"), "pub struct E;\n").unwrap();
        std::fs::write(r.join("src/cli.rs"), "use crate::core::engine::run;\nuse crate::util::helper;\nfn main() { run(); helper(); }\n").unwrap();
        let mut files: Vec<PathBuf> = ["src/lib.rs", "src/core/mod.rs", "src/core/engine.rs", "src/util.rs", "src/error.rs", "src/cli.rs"].iter().map(|p| r.join(p)).collect();
        files.sort();
        let scores = rank(r, &files);
        let by_name: HashMap<String, (f64, usize)> = files.iter().zip(scores).map(|(f, s)| (f.file_name().unwrap().to_string_lossy().to_string(), s)).collect();
        assert_eq!(by_name["engine.rs"].1, 3, "engine is imported by lib, mod.rs and cli: {by_name:?}");
        assert_eq!(by_name["error.rs"].1, 3, "lib (mod), engine and util import error: {by_name:?}");
        assert!(by_name["error.rs"].0 > by_name["cli.rs"].0, "a leaf with importers outranks a root with none");
        let py = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(py.path().join("pkg")).unwrap();
        std::fs::write(py.path().join("pkg/__init__.py"), "").unwrap();
        std::fs::write(py.path().join("pkg/core.py"), "def core():\n    pass\n").unwrap();
        std::fs::write(py.path().join("app.py"), "from pkg.core import core\nimport pkg\n").unwrap();
        let files: Vec<PathBuf> = vec![py.path().join("app.py"), py.path().join("pkg/__init__.py"), py.path().join("pkg/core.py")];
        let scores = rank(py.path(), &files);
        assert_eq!(scores[2].1, 1, "pkg/core.py has one importer");
        assert_eq!(scores[1].1, 1, "pkg/__init__.py has one importer");
    }

    #[test]
    fn test_paths_are_recognised() {
        for p in ["crates/x/tests/util.rs", "a/b_test.go", "src/foo.test.ts", "spec/thing_spec.rb", "pkg/test_core.py"] {
            assert!(is_test_path(Path::new(p)), "{p}");
        }
        assert!(!is_test_path(Path::new("src/testing_tools.rs")));
    }

    #[test]
    fn js_specs_and_go_packages_parse() {
        assert_eq!(js_spec("import x from './a/b'").as_deref(), Some("./a/b"));
        assert_eq!(js_spec("const y = require('../c');").as_deref(), Some("../c"));
        assert_eq!(js_spec("import 'side-effect'").as_deref(), Some("side-effect"));
        let c = import_candidates("import (\n\t\"github.com/x/y/ignore\"\n)\n", Lang::Go, Path::new("a.go"));
        assert!(c.iter().any(|v| v.iter().any(|p| p.ends_with("y/ignore"))));
    }
}
