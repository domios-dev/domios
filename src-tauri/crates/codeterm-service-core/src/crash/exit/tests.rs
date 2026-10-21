use super::*;

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
