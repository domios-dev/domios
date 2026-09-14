use std::collections::HashMap;

use crate::multiplexer::commands::MuxState;
use crate::service::plan::ports::{TaskKeyPort, TaskKeyScope};

/// Issues task keys from the owning workspace record and flushes the session
/// snapshot, so the advanced counter survives a restart.
pub struct MuxTaskKeyPort {
    mux: MuxState,
    app: tauri::AppHandle,
}

impl MuxTaskKeyPort {
    pub fn new(mux: MuxState, app: tauri::AppHandle) -> Self {
        Self { mux, app }
    }
}

pub(crate) fn issue_from_mux(
    mux: &MuxState,
    scope: &TaskKeyScope,
    count: usize,
    high_water: &HashMap<String, u64>,
) -> Vec<String> {
    let mut mux = mux.write();
    let paths = [
        scope.canonical_project_path.as_str(),
        scope.project_path.as_str(),
    ];
    let Some(workspace_id) = mux.task_key_workspace(scope.team_id.as_deref(), &paths) else {
        log::info!(
            "[task-key] plan {} has no owning local workspace; tasks keep the id fallback",
            scope.plan_id
        );
        return Vec::new();
    };
    mux.issue_task_keys(&workspace_id, count, high_water)
}

impl TaskKeyPort for MuxTaskKeyPort {
    fn issue_task_keys(
        &self,
        scope: &TaskKeyScope,
        count: usize,
        high_water: &HashMap<String, u64>,
    ) -> Vec<String> {
        let keys = issue_from_mux(&self.mux, scope, count, high_water);
        if !keys.is_empty() {
            if let Err(error) =
                crate::service::session_persist::write_session_snapshot(&self.app, &self.mux)
            {
                log::warn!("[task-key] session flush after key issue failed: {error}");
            }
        }
        keys
    }
}
