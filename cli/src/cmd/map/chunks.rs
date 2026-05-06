use crate::cmd::map::lang::Lang;
use tree_sitter::{Node, Parser};

pub const MIN_CHUNK_LINES: usize = 4;
pub const MAX_CHUNK_LINES: usize = 80;
pub const MAX_FILE_BYTES: usize = 200_000;
pub const MIN_CHUNK_CHARS: usize = 40;
pub const MAX_CHUNK_CHARS: usize = 6_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

pub fn chunk_file(lang: Lang, source: &str) -> Vec<Chunk> {
    if source.len() > MAX_FILE_BYTES {
        return Vec::new();
    }
    if let Some(out) = chunk_via_tree_sitter(lang, source) {
        if !out.is_empty() {
            return out;
        }
    }
    chunk_via_lines(source)
}

fn chunk_via_tree_sitter(lang: Lang, source: &str) -> Option<Vec<Chunk>> {
    let mut parser = Parser::new();
    parser.set_language(&lang.ts_language()).ok()?;
    let tree = parser.parse(source.as_bytes(), None)?;
    let root = tree.root_node();
    let mut out = Vec::new();
    let mut covered_lines = 0usize;
    let total_lines = source.lines().count();

    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if !is_chunkable(&child) {
            continue;
        }
        let span = child.start_position().row..child.end_position().row + 1;
        let len = span.end - span.start;
        if len < MIN_CHUNK_LINES {
            continue;
        }
        let text = source[child.byte_range()].to_string();
        if text.trim().len() < MIN_CHUNK_CHARS {
            continue;
        }
        if len <= MAX_CHUNK_LINES && text.len() <= MAX_CHUNK_CHARS {
            out.push(Chunk {
                start_line: span.start as u32,
                end_line: (span.end - 1) as u32,
                text,
            });
        } else {
            if is_container_kind(child.kind()) {
                if let Some(header) = collapsed_container_chunk(source, &child) {
                    out.push(header);
                }
            }
            for c in slide_window_chunks(source, span.start as u32, (span.end - 1) as u32) {
                out.push(c);
            }
        }
        covered_lines += len;
    }

    if covered_lines * 2 < total_lines {
        for c in slide_window_chunks(source, 0, total_lines.saturating_sub(1) as u32) {
            if !out.iter().any(|existing| existing.start_line == c.start_line) {
                out.push(c);
            }
        }
    }
    Some(out)
}

fn is_chunkable(node: &Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_item"
            | "function_declaration"
            | "function_definition"
            | "method_declaration"
            | "method_definition"
            | "class_declaration"
            | "class_definition"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "impl_item"
            | "interface_declaration"
            | "lexical_declaration"
            | "export_statement"
            | "module"
            | "namespace_declaration"
    )
}

fn chunk_via_lines(source: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        let end = (start + MAX_CHUNK_LINES).min(lines.len());
        let span_text = lines[start..end].join("\n");
        if span_text.trim().len() >= MIN_CHUNK_CHARS {
            out.push(Chunk {
                start_line: start as u32,
                end_line: (end - 1) as u32,
                text: span_text,
            });
        }
        if end >= lines.len() {
            break;
        }
        start = end;
    }
    out
}

fn is_container_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "class_definition"
            | "impl_item"
            | "trait_item"
            | "interface_declaration"
            | "namespace_declaration"
            | "module"
    )
}

fn collapsed_container_chunk(source: &str, container: &Node<'_>) -> Option<Chunk> {
    let header_line = container.start_position().row;
    let lines: Vec<&str> = source.lines().collect();
    if header_line >= lines.len() {
        return None;
    }
    let mut buf = String::new();
    buf.push_str(lines[header_line].trim_end());
    buf.push('\n');
    let mut cursor = container.walk();
    let mut last_line: usize = header_line;
    for child in container.named_children(&mut cursor) {
        if !is_signature_kind(child.kind()) {
            continue;
        }
        let row = child.start_position().row;
        if row >= lines.len() || row <= last_line {
            continue;
        }
        let sig_line = lines[row].trim_end();
        if sig_line.is_empty() {
            continue;
        }
        buf.push_str("  ");
        buf.push_str(sig_line.trim_start());
        buf.push_str(" { ... }\n");
        last_line = row;
    }
    if buf.trim().len() < MIN_CHUNK_CHARS {
        return None;
    }
    let end_line = container.end_position().row.min(lines.len().saturating_sub(1));
    Some(Chunk {
        start_line: header_line as u32,
        end_line: end_line as u32,
        text: buf,
    })
}

fn is_signature_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "function_declaration"
            | "function_definition"
            | "method_declaration"
            | "method_definition"
    )
}

fn slide_window_chunks(source: &str, start_line: u32, end_line: u32) -> Vec<Chunk> {
    let lines: Vec<&str> = source.lines().collect();
    let s = start_line as usize;
    let e = (end_line as usize + 1).min(lines.len());
    let mut out = Vec::new();
    let mut cur = s;
    while cur < e {
        let stop = (cur + MAX_CHUNK_LINES).min(e);
        let text = lines[cur..stop].join("\n");
        if text.trim().len() >= MIN_CHUNK_CHARS {
            out.push(Chunk {
                start_line: cur as u32,
                end_line: (stop - 1) as u32,
                text,
            });
        }
        if stop >= e {
            break;
        }
        cur = stop;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_rust_functions() {
        let src = "pub fn alpha() -> i32 {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n\npub fn beta() -> i32 {\n    let a = 3;\n    let b = 4;\n    a + b\n}\n";
        let chunks = chunk_file(Lang::Rust, src);
        assert!(chunks.len() >= 2, "got {chunks:?}");
        assert!(chunks.iter().any(|c| c.text.contains("alpha")));
        assert!(chunks.iter().any(|c| c.text.contains("beta")));
    }

    #[test]
    fn skips_huge_files() {
        let big = "fn x() {}\n".repeat(MAX_FILE_BYTES);
        assert!(chunk_file(Lang::Rust, &big).is_empty());
    }

    #[test]
    fn line_fallback_handles_freeform_source() {
        let src = (0..100).map(|i| format!("// line {}\nlet v{} = {};", i, i, i)).collect::<Vec<_>>().join("\n");
        let chunks = chunk_via_lines(&src);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| c.end_line >= c.start_line));
    }

    #[test]
    fn chunks_large_function_via_window() {
        let body = (0..120).map(|i| format!("    let x{} = {};", i, i)).collect::<Vec<_>>().join("\n");
        let src = format!("fn big() {{\n{}\n}}\n", body);
        let chunks = chunk_file(Lang::Rust, &src);
        assert!(chunks.len() >= 2, "expected window split, got {}", chunks.len());
    }
}
