use super::*;

#[test]
fn every_failure_branch_has_a_distinct_bounded_detail() {
    let cases = [
        (FailureCode::NativeDisconnect, "native_disconnect"),
        (FailureCode::RootAckDeadline, "root_ack_deadline"),
        (
            FailureCode::FallbackNavigationFailed,
            "fallback_navigation_failed",
        ),
        (FailureCode::FallbackUnavailable, "fallback_unavailable"),
        (FailureCode::NavigationFailed, "navigation_failed"),
        (FailureCode::StartupStreamFailed, "startup_stream_failed"),
        (FailureCode::NoServerAttachment, "no_server_attachment"),
        (
            FailureCode::RetryNavigationFailed,
            "retry_navigation_failed",
        ),
        (FailureCode::RootRevealFailed, "root_reveal_failed"),
        (
            FailureCode::AttachmentRecoveryFailed,
            "attachment_recovery_failed",
        ),
        (FailureCode::ClientRetryFailed, "client_retry_failed"),
        (
            FailureCode::RetryCancellationFailed,
            "retry_cancellation_failed",
        ),
        (FailureCode::ReconnectFailed, "reconnect_failed"),
        (
            FailureCode::InitialDocumentFailed,
            "initial_document_failed",
        ),
        (
            FailureCode::FrontendInitializationFailed,
            "frontend_initialization_failed",
        ),
        (
            FailureCode::FrontendSchedulingFailed,
            "frontend_scheduling_failed",
        ),
        (
            FailureCode::ServerAttachmentFailed,
            "server_attachment_failed",
        ),
        (
            FailureCode::RecoveryNavigationFailed,
            "recovery_navigation_failed",
        ),
        (
            FailureCode::FailureNavigationFailed,
            "failure_navigation_failed",
        ),
    ];
    let mut codes = std::collections::HashSet::new();
    for (failure, code) in cases {
        assert!(codes.insert(code));
        let mut records = Vec::new();
        let detail = failure.record_with("Server startup readiness deadline exceeded", |detail| {
            records.push(detail.to_owned())
        });
        assert_eq!(
            records,
            vec![format!(
                "code={code} reason=Server startup readiness deadline exceeded"
            )]
        );
        assert_eq!(detail, records[0]);
        assert!(failure.detail(&"x ".repeat(1000)).chars().count() <= 600);
    }
}

#[test]
fn details_remove_sensitive_suffixes_and_line_breaks() {
    for secret in [
        "https://host/path?token=private",
        "C:\\Users\\Anna\\data",
        "/Users/Anna/data",
        "token private-value",
        "Bearer private-value",
        "password: private-value",
        "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz",
    ] {
        assert_eq!(
            sanitize(&format!("Startup failed {secret}")),
            "Startup failed [redacted]"
        );
    }
    assert_eq!(
        sanitize("Daemon\nstartup\r\nfailed"),
        "Daemon startup failed"
    );
}
