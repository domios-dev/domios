use std::cell::RefCell;

use super::*;

#[derive(Debug, PartialEq, Eq)]
enum Half {
    Declare,
    Persist,
}

fn halves_run(is_macos: bool, shutting_down: bool) -> Vec<Half> {
    let calls = RefCell::new(Vec::new());
    execute_exit_plan(
        plan_run_exit(is_macos, shutting_down),
        || calls.borrow_mut().push(Half::Declare),
        || calls.borrow_mut().push(Half::Persist),
    );
    calls.into_inner()
}

#[test]
fn macos_unrequested_exit_persists_without_declaring_clean_exit() {
    assert_eq!(halves_run(true, false), vec![Half::Persist]);
}

#[test]
fn macos_genuine_quit_declares_clean_exit_before_persisting() {
    assert_eq!(halves_run(true, true), vec![Half::Declare, Half::Persist]);
}

#[test]
fn non_macos_exit_runs_both_halves_regardless_of_quit_request() {
    assert_eq!(halves_run(false, false), vec![Half::Declare, Half::Persist]);
    assert_eq!(halves_run(false, true), vec![Half::Declare, Half::Persist]);
}

#[test]
fn every_exit_route_persists_state() {
    for is_macos in [true, false] {
        for shutting_down in [true, false] {
            assert!(plan_run_exit(is_macos, shutting_down).persist_state);
        }
    }
}

#[test]
fn only_a_genuine_quit_declares_clean_exit_on_macos() {
    assert!(!plan_run_exit(true, false).declare_clean_exit);
    assert!(plan_run_exit(true, true).declare_clean_exit);
}

#[test]
fn shutdown_logs_per_tab_sidecar_counts() {
    let report = crate::session::scrollback_sync::SidecarSyncReport {
        written: vec![("tab-written".to_string(), 4096)],
        skipped: vec![
            ("tab-empty".to_string(), "no_buffered_output".to_string()),
            (
                "tab-failed".to_string(),
                "write_failed: disk unavailable".to_string(),
            ),
        ],
    };

    assert_eq!(
        shutdown_sidecar_log_records(&report),
        vec![
            ShutdownSidecarLogRecord {
                level: ShutdownSidecarLogLevel::Info,
                message: "[shutdown] scrollback sidecar tab=tab-written result=written bytes=4096"
                    .to_string(),
            },
            ShutdownSidecarLogRecord {
                level: ShutdownSidecarLogLevel::Warn,
                message: "[shutdown] scrollback sidecar tab=tab-empty result=skipped reason=no_buffered_output"
                    .to_string(),
            },
            ShutdownSidecarLogRecord {
                level: ShutdownSidecarLogLevel::Warn,
                message: "[shutdown] scrollback sidecar tab=tab-failed result=skipped reason=write_failed: disk unavailable"
                    .to_string(),
            },
            ShutdownSidecarLogRecord {
                level: ShutdownSidecarLogLevel::Warn,
                message: "[shutdown] scrollback sidecar synchronization incomplete written=1 skipped=2"
                    .to_string(),
            },
        ]
    );
}

mod persist_state_deadlock {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::lock_hold::{TimedMutex, TimedRwLock};
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::pty::manager::OutputBuffer;
    use crate::session::store::SessionStore;
    use crate::session::SessionState;
    use crate::OutputBufferState;

    /// Regression for rel#65: `save_and_sync_sidecars` (the lock pair
    /// `persist_state` calls on every exit route) used to re-lock
    /// `SessionState` from inside a `match` whose scrutinee held the guard
    /// for the whole match, deadlocking on the non-reentrant parking_lot
    /// mutex. Calling it directly — no `AppHandle`, no Tauri runtime — names
    /// exactly the lock pair at fault instead of the shutdown path that
    /// happens to contain it. This test must both COMPLETE (a regression
    /// hangs forever, bounded only by the external test runner) and prove the
    /// sidecar sync actually executed — a fix that merely stops calling the
    /// sync would complete but fail the assertion below.
    #[test]
    fn save_and_sync_sidecars_completes_and_runs_the_scrollback_sync() {
        let dir = tempfile::tempdir().unwrap();

        let mut mux_state = MultiplexerState::empty();
        let ws = mux_state.create_workspace("ws".to_string(), None);
        let live_tab = mux_state.create_tab(&ws.id).unwrap();
        mux_state.link_pty(&live_tab, "pty-live".to_string());

        let mut buf = OutputBuffer::new(65536);
        buf.append("pty-live", b"live shutdown output");

        let mux: MuxState = Arc::new(TimedRwLock::new("mux", mux_state));
        let output_buf: OutputBufferState = Arc::new(TimedMutex::new("output_buf", buf));
        let session: SessionState = Arc::new(Mutex::new(SessionStore::for_test_dir(
            dir.path().to_path_buf(),
        )));

        // A deadlocked guard re-lock hangs here forever on unmodified develop.
        save_and_sync_sidecars(&mux, &session, Some(&output_buf));

        assert_eq!(
            session.lock().load_scrollback_tail(&live_tab).expect(
                "save_and_sync_sidecars must have run the post-save scrollback sidecar sync"
            ),
            b"live shutdown output",
        );
    }
}
