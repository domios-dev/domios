use crate::cmd::map::store::Store;
use std::path::Path;

#[derive(Debug, serde::Serialize)]
pub struct Status {
    pub workspace_cwd: Option<String>,
    pub schema_version: Option<String>,
    pub indexer_version: Option<String>,
    pub last_index_started_at: Option<String>,
    pub last_index_finished_at: Option<String>,
    pub files: i64,
    pub nodes: i64,
    pub edges: i64,
    pub errors: i64,
    pub db_path: String,
}

pub fn run_status(root: &Path) -> Result<Status, String> {
    let db_path = Store::db_path(root);
    if !db_path.exists() {
        return Err(format!("no map at {} — run `codeterm map index` first", db_path.display()));
    }
    let store = Store::open(root).map_err(|e| format!("open store: {e}"))?;
    let counts = store.counts().map_err(|e| format!("counts: {e}"))?;
    Ok(Status {
        workspace_cwd: store.get_meta("workspace_cwd").ok().flatten(),
        schema_version: store.get_meta("schema_version").ok().flatten(),
        indexer_version: store.get_meta("indexer_version").ok().flatten(),
        last_index_started_at: store.get_meta("last_index_started_at").ok().flatten(),
        last_index_finished_at: store.get_meta("last_index_finished_at").ok().flatten(),
        files: counts.files,
        nodes: counts.nodes,
        edges: counts.edges,
        errors: counts.errors,
        db_path: db_path.to_string_lossy().to_string(),
    })
}
