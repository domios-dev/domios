use codeterm_provider_signals::{ApiErrorKind, AuthStatus, PermissionMode, SessionObservations};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSignals {
    #[serde(default)]
    pub capabilities: Option<super::capability_probe::ProbeSnapshot>,
    #[serde(default)]
    pub background: super::background_work::BackgroundRoster,
    #[serde(skip)]
    pub auth_probe_pending: bool,
    pub auth: Option<AuthStatus>,
    pub api_error: Option<ApiErrorKind>,
    pub permission_mode: Option<PermissionMode>,
    #[serde(skip, default)]
    pub admitted_provider_incidents: Vec<String>,
}

impl ProviderSignals {
    pub fn access_lost(&self) -> bool {
        self.api_error == Some(ApiErrorKind::Authentication)
    }

    pub fn access_state(&self) -> Option<crate::pty::AgentLiveState> {
        self.access_lost().then(|| {
            crate::pty::TabVerdict {
                state: crate::pty::TimelineState::Question,
                subtype: Some(crate::pty::QuestionSubtype::Auth),
            }
            .into()
        })
    }

    pub fn observe(&mut self, incoming: &SessionObservations) {
        if incoming.clears_api_error {
            if let Some(auth) = &mut self.auth {
                auth.logged_in = true;
            }
        }
        if (incoming.api_error.is_some() && !self.access_lost())
            || incoming.api_error == Some(ApiErrorKind::Authentication)
            || incoming.clears_api_error
        {
            self.api_error = incoming.api_error;
        }
        if incoming.permission_mode.is_some() {
            self.permission_mode = incoming.permission_mode;
        }
    }
}
