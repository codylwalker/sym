//! `sym serve` — the three verbs as loopback HTTP JSON, for a gateway (the
//! paid hosted tier) to front. Binds 127.0.0.1 only. With `--root`, every
//! path is canonicalized and must stay under the root.

use crate::{ops, render};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn serve(port: u16, root: Option<PathBuf>) -> Result<(), String> {
    let root = match root {
        Some(r) => Some(r.canonicalize().map_err(|e| format!("--root {}: {e}", r.display()))?),
        None => None,
    };
    let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    eprintln!("sym serve listening on http://127.0.0.1:{port}{}",
        root.as_ref().map(|r| format!(" (root {})", r.display())).unwrap_or_default());
    for mut req in server.incoming_requests() {
        let mut body = String::new();
        let _ = req.as_reader().read_to_string(&mut body);
        let (status, out) = handle(req.method().as_str(), req.url(), &body, root.as_deref());
        let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
            .expect("static header");
        let resp = tiny_http::Response::from_string(out)
            .with_status_code(status)
            .with_header(header);
        let _ = req.respond(resp);
    }
    Ok(())
}

/// Resolve a request path under the jail (when one is set).
fn jail(root: Option<&Path>, p: &str) -> Result<PathBuf, String> {
    let path = Path::new(p);
    let Some(root) = root else {
        return Ok(path.to_path_buf());
    };
    let joined = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("{p}: {e}"))?;
    if !canon.starts_with(root) {
        return Err(format!("{p}: outside the served root"));
    }
    Ok(canon)
}

/// Pure request handler, testable without sockets. Returns (status, body).
pub fn handle(method: &str, url: &str, body: &str, root: Option<&Path>) -> (u16, String) {
    let path = url.split('?').next().unwrap_or(url);
    if method == "GET" && path == "/healthz" {
        return (200, json!({"ok": true, "service": "sym", "version": env!("CARGO_PKG_VERSION")}).to_string());
    }
    if method != "POST" {
        return (405, json!({"ok": false, "error": "POST /ls, /read, /find, /map, /where or GET /healthz"}).to_string());
    }
    let args: Value = match serde_json::from_str(if body.is_empty() { "{}" } else { body }) {
        Ok(v) => v,
        Err(e) => return (400, json!({"ok": false, "error": format!("bad JSON body: {e}")}).to_string()),
    };
    let json_out = args.get("json").and_then(Value::as_bool).unwrap_or(true);
    let s = |k: &str| args.get(k).and_then(Value::as_str);
    let res: Result<(String, Value), String> = (|| match path {
        "/ls" => {
            let f = jail(root, s("file").ok_or("needs `file`")?)?;
            let o = ops::ls(&f)?;
            Ok((render::ls_text(&o), serde_json::to_value(&o).map_err(|e| e.to_string())?))
        }
        "/read" => {
            let f = jail(root, s("file").ok_or("needs `file`")?)?;
            let o = ops::read(&f, s("symbol").ok_or("needs `symbol`")?)?;
            Ok((render::read_text(&o), serde_json::to_value(&o).map_err(|e| e.to_string())?))
        }
        "/find" => {
            let d = jail(root, s("dir").ok_or("needs `dir`")?)?;
            let name = s("name").ok_or("needs `name`")?;
            let prefix = args.get("prefix").and_then(Value::as_bool).unwrap_or(false);
            let o = ops::find(&d, name, prefix)?;
            Ok((render::find_text(&o), serde_json::to_value(&o).map_err(|e| e.to_string())?))
        }
        "/map" => {
            let d = jail(root, s("dir").ok_or("needs `dir`")?)?;
            let budget = args.get("budget").and_then(Value::as_u64).unwrap_or(1000) as usize;
            let o = ops::map(&d, budget)?;
            Ok((render::map_text(&o), serde_json::to_value(&o).map_err(|e| e.to_string())?))
        }
        "/where" => {
            // The index lives beside its checkout (`<dir>.index` by default);
            // the query is embedded by the same service that built it unless
            // `embed_url` says otherwise. Fast: one embedding and a cosine pass.
            let idx = match s("index") {
                Some(i) => jail(root, i)?,
                None => {
                    let d = jail(root, s("dir").ok_or("needs `dir` or `index`")?)?;
                    jail(root, &format!("{}.index", d.display()))?
                }
            };
            let query = s("query").ok_or("needs `query`")?;
            let k = args.get("k").and_then(Value::as_u64).unwrap_or(8) as usize;
            let hybrid = args.get("hybrid").and_then(Value::as_bool).unwrap_or(true);
            let model = s("embed_model").unwrap_or("");
            let embed = s("embed_url").map(|u| (u, model));
            let o = crate::index::find_where(&idx, query, k, embed, hybrid)?;
            Ok((crate::index::where_text(&o), serde_json::to_value(&o).map_err(|e| e.to_string())?))
        }
        _ => Err(format!("no route {path}")),
    })();
    match res {
        Ok((text, data)) => {
            let tokens = crate::tokens_est(text.len());
            let out = if json_out {
                json!({"ok": true, "text": text, "data": data, "tokens_est": tokens})
            } else {
                json!({"ok": true, "text": text, "tokens_est": tokens})
            };
            (200, out.to_string())
        }
        Err(e) => {
            let status = if e.contains("outside the served root") || e.contains("needs `") || e.starts_with("no route") { 400 } else { 404 };
            (status, json!({"ok": false, "error": e}).to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_and_root_jail() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("in.rs");
        std::fs::write(&inside, "pub fn a() {}\n").unwrap();
        let outside = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        std::fs::write(outside.path(), "pub fn b() {}\n").unwrap();
        let root = dir.path().canonicalize().unwrap();

        let (st, body) = handle("GET", "/healthz", "", Some(&root));
        assert_eq!(st, 200);
        assert!(body.contains("\"ok\":true"));

        let (st, body) = handle("POST", "/ls", r#"{"file":"in.rs"}"#, Some(&root));
        assert_eq!(st, 200, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["data"]["symbols"][0]["path"], "a");
        assert!(v["tokens_est"].as_u64().unwrap() > 0);

        let req = json!({"file": outside.path().display().to_string()}).to_string();
        let (st, body) = handle("POST", "/ls", &req, Some(&root));
        assert_eq!(st, 400, "{body}");
        assert!(body.contains("outside the served root"));

        let (st, _) = handle("POST", "/read", r#"{"file":"in.rs"}"#, Some(&root));
        assert_eq!(st, 400);
        let (st, body) = handle("POST", "/read", r#"{"file":"in.rs","symbol":"zz"}"#, Some(&root));
        assert_eq!(st, 404, "{body}");
        let (st, body) = handle("POST", "/map", r#"{"dir":".","budget":50}"#, Some(&root));
        assert_eq!(st, 200, "{body}");
        let (st, _) = handle("PUT", "/ls", "", None);
        assert_eq!(st, 405);
        // No index beside the checkout: a clean 404, never a crash.
        let (st, body) = handle("POST", "/where", r#"{"dir":".","query":"anything"}"#, Some(&root));
        assert_eq!(st, 404, "{body}");
        let out = tempfile::tempdir().unwrap();
        let req = json!({"index": out.path().display().to_string(), "query": "x"}).to_string();
        let (st, body) = handle("POST", "/where", &req, Some(&root));
        assert_eq!(st, 400, "{body}");
        assert!(body.contains("outside the served root"));
    }
}
