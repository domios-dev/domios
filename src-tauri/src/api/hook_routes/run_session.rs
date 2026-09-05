use crate::api::routes::AppState;

pub(super) fn propagate(state: &AppState, tab_id: &str, sid: &str) {
    use tauri::Manager;

    let (worker_id, cwd) = {
        let mux = state.svc.mux.write();
        let tab = mux.get_tab(tab_id);
        (
            tab.and_then(|p| p.worker_id.clone()),
            tab.and_then(|p| p.cwd.clone()),
        )
    };

    if let Some(worker_id) = worker_id {
        state
            .svc
            .workers
            .set_run_session(&worker_id, sid.to_string(), cwd.clone());
        if let Some(info) = state.svc.workers.get(&worker_id) {
            crate::api::events::emit_everywhere(
                &state.svc.app_handle,
                "graph-worker-progress",
                &info,
            );
        }
    }

    if let Some(cron_state) = state.svc.app_handle.try_state::<crate::CronState>() {
        let owner = cron_state
            .lock()
            .get_tab_owner(tab_id)
            .map(|(job_id, run_id)| (job_id.to_string(), run_id.to_string()));
        if let Some((job_id, run_id)) = owner {
            cron_state
                .lock()
                .set_run_session(&job_id, &run_id, sid, cwd.as_deref());
            crate::api::events::emit_everywhere(&state.svc.app_handle, "cron-run-finished", ());
        }
    }
}
