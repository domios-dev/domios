use std::path::Path;
use std::time::Instant;

use codeterm_service_core::multiplexer::commands::MuxState;
use codeterm_service_core::service::team_core::reconcile_dangling_team_plan_bindings_now;
use codeterm_service_core::service::ServiceContext;

#[cfg(test)]
mod tests;

const PROJECT_PROBE_PARALLELISM: usize = 8;

pub fn restore(service: &ServiceContext) {
    let step = Instant::now();
    service.kick_off_pollers_for_existing_agents();
    log_step("agent pollers", step);

    let step = Instant::now();
    let mut workspace_cwds = workspace_cwds(&service.mux);
    workspace_cwds.sort();
    workspace_cwds.dedup();
    let probe_paths: Vec<_> = workspace_cwds
        .iter()
        .map(std::path::PathBuf::from)
        .collect();
    codeterm_plan::service_plan::paths::warm_project_roots(&probe_paths, PROJECT_PROBE_PARALLELISM);
    log_step("project root probes", step);

    let step = Instant::now();
    let mut loaded_project = false;
    for cwd in &workspace_cwds {
        let project = Instant::now();
        match service.load_plan_project_deferring_team_reconcile(Path::new(cwd)) {
            Ok(newly_loaded) => loaded_project |= newly_loaded,
            Err(error) => log::warn!("[plan] startup scan failed for {cwd}: {error}"),
        }
        log::debug!(
            "[boot-operation] plan restore project={cwd} elapsed_ms={}",
            project.elapsed().as_millis()
        );
    }
    log_step("project loading", step);

    let step = Instant::now();
    if loaded_project {
        if let Err(error) = reconcile_dangling_team_plan_bindings_now(service) {
            log::warn!("[plan] startup scan failed: {error}");
        }
    }
    log_step("project team binding reconciliation", step);

    let step = Instant::now();
    if let Err(error) = service.recover_close_inventories() {
        log::error!("[closed-history] interrupted close recovery failed: {error}");
    }
    log_step("close inventory recovery", step);

    let step = Instant::now();
    if let Err(error) = reconcile_dangling_team_plan_bindings_now(service) {
        log::warn!("[plan] dangling team-binding reconciliation failed: {error}");
    }
    log_step("team binding reconciliation", step);

    let step = Instant::now();
    service.settle_finished_lane_history();
    log_step("lane history settlement", step);

    service.mark_plans_startup_loaded();
}

fn log_step(name: &str, started: Instant) {
    log::debug!(
        "[boot-operation] plan restore step={name} elapsed_ms={}",
        started.elapsed().as_millis()
    );
}

fn workspace_cwds(mux: &MuxState) -> Vec<String> {
    mux.read()
        .list_workspaces()
        .into_iter()
        .filter_map(|workspace| workspace.cwd)
        .collect()
}
