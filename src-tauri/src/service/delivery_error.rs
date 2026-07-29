use axum::http::StatusCode;

use crate::multiplexer::types::PendingDeliveryState;
use crate::service::inject_verify::InjectionError;

/// Structured outcome of a tab delivery, carried verbatim across the mesh relay
/// boundary. The `bytes_written` flag is the load-bearing part: a caller may
/// never be told "not found" for a tab that already holds the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryError {
    /// Target tab does not resolve on this host. Nothing was written.
    TabNotReachableOnPeer { tab: String },
    /// Target tab resolved but refused the write (booting agent, live TUI).
    /// Preflight refusal — nothing was written.
    TabBusyRefused { tab: String, detail: String },
    /// Tab resolved but has no usable PTY. Nothing was written.
    PtyUnavailable { tab: String, detail: String },
    /// Bytes reached the PTY but submission was never confirmed. The text may be
    /// parked in the composer — the only variant that admits a partial write.
    SubmitUnconfirmed { tab: String, detail: String },
}

impl DeliveryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TabNotReachableOnPeer { .. } => "tab_not_reachable_on_peer",
            Self::TabBusyRefused { .. } => "tab_busy_refused",
            Self::PtyUnavailable { .. } => "pty_unavailable",
            Self::SubmitUnconfirmed { .. } => "submit_unconfirmed",
        }
    }

    /// True only when the PTY may already hold the payload.
    pub fn bytes_written(&self) -> bool {
        matches!(self, Self::SubmitUnconfirmed { .. })
    }

    pub fn delivery_state(&self) -> PendingDeliveryState {
        match self {
            Self::SubmitUnconfirmed { .. } => PendingDeliveryState::Unconfirmed,
            Self::TabNotReachableOnPeer { .. }
            | Self::TabBusyRefused { .. }
            | Self::PtyUnavailable { .. } => PendingDeliveryState::Pending,
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::TabNotReachableOnPeer { .. } => StatusCode::NOT_FOUND,
            Self::TabBusyRefused { .. } => StatusCode::CONFLICT,
            Self::PtyUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::SubmitUnconfirmed { .. } => StatusCode::BAD_GATEWAY,
        }
    }

    pub fn tab(&self) -> &str {
        match self {
            Self::TabNotReachableOnPeer { tab }
            | Self::TabBusyRefused { tab, .. }
            | Self::PtyUnavailable { tab, .. }
            | Self::SubmitUnconfirmed { tab, .. } => tab,
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::TabNotReachableOnPeer { .. } => "",
            Self::TabBusyRefused { detail, .. }
            | Self::PtyUnavailable { detail, .. }
            | Self::SubmitUnconfirmed { detail, .. } => detail,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ok": false,
            "error": {
                "code": self.code(),
                "tab": self.tab(),
                "bytes_written": self.bytes_written(),
                "delivery_state": self.delivery_state(),
                "detail": self.detail(),
                "message": self.to_string(),
            }
        })
    }

    pub fn ledger_message(&self) -> String {
        format!("{}: {self}", self.code())
    }

    /// Classify a delivery failure whose typed cause was already flattened to a
    /// string. Zero-byte claims are never inferred here — an unrecognised
    /// failure is reported as `submit_unconfirmed` so the caller is warned the
    /// payload may have landed.
    pub fn from_opaque(tab: &str, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if detail.contains("still booting") {
            return Self::TabBusyRefused {
                tab: tab.to_string(),
                detail,
            };
        }
        Self::SubmitUnconfirmed {
            tab: tab.to_string(),
            detail,
        }
    }

    pub fn from_injection(tab: &str, err: InjectionError) -> Self {
        let tab = tab.to_string();
        let detail = err.to_string();
        match err {
            // Verification gates that run BEFORE the paste — nothing was written.
            InjectionError::PromptNeverReady { .. }
            | InjectionError::AgentTuiPresent { .. }
            | InjectionError::AgentInputUnavailable { .. } => Self::TabBusyRefused { tab, detail },
            InjectionError::PtyClosed { .. } => Self::PtyUnavailable { tab, detail },
            // Both fire only after the paste reached the PTY.
            InjectionError::EchoMismatch { .. } | InjectionError::SubmitNeverConfirmed { .. } => {
                Self::SubmitUnconfirmed { tab, detail }
            }
        }
    }
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TabNotReachableOnPeer { tab } => {
                write!(f, "Tab {tab} does not resolve on this peer")
            }
            Self::TabBusyRefused { tab, detail } => {
                write!(f, "Tab {tab} refused delivery (nothing written): {detail}")
            }
            Self::PtyUnavailable { tab, detail } => {
                write!(f, "Tab {tab} has no usable PTY: {detail}")
            }
            Self::SubmitUnconfirmed { tab, detail } => {
                write!(
                    f,
                    "Tab {tab} received the text but never confirmed submission: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for DeliveryError {}

#[cfg(test)]
mod tests;
