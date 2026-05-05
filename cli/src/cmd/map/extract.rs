use serde::{Deserialize, Serialize};
use std::path::Path;
use tree_sitter::{Node, Parser};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
}

impl Lang {
    pub fn from_path(p: &Path) -> Option<Lang> {
        match p.extension().and_then(|s| s.to_str()) {
            Some("rs") => Some(Lang::Rust),
            Some("ts") => Some(Lang::TypeScript),
            Some("tsx") => Some(Lang::Tsx),
            Some("js") | Some("mjs") | Some("cjs") | Some("jsx") => Some(Lang::JavaScript),
            _ => None,
        }
    }

    pub fn extensions() -> &'static [&'static str] {
        // Drives the walker's file-extension filter. Keep aligned with
        // `Lang::from_path` so we don't crawl files we can't extract.
        &["rs", "ts", "tsx", "js", "mjs", "cjs", "jsx"]
    }
}

/// Dispatch on language. Returns `Vec<Tag>` ordered by tree-walk
/// pre-order — callers that need a stable order should sort by
/// `(line, name)`.
pub fn extract(source: &str, lang: Lang) -> Vec<Tag> {
    match lang {
        Lang::Rust => extract_rust(source),
        Lang::TypeScript => extract_ts_family(source, tree_sitter_typescript::language_typescript()),
        Lang::Tsx => extract_ts_family(source, tree_sitter_typescript::language_tsx()),
        Lang::JavaScript => extract_ts_family(source, tree_sitter_javascript::language()),
    }
}

pub fn extract_rust(source: &str) -> Vec<Tag> {
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_rust::language()).is_err() {
        return Vec::new();
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    walk_rust(tree.root_node(), bytes, source, &mut out);
    out
}

/// Shared extractor for the JS/TS/TSX family. They share the bulk of
/// node kinds (`function_declaration`, `class_declaration`,
/// `method_definition`) — TypeScript and TSX add `interface_declaration`,
/// `type_alias_declaration`, and an `enum_declaration` that maps to
/// `enum`. The dispatch table is per-grammar so node kinds match what
/// the actual parser emits, avoiding silent kind drift between languages.
fn extract_ts_family(source: &str, language: tree_sitter::Language) -> Vec<Tag> {
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    walk_ts(tree.root_node(), bytes, source, &mut out);
    out
}

fn walk_rust<'a>(node: Node<'a>, bytes: &'a [u8], source: &str, out: &mut Vec<Tag>) {
    if let Some(tag) = rust_node_to_tag(node, bytes, source) {
        out.push(tag);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_rust(child, bytes, source, out);
    }
}

fn walk_ts<'a>(node: Node<'a>, bytes: &'a [u8], source: &str, out: &mut Vec<Tag>) {
    if let Some(tag) = ts_node_to_tag(node, bytes, source) {
        out.push(tag);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_ts(child, bytes, source, out);
    }
}

fn rust_node_to_tag(node: Node<'_>, bytes: &[u8], source: &str) -> Option<Tag> {
    let kind = match node.kind() {
        "function_item" | "function_signature_item" => "fn",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "trait_item" => "trait",
        "impl_item" => "impl",
        "mod_item" => "mod",
        _ => return None,
    };
    let name = identifier_for(node, kind, bytes)?;
    let line = (node.start_position().row as u32) + 1;
    let signature = signature_line(node, source);
    Some(Tag {
        name,
        kind: kind.to_string(),
        line,
        signature,
    })
}

fn ts_node_to_tag(node: Node<'_>, bytes: &[u8], source: &str) -> Option<Tag> {
    let kind = match node.kind() {
        "function_declaration" | "generator_function_declaration" => "fn",
        "method_definition" => "method",
        "class_declaration" | "abstract_class_declaration" => "class",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "enum_declaration" => "enum",
        _ => return None,
    };
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(bytes).ok()?.to_string();
    let line = (node.start_position().row as u32) + 1;
    let signature = signature_line(node, source);
    Some(Tag {
        name,
        kind: kind.to_string(),
        line,
        signature,
    })
}

fn identifier_for(node: Node<'_>, kind: &str, bytes: &[u8]) -> Option<String> {
    if kind == "impl" {
        return impl_identifier(node, bytes);
    }
    let name_node = node.child_by_field_name("name")?;
    name_node.utf8_text(bytes).ok().map(|s| s.to_string())
}

fn impl_identifier(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    let type_node = node.child_by_field_name("type")?;
    let type_text = type_node.utf8_text(bytes).ok()?.to_string();
    if let Some(trait_node) = node.child_by_field_name("trait") {
        if let Ok(trait_text) = trait_node.utf8_text(bytes) {
            return Some(format!("{} for {}", trait_text, type_text));
        }
    }
    Some(type_text)
}

fn signature_line(node: Node<'_>, source: &str) -> String {
    let start = node.start_byte();
    let bytes = source.as_bytes();
    let limit = bytes.len();
    let mut end = start;
    while end < limit {
        let b = bytes[end];
        if b == b'\n' {
            break;
        }
        if b == b'{' || b == b';' {
            break;
        }
        end += 1;
    }
    let slice = &source[start..end];
    let trimmed = slice.trim_end();
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(tags: &[Tag]) -> Vec<&str> {
        tags.iter().map(|t| t.name.as_str()).collect()
    }

    #[test]
    fn extract_function_signature() {
        let src = "pub fn hello(name: &str) -> String {\n    name.to_string()\n}\n";
        let tags = extract_rust(src);
        assert_eq!(tags.len(), 1);
        let t = &tags[0];
        assert_eq!(t.kind, "fn");
        assert_eq!(t.name, "hello");
        assert_eq!(t.line, 1);
        assert_eq!(t.signature, "pub fn hello(name: &str) -> String");
    }

    #[test]
    fn extract_struct() {
        let src = "pub struct User { id: u64 }\n";
        let tags = extract_rust(src);
        let kinds: Vec<&str> = tags.iter().map(|t| t.kind.as_str()).collect();
        assert!(kinds.contains(&"struct"));
        let s = tags.iter().find(|t| t.kind == "struct").unwrap();
        assert_eq!(s.name, "User");
        assert_eq!(s.signature, "pub struct User");
    }

    #[test]
    fn extract_enum_with_variants() {
        let src = "enum Color { Red, Blue }\n";
        let tags = extract_rust(src);
        let e = tags.iter().find(|t| t.kind == "enum").unwrap();
        assert_eq!(e.name, "Color");
        assert_eq!(e.signature, "enum Color");
    }

    #[test]
    fn extract_trait() {
        let src = "pub trait Greet { fn hi(&self); }\n";
        let tags = extract_rust(src);
        assert!(tags.iter().any(|t| t.kind == "trait" && t.name == "Greet"));
    }

    #[test]
    fn extract_impl_inherent() {
        let src = "struct Foo;\nimpl Foo { fn bar(&self) {} }\n";
        let tags = extract_rust(src);
        let i = tags.iter().find(|t| t.kind == "impl").unwrap();
        assert_eq!(i.name, "Foo");
        assert!(i.signature.starts_with("impl Foo"));
    }

    #[test]
    fn extract_impl_trait_for_type() {
        let src = "struct Foo;\nimpl Display for Foo { fn fmt(&self) {} }\n";
        let tags = extract_rust(src);
        let i = tags.iter().find(|t| t.kind == "impl").unwrap();
        assert_eq!(i.name, "Display for Foo");
    }

    #[test]
    fn extract_mod_with_brace() {
        let src = "pub mod foo {\n    fn x() {}\n}\n";
        let tags = extract_rust(src);
        let m = tags.iter().find(|t| t.kind == "mod").unwrap();
        assert_eq!(m.name, "foo");
    }

    #[test]
    fn extract_nested_fn_inside_impl() {
        let src = "impl Foo {\n    pub fn a(&self) {}\n    fn b(&self) -> i32 { 0 }\n}\n";
        let tags = extract_rust(src);
        let n = names(&tags);
        assert!(n.contains(&"a"));
        assert!(n.contains(&"b"));
        assert!(tags.iter().any(|t| t.kind == "impl"));
    }

    #[test]
    fn extract_line_numbers_one_based() {
        let src = "// header line\nfn second_line_fn() {}\n";
        let tags = extract_rust(src);
        let f = &tags[0];
        assert_eq!(f.line, 2);
    }

    #[test]
    fn extract_signature_strips_body() {
        let src = "fn complex(a: i32, b: i32) -> i32 { a + b }\n";
        let tags = extract_rust(src);
        assert_eq!(tags[0].signature, "fn complex(a: i32, b: i32) -> i32");
    }

    #[test]
    fn extract_signature_strips_semicolon() {
        let src = "trait Animal { fn name(&self) -> &str; }\n";
        let tags = extract_rust(src);
        let inner_fn = tags.iter().find(|t| t.name == "name").unwrap();
        assert_eq!(inner_fn.signature, "fn name(&self) -> &str");
    }

    #[test]
    fn extract_empty_file_no_tags() {
        let tags = extract_rust("");
        assert!(tags.is_empty());
    }

    #[test]
    fn extract_invalid_rust_does_not_panic() {
        let src = "fn (((( broken";
        let _ = extract_rust(src);
    }

    #[test]
    fn extract_multiple_top_level_items() {
        let src = "
struct A;
struct B;
fn one() {}
fn two() {}
enum E { X }
trait T {}
";
        let tags = extract_rust(src);
        let kinds_count = |k: &str| tags.iter().filter(|t| t.kind == k).count();
        assert_eq!(kinds_count("struct"), 2);
        assert_eq!(kinds_count("fn"), 2);
        assert_eq!(kinds_count("enum"), 1);
        assert_eq!(kinds_count("trait"), 1);
    }

    #[test]
    fn lang_from_path_recognizes_known_extensions() {
        use std::path::Path;
        assert_eq!(Lang::from_path(Path::new("a.rs")), Some(Lang::Rust));
        assert_eq!(Lang::from_path(Path::new("a.ts")), Some(Lang::TypeScript));
        assert_eq!(Lang::from_path(Path::new("a.tsx")), Some(Lang::Tsx));
        assert_eq!(Lang::from_path(Path::new("a.js")), Some(Lang::JavaScript));
        assert_eq!(Lang::from_path(Path::new("a.mjs")), Some(Lang::JavaScript));
        assert_eq!(Lang::from_path(Path::new("a.cjs")), Some(Lang::JavaScript));
        assert_eq!(Lang::from_path(Path::new("a.jsx")), Some(Lang::JavaScript));
        assert_eq!(Lang::from_path(Path::new("README.md")), None);
        assert_eq!(Lang::from_path(Path::new("Cargo.toml")), None);
    }

    #[test]
    fn extract_typescript_function_and_class() {
        let src = "export function greet(name: string): string {\n  return `hi ${name}`;\n}\nexport class User {\n  id: number;\n}\n";
        let tags = extract(src, Lang::TypeScript);
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"User"));
        let g = tags.iter().find(|t| t.name == "greet").unwrap();
        assert_eq!(g.kind, "fn");
        assert!(g.signature.contains("greet"));
        let u = tags.iter().find(|t| t.name == "User").unwrap();
        assert_eq!(u.kind, "class");
    }

    #[test]
    fn extract_typescript_interface_type_enum() {
        let src = "interface Shape { area(): number }\ntype ID = string | number;\nenum Color { Red, Blue }\n";
        let tags = extract(src, Lang::TypeScript);
        let kind_for = |name: &str| tags.iter().find(|t| t.name == name).map(|t| t.kind.as_str());
        assert_eq!(kind_for("Shape"), Some("interface"));
        assert_eq!(kind_for("ID"), Some("type"));
        assert_eq!(kind_for("Color"), Some("enum"));
    }

    #[test]
    fn extract_tsx_jsx_components() {
        let src = "export function Button(props: { label: string }) { return <button>{props.label}</button>; }\n";
        let tags = extract(src, Lang::Tsx);
        assert!(tags.iter().any(|t| t.name == "Button" && t.kind == "fn"));
    }

    #[test]
    fn extract_javascript_function_class_method() {
        let src = "function add(a, b) { return a + b; }\nclass Box {\n  open() { return true; }\n}\n";
        let tags = extract(src, Lang::JavaScript);
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"add"));
        assert!(names.contains(&"Box"));
        assert!(names.contains(&"open"));
        assert_eq!(
            tags.iter().find(|t| t.name == "open").unwrap().kind,
            "method"
        );
    }

    #[test]
    fn extract_invalid_typescript_does_not_panic() {
        let src = "function ((( broken";
        let _ = extract(src, Lang::TypeScript);
    }

    #[test]
    fn extract_generic_function_signature_preserved() {
        let src = "pub fn map<T, U>(t: T, f: impl Fn(T) -> U) -> U { f(t) }\n";
        let tags = extract_rust(src);
        assert_eq!(
            tags[0].signature,
            "pub fn map<T, U>(t: T, f: impl Fn(T) -> U) -> U"
        );
    }
}
