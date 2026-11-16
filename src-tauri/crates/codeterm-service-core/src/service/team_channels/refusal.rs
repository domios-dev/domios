use codeterm_plugins::{ChannelFailure, ChannelFailureKind};

use super::TeamChannelKind;

/// A refused team channel action: a stable `code` for clients and plain words for people.
/// Messages never carry internal paths or raw tool output; those go to the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamChannelRefusal {
    pub code: &'static str,
    pub message: String,
}

impl TeamChannelRefusal {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Host failures without a typed cause, such as the registry file failing to save.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::new("failed", message)
    }

    pub(super) fn fetch(failure: &ChannelFailure, kind: &TeamChannelKind) -> Self {
        let source = &failure.source;
        let (code, message) = match failure.kind {
            ChannelFailureKind::FolderNotFound => {
                ("folder_not_found", format!("Folder not found: {source}"))
            }
            ChannelFailureKind::NotFound if *kind == TeamChannelKind::Git => (
                "not_a_repository",
                format!("Not a git repository, or it needs access: {source}"),
            ),
            ChannelFailureKind::NotFound => {
                ("not_found", format!("Nothing is published at {source}"))
            }
            ChannelFailureKind::AccessDenied => (
                "access_denied",
                format!("{source} needs access this machine doesn't have"),
            ),
            ChannelFailureKind::Unreachable => (
                "unreachable",
                format!("Couldn't reach {source}. Check the address and your connection."),
            ),
            ChannelFailureKind::Integrity => (
                "integrity_failed",
                format!("The download from {source} failed its integrity check"),
            ),
            ChannelFailureKind::Unusable => ("not_a_channel", failure.cause.clone()),
            ChannelFailureKind::Other => {
                log::warn!("team channel fetch failed: {}", failure.cause);
                (
                    "fetch_failed",
                    format!("Couldn't fetch the channel from {source}"),
                )
            }
        };
        Self::new(code, message)
    }
}

impl std::fmt::Display for TeamChannelRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
