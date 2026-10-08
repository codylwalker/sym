//! Symbol extraction: walk a tree-sitter tree and list the items worth
//! showing an agent (functions, types, classes, methods, …) with their line
//! ranges and a one-line signature.

use crate::lang::{path_sep, Lang};
use serde::Serialize;
use tree_sitter::Node;

/// One extracted symbol.
#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    /// Leaf name (`run`), and the qualified path for nested items
    /// (`GainOpts::run`, `M.update`, `Server.Serve`).
    pub name: String,
    pub path: String,
    pub kind: &'static str,
    /// 1-based, inclusive.
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
    pub depth: usize,
}

pub fn extract_symbols(src: &str, lang: Lang) -> Result<Vec<Symbol>, String> {
    let mut parser = crate::lang::parser_for(lang)?;
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| "parse failed".to_string())?;
    let mut out = Vec::new();
    walk(tree.root_node(), src, lang, "", 0, &mut out);
    Ok(out)
}

fn walk(node: Node, src: &str, lang: Lang, prefix: &str, depth: usize, out: &mut Vec<Symbol>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(Found { kind, name, qualified }) = symbol_of(child, src, lang) {
            let path = match qualified {
                Some(q) => q,
                None if prefix.is_empty() => name.clone(),
                None => format!("{prefix}{}{name}", path_sep(lang)),
            };
            out.push(Symbol {
                name: name.clone(),
                path: path.clone(),
                kind,
                start_line: child.start_position().row + 1,
                end_line: child.end_position().row + 1,
                signature: signature_of(child, src),
                depth,
            });
            // Recurse into containers so methods get listed under their
            // impl/class.
            if matches!(kind, "impl" | "class" | "mod" | "trait") {
                walk(child, src, lang, &path, depth + 1, out);
            }
        } else {
            // Transparent wrappers (decorated_definition, export_statement,
            // type_declaration, blocks, bodies).
            walk(child, src, lang, prefix, depth, out);
        }
    }
}

struct Found {
    kind: &'static str,
    name: String,
    /// A full path when the grammar carries the container in the node itself
    /// (Go methods: `(s *Server) Serve` → `Server.Serve`).
    qualified: Option<String>,
}

fn found(kind: &'static str, name: String) -> Option<Found> {
    Some(Found {
        kind,
        name,
        qualified: None,
    })
}

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn field_text(node: Node, field: &str, src: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| node_text(n, src).to_string())
}

/// Classify a node as a symbol worth listing.
fn symbol_of(node: Node, src: &str, lang: Lang) -> Option<Found> {
    match (lang, node.kind()) {
        (Lang::Rust, "function_item") => found("fn", field_text(node, "name", src)?),
        (Lang::Rust, "struct_item") => found("struct", field_text(node, "name", src)?),
        (Lang::Rust, "enum_item") => found("enum", field_text(node, "name", src)?),
        (Lang::Rust, "trait_item") => found("trait", field_text(node, "name", src)?),
        (Lang::Rust, "impl_item") => {
            let ty = field_text(node, "type", src)?;
            let name = match field_text(node, "trait", src) {
                Some(tr) => format!("{ty} as {tr}"),
                None => ty,
            };
            found("impl", name)
        }
        (Lang::Rust, "mod_item") => {
            // Only inline mods (with a body) are containers worth walking.
            node.child_by_field_name("body")?;
            found("mod", field_text(node, "name", src)?)
        }
        (Lang::Rust, "const_item") => found("const", field_text(node, "name", src)?),
        (Lang::Rust, "static_item") => found("static", field_text(node, "name", src)?),
        (Lang::Rust, "type_item") => found("type", field_text(node, "name", src)?),
        (Lang::Rust, "macro_definition") => found("macro", field_text(node, "name", src)?),

        (Lang::Python, "function_definition") => found("def", field_text(node, "name", src)?),
        (Lang::Python, "class_definition") => found("class", field_text(node, "name", src)?),

        (Lang::Lua, "function_declaration") => {
            // Name may be dotted (`M.update`) or method (`M:draw`).
            let name = node
                .child_by_field_name("name")
                .map(|n| node_text(n, src).to_string())?;
            found("function", name)
        }
        (Lang::Lua, "assignment_statement") => {
            // `X = function(...) end` / `local X = function(...) end`
            let text = node_text(node, src);
            let eq = text.find('=')?;
            let rhs = text[eq + 1..].trim_start();
            if rhs.starts_with("function") {
                let lhs = text[..eq].trim().trim_start_matches("local ").trim();
                if !lhs.is_empty() && !lhs.contains(',') {
                    return found("function", lhs.to_string());
                }
            }
            None
        }

        // TypeScript / TSX / JavaScript share one shape.
        (
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript,
            "function_declaration" | "generator_function_declaration",
        ) => found("function", field_text(node, "name", src)?),
        (Lang::TypeScript | Lang::Tsx | Lang::JavaScript, "class_declaration") => {
            found("class", field_text(node, "name", src)?)
        }
        (Lang::TypeScript | Lang::Tsx, "abstract_class_declaration") => {
            found("class", field_text(node, "name", src)?)
        }
        (Lang::TypeScript | Lang::Tsx | Lang::JavaScript, "method_definition") => {
            found("method", field_text(node, "name", src)?)
        }
        (Lang::TypeScript | Lang::Tsx, "interface_declaration") => {
            found("interface", field_text(node, "name", src)?)
        }
        (Lang::TypeScript | Lang::Tsx, "type_alias_declaration") => {
            found("type", field_text(node, "name", src)?)
        }
        (Lang::TypeScript | Lang::Tsx, "enum_declaration") => {
            found("enum", field_text(node, "name", src)?)
        }
        (
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript,
            "lexical_declaration" | "variable_declaration",
        ) => {
            // `const f = (…) => …` / `const f = function (…) {…}`
            let mut c = node.walk();
            let decl = node
                .children(&mut c)
                .find(|n| n.kind() == "variable_declarator")?;
            let value = decl.child_by_field_name("value")?;
            if matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "function" | "generator_function"
            ) {
                return found("function", field_text(decl, "name", src)?);
            }
            None
        }

        (Lang::Go, "function_declaration") => found("func", field_text(node, "name", src)?),
        (Lang::Go, "method_declaration") => {
            let name = field_text(node, "name", src)?;
            let recv = field_text(node, "receiver", src).unwrap_or_default();
            let ty = recv
                .trim_matches(|c| c == '(' || c == ')')
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_start_matches('*');
            let ty = ty.split('[').next().unwrap_or("").to_string();
            let qualified = if ty.is_empty() {
                None
            } else {
                Some(format!("{ty}.{name}"))
            };
            Some(Found {
                kind: "method",
                name,
                qualified,
            })
        }
        (Lang::Go, "type_spec") => found("type", field_text(node, "name", src)?),
        (Lang::Go, "const_spec") => found("const", field_text(node, "name", src)?),
        (Lang::Go, "var_spec") => found("var", field_text(node, "name", src)?),

        _ => None,
    }
}

/// First meaningful line of the node, as the signature.
fn signature_of(node: Node, src: &str) -> String {
    let text = node_text(node, src);
    let first = text.lines().next().unwrap_or("").trim();
    let mut sig = first.trim_end_matches('{').trim().to_string();
    if sig.chars().count() > 100 {
        sig = sig.chars().take(100).collect();
        sig.push('…');
    }
    sig
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST_SRC: &str = r#"
/// Doc comment.
pub fn top_level(x: u64) -> u64 { x }

pub struct Widget { pub id: u64 }

impl Widget {
    pub fn new(id: u64) -> Self { Self { id } }
    fn hidden(&self) -> u64 { self.id }
}

mod inner {
    pub fn nested() {}
}
"#;

    const PY_SRC: &str = r#"
def solo(a, b):
    return a + b

class Runner:
    def run(self):
        pass

    @staticmethod
    def helper():
        pass
"#;

    const LUA_SRC: &str = r#"
local M = {}

function M.update(dt)
    return dt
end

function M:draw()
end

local helper = function(x)
    return x
end

return M
"#;

    const TS_SRC: &str = r#"
export function add(a: number, b: number): number { return a + b; }
export const mul = (a: number, b: number): number => a * b;
export interface Shape { area(): number; }
export type Pair = [number, number];
export enum Color { Red, Green }
export class Circle implements Shape {
  constructor(private r: number) {}
  area(): number { return 1; }
  static unit(): Circle { return new Circle(1); }
}
const notAFunction = 3;
"#;

    const JS_SRC: &str = r#"
function greet(name) { return name; }
const shout = (s) => s.toUpperCase();
const legacy = function (x) { return x; };
class Counter {
  constructor() { this.n = 0; }
  inc() { this.n += 1; }
}
"#;

    const GO_SRC: &str = r#"
package fixture

const Limit = 3

var counter int

type Server struct { name string }

type Handler interface { Serve() error }

func New(name string) *Server { return &Server{name: name} }

func (s *Server) Serve() error { return nil }

func (g Generic[T]) Get() T { var z T; return z }
"#;

    fn names(src: &str, lang: Lang) -> Vec<String> {
        extract_symbols(src, lang)
            .unwrap()
            .into_iter()
            .map(|s| s.path)
            .collect()
    }

    #[test]
    fn rust_symbols_extracted_with_nesting() {
        let n = names(RUST_SRC, Lang::Rust);
        for want in ["top_level", "Widget", "Widget::new", "Widget::hidden", "inner::nested"] {
            assert!(n.contains(&want.to_string()), "missing {want} in {n:?}");
        }
    }

    #[test]
    fn rust_line_ranges_cover_bodies() {
        let syms = extract_symbols(RUST_SRC, Lang::Rust).unwrap();
        let new = syms.iter().find(|s| s.path == "Widget::new").unwrap();
        assert!(new.end_line >= new.start_line);
        assert_eq!(new.kind, "fn");
        assert!(new.signature.starts_with("pub fn new"));
    }

    #[test]
    fn python_symbols_extracted() {
        let n = names(PY_SRC, Lang::Python);
        for want in ["solo", "Runner", "Runner.run", "Runner.helper"] {
            assert!(n.contains(&want.to_string()), "missing {want} in {n:?}");
        }
    }

    #[test]
    fn lua_functions_extracted() {
        let n = names(LUA_SRC, Lang::Lua);
        assert!(n.contains(&"M.update".to_string()));
        assert!(n.contains(&"M:draw".to_string()));
        assert!(n.iter().any(|s| s.contains("helper")));
    }

    #[test]
    fn typescript_symbols_extracted() {
        let n = names(TS_SRC, Lang::TypeScript);
        for want in ["add", "mul", "Shape", "Pair", "Color", "Circle", "Circle.area", "Circle.unit"] {
            assert!(n.contains(&want.to_string()), "missing {want} in {n:?}");
        }
        assert!(!n.contains(&"notAFunction".to_string()));
    }

    #[test]
    fn javascript_symbols_extracted() {
        let n = names(JS_SRC, Lang::JavaScript);
        for want in ["greet", "shout", "legacy", "Counter", "Counter.inc"] {
            assert!(n.contains(&want.to_string()), "missing {want} in {n:?}");
        }
    }

    #[test]
    fn go_symbols_extracted_with_receivers() {
        let syms = extract_symbols(GO_SRC, Lang::Go).unwrap();
        let n: Vec<String> = syms.iter().map(|s| s.path.clone()).collect();
        for want in ["Limit", "counter", "Server", "Handler", "New", "Server.Serve", "Generic.Get"] {
            assert!(n.contains(&want.to_string()), "missing {want} in {n:?}");
        }
        let serve = syms.iter().find(|s| s.path == "Server.Serve").unwrap();
        assert_eq!(serve.name, "Serve");
        assert_eq!(serve.kind, "method");
    }

    #[test]
    fn signature_truncates_on_char_boundary() {
        let long = format!("fn {}() {{}}", "é".repeat(120));
        let syms = extract_symbols(&long, Lang::Rust).unwrap();
        assert!(syms[0].signature.ends_with('…'));
        assert_eq!(syms[0].signature.chars().count(), 101);
    }
}
