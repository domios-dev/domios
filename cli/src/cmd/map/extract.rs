use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub signature: String,
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
    walk(tree.root_node(), bytes, source, &mut out);
    out
}

fn walk<'a>(node: Node<'a>, bytes: &'a [u8], source: &str, out: &mut Vec<Tag>) {
    if let Some(tag) = node_to_tag(node, bytes, source) {
        out.push(tag);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, bytes, source, out);
    }
}

fn node_to_tag(node: Node<'_>, bytes: &[u8], source: &str) -> Option<Tag> {
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
    fn extract_generic_function_signature_preserved() {
        let src = "pub fn map<T, U>(t: T, f: impl Fn(T) -> U) -> U { f(t) }\n";
        let tags = extract_rust(src);
        assert_eq!(
            tags[0].signature,
            "pub fn map<T, U>(t: T, f: impl Fn(T) -> U) -> U"
        );
    }
}
