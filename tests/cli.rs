//! End-to-end checks through the built binary.

use std::io::Write as _;
use std::process::{Command, Stdio};

fn sym() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sym"))
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = sym().args(args).output().expect("spawn sym");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn ls_text_matches_the_legacy_shape() {
    let (ok, out, _) = run(&["ls", &fixture("a.rs")]);
    assert!(ok);
    let first = out.lines().next().unwrap();
    assert!(first.ends_with("a.rs — 23 lines, 7 symbols"), "{first}");
    assert!(out.contains("    4-6     pub fn top_level(x: u64) -> u64"), "{out}");
    assert!(out.contains("   13-15      pub fn new(id: u64) -> Self"), "{out}");
    assert!(!out.contains("fn ") || out.lines().all(|l| !l.starts_with("fn")));
}

#[test]
fn ls_json_and_est() {
    let (ok, out, _) = run(&["ls", &fixture("a.go"), "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let paths: Vec<&str> = v["symbols"].as_array().unwrap().iter().map(|s| s["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["Limit", "counter", "Server", "Handler", "New", "Server.Serve"]);
    assert!(v["tokens_est"].as_u64().unwrap() > 10);
    let (ok, out, _) = run(&["ls", &fixture("a.ts"), "--est"]);
    assert!(ok);
    assert!(out.lines().last().unwrap().starts_with("~"), "{out}");
    assert!(out.contains("static unit(): Circle"), "{out}");
}

#[test]
fn read_pulls_doc_block_and_qualified_paths() {
    let (ok, out, _) = run(&["read", &fixture("a.rs"), "Widget::new"]);
    assert!(ok, "{out}");
    assert!(out.starts_with("fn Widget::new ("), "{out}");
    assert!(out.contains("lines 13-15)"), "{out}");
    let (ok, out, _) = run(&["read", &fixture("a.go"), "Serve"]);
    assert!(ok, "{out}");
    assert!(out.contains("// Serve runs it."), "{out}");
    let (ok, _, err) = run(&["read", &fixture("a.rs"), "nope"]);
    assert!(!ok);
    assert!(err.contains("not found"), "{err}");
    let (ok, out, _) = run(&["read", &fixture("a.tsx"), "Button"]);
    assert!(ok, "{out}");
    assert!(out.contains("export const Button"), "{out}");
}

#[test]
fn map_fits_a_budget() {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let (ok, out, _) = run(&["map", &dir, "--budget", "60"]);
    assert!(ok, "{out}");
    assert!(out.contains("budget reached"), "{out}");
    let (ok, out, _) = run(&["map", &dir, "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["omitted"], 0);
    assert_eq!(v["total_files"], 7);
}

#[test]
fn mcp_over_stdio_lists_and_calls_tools() {
    let mut child = sym()
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let f = fixture("a.py");
    let script = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{}}}}\n\
         {{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}}\n\
         {{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{{\"name\":\"sym_read\",\"arguments\":{{\"file\":\"{f}\",\"symbol\":\"helper\"}}}}}}\n"
    );
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[1].contains("sym_ls") && lines[1].contains("sym_map"));
    assert!(lines[2].contains("@staticmethod"), "{}", lines[2]);
}

#[test]
fn hook_pre_speaks_only_for_big_files() {
    let f = fixture("a.rs");
    let input = format!(r#"{{"tool_name":"Read","tool_input":{{"file_path":"{f}"}}}}"#);
    let mut child = sym().args(["hook", "pre", "--min-lines", "5"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("additionalContext"));
    let mut child = sym().args(["hook", "pre"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn find_lists_definitions_across_fixtures() {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let (ok, out, _) = run(&["find", "Serve", &dir]);
    assert!(ok, "{out}");
    assert!(out.contains("a.go:") && out.contains("Server.Serve"), "{out}");
    let (ok, out, _) = run(&["find", "Ser", &dir, "--prefix", "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["hits"].as_array().unwrap().len() >= 2);
}
