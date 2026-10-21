use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The plan facts a host needs to find the one workspace that owns its task keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskKeyScope {
    pub plan_id: String,
    pub team_id: Option<String>,
    pub project_path: String,
    pub canonical_project_path: String,
}

pub trait TaskKeyPort: Send + Sync {
    /// Issues `count` keys atomically and durably, or none when no workspace owns the scope.
    fn issue_task_keys(
        &self,
        scope: &TaskKeyScope,
        count: usize,
        high_water: &HashMap<String, u64>,
    ) -> Vec<String>;
}

#[derive(Clone)]
pub struct PlanIndexQuery {
    pub query: String,
    pub kind: Option<String>,
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
