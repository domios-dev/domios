#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FailureCode {
    NativeDisconnect,
    RootAckDeadline,
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
    StartupRefused,
    ServerAttachmentRefused,
    ClientRetryRefused,
    AutomaticRecoveryDeadline,
}

impl FailureCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NativeDisconnect => "native_disconnect",
            Self::RootAckDeadline => "root_ack_deadline",
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
            Self::StartupRefused => "daemon_startup_refused",
            Self::ServerAttachmentRefused => "server_attachment_refused",
            Self::ClientRetryRefused => "client_retry_refused",
            Self::AutomaticRecoveryDeadline => "automatic_recovery_deadline",
        }
    }

    pub(super) fn recoverable(self) -> bool {
        !matches!(
            self,
            Self::StartupRefused
                | Self::ServerAttachmentRefused
                | Self::ClientRetryRefused
                | Self::RecoveryNavigationFailed
                | Self::FailureNavigationFailed
                | Self::FallbackUnavailable
        )
    }

    pub(super) fn record_attempt(self, attempt: u8, outcome: &str, reason: &str) {
        crate::host_ports::native_client::record_frontend_recovery(&format!(
            "code={} attempt={attempt} outcome={outcome} reason={}",
            self.as_str(),
            sanitize(reason)
        ));
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FrontendFailure {
    pub(super) code: FailureCode,
    pub(super) reason: String,
}

impl FrontendFailure {
    pub(super) fn new(code: FailureCode, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }
}

impl From<String> for FrontendFailure {
    fn from(reason: String) -> Self {
        Self::new(FailureCode::StartupStreamFailed, reason)
    }
}

impl std::fmt::Display for FrontendFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl From<&str> for FrontendFailure {
    fn from(reason: &str) -> Self {
        reason.to_owned().into()
    }
}
