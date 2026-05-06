use crate::cmd::map::lang::Lang;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub role: TagRole,
    pub kind: TagKind,
    pub name: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagRole {
    Def,
    Ref,
}

impl TagRole {
    pub fn as_str(self) -> &'static str {
        match self {
            TagRole::Def => "def",
            TagRole::Ref => "ref",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKind {
    Class,
    Interface,
    Function,
    Method,
    Module,
    Macro,
    Constant,
    Variable,
    Other,
}

impl TagKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TagKind::Class => "class",
            TagKind::Interface => "interface",
            TagKind::Function => "function",
            TagKind::Method => "method",
            TagKind::Module => "module",
            TagKind::Macro => "macro",
            TagKind::Constant => "constant",
            TagKind::Variable => "variable",
            TagKind::Other => "symbol",
        }
    }

    fn from_capture_suffix(suffix: &str) -> TagKind {
        match suffix {
            "class" | "struct" | "union" | "enum" | "trait" | "type" => TagKind::Class,
            "interface" => TagKind::Interface,
            "function" => TagKind::Function,
            "method" => TagKind::Method,
            "module" | "package" | "namespace" => TagKind::Module,
            "macro" => TagKind::Macro,
            "constant" => TagKind::Constant,
            "variable" | "field" | "property" => TagKind::Variable,
            _ => TagKind::Other,
        }
    }
}

pub fn extract(lang: Lang, source: &str) -> Result<Vec<Tag>, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .map_err(|e| format!("set language failed: {e}"))?;
    let tree = parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| "tree-sitter parse returned None".to_string())?;
    let query = Query::new(&lang.ts_language(), lang.tags_query())
        .map_err(|e| format!("query compile failed: {e}"))?;

    let names: Vec<&str> = query.capture_names().iter().copied().collect();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        let mut role_kind: Option<(TagRole, TagKind)> = None;
        let mut name_node: Option<tree_sitter::Node> = None;
        for cap in m.captures {
            let cap_name = names.get(cap.index as usize).copied().unwrap_or("");
            if cap_name == "name" {
                name_node = Some(cap.node);
            } else if let Some(suffix) = cap_name.strip_prefix("definition.") {
                role_kind = Some((TagRole::Def, TagKind::from_capture_suffix(suffix)));
            } else if let Some(suffix) = cap_name.strip_prefix("reference.") {
                role_kind = Some((TagRole::Ref, TagKind::from_capture_suffix(suffix)));
            }
        }
        let (Some(node), Some((role, kind))) = (name_node, role_kind) else { continue };
        let text = &source[node.byte_range()];
        if text.is_empty() {
            continue;
        }
        let start = node.start_position();
        let end = node.end_position();
        out.push(Tag {
            role,
            kind,
            name: text.to_string(),
            line: start.row as u32,
            col: start.column as u32,
            end_line: end.row as u32,
            end_col: end.column as u32,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_def_and_ref() {
        let src = r#"
fn greet(name: &str) -> String {
    format!("hi {}", name)
}

fn main() {
    let _ = greet("world");
}
"#;
        let tags = extract(Lang::Rust, src).unwrap();
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"greet"), "missing greet def: {names:?}");
        assert!(names.contains(&"main"), "missing main def: {names:?}");
        let has_def = tags.iter().any(|t| t.role == TagRole::Def && t.name == "greet");
        let has_ref = tags.iter().any(|t| t.role == TagRole::Ref && t.name == "greet");
        assert!(has_def && has_ref, "expected both def and ref of greet, got {tags:?}");
    }

    #[test]
    fn extracts_python() {
        let src = "def add(a, b):\n    return a + b\n\ndef main():\n    return add(1, 2)\n";
        let tags = extract(Lang::Python, src).unwrap();
        let defs: Vec<&str> = tags.iter().filter(|t| t.role == TagRole::Def).map(|t| t.name.as_str()).collect();
        assert!(defs.contains(&"add"));
        assert!(defs.contains(&"main"));
    }

    #[test]
    fn extracts_typescript() {
        let src = "export function greet(name: string) { return 'hi ' + name }\nfunction main() { return greet('w') }";
        let tags = extract(Lang::TypeScript, src).unwrap();
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"greet"), "TS tags: {names:?}");
    }

    #[test]
    fn unsupported_returns_no_tags_or_empty() {
        let tags = extract(Lang::Go, "").unwrap();
        assert!(tags.is_empty());
    }
}
