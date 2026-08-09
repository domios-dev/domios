use tauri::Manager;
use tokio::task::JoinSet;

use crate::service::ServiceState;

const SPAWN_QUIESCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const PTY_REAP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Step 2-9 from `docs/plans/auto-update.md`. Step 1 (user confirmation) and
/// step 6 (binary swap, handled by `tauri-plugin-updater` before this runs)
/// happen outside. Steps 7-9 (relaunch + session restore + per-tab resume)
/// happen in the NEW process via `lib.rs::init_states` (snapshot reload) and
/// `commands::spawn_pty` (auto-resume by `agent_session_id`).
pub async fn graceful_shutdown(
    app: &tauri::AppHandle,
    _initiating_tab_id: Option<&str>,
) -> Result<Vec<String>, String> {
    log::info!("[updater] graceful_shutdown: starting");

    // Stop the liveness watchdog treating the imminent API-down as a crash.
    crate::crash::shutdown::mark_shutting_down_for(crate::crash::lifecycle::ShutdownReason::Update);

    let svc = app
        .try_state::<ServiceState>()
        .ok_or_else(|| "ServiceState not registered".to_string())?
        .inner()
        .clone();

    quiesce_pty_spawns(&svc).await?;

    let swept = collect_owned_pty_tabs(&svc);
    log::info!(
        "[updater] graceful_shutdown: preserving authority for {} owned PTY Tab(s)",
        swept.len()
    );

    crate::crash::shutdown::mark_update_teardown_started();
    let reaped = reap_owned_ptys(&svc).await?;
    let conpty_hosts = super::conpty_cleanup::cleanup_before_installer().await?;

    persist_session(app, &svc)?;

    if let Err(e) = svc.mem.checkpoint_and_close() {
        log::warn!("[updater] mem WAL checkpoint failed: {}", e);
    }

    log::info!(
        "[updater] graceful_shutdown: complete; {} PTY session(s) reaped, {} bundled ConPTY host(s) reaped",
        reaped,
        conpty_hosts
    );
    Ok(swept)
}

fn collect_owned_pty_tabs(svc: &ServiceState) -> Vec<String> {
    let snapshot = svc.mux.read().snapshot();
    select_owned_pty_tabs(
        snapshot
            .tabs
            .values()
            .map(|tab| (tab.id.as_str(), tab.pty_id.as_deref())),
    )
}

fn select_owned_pty_tabs<'a>(
    tabs: impl Iterator<Item = (&'a str, Option<&'a str>)>,
) -> Vec<String> {
    tabs.filter_map(|(tab_id, pty_id)| pty_id.map(|_| tab_id.to_string()))
        .collect()
}

async fn quiesce_pty_spawns(svc: &ServiceState) -> Result<(), String> {
    let spawn_gate = svc.pty.lock().spawn_gate();
    spawn_gate.quiesce();
    tokio::task::spawn_blocking(move || {
        spawn_gate.wait_for_idle(std::time::Instant::now() + SPAWN_QUIESCE_TIMEOUT)
    })
    .await
    .map_err(|error| format!("PTY spawn quiesce task failed: {error}"))?
}

async fn reap_owned_ptys(svc: &ServiceState) -> Result<usize, String> {
    let reapers = svc.pty.lock().begin_shutdown()?;
    let pty_ids = reapers
        .iter()
        .map(|reaper| reaper.pty_id().to_string())
        .collect::<Vec<_>>();
    svc.pty_ownership.release_ptys(&pty_ids);

    let count = reapers.len();
    let mut set = JoinSet::new();
    for reaper in reapers {
        set.spawn_blocking(move || reaper.wait_bounded(PTY_REAP_TIMEOUT));
    }
    let mut failures = Vec::new();
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(error),
            Err(error) => failures.push(format!("PTY reap task failed: {error}")),
        }
    }
    if failures.is_empty() {
        Ok(count)
    } else {
        Err(failures.join("; "))
    }
}

pub(crate) fn persist_session(app: &tauri::AppHandle, svc: &ServiceState) -> Result<(), String> {
    let result = crate::service::session_persist::write_session_snapshot(app, &svc.mux);
    if matches!(result, Ok(crate::session::store::SaveOutcome::Saved)) {
        let snapshot = svc.mux.read().to_session_snapshot();
        log::info!(
            "[updater] session snapshot saved ({} workspaces)",
            snapshot.workspaces.len()
        );
    }
    shutdown_save_disposition(result)
}

/// A refused reducing save at shutdown is the guard SUCCEEDING: the fuller
/// desk on disk stays authoritative and the next boot restores it, so the
/// update proceeds. Only a genuine persist failure — where the state of the
/// disk is unknown — may abort the teardown.
fn shutdown_save_disposition(
    result: Result<crate::session::store::SaveOutcome, String>,
) -> Result<(), String> {
    match result {
        Ok(crate::session::store::SaveOutcome::Saved) => Ok(()),
        Ok(crate::session::store::SaveOutcome::RefusedReduction(refusal)) => {
            log::error!(
                "[updater] shutdown snapshot {refusal}; proceeding — next boot restores the fuller persisted desk"
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_teardown_reports_every_owned_pty_tab() {
        let tabs = vec![
            ("initiating-agent", Some("pty-ga")),
            ("worker", Some("pty-worker")),
            ("plain-shell", Some("pty-shell")),
            ("chat-backend", None),
        ];
        assert_eq!(
            select_owned_pty_tabs(tabs.into_iter()),
            vec!["initiating-agent", "worker", "plain-shell"]
        );
    }

    #[test]
    fn reducing_snapshot_at_shutdown_proceeds_and_keeps_fuller_desk_on_disk() {
        use crate::session::store::{SessionStore, WorkspaceSnapshot};

        fn desk(workspace_ids: &[&str]) -> crate::session::store::SessionSnapshot {
            crate::session::store::SessionSnapshot {
                version: "test".into(),
                timestamp: 0,
                active_workspace_id: None,
                teams: vec![],
                workspaces: workspace_ids
                    .iter()
                    .map(|id| WorkspaceSnapshot {
                        id: (*id).into(),
                        name: (*id).into(),
                        tabs: vec![],
                        active_tab_id: None,
                        cwd: None,
                        protected: false,
                        sort_order: 0,
                    })
                    .collect(),
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(dir.path().to_path_buf());
        store.save(&desk(&["ws_a", "ws_b", "ws_c"])).unwrap();

        // The wipe scenario: shutdown persists a strict subset of the desk.
        let outcome = store.save(&desk(&["ws_a"])).unwrap();
        assert!(
            shutdown_save_disposition(Ok(outcome)).is_ok(),
            "a refused reduction must not abort the shutdown/update path"
        );
        let persisted = store.load_last().unwrap().expect("last.json readable");
        assert_eq!(
            persisted.workspaces.len(),
            3,
            "the fuller desk stays on disk for the next boot"
        );
    }

    #[test]
    fn genuine_persist_failure_at_shutdown_still_aborts() {
        use crate::session::store::SessionStore;

        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(dir.path().to_path_buf());
        // A directory squatting on the tmp path makes the atomic write fail for
        // real — disk state is unknown, so the teardown must not proceed.
        std::fs::create_dir_all(dir.path().join("last.json.tmp")).unwrap();
        let result = store.save(&crate::session::store::SessionSnapshot {
            version: "test".into(),
            timestamp: 0,
            active_workspace_id: None,
            teams: vec![],
            workspaces: vec![],
        });
        let err = result.expect_err("write into a directory path must fail");
        assert!(
            shutdown_save_disposition(Err(err)).is_err(),
            "an unknown-disk-state failure keeps the cancel semantics"
        );
    }

    #[test]
    fn mem_checkpoint_returns_pragma_row() {
        let mem = crate::mem::MemManager::open_in_memory().unwrap();
        let (busy, log_frames, checkpointed) = mem
            .checkpoint_and_close()
            .expect("checkpoint should succeed on a fresh in-memory db");
        // mode"). We only assert the call path is wired.
        assert!(busy >= -1);
        assert!(log_frames >= -1);
        assert!(checkpointed >= -1);
    }
}
