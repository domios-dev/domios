use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AuthState {
    #[default]
    Unknown,
    SignedOut,
    SignedIn {
        kind: String,
        plan: Option<String>,
    },
}

pub fn account_state(response: &Value) -> AuthState {
    match response.get("account") {
        Some(Value::Null)
            if response.get("requiresOpenaiAuth").and_then(Value::as_bool) == Some(true) =>
        {
            AuthState::SignedOut
        }
        Some(account) if account.is_object() => account
            .get("type")
            .and_then(Value::as_str)
            .map(|kind| AuthState::SignedIn {
                kind: kind.into(),
                plan: account
                    .get("planType")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
            .unwrap_or_default(),
        _ => AuthState::Unknown,
    }
}

pub(super) fn session_observation(
    method: &str,
    params: &Value,
) -> Option<codeterm_provider_signals::SessionObservations> {
    use codeterm_provider_signals::{ApiErrorKind, SessionObservations};
    let error = match method {
        "error" => params.get("error"),
        "turn/completed" if params["turn"]["status"] == "failed" => params["turn"].get("error"),
        _ => None,
    };
    if let Some(error) = error {
        log::debug!("[provider-access] RPC error record");
        let turn = params.get("turn");
        return Some(SessionObservations {
            api_error: Some(ApiErrorKind::from_error(Some(error), error)),
            retry_after_secs: ApiErrorKind::retry_after(Some(error), params),
            turn_id: turn
                .and_then(|turn| turn.get("id").or_else(|| turn.get("turnId")))
                .or_else(|| params.get("turnId"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_string),
            retrying: ApiErrorKind::retrying(Some(error), params)
                || turn
                    .and_then(|turn| turn.get("willRetry"))
                    .and_then(Value::as_bool)
                    == Some(true),
            ..Default::default()
        });
    }
    if method == "turn/completed" && params["turn"]["status"] == "completed" {
        log::debug!("[provider-access] RPC successful turn");
        return Some(SessionObservations {
            clears_api_error: true,
            ..Default::default()
        });
    }
    log::debug!("[provider-access] RPC no access observation");
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn codex_rpc_access_errors_are_structural_and_clear_only_on_success() {
        use codeterm_provider_signals::ApiErrorKind;
        for (error, expected) in [
            (
                json!({"codexErrorInfo":"authError"}),
                ApiErrorKind::Authentication,
            ),
            (
                json!({"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":403}}}),
                ApiErrorKind::Authentication,
            ),
            (
                json!({"codexErrorInfo":"usageLimitExceeded"}),
                ApiErrorKind::UsageLimit,
            ),
            (
                json!({"message":"authentication_error"}),
                ApiErrorKind::Other,
            ),
        ] {
            assert_eq!(
                session_observation("error", &json!({"error":error}))
                    .unwrap()
                    .api_error,
                Some(expected)
            );
        }
        assert!(
            session_observation("turn/completed", &json!({"turn":{"status":"completed"}}))
                .unwrap()
                .clears_api_error
        );
        assert!(
            session_observation("turn/completed", &json!({"turn":{"status":"interrupted"}}))
                .is_none()
        );
        let failed = session_observation(
            "turn/completed",
            &json!({"turn":{"id":"turn-a","status":"failed","error":{"codexErrorInfo":"usageLimitExceeded","retryAfter":12}}}),
        )
        .unwrap();
        assert_eq!(failed.api_error, Some(ApiErrorKind::UsageLimit));
        assert_eq!(failed.turn_id.as_deref(), Some("turn-a"));
        assert_eq!(failed.retry_after_secs, Some(12));
        assert!(!failed.retrying);
    }

    #[test]
    fn codex_rpc_auth_account_read_has_no_guessed_login_state() {
        assert_eq!(
            account_state(&json!({"account":null,"requiresOpenaiAuth":true})),
            AuthState::SignedOut
        );
        assert_eq!(
            account_state(&json!({"account":null,"requiresOpenaiAuth":false})),
            AuthState::Unknown
        );
        assert_eq!(
            account_state(
                &json!({"account":{"type":"chatgpt","planType":"pro","email":"private"}})
            ),
            AuthState::SignedIn {
                kind: "chatgpt".into(),
                plan: Some("pro".into())
            }
        );
        assert_eq!(account_state(&json!({})), AuthState::Unknown);
    }
}
const _SPLIT_METRIC_PROBE: u8 = 0;
