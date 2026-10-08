//! `sym mcp` — the three verbs as MCP tools over stdio (newline-delimited
//! JSON-RPC 2.0 on stdin/stdout). Hand-rolled: no async runtime, and stdout
//! carries nothing but protocol bytes.

use crate::{cli, ops, render};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

const PROTOCOL_VERSION: &str = "2024-11-05";

pub fn serve_stdio() -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve_io(stdin.lock(), stdout.lock())
}

/// The stdio JSON-RPC loop, generic over reader/writer so it is testable
/// with in-memory buffers. One message per line; one response per line.
pub fn serve_io<R: BufRead, W: Write>(mut reader: R, mut out: W) -> Result<(), String> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let msg = line.trim();
        if msg.is_empty() {
            continue;
        }
        let msg = msg.strip_prefix('\u{feff}').unwrap_or(msg);
        let req: Value = match serde_json::from_str(msg) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(resp) = handle_message(&req) {
            writeln!(out, "{}", serde_json::to_string(&resp).unwrap_or_default())
                .map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn handle_message(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => {
            let ver = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSION);
            Some(ok_result(
                id,
                json!({
                    "protocolVersion": ver,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "sym", "version": env!("CARGO_PKG_VERSION") },
                }),
            ))
        }
        "notifications/initialized" | "notifications/cancelled" => None,
        "ping" => Some(ok_result(id, json!({}))),
        "tools/list" => Some(ok_result(id, json!({ "tools": tool_defs() }))),
        "tools/call" => Some(handle_tool_call(id, &params)),
        _ if id.is_some() => Some(err_result(id, -32601, &format!("method not found: {method}"))),
        _ => None,
    }
}

fn ok_result(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result })
}

fn err_result(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null),
            "error": { "code": code, "message": message } })
}

/// Every tool returns through here: a failed read is a normal `isError`
/// result, not a protocol error.
fn tool_text(id: Option<Value>, text: String, is_error: bool) -> Value {
    ok_result(
        id,
        json!({ "content": [ { "type": "text", "text": text } ], "isError": is_error }),
    )
}

fn obj_schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

pub fn tool_defs() -> Value {
    let json_flag = json!({ "type": "boolean", "description": "Return JSON instead of text (default false)." });
    json!([
        {
            "name": "sym_ls",
            "description": "Skeleton of one source file: every symbol with its line range and one-line signature. Use this BEFORE reading a file of more than ~200 lines; the ranges make a ranged Read the fallback. Languages: rs/lua/py/ts/tsx/js/go.",
            "inputSchema": obj_schema(json!({
                "file": { "type": "string", "description": "Path to the source file." },
                "json": json_flag,
            }), &["file"]),
        },
        {
            "name": "sym_read",
            "description": "One symbol's source, line-numbered, with the doc comment or attributes directly above it. `symbol` is the leaf name or the qualified path from sym_ls (Widget::new, Runner.helper, Server.Serve); `impl Trait for Type`, `Type as Trait`, `Type::method` and short trait paths are accepted too. One call answers \"where is X defined and what does it do\".",
            "inputSchema": obj_schema(json!({
                "file": { "type": "string" },
                "symbol": { "type": "string" },
                "json": json_flag,
            }), &["file", "symbol"]),
        },
        {
            "name": "sym_find",
            "description": "Definitions by name across a directory tree: every symbol whose leaf name or qualified path equals `name` (or starts with it when `prefix` is true), with kind, qualified path, file and line range. Grep finds mentions; this finds the definition.",
            "inputSchema": obj_schema(json!({
                "name": { "type": "string" },
                "dir": { "type": "string", "description": "Directory to search (default: current)." },
                "prefix": { "type": "boolean" },
                "json": json_flag,
            }), &["name"]),
        },
        {
            "name": "sym_map",
            "description": "Repo map fitted to a token budget: per-file top-level signatures, files ranked by how often others import them. Use it to orient in an unfamiliar directory instead of listing and reading files.",
            "inputSchema": obj_schema(json!({
                "dir": { "type": "string" },
                "budget": { "type": "integer", "description": "Approximate token budget (default 1000)." },
                "json": json_flag,
            }), &["dir"]),
        },
    ])
}

fn handle_tool_call(id: Option<Value>, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let json_out = args.get("json").and_then(Value::as_bool).unwrap_or(false);
    let str_arg = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
    let result: Result<String, String> = match name {
        "sym_ls" => match str_arg("file") {
            Some(f) => ops::ls(Path::new(&f))
                .and_then(|o| cli::finish(render::ls_text(&o), &o, json_out, false)),
            None => Err("sym_ls needs `file`".into()),
        },
        "sym_read" => match (str_arg("file"), str_arg("symbol")) {
            (Some(f), Some(s)) => ops::read(Path::new(&f), &s)
                .and_then(|o| cli::finish(render::read_text(&o), &o, json_out, false)),
            _ => Err("sym_read needs `file` and `symbol`".into()),
        },
        "sym_find" => match str_arg("name") {
            Some(n) => {
                let dir = str_arg("dir").unwrap_or_else(|| ".".into());
                let prefix = args.get("prefix").and_then(Value::as_bool).unwrap_or(false);
                ops::find(Path::new(&dir), &n, prefix)
                    .and_then(|o| cli::finish(render::find_text(&o), &o, json_out, false))
            }
            None => Err("sym_find needs `name`".into()),
        },
        "sym_map" => match str_arg("dir") {
            Some(d) => {
                let budget = args.get("budget").and_then(Value::as_u64).unwrap_or(1000) as usize;
                ops::map(Path::new(&d), budget)
                    .and_then(|o| cli::finish(render::map_text(&o), &o, json_out, false))
            }
            None => Err("sym_map needs `dir`".into()),
        },
        _ => return err_result(id, -32602, &format!("unknown tool: {name}")),
    };
    match result {
        Ok(text) => tool_text(id, text, false),
        Err(e) => tool_text(id, e, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(req: &str) -> Option<Value> {
        handle_message(&serde_json::from_str(req).unwrap())
    }

    #[test]
    fn initialize_and_list_tools() {
        let r = call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "sym");
        let r = call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);
        for t in tools {
            assert!(t["description"].as_str().unwrap().len() > 40);
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn tool_call_reads_a_file_and_errors_are_soft() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.py");
        std::fs::write(&f, "def a():\n    pass\n").unwrap();
        let req = json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"sym_ls","arguments":{"file": f.display().to_string()}}});
        let r = handle_message(&req).unwrap();
        assert_eq!(r["result"]["isError"], false);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("def a()"));
        let req = json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"sym_read","arguments":{"file": f.display().to_string(), "symbol":"zz"}}});
        let r = handle_message(&req).unwrap();
        assert_eq!(r["result"]["isError"], true);
        let r = call(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#).unwrap();
        assert_eq!(r["error"]["code"], -32602);
        let r = call(r#"{"jsonrpc":"2.0","id":6,"method":"bogus"}"#).unwrap();
        assert_eq!(r["error"]["code"], -32601);
        assert!(call(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn serve_io_frames_one_response_per_line() {
        let input = "\u{feff}{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\nnot json\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
        let mut out = Vec::new();
        serve_io(std::io::Cursor::new(input), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().nth(1).unwrap().contains("sym_map"));
    }
}
