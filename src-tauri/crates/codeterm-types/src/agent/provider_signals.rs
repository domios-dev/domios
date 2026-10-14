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
    UsageLimit,
    ServerOverloaded,
    AuthExpiry,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_model: Option<String>,
    pub api_error: Option<ApiErrorKind>,
    pub permission_mode: Option<PermissionMode>,
    #[serde(default)]
    pub clears_api_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub retrying: bool,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnIncident {
    pub worker: String,
    pub task: Option<String>,
    pub provider: String,
    pub class: ApiErrorKind,
    pub retry_after: Option<u64>,
    pub turn_stopped: bool,
    pub idempotency_key: String,
    pub pauses_for_exhaustion: bool,
}

impl ProviderTurnIncident {
    pub fn from_observation(
        worker: &str,
        task: Option<&str>,
        provider: &str,
        observation: &SessionObservations,
        record_id: Option<&str>,
    ) -> Option<Self> {
        let class = observation
            .api_error
            .filter(|class| class.reports_turn_incident())?;
        if observation.retrying || observation.clears_api_error || worker.is_empty() {
            return None;
        }
        let turn_id = observation
            .turn_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .or(record_id.filter(|id| !id.is_empty()))?;
        Some(Self {
            worker: worker.to_string(),
            task: task.filter(|id| !id.is_empty()).map(str::to_string),
            provider: provider.to_string(),
            class,
            retry_after: observation.retry_after_secs,
            turn_stopped: true,
            idempotency_key: format!("provider-error:{worker}:{turn_id}:{}", class.as_str()),
            pauses_for_exhaustion: class.pauses_for_exhaustion(),
        })
    }
}

pub fn admit_key(seen: &mut Vec<String>, key: &str) -> bool {
    if seen.iter().any(|existing| existing == key) {
        return false;
    }
    if seen.len() == 64 {
        seen.remove(0);
    }
    seen.push(key.to_string());
    true
}

impl ApiErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RateLimit => "rate_limit",
            Self::UsageLimit => "usage_limit",
            Self::ServerOverloaded => "server_overloaded",
            Self::AuthExpiry => "auth_expiry",
            Self::Authentication => "authentication",
            Self::Other => "other",
        }
    }

    pub fn reports_turn_incident(self) -> bool {
        matches!(
            self,
            Self::RateLimit | Self::UsageLimit | Self::ServerOverloaded | Self::AuthExpiry
        )
    }

    pub fn pauses_for_exhaustion(self) -> bool {
        self == Self::UsageLimit
    }

    pub fn retry_after(
        error: Option<&serde_json::Value>,
        record: &serde_json::Value,
    ) -> Option<u64> {
        fn read(value: &serde_json::Value) -> Option<u64> {
            for key in [
                "retryAfter",
                "retry_after",
                "retryAfterSeconds",
                "retry_after_secs",
                "retryAfterSecs",
            ] {
                let Some(field) = value.get(key) else {
                    continue;
                };
                if let Some(secs) = field.as_u64() {
                    return Some(secs);
                }
                if let Some(secs) = field.as_str().and_then(|text| text.parse().ok()) {
                    return Some(secs);
                }
            }
            None
        }
        error.and_then(read).or_else(|| read(record))
    }

    pub fn turn_id(record: &serde_json::Value) -> Option<String> {
        fn text(value: &serde_json::Value, key: &str) -> Option<String> {
            value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        }
        for key in ["uuid", "turn_id", "turnId", "requestId"] {
            if let Some(id) = text(record, key) {
                return Some(id);
            }
        }
        record.get("payload").and_then(|payload| {
            ["turn_id", "turnId", "uuid", "requestId"]
                .into_iter()
                .find_map(|key| text(payload, key))
        })
    }

    pub fn retrying(error: Option<&serde_json::Value>, record: &serde_json::Value) -> bool {
        let flagged = |value: &serde_json::Value| {
            value.get("willRetry").and_then(serde_json::Value::as_bool) == Some(true)
        };
        flagged(record) || error.is_some_and(flagged)
    }

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
        let info_keys = info
            .and_then(Value::as_object)
            .map(|info| info.keys().map(String::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
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
        fn text<'a>(value: &&'a Value) -> Option<&'a str> {
            value.as_str()
        }
        let status = |value: &&Value| {
            value
                .as_u64()
                .and_then(|code| u16::try_from(code).ok())
                .or_else(|| text(value).and_then(|code| code.parse().ok()))
        };
        let named = |codes: &[&str]| {
            fields
                .iter()
                .any(|value| text(value).is_some_and(|code| codes.contains(&code)))
                || info_keys.iter().any(|key| codes.contains(key))
        };
        let prefixed = |prefix: &str| {
            fields
                .iter()
                .any(|value| text(value).is_some_and(|code| code.starts_with(prefix)))
        };
        if named(&[
            "token_expired",
            "refresh_token_expired",
            "auth_expired",
            "auth_expiry",
        ]) {
            ApiErrorKind::AuthExpiry
        } else if info_keys.iter().any(|key| *key == "authError")
            || prefixed("oauth_")
            || prefixed("authentication_")
            || fields
                .iter()
                .any(|value| matches!(status(value), Some(401 | 403)))
            || named(&[
                "401",
                "403",
                "authentication_error",
                "auth",
                "unauthorized",
                "permission_error",
                "permission_denied",
                "forbidden",
                "subscription_disabled",
                "organization_disabled",
                "oauth_refresh_failed",
                "token_revoked",
                "refresh_token_reused",
                "refresh_token_invalidated",
                "logged_out",
                "signed_out",
                "unauthenticated",
                "authError",
            ])
        {
            ApiErrorKind::Authentication
        } else if named(&["usageLimitExceeded", "usage_limit", "sessionBudgetExceeded"]) {
            ApiErrorKind::UsageLimit
        } else if fields.iter().any(|value| status(value) == Some(429))
            || named(&["429", "rate_limit", "rate_limit_error", "rateLimitExceeded"])
        {
            ApiErrorKind::RateLimit
        } else if fields
            .iter()
            .any(|value| matches!(status(value), Some(500..=599)))
            || named(&[
                "overloaded",
                "overloaded_error",
                "serverOverloaded",
                "server_overloaded",
                "internalServerError",
                "internal_server_error",
            ])
        {
            ApiErrorKind::ServerOverloaded
        } else {
            ApiErrorKind::Other
        }
    }
}

#[cfg(test)]
mod tests;
