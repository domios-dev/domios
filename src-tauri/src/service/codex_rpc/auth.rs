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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
