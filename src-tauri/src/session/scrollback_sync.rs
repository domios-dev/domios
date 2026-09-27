//! Writes scrollback sidecars at the existing snapshot cadence. Capture is
//! read-only over `OutputBuffer`; restore never goes through this module.

use tauri::Manager;

use crate::multiplexer::commands::MuxState;
use crate::pty::scrollback_capture::capture_scrollback_tails;
use crate::session::scrollback::DEFAULT_SCROLLBACK_TAIL_BYTES;
use crate::session::store::SaveOutcome;
use crate::session::SessionState;
use crate::OutputBufferState;

#[derive(Default)]
pub(crate) struct SidecarSyncReport {
    pub written: Vec<(String, usize)>,
    pub skipped: Vec<(String, String)>,
}

fn sidecar_state(app: &tauri::AppHandle) -> Option<(MuxState, OutputBufferState, SessionState)> {
    let (Some(mux), Some(output_buf), Some(session)) = (
        app.try_state::<MuxState>(),
        app.try_state::<OutputBufferState>(),
        app.try_state::<SessionState>(),
    ) else {
        return None;
    };
    Some((
        mux.inner().clone(),
        output_buf.inner().clone(),
        session.inner().clone(),
    ))
}

/// The single gate every post-save sidecar sync goes through: pruning keys off
/// the LIVE tab set, which a refused reduction has just declared less
/// trustworthy than the desk already on disk.
pub(crate) fn sync_scrollback_sidecars_after_save(app: &tauri::AppHandle, outcome: &SaveOutcome) {
    if let Some((mux, output_buf, session)) = sidecar_state(app) {
        sync_scrollback_sidecars_after_save_with(outcome, &mux, &output_buf, &session);
    }
}

pub(crate) fn sync_scrollback_sidecars_after_save_with(
    outcome: &SaveOutcome,
    mux: &MuxState,
    output_buf: &OutputBufferState,
    session: &SessionState,
) -> SidecarSyncReport {
    if !matches!(outcome, SaveOutcome::Saved) {
        return skipped_report(mux, "session_snapshot_not_saved");
    }
    sync_scrollback_sidecars_with(mux, output_buf, session)
}

pub(crate) fn sync_scrollback_sidecars(app: &tauri::AppHandle) {
    if let Some((mux, output_buf, session)) = sidecar_state(app) {
        sync_scrollback_sidecars_with(&mux, &output_buf, &session);
    }
}

pub(crate) fn sync_scrollback_sidecars_with(
    mux: &MuxState,
    output_buf: &OutputBufferState,
    session: &SessionState,
) -> SidecarSyncReport {
    let capture = capture_scrollback_tails(mux, output_buf, DEFAULT_SCROLLBACK_TAIL_BYTES);
    let mut report = SidecarSyncReport {
        written: Vec::new(),
        skipped: capture
            .skipped_tabs
            .into_iter()
            .map(|(tab_id, reason)| (tab_id, reason.to_string()))
            .collect(),
    };
    let store = session.lock();
    for (tab_id, tail) in &capture.tails {
        match store.save_scrollback_tail(tab_id, tail, DEFAULT_SCROLLBACK_TAIL_BYTES) {
            Ok(()) => report.written.push((tab_id.clone(), tail.len())),
            Err(error) => {
                log::warn!("[scrollback] sidecar save failed for {tab_id}: {error}");
                report
                    .skipped
                    .push((tab_id.clone(), format!("write_failed: {error}")));
            }
        }
    }
    store.prune_scrollback_tails(&capture.live_tab_ids);
    report
}

pub(crate) fn skipped_report(mux: &MuxState, reason: &str) -> SidecarSyncReport {
    let skipped = {
        let state = mux.read();
        state
            .all_tabs()
            .into_iter()
            .filter(crate::pty::scrollback_capture::scrollback_eligible)
            .map(|tab| (tab.id.clone(), reason.to_string()))
            .collect()
    };
    SidecarSyncReport {
        written: Vec::new(),
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::{TimedMutex, TimedRwLock};
    use crate::multiplexer::state::MultiplexerState;
    use crate::pty::manager::OutputBuffer;
    use crate::session::store::{ReductionRefusal, SessionStore};
    use parking_lot::Mutex;
    use std::sync::Arc;

    const PROTECTED_TAIL: &[u8] = b"scrollback the refusal protected";

    /// One live tab plus a sidecar for a tab the mux no longer knows about —
    /// exactly the state a refused reduction leaves behind.
    struct SidecarFixture {
        _dir: tempfile::TempDir,
        mux: MuxState,
        output_buf: OutputBufferState,
        session: SessionState,
        absent_tab: String,
    }

    fn fixture_with_absent_tab_sidecar() -> SidecarFixture {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(dir.path().to_path_buf());
        let absent_tab = "tab-absent-from-mux".to_string();
        store
            .save_scrollback_tail(&absent_tab, PROTECTED_TAIL, 1024)
            .unwrap();

        let mut state = MultiplexerState::empty();
        let ws = state.create_workspace("ws".to_string(), None);
        let live = state.create_tab(&ws.id).unwrap();
        state.link_pty(&live, "pty-live".to_string());

        let mut buf = OutputBuffer::new(65536);
        buf.append("pty-live", b"live output");

        SidecarFixture {
            _dir: dir,
            mux: Arc::new(TimedRwLock::new("mux", state)),
            output_buf: Arc::new(TimedMutex::new("output_buf", buf)),
            session: Arc::new(Mutex::new(store)),
            absent_tab,
        }
    }

    fn refusal() -> SaveOutcome {
        SaveOutcome::RefusedReduction(ReductionRefusal {
            dropped_workspaces: 1,
            dropped_tabs: 2,
            quarantine: "quarantine/rejected.json".to_string(),
        })
    }

    #[test]
    fn refused_reduction_does_not_prune_scrollback_sidecars() {
        let fixture = fixture_with_absent_tab_sidecar();

        sync_scrollback_sidecars_after_save_with(
            &refusal(),
            &fixture.mux,
            &fixture.output_buf,
            &fixture.session,
        );

        assert_eq!(
            fixture
                .session
                .lock()
                .load_scrollback_tail(&fixture.absent_tab)
                .expect("refused reduction must leave the sidecar on disk"),
            PROTECTED_TAIL,
            "pruning keys off a live tab set the refusal just declared untrustworthy"
        );
    }

    #[test]
    fn successful_save_prunes_scrollback_sidecars() {
        let fixture = fixture_with_absent_tab_sidecar();

        sync_scrollback_sidecars_after_save_with(
            &SaveOutcome::Saved,
            &fixture.mux,
            &fixture.output_buf,
            &fixture.session,
        );

        assert!(
            fixture
                .session
                .lock()
                .load_scrollback_tail(&fixture.absent_tab)
                .is_none(),
            "a saved snapshot is the only outcome whose live tab set may prune"
        );
    }

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
