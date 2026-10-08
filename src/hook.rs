//! Claude Code PreToolUse helper. When the agent is about to `Read` a whole
//! source file that `sym` understands, add context pointing at the skeleton.
//! Never blocks; never prints when it has nothing to say.

use crate::lang::lang_of;
use serde_json::{json, Value};
use std::path::Path;

/// How the hook speaks: `Hint` adds context and lets the Read proceed;
/// `Deny` refuses a whole-file Read of a big source file (a ranged Read, a
/// `sym ls` or a `sym read` all still work), which is what actually changes
/// an agent's habit — the hint alone measured as ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hint,
    Deny,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        if s.eq_ignore_ascii_case("deny") {
            Mode::Deny
        } else {
            Mode::Hint
        }
    }
}

/// `input` is the hook's stdin JSON. Returns the JSON to print, or None.
pub fn pre_read(input: &str, min_lines: usize) -> Option<String> {
    pre_read_mode(input, min_lines, Mode::Hint)
}

pub fn pre_read_mode(input: &str, min_lines: usize, mode: Mode) -> Option<String> {
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
        "{shown} is {lines} lines (~{tokens} tokens): do not read it whole. First run `sym ls {shown}` \
         (Bash) or the sym_ls tool to get the symbols and their line ranges; then `sym read {shown} <symbol>` \
         / sym_read for the one you need, or Read with offset/limit taken from those ranges."
    );
    let out = match mode {
        Mode::Hint => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": msg,
            }
        }),
        Mode::Deny => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": format!("Whole-file Read refused by sym: {msg}"),
            }
        }),
    };
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
        assert!(!out.contains("permissionDecision"));
        let out = pre_read_mode(&input(&big, ""), 200, Mode::Deny).unwrap();
        assert!(out.contains("\"permissionDecision\":\"deny\""), "{out}");
        assert!(pre_read_mode(&input(&big, r#","offset":10"#), 200, Mode::Deny).is_none());
        assert_eq!(Mode::parse("DENY"), Mode::Deny);
        assert_eq!(Mode::parse("anything"), Mode::Hint);
        assert!(pre_read(&input(&small, ""), 200).is_none());
        assert!(pre_read(&input(&big, r#","offset":10"#), 200).is_none());
        let other = format!(r#"{{"tool_name":"Bash","tool_input":{{"command":"cat {big}"}}}}"#);
        assert!(pre_read(&other, 200).is_none());
        assert!(pre_read("not json", 200).is_none());
    }
}
