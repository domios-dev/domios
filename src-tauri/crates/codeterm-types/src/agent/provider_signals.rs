use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub logged_in: bool,
    pub auth_method: Option<String>,
    pub subscription_type: Option<String>,
    pub config_directory: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorKind {
    RateLimit,
    Authentication,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "acceptEdits")]
    AcceptEdits,
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "dontAsk")]
    DontAsk,
    #[serde(rename = "auto")]
    Auto,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionObservations {
    pub api_error: Option<ApiErrorKind>,
    pub permission_mode: Option<PermissionMode>,
    #[serde(default)]
    pub clears_api_error: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSignals {
    #[serde(default)]
    pub capabilities: Option<super::capability_probe::ProbeSnapshot>,
    #[serde(default)]
    pub background: super::background_work::BackgroundRoster,
    #[serde(skip)]
    pub auth_probe_pending: bool,
    #[serde(default)]
    pub access_incident: Option<String>,
    pub auth: Option<AuthStatus>,
    pub api_error: Option<ApiErrorKind>,
    pub permission_mode: Option<PermissionMode>,
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

impl ApiErrorKind {
    pub fn from_error(error: Option<&serde_json::Value>, record: &serde_json::Value) -> Self {
        use serde_json::Value;
        let mut fields: Vec<&Value> = error.into_iter().collect();
        if let Some(error) = error {
            for key in [
                "type",
                "code",
                "status",
                "statusCode",
                "status_code",
                "httpStatusCode",
            ] {
                fields.extend(error.get(key));
            }
        }
        let info = error.and_then(|error| error.get("codexErrorInfo"));
        let info_auth = info
            .and_then(Value::as_object)
            .is_some_and(|info| info.contains_key("authError"));
        let info_rate = info
            .and_then(Value::as_object)
            .is_some_and(|info| info.contains_key("usageLimitExceeded"));
        if let Some(info) = info {
            fields.push(info);
            if let Some(info) = info.as_object() {
                for value in info.values() {
                    fields.extend(value.get("httpStatusCode"));
                }
            }
        }
        for key in [
            "status",
            "statusCode",
            "status_code",
            "apiErrorStatus",
            "apiErrorCode",
        ] {
            fields.extend(record.get(key));
        }
        if info_auth
            || fields.iter().any(|value| {
                value.as_str().is_some_and(|code| {
                    code.starts_with("oauth_") || code.starts_with("authentication_")
                }) || matches!(value.as_u64(), Some(401 | 403))
                    || matches!(
                        value.as_str(),
                        Some(
                            "401"
                                | "403"
                                | "authentication_error"
                                | "auth"
                                | "unauthorized"
                                | "permission_error"
                                | "permission_denied"
                                | "forbidden"
                                | "subscription_disabled"
                                | "organization_disabled"
                                | "oauth_refresh_failed"
                                | "token_expired"
                                | "token_revoked"
                                | "refresh_token_reused"
                                | "refresh_token_expired"
                                | "refresh_token_invalidated"
                                | "logged_out"
                                | "signed_out"
                                | "unauthenticated"
                                | "authError"
                        )
                    )
            })
        {
            ApiErrorKind::Authentication
        } else if info_rate
            || fields.iter().any(|value| {
                value.as_u64() == Some(429)
                    || matches!(
                        value.as_str(),
                        Some("429" | "rate_limit" | "rate_limit_error" | "usageLimitExceeded")
                    )
            })
        {
            ApiErrorKind::RateLimit
        } else {
            ApiErrorKind::Other
        }
    }
}
