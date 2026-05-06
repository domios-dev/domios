use crate::cmd::map::store::{NewNode, Store};
use protobuf::Message;
use scip::types::{symbol_information::Kind as ScipKind, Index};
use std::path::Path;

#[derive(Debug, Default, serde::Serialize)]
pub struct ScipReport {
    pub indexes: usize,
    pub documents: usize,
    pub symbols: usize,
    pub edges: usize,
}

pub fn load_workspace(workspace_root: &Path, store: &Store) -> Result<ScipReport, String> {
    let mut report = ScipReport::default();
    let scip_files = find_scip_files(workspace_root);
    for path in scip_files {
        match load_one(&path, store, workspace_root, &mut report) {
            Ok(()) => report.indexes += 1,
            Err(e) => {
                store.record_error(None, &format!("scip {}: {}", path.display(), e)).ok();
            }
        }
    }
    Ok(report)
}

fn find_scip_files(root: &Path) -> Vec<std::path::PathBuf> {
    let candidates = [
        root.join("index.scip"),
        root.join(".scip").join("index.scip"),
    ];
    candidates.into_iter().filter(|p| p.is_file()).collect()
}

fn load_one(
    path: &Path,
    store: &Store,
    workspace_root: &Path,
    report: &mut ScipReport,
) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let index = Index::parse_from_bytes(&bytes).map_err(|e| format!("parse: {e}"))?;
    for doc in &index.documents {
        report.documents += 1;
        load_document(doc, store, workspace_root, report)?;
    }
    Ok(())
}

fn load_document(
    doc: &scip::types::Document,
    store: &Store,
    workspace_root: &Path,
    report: &mut ScipReport,
) -> Result<(), String> {
    let rel_path = doc.relative_path.replace('\\', "/");
    let abs = workspace_root.join(&rel_path);
    let mtime = abs
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    let size = abs.metadata().map(|m| m.len() as i64).unwrap_or(0);
    let language = doc.language.to_lowercase();
    let lang_name = if language.is_empty() { None } else { Some(language.as_str()) };
    let file_id = store
        .upsert_file(&rel_path, lang_name, mtime, size)
        .map_err(|e| format!("upsert file: {e}"))?;

    let file_serial = format!("file:{}", rel_path);
    let file_node_id = store
        .insert_node(&NewNode {
            kind: "file",
            serialized_name: &file_serial,
            display_name: rel_path.rsplit('/').next().unwrap_or(&rel_path),
            file_id: Some(file_id),
            start_line: None,
            start_col: None,
            end_line: None,
            end_col: None,
            metadata: None,
        })
        .map_err(|e| format!("insert file node: {e}"))?;

    for sym in &doc.symbols {
        if sym.symbol.is_empty() {
            continue;
        }
        report.symbols += 1;
        let kind = scip_kind(sym.kind.enum_value_or_default());
        let display = if sym.display_name.is_empty() {
            extract_terminal(&sym.symbol).unwrap_or(&sym.symbol).to_string()
        } else {
            sym.display_name.clone()
        };
        let serial = format!("scip:{}", sym.symbol);
        let metadata = build_metadata(sym);
        let node_id = store
            .insert_node(&NewNode {
                kind,
                serialized_name: &serial,
                display_name: &display,
                file_id: Some(file_id),
                start_line: None,
                start_col: None,
                end_line: None,
                end_col: None,
                metadata: metadata.as_deref(),
            })
            .map_err(|e| format!("insert scip node: {e}"))?;
        store
            .insert_edge_with_metadata("contain", file_node_id, node_id, 1.0, None)
            .map_err(|e| format!("insert contain edge: {e}"))?;
        report.edges += 1;
    }
    Ok(())
}

fn scip_kind(k: ScipKind) -> &'static str {
    match k {
        ScipKind::Class | ScipKind::Struct => "class",
        ScipKind::Trait | ScipKind::Interface => "interface",
        ScipKind::Method => "method",
        ScipKind::Function | ScipKind::Constructor | ScipKind::StaticMethod => "function",
        ScipKind::Module | ScipKind::Namespace | ScipKind::Package => "module",
        ScipKind::Macro => "macro",
        ScipKind::Constant => "constant",
        ScipKind::Variable | ScipKind::Field | ScipKind::Property | ScipKind::Parameter => "variable",
        _ => "symbol",
    }
}

fn extract_terminal(symbol: &str) -> Option<&str> {
    let trimmed = symbol.trim_end_matches([' ', '\t', '\n']);
    let stripped = trimmed.trim_end_matches('#').trim_end_matches('/');
    let last_break = stripped
        .rfind([' ', '/', '#'])
        .map(|i| i + 1)
        .unwrap_or(0);
    let candidate = &stripped[last_break..];
    if candidate.is_empty() { None } else { Some(candidate) }
}

fn build_metadata(sym: &scip::types::SymbolInformation) -> Option<String> {
    let mut obj = serde_json::Map::new();
    obj.insert("scip".into(), serde_json::Value::Bool(true));
    if !sym.documentation.is_empty() {
        let doc = sym.documentation.join("\n");
        obj.insert("doc".into(), serde_json::Value::String(doc));
    }
    if obj.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(obj).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_terminal_handles_common_shapes() {
        assert_eq!(extract_terminal("rust-analyzer cargo my-crate 0.1 src/lib.rs/foo()."), Some("foo()."));
        assert_eq!(extract_terminal("npm @org/foo 1.0 src/Bar.ts/baz#"), Some("baz"));
        assert_eq!(extract_terminal("local 7"), Some("7"));
    }

    #[test]
    fn scip_kind_falls_back() {
        assert_eq!(scip_kind(ScipKind::Class), "class");
        assert_eq!(scip_kind(ScipKind::Method), "method");
        assert_eq!(scip_kind(ScipKind::UnspecifiedKind), "symbol");
    }
}
