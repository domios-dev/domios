use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

use crate::multiplexer::commands::MuxState;
use crate::session::SessionState;

/// Focus/order mutations coalesce to this window; renames flush immediately.
pub(crate) const SESSION_PERSIST_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionPersistUrgency {
    Prompt,
    Debounced,
}

pub(crate) fn write_session_snapshot(app: &tauri::AppHandle, mux: &MuxState) -> Result<(), String> {
    let session = app
        .try_state::<SessionState>()
        .ok_or_else(|| "SessionState not registered".to_string())?
        .inner()
        .clone();
    let snapshot = mux.read().to_session_snapshot();
    let save_result = {
        let store = session.lock();
        store.save(&snapshot)
    };
    save_result.map_err(|e| format!("save session: {}", e))?;
    crate::session::scrollback_sync::sync_scrollback_sidecars(app);
    Ok(())
}

pub(crate) fn persist_session_after_mutation(
    svc: &super::ServiceContext,
    urgency: SessionPersistUrgency,
) -> Result<(), String> {
    match urgency {
        SessionPersistUrgency::Prompt => write_session_snapshot(&svc.app_handle, &svc.mux),
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
}
