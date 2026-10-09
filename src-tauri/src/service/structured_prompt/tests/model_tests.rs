use super::*;
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use serde_json::json;
use std::path::PathBuf;

struct StructuredProvider;

impl AgentProvider for StructuredProvider {
    fn id(&self) -> &str {
        "fixture-structured"
    }
    fn display_name(&self) -> &str {
        self.id()
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn uses_structured_chat(&self) -> bool {
        true
    }
}

fn fixture() -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-structured".into());
    tab.agent_session_id = Some("ses_1".into());
    tab.session_binding_nonce = Some("owned-nonce".into());
    tab.reported_session_binding_nonce = Some("owned-nonce".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(StructuredProvider));
    (fixture, tab_id)
}

fn captured() -> Value {
    serde_json::from_str(include_str!("../fixtures/captured_message_updated.json")).unwrap()
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn switched_turn_records_actual_model_from_sse_and_http() {
    let (fixture, tab_id) = fixture();
    let svc = &fixture.service;
    svc.record_live_model_request(
        &tab_id,
        "fixture-structured",
        "ses_1",
        "deepseek/deepseek-v4-pro",
        None,
    )
    .unwrap();
    assert!(svc
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .agent_session_model
        .is_none());
    let mut state = ChatStreamState::default();
    let event = captured();
    handle_event(svc, &tab_id, &event, &mut state);
    {
        let mux = svc.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(
            tab.agent_session_model.as_deref(),
            Some("deepseek/deepseek-v4-pro")
        );
        let identity = tab.agent_session_model_identity.as_ref().unwrap();
        assert_eq!(identity.session_id, "ses_1");
        assert_eq!(identity.session_binding_nonce, "owned-nonce");
        assert!(tab.model_identity_mismatch.is_none());
    }
    let part = serde_json::from_str(include_str!(
        "../fixtures/captured_message_part_updated_text.json"
    ))
    .unwrap();
    let ParsedEvent::PartUpdated { update } = parse_event(&part) else {
        panic!("part");
    };
    let message = state.messages["msg_1"].clone();
    let row = state.prepare_chat_part(&update, &message).unwrap();
    assert_eq!(row.content, "121932631112635269");
    {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_model = None;
        tab.agent_session_model_identity = None;
    }
    let events = reconcile::snapshot_events(
        "ses_1",
        &json!([{"info": event["properties"]["info"], "parts": []}]),
    )
    .unwrap();
    for event in events {
        handle_event(svc, &tab_id, &event, &mut state);
    }
    assert_eq!(
        svc.mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_model
            .as_deref(),
        Some("deepseek/deepseek-v4-pro")
    );
    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.part_text["prt_text"], "121932631112635269");
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn turn_model_rejects_foreign_session_and_unverified_binding() {
    let (fixture, tab_id) = fixture();
    let svc = &fixture.service;
    let mut state = ChatStreamState::default();
    let mut event = captured();
    event["properties"]["info"]["sessionID"] = json!("foreign");
    handle_event(svc, &tab_id, &event, &mut state);
    assert!(state.messages.is_empty());
    assert!(svc
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .agent_session_model
        .is_none());
    svc.mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some("stale-nonce".into());
    handle_event(svc, &tab_id, &captured(), &mut state);
    let mux = svc.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(tab.agent_session_model.is_none());
    assert!(tab.agent_session_model_identity.is_none());
    assert_eq!(state.messages.len(), 1);
}
