use crate::cmd::map::store::{NodeRow, Store};
use std::path::Path;

pub fn run_search(
    root: &Path,
    query: &str,
    kind: Option<&str>,
    limit: u32,
) -> Result<Vec<NodeRow>, String> {
    let store = Store::open(root).map_err(|e| format!("open store: {e}"))?;
    store
        .search_nodes(query, kind, limit)
        .map_err(|e| format!("search: {e}"))
}
