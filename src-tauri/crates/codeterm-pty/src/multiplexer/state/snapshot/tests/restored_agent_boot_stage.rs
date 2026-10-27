use super::*;
use codeterm_types::pty::BootStage;

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
