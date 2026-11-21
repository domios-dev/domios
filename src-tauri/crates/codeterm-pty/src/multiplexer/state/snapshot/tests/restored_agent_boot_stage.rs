use super::*;
use crate::multiplexer::types::AgentRole;
use codeterm_types::pty::BootStage;

#[test]
fn failed_role_launch_roundtrips_failure_without_creating_live_identity() {
    for (reason, identity) in [
        (None, false),
        (Some("opaque supervision failure"), false),
        (Some("opaque supervision failure"), true),
    ] {
        let (mut state, id) = MultiplexerState::new();
        let tab = state.get_tab_mut(&id).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("orchestrator".into());
        tab.team_id = Some("owned-team".into());
        tab.parent_tab_id = Some("owned-parent".into());
        tab.pty_id = Some("surviving-shell".into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        tab.boot_failure_reason = reason.map(str::to_owned);
        if identity {
            tab.agent_type = Some("historical-provider".into());
            tab.agent_session_id = Some("historical-session".into());
            tab.session_binding_nonce = Some("historical-nonce".into());
        }
        let encoded = serde_json::to_vec(&state.to_session_snapshot()).unwrap();
        let snapshot = serde_json::from_slice(&encoded).unwrap();
        let restored = MultiplexerState::from_session_snapshot(&snapshot);
        let tab = restored.get_tab(&id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
        assert_eq!(tab.boot_failure_reason.as_deref(), reason);
        assert_eq!(tab.role, Some(AgentRole::Manager));
        assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
        assert_eq!(tab.team_id.as_deref(), Some("owned-team"));
        assert_eq!(tab.parent_tab_id.as_deref(), Some("owned-parent"));
        assert_eq!(tab.backing, TabBacking::Pty);
        assert!(tab.frozen);
        assert!(tab.pty_id.is_none());
        assert_eq!(
            tab.agent_type.as_deref(),
            identity.then_some("historical-provider")
        );
        assert_eq!(
            tab.agent_session_id.as_deref(),
            identity.then_some("historical-session")
        );
        assert_eq!(
            tab.session_binding_nonce.as_deref(),
            identity.then_some("historical-nonce")
        );
        assert!(tab.reported_session_binding_nonce.is_none());
        assert!(tab.agent_runtime_state.is_none());
        assert!(!tab.agent_booting);
        assert!(!tab.launch_session_authenticated());
        assert_eq!(
            crate::multiplexer::state::tab_status_label(tab, None),
            "error"
        );
    }
}

#[test]
fn contradictory_failed_binding_survives_snapshot_as_refusal_without_authentication() {
    let (mut state, id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.role = Some(AgentRole::Manager);
    tab.role_profile = Some("orchestrator".into());
    tab.agent_type = Some("retained-provider".into());
    tab.agent_session_id = Some("retained-session".into());
    tab.session_binding_nonce = Some("contradictory-proof".into());
    tab.reported_session_binding_nonce = Some("contradictory-proof".into());
    tab.boot_stage = Some(BootStage::SpawnFailed);
    tab.boot_failure_reason = Some("actual failure".into());
    let snapshot = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    let tab = restored.get_tab(&id).unwrap();
    assert_eq!(
        tab.reported_session_binding_nonce.as_deref(),
        Some("contradictory-proof")
    );
    assert!(tab.session_binding_nonce.is_none());
    assert!(!tab.launch_session_authenticated());
    assert!(tab
        .prior_session_binding_nonces
        .iter()
        .any(|prior| prior.nonce == "contradictory-proof"
            && prior.session_id.as_deref() == Some("retained-session")));
    assert_eq!(tab.boot_failure_reason.as_deref(), Some("actual failure"));
    assert_eq!(tab.role, Some(AgentRole::Manager));
    assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert!(tab.frozen);
    let recaptured = serde_json::to_value(restored.to_session_snapshot()).unwrap();
    assert!(recaptured.to_string().contains("contradictory-proof"));
}

#[test]
fn restored_chat_backend_waits_for_runtime_instead_of_a_pty() {
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-runtime".into());
    tab.agent_session_id = Some("saved-thread".into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "fixture-runtime".into(),
        session_id: "saved-thread".into(),
    };
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_runtime_state = Some(serde_json::json!({"state":"loaded"}));

    let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
    let tab = restored.get_tab(&tab_id).unwrap();
    assert!(matches!(tab.backing, TabBacking::ChatBackend { .. }));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentLaunching));
    assert!(tab.pty_id.is_none());
}

#[test]
fn restoring_agent_identity_does_not_restore_stale_input_readiness() {
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("saved-session".into());
    tab.boot_stage = Some(BootStage::AgentReady);

    let snapshot = state.to_session_snapshot();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    let tab = restored.get_tab(&tab_id).expect("restored agent tab");

    assert_eq!(tab.agent_type.as_deref(), Some("fixture-provider"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
    assert_eq!(tab.boot_stage, Some(BootStage::PtyStarting));
    assert!(tab.restored_from_snapshot);
    assert!(tab.pty_id.is_none());
}

#[test]
fn restored_provider_activity_requires_fresh_own_session_evidence() {
    use codeterm_types::pty::{TabVerdict, TimelineState};

    for provider in ["grok", "fixture-agent", "fixture-provider"] {
        for saved in [
            TimelineState::Working,
            TimelineState::Question,
            TimelineState::Idle,
        ] {
            let (mut state, tab_id) = MultiplexerState::new();
            let tab = state.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some(provider.into());
            tab.agent_session_id = Some("own-session".into());
            tab.agent_state = Some(
                TabVerdict {
                    state: saved,
                    subtype: None,
                }
                .into(),
            );

            let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
            let tab = restored.get_tab(&tab_id).unwrap();
            assert_eq!(tab.agent_session_id.as_deref(), Some("own-session"));
            assert!(
                tab.agent_state.is_none(),
                "{provider}: {saved:?} is historical"
            );
            assert_eq!(
                crate::multiplexer::state::effective_tab_status(tab, None),
                codeterm_types::pty::TabStatus::Idle
            );
        }
    }
}
