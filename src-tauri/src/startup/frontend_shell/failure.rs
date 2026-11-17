#[derive(Clone, Copy, Debug)]
pub(crate) enum FailureCode {
    NativeDisconnect,
    RootAckDeadline,
    FallbackNavigationFailed,
    FallbackUnavailable,
    NavigationFailed,
    StartupStreamFailed,
    NoServerAttachment,
    RetryNavigationFailed,
    RootRevealFailed,
    AttachmentRecoveryFailed,
    ClientRetryFailed,
    RetryCancellationFailed,
    ReconnectFailed,
    InitialDocumentFailed,
    FrontendInitializationFailed,
    FrontendSchedulingFailed,
    ServerAttachmentFailed,
    RecoveryNavigationFailed,
    FailureNavigationFailed,
}

impl FailureCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NativeDisconnect => "native_disconnect",
            Self::RootAckDeadline => "root_ack_deadline",
            Self::FallbackNavigationFailed => "fallback_navigation_failed",
            Self::FallbackUnavailable => "fallback_unavailable",
            Self::NavigationFailed => "navigation_failed",
            Self::StartupStreamFailed => "startup_stream_failed",
            Self::NoServerAttachment => "no_server_attachment",
            Self::RetryNavigationFailed => "retry_navigation_failed",
            Self::RootRevealFailed => "root_reveal_failed",
            Self::AttachmentRecoveryFailed => "attachment_recovery_failed",
            Self::ClientRetryFailed => "client_retry_failed",
            Self::RetryCancellationFailed => "retry_cancellation_failed",
            Self::ReconnectFailed => "reconnect_failed",
            Self::InitialDocumentFailed => "initial_document_failed",
            Self::FrontendInitializationFailed => "frontend_initialization_failed",
            Self::FrontendSchedulingFailed => "frontend_scheduling_failed",
            Self::ServerAttachmentFailed => "server_attachment_failed",
            Self::RecoveryNavigationFailed => "recovery_navigation_failed",
            Self::FailureNavigationFailed => "failure_navigation_failed",
        }
    }

    pub(crate) fn record(self, reason: &str) -> String {
        self.record_with(
            reason,
            crate::host_ports::native_client::record_frontend_failure,
        )
    }

    fn record_with(self, reason: &str, write: impl FnOnce(&str)) -> String {
        let detail = self.detail(reason);
        write(&detail);
        detail
    }

    pub(super) fn detail(self, reason: &str) -> String {
        format!("code={} reason={}", self.as_str(), sanitize(reason))
    }
}

fn sanitize(reason: &str) -> String {
    let mut result = Vec::new();
    for word in reason.split_whitespace().take(600) {
        let lower = word.to_ascii_lowercase();
        if word.contains(['/', '\\', '@', '='])
            || word.chars().count() > 48
            || [
                "token",
                "bearer",
                "password",
                "secret",
                "authorization",
                "credential",
            ]
            .iter()
            .any(|key| lower.contains(key))
        {
            result.push("[redacted]");
            break;
        }
        result.push(word);
    }
    result.join(" ").chars().take(480).collect()
}

#[cfg(test)]
mod tests;
