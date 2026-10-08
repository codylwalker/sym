//! Language detection, parser construction, and per-language conventions.

use serde::Serialize;
use std::path::Path;
use tree_sitter::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Rust,
    Lua,
    Python,
    TypeScript,
    Tsx,
    JavaScript,
    Go,
}

/// The extension list quoted in error messages.
pub const EXTENSIONS: &str = "rs/lua/py/ts/tsx/js/jsx/mjs/cjs/go";

pub fn lang_of(path: &Path) -> Option<Lang> {
    match path.extension().and_then(|e| e.to_str())? {
        "rs" => Some(Lang::Rust),
        "lua" => Some(Lang::Lua),
        "py" | "pyi" => Some(Lang::Python),
        "ts" | "mts" | "cts" => Some(Lang::TypeScript),
        "tsx" => Some(Lang::Tsx),
        "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
        "go" => Some(Lang::Go),
        _ => None,
    }
}

pub fn parser_for(lang: Lang) -> Result<Parser, String> {
    let mut p = Parser::new();
    let l: tree_sitter::Language = match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::Lua => tree_sitter_lua::LANGUAGE.into(),
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Lang::Go => tree_sitter_go::LANGUAGE.into(),
    };
    p.set_language(&l)
        .map_err(|e| format!("tree-sitter language init failed: {e}"))?;
    Ok(p)
}

/// Separator between a container and its member in a qualified path.
pub fn path_sep(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => "::",
        _ => ".",
    }
}

/// Whether a (left-trimmed) line directly above a symbol belongs to its
/// documentation or attributes and should be included by `read`.
pub fn is_doc_line(lang: Lang, prev: &str) -> bool {
    match lang {
        Lang::Rust => prev.starts_with("///") || prev.starts_with("//!") || prev.starts_with("#["),
        Lang::Lua => prev.starts_with("--"),
        Lang::Python => prev.starts_with('#') || prev.starts_with('@'),
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
            prev.starts_with("//")
                || prev.starts_with("/*")
                || prev.starts_with('*')
                || prev.starts_with('@')
        }
        Lang::Go => prev.starts_with("//"),
    }
}

/// Lines that look like imports, for the repo map's fan-in ranking.
pub fn is_import_line(t: &str) -> bool {
    t.starts_with("use ")
        || t.contains("require(")
        || t.contains("require \"")
        || t.contains("require'")
        || t.starts_with("import ")
        || t.starts_with("from ")
        || t.starts_with("export ") && t.contains(" from ")
}
