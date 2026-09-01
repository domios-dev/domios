use serde::Serialize;
use std::path::Path;

/// Why a repository watch is not live. Every degraded path carries one of
/// these so the UI can render an honest unknown instead of a stale bubble.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoWatchUnavailable {
    NotARepository,
    RepositoryGone,
    WatchRefused,
}

impl RepoWatchUnavailable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotARepository => "not_a_repository",
            Self::RepositoryGone => "repository_gone",
            Self::WatchRefused => "watch_refused",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoWatchError {
    pub reason: RepoWatchUnavailable,
    pub identity: String,
    pub detail: String,
}

impl RepoWatchError {
    pub fn not_a_repository(path: &Path, detail: impl Into<String>) -> Self {
        Self {
            reason: RepoWatchUnavailable::NotARepository,
            identity: path.to_string_lossy().to_string(),
            detail: detail.into(),
        }
    }

    pub fn repository_gone(identity: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            reason: RepoWatchUnavailable::RepositoryGone,
            identity: identity.into(),
            detail: detail.into(),
        }
    }

    pub fn watch_refused(identity: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            reason: RepoWatchUnavailable::WatchRefused,
            identity: identity.into(),
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for RepoWatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "repository watch unavailable ({}) for {}: {}",
            self.reason.as_str(),
            self.identity,
            self.detail
        )
    }
}

impl std::error::Error for RepoWatchError {}

/// Where watch events go. Production emits to the webview and the SSE mirror;
/// tests use a channel.
pub trait RepoEventSink: Send + Sync {
    fn repository_changed(&self, identity: &str);
    fn repository_unavailable(&self, identity: &str, reason: RepoWatchUnavailable);
}

pub struct TauriRepoEventSink {
    app: tauri::AppHandle,
}

impl TauriRepoEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl RepoEventSink for TauriRepoEventSink {
    fn repository_changed(&self, identity: &str) {
        crate::api::events::emit_everywhere(
            &self.app,
            "repository-changed",
            serde_json::json!({ "identity": identity }),
        );
    }

    fn repository_unavailable(&self, identity: &str, reason: RepoWatchUnavailable) {
        log::warn!("[repowatch] {identity} unavailable: {}", reason.as_str());
        crate::api::events::emit_everywhere(
            &self.app,
            "repository-unavailable",
            serde_json::json!({ "identity": identity, "reason": reason.as_str() }),
        );
    }
}
