//! Writes scrollback sidecars at the existing snapshot cadence. Capture is
//! read-only over `OutputBuffer`; restore never goes through this module.

use tauri::Manager;

use crate::multiplexer::commands::MuxState;
use crate::pty::scrollback_capture::capture_scrollback_tails;
use crate::session::scrollback::DEFAULT_SCROLLBACK_TAIL_BYTES;
use crate::session::SessionState;
use crate::OutputBufferState;

pub(crate) fn sync_scrollback_sidecars(app: &tauri::AppHandle) {
    let (Some(mux), Some(output_buf), Some(session)) = (
        app.try_state::<MuxState>(),
        app.try_state::<OutputBufferState>(),
        app.try_state::<SessionState>(),
    ) else {
        return;
    };
    sync_scrollback_sidecars_with(&mux, &output_buf, &session);
}

pub(crate) fn sync_scrollback_sidecars_with(
    mux: &MuxState,
    output_buf: &OutputBufferState,
    session: &SessionState,
) {
    let capture = capture_scrollback_tails(mux, output_buf, DEFAULT_SCROLLBACK_TAIL_BYTES);
    let store = session.lock();
    for (tab_id, tail) in &capture.tails {
        if let Err(error) = store.save_scrollback_tail(tab_id, tail, DEFAULT_SCROLLBACK_TAIL_BYTES)
        {
            log::warn!("[scrollback] sidecar save failed for {tab_id}: {error}");
        }
    }
    store.prune_scrollback_tails(&capture.live_tab_ids);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::{TimedMutex, TimedRwLock};
    use crate::multiplexer::state::MultiplexerState;
    use crate::pty::manager::OutputBuffer;
    use crate::session::store::SessionStore;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn scrollback_sync_writes_plain_shell_and_prunes_agent_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(dir.path().to_path_buf());

        let mut state = MultiplexerState::empty();
        let ws = state.create_workspace("ws".to_string(), None);
        let shell_tab = state.create_tab(&ws.id).unwrap();
        state.link_pty(&shell_tab, "pty-shell".to_string());
        let agent_tab = state.create_tab(&ws.id).unwrap();
        state.link_pty(&agent_tab, "pty-agent".to_string());
        state.get_tab_mut(&agent_tab).unwrap().agent_type = Some("claude".into());

        // A leftover sidecar from before this tab became an agent tab.
        store
            .save_scrollback_tail(&agent_tab, b"stale shell history", 1024)
            .unwrap();

        let mut buf = OutputBuffer::new(65536);
        buf.append("pty-shell", b"$ ls\r\nsrc\r\n");
        buf.append("pty-agent", b"provider-owned output");

        let mux: MuxState = Arc::new(TimedRwLock::new("mux", state));
        let output_buf: OutputBufferState = Arc::new(TimedMutex::new("output_buf", buf));
        let session: SessionState = Arc::new(Mutex::new(store));

        sync_scrollback_sidecars_with(&mux, &output_buf, &session);

        let store = session.lock();
        assert_eq!(
            store.load_scrollback_tail(&shell_tab).unwrap(),
            b"$ ls\r\nsrc\r\n"
        );
        assert!(
            store.load_scrollback_tail(&agent_tab).is_none(),
            "agent tab sidecar must be pruned — provider resume owns restoration"
        );
    }

    #[test]
    fn scrollback_sync_removes_sidecar_of_closed_tab() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(dir.path().to_path_buf());

        let mut state = MultiplexerState::empty();
        let ws = state.create_workspace("ws".to_string(), None);
        let kept = state.create_tab(&ws.id).unwrap();
        state.link_pty(&kept, "pty-kept".to_string());
        let closed = state.create_tab(&ws.id).unwrap();
        state.link_pty(&closed, "pty-closed".to_string());

        let mut buf = OutputBuffer::new(65536);
        buf.append("pty-kept", b"kept");
        buf.append("pty-closed", b"closed");

        let mux: MuxState = Arc::new(TimedRwLock::new("mux", state));
        let output_buf: OutputBufferState = Arc::new(TimedMutex::new("output_buf", buf));
        let session: SessionState = Arc::new(Mutex::new(store));

        sync_scrollback_sidecars_with(&mux, &output_buf, &session);
        assert!(session.lock().load_scrollback_tail(&closed).is_some());

        mux.write().remove_tab(&closed);
        output_buf.lock().remove("pty-closed");
        sync_scrollback_sidecars_with(&mux, &output_buf, &session);

        let store = session.lock();
        assert_eq!(store.load_scrollback_tail(&kept).unwrap(), b"kept");
        assert!(
            store.load_scrollback_tail(&closed).is_none(),
            "closing the tab removes the sidecar at the next snapshot"
        );
    }
}
