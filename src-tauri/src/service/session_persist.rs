use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

use crate::multiplexer::commands::MuxState;
use crate::session::store::{SaveIntent, SaveOutcome};
use crate::session::SessionState;

/// Focus/order mutations coalesce to this window; renames flush immediately.
pub(crate) const SESSION_PERSIST_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionPersistUrgency {
    Prompt,
    Debounced,
}

/// `Ok(RefusedReduction)` is the guard succeeding (fuller desk kept on disk,
/// already logged at error by the store); `Err` is a genuine persist failure.
pub(crate) fn write_session_snapshot(
    app: &tauri::AppHandle,
    mux: &MuxState,
) -> Result<SaveOutcome, String> {
    write_session_snapshot_with_intent(app, mux, SaveIntent::PreserveDesk)
}

pub(crate) fn write_session_snapshot_with_intent(
    app: &tauri::AppHandle,
    mux: &MuxState,
    intent: SaveIntent,
) -> Result<SaveOutcome, String> {
    let session = app
        .try_state::<SessionState>()
        .ok_or_else(|| "SessionState not registered".to_string())?
        .inner()
        .clone();
    let outcome = crate::session::save_current_snapshot(&session, mux, intent)?;
    crate::session::scrollback_sync::sync_scrollback_sidecars_after_save(app, &outcome);
    Ok(outcome)
}

/// Prompt save after a deliberate removal (close verb, policy close, cleanup)
/// — the paths allowed to shrink the persisted desk. The removal already
/// happened; a failed save must not retroactively fail the verb, so callers
/// log instead of propagating.
pub(crate) fn persist_session_after_close(svc: &super::ServiceContext) {
    if let Err(error) = write_session_snapshot_with_intent(
        &svc.app_handle,
        &svc.mux,
        SaveIntent::AuthorizedReduction,
    ) {
        log::warn!("[session-persist] post-close save failed: {}", error);
    }
}

pub(crate) fn persist_session_after_mutation(
    svc: &super::ServiceContext,
    urgency: SessionPersistUrgency,
) -> Result<(), String> {
    match urgency {
        // A refusal is not this mutation's failure: the mutation landed in
        // memory and the guard (loudly) kept the fuller desk on disk.
        SessionPersistUrgency::Prompt => {
            write_session_snapshot(&svc.app_handle, &svc.mux).map(|_| ())
        }
        SessionPersistUrgency::Debounced => {
            svc.schedule_session_persist_debounced();
            Ok(())
        }
    }
}

impl super::ServiceContext {
    pub(crate) fn schedule_session_persist_debounced(&self) {
        if self.session_persist_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let app = self.app_handle.clone();
        let mux = self.mux.clone();
        let pending = self.session_persist_pending.clone();
        std::thread::spawn(move || {
            Self::coalesce_session_persist(&app, &mux, &pending, SESSION_PERSIST_DEBOUNCE);
        });
    }

    fn coalesce_session_persist(
        app: &tauri::AppHandle,
        mux: &MuxState,
        pending: &Arc<AtomicBool>,
        delay: Duration,
    ) {
        std::thread::sleep(delay);
        pending.store(false, Ordering::Release);
        if let Err(e) = write_session_snapshot(app, mux) {
            log::warn!("[session-persist] debounced save failed: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::TimedRwLock;
    use crate::multiplexer::state::MultiplexerState;
    use crate::session::store::SessionStore;
    use parking_lot::Mutex;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_store(dir: &TempDir) -> SessionStore {
        SessionStore::for_test_dir(dir.path().to_path_buf())
    }

    #[test]
    fn rename_workspace_persisted_promptly_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(test_store(&dir)));
        let mux: MuxState = Arc::new(TimedRwLock::new("mux", MultiplexerState::empty()));
        let ws = mux.write().create_workspace("before".to_string(), None);

        mux.write()
            .rename_workspace(&ws.id, "after-rename")
            .unwrap();
        let snapshot = mux.read().to_session_snapshot();
        store.lock().save(&snapshot).unwrap();

        let loaded = store.lock().load_last().unwrap().expect("snapshot on disk");
        let ws_snap = loaded
            .workspaces
            .iter()
            .find(|w| w.id == ws.id)
            .expect("workspace in snapshot");
        assert_eq!(ws_snap.name, "after-rename");
    }

    #[test]
    fn rename_tab_persisted_promptly_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(test_store(&dir)));
        let mux: MuxState = Arc::new(TimedRwLock::new("mux", MultiplexerState::empty()));
        let ws = mux.write().create_workspace("ws".to_string(), None);
        let tab_id = mux.write().create_tab(&ws.id).unwrap();

        mux.write().rename_tab(&tab_id, "renamed-tab").unwrap();
        let snapshot = mux.read().to_session_snapshot();
        store.lock().save(&snapshot).unwrap();

        let loaded = store.lock().load_last().unwrap().expect("snapshot on disk");
        let ws_snap = loaded.workspaces.iter().find(|w| w.id == ws.id).unwrap();
        let tab = ws_snap.tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert_eq!(tab.title, "renamed-tab");
    }

    #[test]
    fn storage_desk_app_adapter_imports_and_flushes_exact_selection() {
        let root = tempfile::tempdir().unwrap();
        let adapter = crate::session::store::StorageDeskAdapter::open(root.path()).unwrap();
        let snapshot = crate::session::store::SessionSnapshot {
            version: "test".into(),
            timestamp: 17,
            active_workspace_id: Some("ws-a".into()),
            teams: vec![],
            workspaces: vec![crate::session::store::WorkspaceSnapshot {
                id: "ws-a".into(),
                name: "Alpha".into(),
                tabs: vec![],
                active_tab_id: None,
                cwd: None,
                provisioned_cwd: None,
                protected: true,
                sort_order: 8,
                task_keys: Default::default(),
            }],
        };

        adapter.import_snapshot(&snapshot, Vec::new()).unwrap();
        let initial = adapter.state().unwrap();
        let workspace_revision = initial.workspace_revisions["ws-a"];
        let committed_revision = adapter
            .rename_workspace("ws-a".into(), workspace_revision, "Renamed".into())
            .unwrap();
        assert!(committed_revision > initial.revision);
        assert!(adapter
            .rename_workspace("ws-a".into(), workspace_revision, "Stale".into())
            .is_err());
        let mut backup = snapshot.clone();
        backup.timestamp = 99;
        backup.active_workspace_id = Some("ws-b".into());
        backup.workspaces[0].name = "Stale backup".into();
        backup
            .workspaces
            .push(crate::session::store::WorkspaceSnapshot {
                id: "ws-b".into(),
                name: "Restored".into(),
                tabs: vec![],
                active_tab_id: None,
                cwd: None,
                provisioned_cwd: None,
                protected: false,
                sort_order: 19,
                task_keys: Default::default(),
            });
        adapter.additive_restore(&backup, Vec::new()).unwrap();
        adapter.additive_restore(&backup, Vec::new()).unwrap();
        adapter.flush().unwrap();
        let restored = adapter.snapshot().unwrap();

        assert_eq!(restored.active_workspace_id, Some("ws-a".into()));
        assert_eq!(restored.workspaces[0].active_tab_id, None);
        assert_eq!(restored.workspaces[0].name, "Renamed");
        assert_eq!(restored.workspaces[0].cwd, None);
        assert_eq!(restored.workspaces[0].provisioned_cwd, None);
        assert_eq!(restored.workspaces[0].sort_order, 8);
        assert!(restored.workspaces[0].protected);
        assert_eq!(restored.timestamp, 17);
        assert_eq!(restored.workspaces.len(), 2);
        assert_eq!(restored.workspaces[1].name, "Restored");
    }
}
