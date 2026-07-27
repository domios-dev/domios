use rusqlite::Connection;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct PlanIndexQuery {
    pub query: String,
    pub scope: Option<String>,
    pub limit: Option<usize>,
}

pub struct PlanIndexHit {
    pub score: f64,
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub scope: String,
    pub source: Option<String>,
    pub updated_at: String,
}

pub trait PlanEventSink: Send + Sync {
    fn emit_plan_changed(&self, plan_id: &str);
}

pub trait PlanSearchPort: Send + Sync {
    fn data_dir(&self) -> PathBuf;
    fn ensure_embedding_registered(&self);
    fn init_embedding_ext(&self, conn: &Connection);
    fn init_schema(&self, conn: &Connection) -> Result<(), String>;
    fn embed(&self, conn: &Connection, id: &str, body: &str) -> Result<(), String>;
    fn run_search(
        &self,
        conn: &Connection,
        q: &PlanIndexQuery,
        blobs_dir: &Path,
    ) -> Result<Vec<PlanIndexHit>, String>;
}

pub trait PlanSkillPort: Send + Sync {
    fn skill_file_content(&self, plan_id: &str, slug: &str) -> Option<String>;
}

pub trait PlanClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub trait PlanLaneStatePort {
    fn repoint_parent(&mut self, tab_id: &str, new_parent_id: &str) -> bool;
}
