//! App-supplied ports — traits defined here, implemented in the app crate.

use std::collections::HashMap;
use std::sync::Arc;

use crate::workers::GraphWorkerInfo;
use crate::MemManager;

#[derive(Debug, Clone)]
pub struct MemCronJob {
    pub id: String,
    pub name: String,
    pub agent: String,
    pub schedule: String,
    pub enabled: bool,
    pub oneshot: bool,
    pub next_run: Option<u64>,
    pub jitter_max_ms: Option<u64>,
    pub tz: Option<String>,
    pub protected: bool,
}

#[derive(Debug, Clone)]
pub struct MemCronCreateParams {
    pub name: String,
    pub workspace_id: String,
    pub schedule: String,
    pub task: String,
    pub description: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub role: Option<String>,
    pub creator_tab_id: Option<String>,
    pub enabled: Option<bool>,
    pub max_history: Option<usize>,
    pub oneshot: Option<bool>,
    pub jitter_max_ms: Option<u64>,
    pub tz: Option<String>,
    pub timeout_seconds: Option<u64>,
}

pub trait MemSchedulerPort {
    fn list_jobs(&self) -> Vec<MemCronJob>;
    fn find_job_by_name(&self, name: &str) -> Option<MemCronJob>;
    fn update_job(
        &mut self,
        id: &str,
        update: &mut dyn FnMut(&mut MemCronJob),
    ) -> Result<(), String>;
    fn set_job_protected(&mut self, id: &str, protected: bool) -> Result<(), String>;
    fn create_job(&mut self, params: MemCronCreateParams) -> Result<(), String>;
    fn delete_job(&mut self, id: &str) -> Result<(), String>;
    fn save(&mut self) -> Result<(), String>;
    fn compute_next_run_jittered_in(
        &self,
        schedule: &str,
        jitter_max_ms: Option<u64>,
        tz: Option<&str>,
    ) -> Result<Option<u64>, String>;
}

pub trait MemEventSink: Send + Sync {
    fn emit_graph_worker_spawned(&self, info: &GraphWorkerInfo);
}

pub trait MemHostContext: Send + Sync {
    fn mem(&self) -> Arc<MemManager>;
    fn get_taxonomy(
        &self,
        workspace: Option<&str>,
    ) -> Result<HashMap<String, HashMap<String, u64>>, String>;
    fn query_graph_type_counts(&self) -> (Vec<(String, u64)>, Vec<(String, u64)>);
}
