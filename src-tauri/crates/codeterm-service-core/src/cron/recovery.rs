use std::collections::HashSet;

use crate::multiplexer::state::MultiplexerState;

use super::scheduler::CronState;
use super::store::{finish_run_with_summary, CronRunStatus};

#[cfg(test)]
#[path = "recovery/tests.rs"]
mod tests;

pub struct RestoredCronSession {
    workspaces: HashSet<String>,
    tabs: HashSet<String>,
}

impl RestoredCronSession {
    pub fn from_mux(mux: &MultiplexerState) -> Self {
        let workspaces = mux.list_workspaces();
        let workspace_ids = workspaces
            .iter()
            .map(|workspace| workspace.id.clone())
            .collect::<HashSet<_>>();
        let workspace_tabs = workspaces
            .iter()
            .flat_map(|workspace| workspace.tab_ids.iter().cloned())
            .collect::<HashSet<_>>();
        let live_tabs = mux
            .all_tabs()
            .map(|tab| tab.id.clone())
            .collect::<HashSet<_>>();
        Self {
            workspaces: workspace_ids,
            tabs: workspace_tabs.intersection(&live_tabs).cloned().collect(),
        }
    }
}

pub fn reconcile_restored_runs(
    cron: &CronState,
    restored: &RestoredCronSession,
) -> Result<(usize, usize), String> {
    let active_runs = cron.lock().active_runs();
    let mut retained = 0;
    let mut terminalized = 0;

    for (job_id, run) in active_runs {
        let live_tab = run
            .tab_id
            .as_deref()
            .filter(|tab_id| restored.tabs.contains(*tab_id));
        if let Some(tab_id) = live_tab {
            cron.lock().register_tab(tab_id, &job_id, &run.run_id);
            retained += 1;
            continue;
        }

        let job_workspace = cron
            .lock()
            .get(&job_id)
            .map(|job| job.workspace_id.clone())
            .unwrap_or_default();
        let reason = orphan_reason(run.tab_id.as_deref(), &job_workspace, &restored.workspaces);
        finish_run_with_summary(
            cron,
            &job_id,
            &run.run_id,
            CronRunStatus::Cancelled,
            &reason,
        )
        .map_err(|error| error.to_string())?;
        terminalized += 1;
    }

    Ok((retained, terminalized))
}

pub(crate) fn restored_spawn_workspace(
    mux: &MultiplexerState,
    configured_workspace: &str,
) -> Option<String> {
    if !configured_workspace.is_empty() && mux.get_workspace(configured_workspace).is_some() {
        return Some(configured_workspace.to_string());
    }
    mux.get_active_workspace()
        .map(|workspace| workspace.id.clone())
        .or_else(|| {
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone())
        })
}

fn orphan_reason(
    tab_id: Option<&str>,
    workspace_id: &str,
    restored_workspaces: &HashSet<String>,
) -> String {
    let tab_reason = match tab_id {
        Some(tab_id) => format!("owner tab {tab_id} was not restored"),
        None => "owner tab was not recorded".to_string(),
    };
    if restored_workspaces.contains(workspace_id) || workspace_id.is_empty() {
        format!("Run cancelled during startup reconciliation: {tab_reason}.")
    } else {
        format!(
            "Run cancelled during startup reconciliation: {tab_reason}; workspace {workspace_id} was not restored."
        )
    }
}
