//! The semantic index: one chunk per symbol, embedded once, searched by
//! cosine (plus a name-overlap boost). Files: `symbols.ndjson` (every chunk),
//! `files.json` (path → hash), `meta.json`, `vectors.f32` (row-major,
//! L2-normalized, one row per chunk in `symbols.ndjson` order).

use crate::extract::extract_symbols;
use crate::lang::lang_of;
use crate::ops::{display, load, SKIP_DIRS};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub const TEXT_CAP: usize = 1_500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub file: String,
    pub path: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
    /// The symbol's source, capped at TEXT_CAP chars.
    pub text: String,
    /// Hash of (file, path, text): the embedding's identity across rebuilds.
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Meta {
    pub model: String,
    pub dim: usize,
    pub rows: usize,
    pub built_at: String,
    pub embed_url: String,
}

fn h64(parts: &[&str]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
        0u8.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

fn source_files(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref())))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && lang_of(e.path()).is_some())
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Every symbol of every supported file under `dir`, as chunks.
pub fn chunks(dir: &Path) -> Result<Vec<Chunk>, String> {
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", display(dir)));
    }
    let mut out = Vec::new();
    for f in source_files(dir) {
        out.extend(chunks_of(dir, &f));
    }
    Ok(out)
}

fn chunks_of(dir: &Path, f: &Path) -> Vec<Chunk> {
    let Ok((src, lang)) = load(f) else { return Vec::new() };
    let Ok(symbols) = extract_symbols(&src, lang) else { return Vec::new() };
    let lines: Vec<&str> = src.lines().collect();
    let rel = display(f.strip_prefix(dir).unwrap_or(f));
    symbols
        .into_iter()
        .map(|s| {
            let end = s.end_line.min(lines.len());
            let start = s.start_line.saturating_sub(1).min(end);
            let mut text: String = lines[start..end].join("\n");
            if text.chars().count() > TEXT_CAP {
                text = text.chars().take(TEXT_CAP).collect();
            }
            let id = h64(&[&rel, &s.path, &text]);
            Chunk { file: rel.clone(), path: s.path, kind: s.kind.to_string(), start_line: s.start_line, end_line: s.end_line, signature: s.signature, text, id }
        })
        .collect()
}

pub struct BuildReport {
    pub chunks: usize,
    pub embedded: usize,
    pub reused: usize,
    pub files: usize,
}

/// Build or refresh the index at `out` for `dir`. Chunks whose id already
/// has a vector keep it; the rest are embedded in batches.
pub fn build(dir: &Path, out: &Path, embed: Option<(&str, &str)>, batch: usize) -> Result<BuildReport, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", display(out)))?;
    let all = chunks(dir)?;
    let files = source_files(dir).len();
    // Previous vectors by chunk id.
    let mut old: HashMap<String, Vec<f32>> = HashMap::new();
    if let (Ok(prev), Ok(bytes)) = (std::fs::read_to_string(out.join("symbols.ndjson")), std::fs::read(out.join("vectors.f32"))) {
        let prev_chunks: Vec<Chunk> = prev.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        if let Ok(meta) = read_meta(out) {
            if meta.dim > 0 && bytes.len() == prev_chunks.len() * meta.dim * 4 {
                for (i, c) in prev_chunks.iter().enumerate() {
                    let row = &bytes[i * meta.dim * 4..(i + 1) * meta.dim * 4];
                    old.insert(c.id.clone(), row.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect());
                }
            }
        }
    }
    let mut vectors: Vec<Option<Vec<f32>>> = all.iter().map(|c| old.get(&c.id).cloned()).collect();
    let reused = vectors.iter().filter(|v| v.is_some()).count();
    let mut embedded = 0usize;
    let mut dim = old.values().next().map(|v| v.len()).unwrap_or(0);
    let mut model = String::new();
    let mut url = String::new();
    if let Some((u, m)) = embed {
        url = u.to_string();
        model = m.to_string();
        let todo: Vec<usize> = (0..all.len()).filter(|&i| vectors[i].is_none()).collect();
        for group in todo.chunks(batch.max(1)) {
            let texts: Vec<String> = group.iter().map(|&i| format!("{} {}\n{}", all[i].kind, all[i].path, all[i].text)).collect();
            let vecs = crate::embed::embed(u, m, &texts)?;
            for (&i, v) in group.iter().zip(vecs) {
                dim = v.len();
                vectors[i] = Some(v);
                embedded += 1;
            }
        }
    }
    // Write the chunks; vectors only when every row has one.
    let mut nd = String::new();
    for c in &all {
        nd.push_str(&serde_json::to_string(c).map_err(|e| e.to_string())?);
        nd.push('\n');
    }
    std::fs::write(out.join("symbols.ndjson"), nd).map_err(|e| e.to_string())?;
    let complete = dim > 0 && vectors.iter().all(|v| v.as_ref().map(|x| x.len() == dim).unwrap_or(false));
    if complete {
        let mut bytes = Vec::with_capacity(all.len() * dim * 4);
        for v in vectors.iter().flatten() {
            for x in v {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        std::fs::write(out.join("vectors.f32"), bytes).map_err(|e| e.to_string())?;
    } else {
        let _ = std::fs::remove_file(out.join("vectors.f32"));
    }
    let meta = Meta {
        model,
        dim: if complete { dim } else { 0 },
        rows: all.len(),
        built_at: chrono_now(),
        embed_url: url,
    };
    std::fs::write(out.join("meta.json"), serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(BuildReport { chunks: all.len(), embedded, reused, files })
}

fn chrono_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}")
}

pub fn read_meta(out: &Path) -> Result<Meta, String> {
    let s = std::fs::read_to_string(out.join("meta.json")).map_err(|e| format!("{}: {e}", display(&out.join("meta.json"))))?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub score: f32,
    pub chunk: Chunk,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhereOut {
    pub query: String,
    pub index: String,
    pub hits: Vec<Hit>,
}

/// Query the index: cosine over the vectors, plus (when `hybrid`) a small
/// boost for query words that appear in the symbol's path or signature.
pub fn find_where(index: &Path, query: &str, k: usize, embed: Option<(&str, &str)>, hybrid: bool) -> Result<WhereOut, String> {
    let meta = read_meta(index)?;
    if meta.dim == 0 {
        return Err("the index has no vectors yet (build it with --embed-url)".into());
    }
    let nd = std::fs::read_to_string(index.join("symbols.ndjson")).map_err(|e| e.to_string())?;
    let chunks: Vec<Chunk> = nd.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let bytes = std::fs::read(index.join("vectors.f32")).map_err(|e| e.to_string())?;
    if bytes.len() != chunks.len() * meta.dim * 4 {
        return Err("vectors.f32 does not match symbols.ndjson; rebuild the index".into());
    }
    let (url, model) = match embed {
        Some((u, m)) => (u.to_string(), if m.is_empty() { meta.model.clone() } else { m.to_string() }),
        None => (meta.embed_url.clone(), meta.model.clone()),
    };
    let q = crate::embed::embed(&url, &model, &[query.to_string()])?.remove(0);
    let words: Vec<String> = if hybrid {
        query.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| w.len() > 2).map(|w| w.to_lowercase()).collect()
    } else {
        Vec::new()
    };
    let mut scored: Vec<(f32, usize)> = (0..chunks.len())
        .map(|i| {
            let row = &bytes[i * meta.dim * 4..(i + 1) * meta.dim * 4];
            let mut dot = 0.0f32;
            for (b, qx) in row.chunks_exact(4).zip(&q) {
                dot += f32::from_le_bytes([b[0], b[1], b[2], b[3]]) * qx;
            }
            if !words.is_empty() {
                let hay = format!("{} {}", chunks[i].path, chunks[i].signature).to_lowercase();
                let hits = words.iter().filter(|w| hay.contains(w.as_str())).count();
                dot += 0.05 * hits as f32;
            }
            (dot, i)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(WhereOut {
        query: query.to_string(),
        index: display(index),
        hits: scored.into_iter().take(k).map(|(score, i)| Hit { score, chunk: chunks[i].clone() }).collect(),
    })
}

pub fn where_text(o: &WhereOut) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for h in &o.hits {
        let _ = writeln!(s, "{:.3}  {}:{}-{}  {}  {}  {}", h.score, h.chunk.file, h.chunk.start_line, h.chunk.end_line, h.chunk.kind, h.chunk.path, h.chunk.signature);
    }
    if o.hits.is_empty() {
        s.push_str("no hits\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    /// A fake embedder: the vector of a text is its letter histogram, so
    /// texts that share words land close. Enough to test the plumbing.
    fn fake_server() -> (String, std::thread::JoinHandle<()>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", server.server_addr());
        let h = std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let inputs: Vec<String> = v["input"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                let data: Vec<serde_json::Value> = inputs.iter().map(|t| {
                    let mut h = vec![0.0f32; 26];
                    for c in t.to_lowercase().chars() { if c.is_ascii_lowercase() { h[(c as u8 - b'a') as usize] += 1.0; } }
                    serde_json::json!({ "embedding": h })
                }).collect();
                let out = serde_json::json!({ "data": data }).to_string();
                let _ = req.respond(tiny_http::Response::from_string(out));
                if inputs.iter().any(|t| t == "STOP") { break; }
            }
        });
        (addr, h)
    }

    #[test]
    fn index_is_incremental_and_where_finds_by_meaning_and_name() {
        let (url, h) = fake_server();
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.py"), "def parse_glob(pattern):\n    return pattern\n\ndef zzz():\n    pass\n").unwrap();
        let r = build(dir.path(), out.path(), Some((&url, "fake")), 8).unwrap();
        assert_eq!((r.chunks, r.embedded, r.reused), (2, 2, 0));
        // A second build with one new symbol embeds only that one.
        std::fs::write(dir.path().join("b.rs"), "pub fn glob_match() {}\n").unwrap();
        let r = build(dir.path(), out.path(), Some((&url, "fake")), 8).unwrap();
        assert_eq!((r.chunks, r.embedded, r.reused), (3, 1, 2));
        let w = find_where(out.path(), "glob pattern parsing", 2, None, true).unwrap();
        assert!(w.hits[0].chunk.path.contains("glob"), "{:?}", w.hits.iter().map(|h| &h.chunk.path).collect::<Vec<_>>());
        let text = where_text(&w);
        assert!(text.contains("a.py:1-2") || text.contains("b.rs:1-1"), "{text}");
        let _ = crate::embed::embed(&url, "fake", &["STOP".to_string()]);
        let _ = h.join();
    }

    #[test]
    fn chunks_cap_text_and_carry_ids() {
        let dir = tempfile::tempdir().unwrap();
        let big = format!("fn big() {{\n{}}}\n", "    let x = 1;\n".repeat(400));
        std::fs::write(dir.path().join("x.rs"), big).unwrap();
        let c = chunks(dir.path()).unwrap();
        assert_eq!(c.len(), 1);
        assert!(c[0].text.chars().count() <= TEXT_CAP);
        assert_eq!(c[0].id.len(), 16);
        assert_eq!(crate::embed::endpoint("http://h:1"), "http://h:1/v1/embeddings");
        assert_eq!(crate::embed::endpoint("http://h:1/v1/"), "http://h:1/v1/embeddings");
    }
}
