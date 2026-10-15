use crate::cron::store::{commit, Prepared, RunSession};
use crate::service::ServiceContext;
use crate::CronState;

pub(super) fn propagate(svc: &ServiceContext, tab_id: &str, sid: &str) {
    use tauri::Manager;

    let (worker_id, cwd) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id);
        (
            tab.and_then(|p| p.worker_id.clone()),
            tab.and_then(|p| p.cwd.clone()),
        )
    };

    if let Some(worker_id) = worker_id {
        svc.workers
            .set_run_session(&worker_id, sid.to_string(), cwd.clone());
        if let Some(info) = svc.workers.get(&worker_id) {
            crate::events::emit_everywhere(&svc.app_handle, "graph-worker-progress", &info);
        }
    }

    if let Some(cron_state) = svc.app_handle.try_state::<CronState>() {
        if commit_cron_run_session(cron_state.inner(), tab_id, sid, cwd.as_deref()) {
            crate::events::emit_everywhere(&svc.app_handle, "cron-run-finished", ());
        }
    }
}

/// Returns whether the tab owned a cron run whose session the cache now holds.
fn commit_cron_run_session(cron: &CronState, tab_id: &str, sid: &str, cwd: Option<&str>) -> bool {
    let session = RunSession {
        session_id: sid,
        cwd,
    };
    let committed = commit(cron, |store| {
        let Some((job_id, run_id)) = store.get_tab_owner(tab_id).cloned() else {
            return Ok(Prepared::unchanged(false));
        };
        let prepared = store.prepare_set_run_session(&job_id, &run_id, session);
        Ok(Prepared {
            changes: prepared.changes,
            value: true,
        })
    });
    match committed {
        Ok(committed) => committed.value,
        Err(error) => {
            log::warn!("cron run session for tab {tab_id} not committed: {error}");
            error.applied()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::commit_cron_run_session;
    use crate::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};
    use crate::CronState;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn params() -> CronCreateParams {
        CronCreateParams {
            name: "session".to_string(),
            workspace_id: "ws".to_string(),
            team_id: None,
            detach_from_team: false,
            schedule: "0 * * * *".to_string(),
            task: "task".to_string(),
            description: None,
            agent: Some("claude".to_string()),
            model: None,
            reasoning_effort: None,
            cwd: None,
            role: None,
            skill: None,
            creator_tab_id: None,
            report_target_tab_id: None,
            enabled: None,
            max_history: None,
            oneshot: None,
            jitter_max_ms: Some(0),
            tz: None,
            timeout_seconds: None,
        }
    }

    fn running(run_id: &str) -> CronRunInfo {
        CronRunInfo {
            run_id: run_id.to_string(),
            started_at: 1,
            finished_at: None,
            status: CronRunStatus::Running,
            tab_id: Some("tab-1".to_string()),
            session_id: None,
            cwd: None,
            summary: None,
            report_delivery: None,
        }
    }

    fn owned_run() -> (tempfile::TempDir, CronState, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = CronStore::new_with_path(dir.path().join("crons.json"));
        let job = store.create(params()).unwrap();
        store.record_run(&job.id, running("r1")).unwrap();
        store.register_tab("tab-1", &job.id, "r1");
        (dir, Arc::new(Mutex::new(store)), job.id)
    }

    #[test]
    fn cron_run_session_commit_records_history_last_run_and_file() {
        let (dir, cron, job_id) = owned_run();

        assert!(commit_cron_run_session(
            &cron,
            "tab-1",
            "sid-9",
            Some("/cwd")
        ));

        let store = cron.lock();
        let history = store.get_history(&job_id);
        let run = history.iter().find(|r| r.run_id == "r1").unwrap();
        assert_eq!(run.session_id.as_deref(), Some("sid-9"));
        assert_eq!(run.cwd.as_deref(), Some("/cwd"));
        let last = store.get(&job_id).unwrap().last_run.clone().unwrap();
        assert_eq!(last.session_id.as_deref(), Some("sid-9"));
        assert_eq!(last.cwd.as_deref(), Some("/cwd"));
        drop(store);

        let reloaded = CronStore::new_with_path(dir.path().join("crons.json"));
        let saved = reloaded.get_history(&job_id);
        let saved = saved.iter().find(|r| r.run_id == "r1").unwrap();
        assert_eq!(saved.session_id.as_deref(), Some("sid-9"));
        assert_eq!(saved.cwd.as_deref(), Some("/cwd"));
    }

    #[test]
    fn cron_run_session_commit_keeps_cwd_when_absent() {
        let (_dir, cron, job_id) = owned_run();
        assert!(commit_cron_run_session(
            &cron,
            "tab-1",
            "sid-1",
            Some("/first")
        ));

        assert!(commit_cron_run_session(&cron, "tab-1", "sid-2", None));

        let store = cron.lock();
        let last = store.get(&job_id).unwrap().last_run.clone().unwrap();
        assert_eq!(last.session_id.as_deref(), Some("sid-2"));
        assert_eq!(last.cwd.as_deref(), Some("/first"));
    }

    #[test]
    fn cron_run_session_commit_ignores_a_tab_without_a_run() {
        let (_dir, cron, job_id) = owned_run();

        assert!(!commit_cron_run_session(
            &cron,
            "other-tab",
            "sid-9",
            Some("/cwd")
        ));

        let store = cron.lock();
        assert_eq!(store.get_history(&job_id)[0].session_id, None);
        assert_eq!(
            store
                .get(&job_id)
                .unwrap()
                .last_run
                .as_ref()
                .unwrap()
                .session_id,
            None
        );
    }
}
