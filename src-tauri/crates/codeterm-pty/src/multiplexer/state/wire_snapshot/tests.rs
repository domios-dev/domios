use super::super::MultiplexerState;
use crate::multiplexer::types::{PriorBindingNonce, RestorableLaunchParameters};
use crate::session::store::SessionSnapshot;
use serde_json::json;

fn populate(state: &mut MultiplexerState, id: &str, config: usize) {
    let tab = state.get_tab_mut(id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_runtime_config = Some(json!({
        "systemPrompt": format!("{config}:{}", "p".repeat(42_000)),
        "model": format!("model-{config}"),
        "options": {"temperature": 0.5}
    }));
    tab.restorable_launch = RestorableLaunchParameters {
        captured: true,
        model: Some(format!("model-{config}")),
        identity_instructions: Some("i".repeat(1_250)),
        ..Default::default()
    };
    tab.prior_session_binding_nonces = (0..8)
        .map(|n| PriorBindingNonce {
            session_id: Some(format!("session-{n}")),
            resume_target_session_id: None,
            nonce: format!("nonce-{n}"),
        })
        .collect();
}

#[test]
fn wire_snapshot_140_tabs_stays_below_450kb() {
    let (mut state, first) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    populate(&mut state, &first, 0);
    for n in 1..140 {
        let id = state.create_tab(&workspace).unwrap();
        populate(&mut state, &id, n % 12);
    }
    for n in 0..100 {
        let id = state.create_tab(&workspace).unwrap();
        populate(&mut state, &id, n % 12);
        state.close_tab(&id).unwrap();
    }
    let encoded = serde_json::to_vec(&state.snapshot()).unwrap();
    assert!(encoded.len() < 450_000, "{} bytes", encoded.len());
    let wire: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    let tabs = wire["tabs"].as_object().unwrap();
    assert_eq!(tabs.len(), 140);
    for tab in tabs.values() {
        let config = tab["agent_runtime_config"].as_object().unwrap();
        assert!(!config.contains_key("systemPrompt"));
        assert_eq!(config["options"], json!({"temperature": 0.5}));
        let nonces = tab["prior_session_binding_nonces"].as_array().unwrap();
        assert_eq!(nonces.len(), 2);
        assert_eq!(nonces[0]["nonce"], "nonce-6");
        assert_eq!(nonces[1]["nonce"], "nonce-7");
    }
    let closed = wire["recently_closed"].as_array().unwrap();
    assert_eq!(closed.len(), 100);
    assert!(closed
        .iter()
        .all(|tab| tab.get("restorable_launch").is_none()));
    assert!(!String::from_utf8(encoded).unwrap().contains("systemPrompt"));
}

#[test]
fn wire_serialization_preserves_persisted_runtime_launch_and_nonce_history() {
    let (mut state, id) = MultiplexerState::new();
    populate(&mut state, &id, 4);
    let original = state.get_tab(&id).unwrap().clone();
    serde_json::to_vec(&state.snapshot()).unwrap();
    let saved = serde_json::to_vec(&state.to_session_snapshot()).unwrap();
    let loaded: SessionSnapshot = serde_json::from_slice(&saved).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&loaded);
    let tab = restored.get_tab(&id).unwrap();
    assert_eq!(tab.agent_runtime_config, original.agent_runtime_config);
    assert_eq!(tab.restorable_launch, original.restorable_launch);
    assert_eq!(
        tab.prior_session_binding_nonces,
        original.prior_session_binding_nonces
    );
    assert_eq!(tab.prior_session_binding_nonces.len(), 8);
}

#[test]
fn wire_serialization_preserves_server_side_closed_tab_restore() {
    let (mut state, id) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    populate(&mut state, &id, 7);
    let launch = state.get_tab(&id).unwrap().restorable_launch.clone();
    state.close_tab(&id).unwrap();
    let wire = serde_json::to_value(state.snapshot()).unwrap();
    assert!(wire["recently_closed"][0]
        .get("restorable_launch")
        .is_none());
    assert_eq!(
        state.list_recently_closed(&workspace)[0].restorable_launch,
        launch
    );
    let restored = state.reopen_closed_tab(&workspace, None).unwrap();
    assert_eq!(state.get_tab(&restored).unwrap().restorable_launch, launch);
}

#[test]
fn wire_cloud_mode_is_the_exact_persisted_launch_and_survives_restore() {
    let (mut state, id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.restorable_launch.execution_mode = Some("opaque-cloud".into());
    let encoded = serde_json::to_string(&state.to_session_snapshot()).unwrap();
    let saved: SessionSnapshot = serde_json::from_str(&encoded).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&saved);
    assert_eq!(
        restored
            .get_tab(&id)
            .unwrap()
            .restorable_launch
            .execution_mode
            .as_deref(),
        Some("opaque-cloud")
    );
    let wire = serde_json::to_value(restored.snapshot()).unwrap();
    assert_eq!(
        wire["tabs"][&id]["agent_runtime_config"]["executionMode"],
        "opaque-cloud"
    );
}
