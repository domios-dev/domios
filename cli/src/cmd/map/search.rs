use crate::cmd::map::embed::{vec_to_le_bytes, EmbedClient};
use crate::cmd::map::store::{NodeRow, Store};
use std::collections::HashSet;
use std::path::Path;

pub struct SearchOptions<'a> {
    pub query: &'a str,
    pub kind: Option<&'a str>,
    pub limit: u32,
    pub semantic_only: bool,
    pub vec_k: usize,
}

pub fn run_search(root: &Path, opts: SearchOptions<'_>) -> Result<Vec<NodeRow>, String> {
    let store = Store::open(root).map_err(|e| format!("open store: {e}"))?;

    let semantic = run_semantic(&store, &opts).unwrap_or_else(|e| {
        if opts.semantic_only {
            eprintln!("project-map: semantic search failed: {e}");
        }
        Vec::new()
    });

    if opts.semantic_only {
        return Ok(semantic.into_iter().take(opts.limit as usize).collect());
    }

    let substr = store
        .search_nodes(opts.query, opts.kind, opts.limit.saturating_mul(2))
        .map_err(|e| format!("search: {e}"))?;

    Ok(merge(semantic, substr, opts.limit as usize, opts.kind))
}

fn run_semantic(store: &Store, opts: &SearchOptions<'_>) -> Result<Vec<NodeRow>, String> {
    let (total, embedded) = store.count_chunks().map_err(|e| format!("count chunks: {e}"))?;
    if total == 0 || embedded == 0 {
        return Ok(Vec::new());
    }
    let mut client = EmbedClient::new();
    let qvec = client.embed(opts.query)?;
    let bytes = vec_to_le_bytes(&qvec);
    let hits = store
        .vec_search(&bytes, opts.vec_k.max(1))
        .map_err(|e| format!("vec_search: {e}"))?;
    let mut seen_nodes: HashSet<i64> = HashSet::new();
    let mut out = Vec::new();
    for (chunk_id, _dist) in hits {
        let Some(loc) = store
            .chunk_to_node(chunk_id)
            .map_err(|e| format!("chunk_to_node: {e}"))?
        else {
            continue;
        };
        if !seen_nodes.insert(loc.node_id) {
            continue;
        }
        if let Some(node) = node_row_by_id(store, loc.node_id, opts.kind)? {
            out.push(node);
        }
    }
    Ok(out)
}

fn node_row_by_id(
    store: &Store,
    node_id: i64,
    kind_filter: Option<&str>,
) -> Result<Option<NodeRow>, String> {
    let mut stmt = store
        .conn
        .prepare(
            "SELECT n.kind, n.serialized_name, n.display_name, f.path, n.start_line \
             FROM node n LEFT JOIN file f ON n.file_id = f.id WHERE n.id = ?1",
        )
        .map_err(|e| format!("prepare: {e}"))?;
    let row = stmt
        .query_row(rusqlite::params![node_id], |r| {
            Ok(NodeRow {
                kind: r.get(0)?,
                serialized_name: r.get(1)?,
                display_name: r.get(2)?,
                path: r.get::<_, Option<String>>(3)?,
                start_line: r.get::<_, Option<i64>>(4)?,
            })
        })
        .ok();
    Ok(row.filter(|r| kind_filter.map(|k| r.kind == k).unwrap_or(true)))
}

fn merge(
    semantic: Vec<NodeRow>,
    substr: Vec<NodeRow>,
    limit: usize,
    kind_filter: Option<&str>,
) -> Vec<NodeRow> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for row in semantic.into_iter().chain(substr.into_iter()) {
        if let Some(k) = kind_filter {
            if row.kind != k {
                continue;
            }
        }
        if seen.insert(row.serialized_name.clone()) {
            out.push(row);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}
