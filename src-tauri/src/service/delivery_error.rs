use axum::http::StatusCode;

use crate::service::inject_verify::InjectionError;

/// Structured outcome of a pane delivery, carried verbatim across the mesh relay
/// boundary. The `bytes_written` flag is the load-bearing part: a caller may
/// never be told "not found" for a pane that already holds the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryError {
    /// Target pane does not resolve on this host. Nothing was written.
    PaneNotReachableOnPeer { pane: String },
    /// Target pane resolved but refused the write (booting agent, live TUI).
    /// Preflight refusal — nothing was written.
    PaneBusyRefused { pane: String, detail: String },
    /// Pane resolved but has no usable PTY. Nothing was written.
    PtyUnavailable { pane: String, detail: String },
    /// Bytes reached the PTY but submission was never confirmed. The text may be
    /// parked in the composer — the only variant that admits a partial write.
    SubmitUnconfirmed { pane: String, detail: String },
}

impl DeliveryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::PaneNotReachableOnPeer { .. } => "pane_not_reachable_on_peer",
            Self::PaneBusyRefused { .. } => "pane_busy_refused",
            Self::PtyUnavailable { .. } => "pty_unavailable",
            Self::SubmitUnconfirmed { .. } => "submit_unconfirmed",
        }
    }

    /// True only when the PTY may already hold the payload.
    pub fn bytes_written(&self) -> bool {
        matches!(self, Self::SubmitUnconfirmed { .. })
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::PaneNotReachableOnPeer { .. } => StatusCode::NOT_FOUND,
            Self::PaneBusyRefused { .. } => StatusCode::CONFLICT,
            Self::PtyUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::SubmitUnconfirmed { .. } => StatusCode::BAD_GATEWAY,
        }
    }

    pub fn pane(&self) -> &str {
        match self {
            Self::PaneNotReachableOnPeer { pane }
            | Self::PaneBusyRefused { pane, .. }
            | Self::PtyUnavailable { pane, .. }
            | Self::SubmitUnconfirmed { pane, .. } => pane,
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::PaneNotReachableOnPeer { .. } => "",
            Self::PaneBusyRefused { detail, .. }
            | Self::PtyUnavailable { detail, .. }
            | Self::SubmitUnconfirmed { detail, .. } => detail,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ok": false,
            "error": {
                "code": self.code(),
                "pane": self.pane(),
                "bytes_written": self.bytes_written(),
                "detail": self.detail(),
                "message": self.to_string(),
            }
        })
    }

    /// Classify a delivery failure whose typed cause was already flattened to a
    /// string. Zero-byte claims are never inferred here — an unrecognised
    /// failure is reported as `submit_unconfirmed` so the caller is warned the
    /// payload may have landed.
    pub fn from_opaque(pane: &str, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if detail.contains("still booting") {
            return Self::PaneBusyRefused {
                pane: pane.to_string(),
                detail,
            };
        }
        Self::SubmitUnconfirmed {
            pane: pane.to_string(),
            detail,
        }
    }

    pub fn from_injection(pane: &str, err: InjectionError) -> Self {
        let pane = pane.to_string();
        let detail = err.to_string();
        match err {
            // Verification gates that run BEFORE the paste — nothing was written.
            InjectionError::PromptNeverReady { .. }
            | InjectionError::AgentTuiPresent { .. }
            | InjectionError::AgentInputUnavailable { .. } => {
                Self::PaneBusyRefused { pane, detail }
            }
            InjectionError::PtyClosed { .. } => Self::PtyUnavailable { pane, detail },
            // Both fire only after the paste reached the PTY.
            InjectionError::EchoMismatch { .. } | InjectionError::SubmitNeverConfirmed { .. } => {
                Self::SubmitUnconfirmed { pane, detail }
            }
        }
    }
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PaneNotReachableOnPeer { pane } => {
                write!(f, "pane {pane} does not resolve on this peer")
            }
            Self::PaneBusyRefused { pane, detail } => {
                write!(
                    f,
                    "pane {pane} refused delivery (nothing written): {detail}"
                )
            }
            Self::PtyUnavailable { pane, detail } => {
                write!(f, "pane {pane} has no usable PTY: {detail}")
            }
            Self::SubmitUnconfirmed { pane, detail } => {
                write!(
                    f,
                    "pane {pane} received the text but never confirmed submission: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for DeliveryError {}

#[cfg(test)]
mod tests;
