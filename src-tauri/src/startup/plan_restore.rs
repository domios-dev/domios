use std::path::Path;

use crate::multiplexer::commands::MuxState;
use crate::service::ServiceContext;

#[cfg(test)]
mod tests;

const PROJECT_PROBE_PARALLELISM: usize = 8;

pub(crate) fn restore(service: &ServiceContext) {
    service.kick_off_pollers_for_existing_agents();

    let mut workspace_cwds = workspace_cwds(&service.mux);
    workspace_cwds.sort();
    workspace_cwds.dedup();
    let probe_paths: Vec<_> = workspace_cwds.iter().map(std::path::PathBuf::from).collect();
    codeterm_plan::service_plan::paths::warm_project_roots(&probe_paths, PROJECT_PROBE_PARALLELISM);

    for cwd in workspace_cwds {
        if let Err(error) = service.ensure_plan_project_loaded(Path::new(&cwd)) {
            log::warn!("[plan] startup scan failed for {cwd}: {error}");
        }
    }

    let live_team_ids = service.live_team_ids();
    if let Err(error) = service.reconcile_dangling_team_plan_bindings(
        &live_team_ids,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    ) {
        log::warn!("[plan] dangling team-binding reconciliation failed: {error}");
    }

    service.mark_plans_startup_loaded();
}

fn workspace_cwds(mux: &MuxState) -> Vec<String> {
    mux.read()
        .list_workspaces()
        .into_iter()
        .filter_map(|workspace| workspace.cwd)
        .collect()
}
