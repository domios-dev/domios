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
fn parity_claude_background_work_and_fork_capabilities() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    assert!(native.tracks_background_work());
    let cwd = "/background-parity-fixture";
    let session = format!("background-parity-{}", uuid::Uuid::new_v4());
    assert_eq!(
        native.session_jsonl_path(cwd, &session),
        js.session_jsonl_path(cwd, &session)
    );
    assert_eq!(native.tracks_background_work(), js.tracks_background_work());
    assert_eq!(native.fork_session_args(), js.fork_session_args());
    assert_eq!(
        native.fork_session_args(),
        Some(vec!["--fork-session".into()])
    );
    for record in [
        json!({"toolUseResult":{"backgroundTaskId":"shell"}}),
        json!({"toolUseResult":{"agentId":"agent","status":"async_launched"}}),
        json!({"type":"user","message":{"content":"<task-notification><task-id>shell</task-id><status>completed</status></task-notification>"}}),
        json!({"attachment":{"prompt":"<task-notification><task-id>agent</task-id><status>killed</status></task-notification>"}}),
        json!({"toolUseResult":{"agentId":"agent","status":"opaque"}}),
    ] {
        assert_eq!(
            native.parse_background_work(&record),
            js.parse_background_work(&record)
        );
    }
}

#[test]
fn parity_claude_transcript_observations() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    assert_eq!(
        native
            .parse_session_observations(&json!({"permissionMode":"auto"}))
            .and_then(|observations| observations.permission_mode),
        Some(codeterm_provider_signals::PermissionMode::Auto)
    );
    for record in [
        json!({"isApiErrorMessage":true,"error":"rate_limit"}),
        json!({"isApiErrorMessage":true,"error":{"type":"authentication_error"}}),
        json!({"isApiErrorMessage":false,"error":"rate_limit"}),
        json!({"permissionMode":"plan"}),
        json!({"permissionMode":"auto"}),
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

#[test]
fn parity_claude_initialize_capability_declaration() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    assert_eq!(
        native.capability_probe(),
        Some(crate::provider::CapabilityProbe::StreamJsonInitialize)
    );
    assert_eq!(native.capability_probe(), js.capability_probe());
}
