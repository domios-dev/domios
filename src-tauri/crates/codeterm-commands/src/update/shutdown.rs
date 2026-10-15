use tauri::Manager;
use tokio::task::JoinSet;

use codeterm_service_core::service::ServiceState;

const SPAWN_QUIESCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Steps 2-9 of the auto-update sequence. Step 1 (user confirmation) and
/// step 6 (binary swap, handled by `tauri-plugin-updater` before this runs)
/// happen outside. Steps 7-9 (relaunch + session restore + per-tab resume)
/// happen in the NEW process via `lib.rs::init_states` (snapshot reload) and
/// `commands::spawn_pty` (auto-resume by `agent_session_id`).
pub async fn graceful_shutdown(
    app: &codeterm_host_runtime::app_runtime::AppHandle,
    _initiating_tab_id: Option<&str>,
) -> Result<Vec<String>, String> {
    log::info!("[updater] graceful_shutdown: starting");

    // Arms the watchdog's clean-quit signal AND role-based tab-prune suppression
    // before any PTY can die, so teardown EOFs never read as agent crashes.
    codeterm_service_core::crash::shutdown::mark_shutting_down_for(
        codeterm_service_core::crash::lifecycle::ShutdownReason::Update,
    );

    let svc = app
        .try_state::<ServiceState>()
        .ok_or_else(|| "ServiceState not registered".to_string())?
        .inner()
        .clone();

    quiesce_pty_spawns(&svc).await?;
    let owner = svc.clone();
    let (surfaces, swept) = tokio::task::spawn_blocking(move || {
        let surfaces = codeterm_service_core::service::surface::begin_surface_shutdown(&owner);
        let swept = collect_owned_pty_tabs(&owner);
        (surfaces, swept)
    })
    .await
    .map_err(|error| format!("shutdown preflight task failed: {error}"))?;
    log::info!(
        "[updater] graceful_shutdown: preserving authority for {} owned PTY Tab(s)",
        swept.len()
    );

    let (desk, reaped) = capture_desk_before(&svc, reap_owned_ptys(&svc)).await?;

    let persist_result = persist_desk_off_runtime(&svc, &svc.mux, desk).await;
    let shutdown_result = shutdown_storage_off_runtime(app).await;
    persist_result?;
    shutdown_result?;

    if let Err(error) = tokio::task::spawn_blocking(move || surfaces.wait()).await {
        log::warn!("[updater] surface shutdown wait failed: {error}");
    }

    let mem_owner = svc.clone();
    let checkpoint = tokio::task::spawn_blocking(move || mem_owner.mem.checkpoint_and_close())
        .await
        .map_err(|error| format!("mem checkpoint task failed: {error}"))?;
    if let Err(e) = checkpoint {
        log::warn!("[updater] mem WAL checkpoint failed: {}", e);
    }

    super::install_holders::release_before_installer().await?;
    log::info!("[updater] graceful_shutdown: complete; {reaped} PTY session(s) reaped and install holders released");
    Ok(swept)
}

async fn shutdown_storage_off_runtime(
    app: &codeterm_host_runtime::app_runtime::AppHandle,
) -> Result<(), String> {
    let svc = app.state::<ServiceState>().inner().clone();
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::session_persist::stop_worker(&svc)?;
        let storage = app
            .try_state::<codeterm_service_core::startup::StorageAuthority>()
            .ok_or_else(|| "StorageAuthority not registered".to_string())?;
        codeterm_service_core::startup::shutdown_bounded(storage.inner())
    })
    .await
    .map_err(|error| format!("Storage shutdown task failed: {error}"))?
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
    let svc = svc.clone();
    tokio::task::spawn_blocking(move || {
        let spawn_gate = svc.pty.lock().spawn_gate();
        spawn_gate.quiesce();
        spawn_gate.wait_for_idle(std::time::Instant::now() + SPAWN_QUIESCE_TIMEOUT)
    })
    .await
    .map_err(|error| format!("PTY spawn quiesce task failed: {error}"))?
}

async fn reap_owned_ptys(svc: &ServiceState) -> Result<usize, String> {
    let owner = svc.clone();
    let (reapers, blockers) = tokio::task::spawn_blocking(move || {
        let svc = owner;
        let reapers = svc.pty.lock().begin_shutdown()?;
        let pty_ids = reapers
            .iter()
            .map(|reaper| reaper.pty_id().to_string())
            .collect::<Vec<_>>();
        svc.pty_ownership.release_ptys(&pty_ids);

        let blockers = svc
            .mux
            .read()
            .snapshot()
            .tabs
            .values()
            .filter_map(|tab| {
                tab.pty_id
                    .as_ref()
                    .map(|pty| (pty.clone(), format!("Tab {} ({})", tab.title, tab.id)))
            })
            .collect::<std::collections::HashMap<_, _>>();
        Ok::<_, String>((reapers, blockers))
    })
    .await
    .map_err(|error| format!("PTY shutdown snapshot task failed: {error}"))??;
    let count = reapers.len();
    let mut set = JoinSet::new();
    for reaper in reapers {
        let blocker = blockers
            .get(reaper.pty_id())
            .cloned()
            .unwrap_or_else(|| format!("PTY {}", reaper.pty_id()));
        set.spawn_blocking(move || {
            reaper
                .wait_for_termination()
                .map_err(|error| format!("{blocker}: {error}"))
        });
    }
    let mut failures = Vec::new();
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(error),
            Err(error) => failures.push(format!("PTY reap task failed: {error}")),
        }
    }
    pty_shutdown_disposition(count, failures)
}

fn pty_shutdown_disposition(count: usize, failures: Vec<String>) -> Result<usize, String> {
    if failures.is_empty() {
        Ok(count)
    } else {
        Err(failures.join("; "))
    }
}

pub fn persist_session(svc: &ServiceState) -> Result<(), String> {
    let result =
        codeterm_service_core::service::session_persist::write_session_snapshot(svc, &svc.mux);
    if matches!(
        result,
        Ok(codeterm_service_core::session::store::SaveOutcome::Saved)
    ) {
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
    result: Result<codeterm_service_core::session::store::SaveOutcome, String>,
) -> Result<(), String> {
    match result {
        Ok(codeterm_service_core::session::store::SaveOutcome::Saved) => Ok(()),
        Ok(codeterm_service_core::session::store::SaveOutcome::RefusedReduction(refusal)) => {
            log::error!(
                "[updater] shutdown snapshot {refusal}; proceeding — next boot restores the fuller persisted desk"
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Snapshot the desk before polling `teardown` (futures are lazy), so teardown-time
/// tab mutations can never shrink what later gets persisted.
async fn capture_desk_before<T>(
    svc: &codeterm_service_core::service::ServiceContext,
    teardown: impl std::future::Future<Output = Result<T, String>>,
) -> Result<(codeterm_service_core::session::SessionSnapshot, T), String> {
    let mux = svc.mux.clone();
    let desk = tokio::task::spawn_blocking(move || mux.read().to_session_snapshot())
        .await
        .map_err(|error| format!("desk snapshot task failed: {error}"))?;
    let result = teardown.await?;
    Ok((desk, result))
}

/// Saves the pre-teardown desk and refreshes live tab identities before committing.
async fn persist_desk_off_runtime(
    svc: &codeterm_service_core::service::ServiceContext,
    mux: &codeterm_service_core::multiplexer::commands::MuxState,
    desk: codeterm_service_core::session::SessionSnapshot,
) -> Result<(), String> {
    let mux = mux.clone();
    let svc = svc.clone();
    tokio::task::spawn_blocking(move || {
        let result = codeterm_service_core::service::session_persist::write_captured_snapshot_with_live_identities(
            &svc,
            &mux,
            &desk,
            codeterm_service_core::session::store::SaveIntent::PreserveDesk,
        );
        if matches!(result, Ok(codeterm_service_core::session::store::SaveOutcome::Saved)) {
            log::info!(
                "[updater] pre-teardown desk snapshot saved ({} workspaces)",
                desk.workspaces.len()
            );
        }
        shutdown_save_disposition(result)
    })
    .await
    .map_err(|error| format!("desk persist task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminated_ptys_do_not_abort_update() {
        assert_eq!(pty_shutdown_disposition(3, Vec::new()), Ok(3));
    }

    #[test]
    fn any_pty_termination_failure_aborts_update() {
        let failures = vec!["live process tree".into(), "termination task failed".into()];
        assert_eq!(
            pty_shutdown_disposition(3, failures),
            Err("live process tree; termination task failed".into())
        );
    }

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

    fn snapshot_tab_ids(snapshot: &codeterm_service_core::session::SessionSnapshot) -> Vec<String> {
        snapshot
            .workspaces
            .iter()
            .flat_map(|ws| ws.tabs.iter().map(|t| t.id.clone()))
            .collect()
    }

    #[test]
    fn recorded_desk_keeps_tabs_removed_during_teardown() {
        use codeterm_service_core::multiplexer::state::MultiplexerState;
        use codeterm_service_core::multiplexer::types::{AgentRole, TabCloseReason};
        use codeterm_service_core::session::store::StorageDeskAdapter;
        use codeterm_service_core::storage::{Storage, StorageConfig};

        let (mut mux, orch) = MultiplexerState::new();
        let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
        let worker = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&orch))
            .unwrap();
        let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
        let svc = &fixture.service;
        let storage = Storage::open(
            fixture.temp.path().join("instance"),
            StorageConfig::default(),
        )
        .expect("storage owner");
        let adapter = StorageDeskAdapter::from_storage(storage.clone());
        adapter
            .import_snapshot(&svc.mux.read().to_session_snapshot(), Vec::new())
            .expect("seed the Storage desk before teardown persistence");
        codeterm_service_core::service::session_persist::install_storage_desk_adapter(
            &fixture.service,
            StorageDeskAdapter::from_storage(storage.clone()),
        )
        .expect("install Storage desk adapter");
        let teardown = async {
            let reservation = svc.mux.write().reserve_tab_close(&worker)?;
            svc.mux
                .write()
                .close_reserved_tab_with_reason(reservation, TabCloseReason::User)
                .map(|_| ())
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (desk, ()) = rt.block_on(capture_desk_before(svc, teardown)).unwrap();

        assert!(
            !snapshot_tab_ids(&svc.mux.read().to_session_snapshot()).contains(&worker),
            "teardown must have removed the worker tab from the live desk"
        );

        rt.block_on(persist_desk_off_runtime(
            &fixture.service,
            &fixture.service.mux,
            desk,
        ))
        .unwrap();
        let recorded = adapter.snapshot().expect("desk on Storage");
        assert!(
            snapshot_tab_ids(&recorded).contains(&worker),
            "recorded desk must keep the pre-teardown tab set"
        );
        codeterm_service_core::service::session_persist::stop_worker(&fixture.service).unwrap();
        storage
            .shutdown()
            .wait(std::time::Duration::from_secs(30))
            .unwrap();
    }

    #[test]
    fn reducing_snapshot_at_shutdown_does_not_abort_update() {
        use codeterm_service_core::session::store::{ReductionRefusal, SaveOutcome};

        assert!(
            shutdown_save_disposition(Ok(SaveOutcome::RefusedReduction(ReductionRefusal {
                dropped_workspaces: 2,
                dropped_tabs: 0,
                quarantine: "unused in Storage".into(),
            })))
            .is_ok(),
            "a refused reduction must not abort the shutdown/update path"
        );
    }

    #[test]
    fn captured_desk_disposition_continues_on_refusal_and_aborts_on_error() {
        use codeterm_service_core::session::store::{ReductionRefusal, SaveOutcome};

        let refusal = SaveOutcome::RefusedReduction(ReductionRefusal {
            dropped_workspaces: 1,
            dropped_tabs: 1,
            quarantine: "unused in Storage".into(),
        });
        assert!(shutdown_save_disposition(Ok(refusal)).is_ok());
        assert!(shutdown_save_disposition(Err("commit outcome unknown".into())).is_err());
    }

    #[test]
    fn genuine_persist_failure_at_shutdown_still_aborts() {
        assert!(
            shutdown_save_disposition(Err("storage commit failed".into())).is_err(),
            "an unknown-disk-state failure keeps the cancel semantics"
        );
    }

    #[test]
    fn mem_checkpoint_returns_pragma_row() {
        let mem = codeterm_service_core::mem::MemManager::open_in_memory().unwrap();
        let (busy, log_frames, checkpointed) = mem
            .checkpoint_and_close()
            .expect("checkpoint should succeed on a fresh in-memory db");
        // mode"). We only assert the call path is wired.
        assert!(busy >= -1);
        assert!(log_frames >= -1);
        assert!(checkpointed >= -1);
    }
}
