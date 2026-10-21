pub(super) fn worker_brief_delivery_incident(tab_id: &str, error: &str) -> (String, String) {
    (
        "Orchestration incident: launch brief undelivered".to_string(),
        format!(
            "Impact: Tab {tab_id} was not marked ready, so its assigned work must not proceed. \
             Cleanup: confirmed-delivery cleanup was skipped and the pending brief was retained. \
             Fallback: the launch failed after the bounded submit-key attempt; retry the pending assignment explicitly. \
             Cause: {error}"
        ),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LaunchFailure {
    PtySpawn(String),
    ReaderSetup(String),
    ReaderRuntime(String),
    ReaderReadyTimeout,
    ShellReadyTimeout,
    PtyEof,
    TabClosed,
    LaunchWrite(String),
    TaskDelivery(String),
    ProviderInputNotReady(String),
    SessionBindingNotObserved(String),
    StartupSupervision(String),
}

impl LaunchFailure {
    pub(super) fn reason(&self) -> String {
        match self {
            Self::PtySpawn(error) => format!("PtySpawnFailed: {error}"),
            Self::ReaderSetup(error) => format!("ReaderSetupFailed: {error}"),
            Self::ReaderRuntime(error) => format!("ReaderRuntimeFailed: {error}"),
            Self::ReaderReadyTimeout => "ReaderReadyTimeout".into(),
            Self::ShellReadyTimeout => "ShellReadyTimeout".into(),
            Self::PtyEof => "PtyEof".into(),
            Self::TabClosed => "TabClosedBeforePtyLink".into(),
            Self::LaunchWrite(error) => format!("LaunchWriteFailed: {error}"),
            Self::TaskDelivery(error) => format!("TaskDeliveryFailed: {error}"),
            Self::ProviderInputNotReady(reason) => format!("ProviderInputNotReady: {reason}"),
            Self::SessionBindingNotObserved(reason) => {
                format!("SessionBindingNotObserved: {reason}")
            }
            Self::StartupSupervision(error) => format!("StartupSupervisionFailed: {error}"),
        }
    }

    pub(super) fn proves_process_gone(&self) -> bool {
        matches!(
            self,
            Self::PtySpawn(_)
                | Self::PtyEof
                | Self::TabClosed
                | Self::ReaderSetup(_)
                | Self::ReaderRuntime(_)
        )
    }
}
