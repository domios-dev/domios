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
    if result.is_ok() {
        let snapshot = svc.mux.read().to_session_snapshot();
        log::info!(
            "[updater] session snapshot saved ({} workspaces)",
            snapshot.workspaces.len()
        );
    }
    result
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
