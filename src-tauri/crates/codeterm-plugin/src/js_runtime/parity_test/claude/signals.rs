use super::*;
use serde_json::json;

#[test]
fn parity_claude_auth_probe_and_status() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    assert_eq!(native.auth_status_probe(), js.auth_status_probe());
    assert!(native.cli_login().is_some());
    assert_eq!(native.cli_login(), js.cli_login());
    assert_eq!(native.describe().cli_login, js.describe().cli_login);
    let capability = native.cli_login().unwrap();
    let pattern = regex::Regex::new(&capability.url_pattern).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(plugin_fixture_root().join("tests/fixtures/login-osc.json"))
            .unwrap(),
    )
    .unwrap();
    let screen = fixture["screen"].as_str().unwrap();
    assert_eq!(
        pattern.captures(screen).unwrap().get(1).unwrap().as_str(),
        fixture["url"].as_str().unwrap()
    );
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
        json!({"type":"system","subtype":"compact_boundary"}),
        json!({"type":"summary"}),
        json!({"type":"user","isCompactSummary":true}),
        json!({"type":"system","subtype":"init"}),
        json!({"type":"system","subtype":"resume"}),
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
    let fixture =
        std::fs::read_to_string(plugin_fixture_root().join("tests/fixtures/background-work.jsonl"))
            .unwrap();
    let mut native_roster = codeterm_types::agent::background_work::BackgroundRoster::default();
    let mut js_roster = native_roster.clone();
    for line in fixture.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        for observation in native.parse_background_work(&value) {
            native_roster.observe(&observation, "");
        }
        for observation in js.parse_background_work(&value) {
            js_roster.observe(&observation, "");
        }
    }
    assert_eq!(native_roster, js_roster);
    assert_eq!(
        native_roster
            .running
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["live"]
    );
    assert!(native_roster.unknown.contains_key("dropped"));
    assert!(native_roster.ended.contains_key("finished"));
    assert!(native_roster.ended.contains_key("opaque-stopped"));
    let stopped: serde_json::Value = serde_json::from_str(fixture.lines().last().unwrap()).unwrap();
    assert_eq!(
        native.parse_background_work(&stopped)[0].state,
        codeterm_types::agent::background_work::BackgroundWorkState::Killed
    );
    let replay: serde_json::Value = serde_json::from_str(fixture.lines().next().unwrap()).unwrap();
    for observation in native.parse_background_work(&replay) {
        native_roster.observe(&observation, "");
    }
    assert!(!native_roster.running.contains_key("dropped"));
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
