use super::*;
use serde_json::json;

#[test]
fn parity_claude_auth_probe_and_status() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    assert_eq!(native.auth_status_probe(), js.auth_status_probe());
    let section = serde_json::to_value(crate::provider::structured_signals_section()).unwrap();
    for schema in [native.settings_schema(), js.settings_schema()] {
        assert!(serde_json::to_value(schema)
            .unwrap()
            .as_array()
            .unwrap()
            .contains(&section));
    }
    for record in [
        json!({"loggedIn":true,"authMethod":"oauth","subscriptionType":"max","configDirectory":"fixture"}),
        json!({"loggedIn":false}),
        json!({"loggedIn":"true"}),
        json!({}),
        json!({"loggedIn":true,"authMethod":9}),
    ] {
        assert_eq!(
            native.parse_auth_status(&record),
            js.parse_auth_status(&record)
        );
    }
}

#[test]
fn parity_claude_transcript_observations() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    for record in [
        json!({"isApiErrorMessage":true,"error":"rate_limit"}),
        json!({"isApiErrorMessage":true,"error":{"type":"authentication_error"}}),
        json!({"isApiErrorMessage":false,"error":"rate_limit"}),
        json!({"permissionMode":"plan"}),
        json!({"permissionMode":"opaque"}),
        json!({"type":"assistant","message":{}}),
        json!({}),
    ] {
        assert_eq!(
            native.parse_session_observations(&record),
            js.parse_session_observations(&record)
        );
    }
}
