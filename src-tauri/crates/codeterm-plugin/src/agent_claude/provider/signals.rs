use crate::provider::{AuthStatus, AuthStatusProbe, SessionObservations};
use codeterm_provider_signals::{ApiErrorKind, PermissionMode};
use serde_json::Value;

pub(super) fn probe() -> AuthStatusProbe {
    AuthStatusProbe {
        args: vec!["auth".into(), "status".into()],
        timeout_ms: 3000,
    }
}

pub(super) fn auth(record: &Value) -> Option<AuthStatus> {
    let field = |key: &str| -> Option<Option<String>> {
        match record.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(Value::String(value)) if value.encode_utf16().count() <= 4096 => {
                Some(Some(value.clone()))
            }
            _ => None,
        }
    };
    Some(AuthStatus {
        logged_in: record.get("loggedIn")?.as_bool()?,
        auth_method: field("authMethod")?,
        subscription_type: field("subscriptionType")?,
        config_directory: field("configDirectory")?,
    })
}

pub(super) fn session(record: &Value) -> Option<SessionObservations> {
    let permission_mode = record
        .get("permissionMode")
        .cloned()
        .and_then(|value| serde_json::from_value::<PermissionMode>(value).ok());
    let is_error = record.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true);
    let api_error = is_error.then(|| ApiErrorKind::from_error(record.get("error"), record));
    log::debug!("[provider-access] Claude record error={api_error:?}");
    let clears_api_error = record.get("type").and_then(Value::as_str) == Some("assistant")
        && !is_error
        && record.get("message").is_some_and(Value::is_object);
    (is_error || permission_mode.is_some() || clears_api_error).then_some(SessionObservations {
        unavailable_model: None,
        api_error,
        permission_mode,
        clears_api_error,
        retry_after_secs: ApiErrorKind::retry_after(record.get("error"), record),
        turn_id: ApiErrorKind::turn_id(record),
        retrying: ApiErrorKind::retrying(record.get("error"), record),
    })
}
