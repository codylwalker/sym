//! Claude Code PreToolUse helper. When the agent is about to `Read` a whole
//! source file that `sym` understands, add context pointing at the skeleton.
//! Never blocks; never prints when it has nothing to say.

use crate::lang::lang_of;
use serde_json::{json, Value};
use std::path::Path;

/// `input` is the hook's stdin JSON. Returns the JSON to print, or None.
pub fn pre_read(input: &str, min_lines: usize) -> Option<String> {
    let v: Value = serde_json::from_str(input).ok()?;
    if v.get("tool_name").and_then(Value::as_str) != Some("Read") {
        return None;
    }
    let ti = v.get("tool_input")?;
    // A ranged read is already the fallback we recommend; stay quiet.
    if ti.get("offset").is_some() || ti.get("limit").is_some() {
        return None;
    }
    let path = ti.get("file_path").and_then(Value::as_str)?;
    let p = Path::new(path);
    lang_of(p)?;
    let src = std::fs::read_to_string(p).ok()?;
    let lines = src.lines().count();
    if lines <= min_lines {
        return None;
    }
    let tokens = crate::tokens_est(src.len());
    let shown = crate::ops::display(p);
    let msg = format!(
        "{shown} is {lines} lines (~{tokens} tokens). Prefer `sym ls {shown}` for the skeleton \
         and `sym read {shown} <symbol>` for one symbol, or Read with offset/limit from the ranges."
    );
    let out = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": msg,
        }
    });
    Some(format!("{out}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, lines: usize) -> String {
        let p = dir.join(name);
        let body: String = (0..lines).map(|i| format!("fn f{i}() {{}}\n")).collect();
        std::fs::write(&p, body).unwrap();
        p.display().to_string()
    }

    #[test]
    fn big_read_gets_context_small_read_and_ranged_read_stay_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let big = write(dir.path(), "big.rs", 300);
        let small = write(dir.path(), "small.rs", 10);
        let input = |p: &str, extra: &str| {
            format!(r#"{{"tool_name":"Read","tool_input":{{"file_path":"{p}"{extra}}}}}"#)
        };
        let out = pre_read(&input(&big, ""), 200).unwrap();
        assert!(out.contains("hookSpecificOutput"));
        assert!(out.contains("sym ls"));
        assert!(pre_read(&input(&small, ""), 200).is_none());
        assert!(pre_read(&input(&big, r#","offset":10"#), 200).is_none());
        let other = format!(r#"{{"tool_name":"Bash","tool_input":{{"command":"cat {big}"}}}}"#);
        assert!(pre_read(&other, 200).is_none());
        assert!(pre_read("not json", 200).is_none());
    }
}
