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
fn storage_shutdown_reports_failed_snapshot_as_incomplete() {
    let errors = vec!["Storage desk snapshot failed: UNIQUE constraint".into()];

    assert_eq!(
        shutdown_state_persistence_result(&errors),
        Err("Storage desk snapshot failed: UNIQUE constraint".into())
    );
    assert_eq!(shutdown_state_persistence_result(&[]), Ok(()));
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
