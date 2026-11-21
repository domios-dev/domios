use super::*;
use codeterm_service_core::{
    agent::chat_types::ChatMsg,
    multiplexer::state::MultiplexerState,
    multiplexer::types::AgentRole,
    service::agent_runtime::engine::{self, test_support},
};
use crate::traits::HierarchyExt;
use crate::test_fixture::ServiceFixture;
use codeterm_service_core::agent::reports::ReportDetails;

#[test]
fn report_position_keeps_the_untimed_source_row_before_final_text() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    let tab = tab.clone();
    let fixture = ServiceFixture::new(mux);
    let _runtime = test_support::live_runtime(&tab);
    let session = engine::for_tab(&tab_id).unwrap();
    session.cache.lock().reconcile(ChatMsg {
        id: "report-tool".into(),
        kind: "tool_call".into(),
        ..Default::default()
    });
    let position = sender_position(&fixture.service, &tab_id);
    session.cache.lock().reconcile(ChatMsg {
        id: "final-text".into(),
        kind: "assistant".into(),
        content: "finished".into(),
        ..Default::default()
    });
    assert_eq!(
        position,
        Some(codeterm_service_core::agent::reports::ReportSenderPosition {
            source_session_id: "own-session".into(),
            message_id: "report-tool".into(),
        })
    );
    assert_eq!(
        sender_position(&fixture.service, &tab_id)
            .as_ref()
            .map(|position| position.message_id.as_str()),
        Some("final-text")
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .session_binding_nonce = Some("new-nonce".into());
    assert_eq!(sender_position(&fixture.service, &tab_id), None);
}

#[test]
fn normal_hierarchy_report_stores_and_emits_its_creation_position() {
    let (mut mux, orchestrator_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator_id).unwrap();
    let worker_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker_id, AgentRole::Worker, Some(&orchestrator_id))
        .unwrap();
    mux.get_tab_mut(&worker_id).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new("fixture"),
    ));
    let _pty = fixture.bind_test_pty_session(&worker_id, "worker-session", "worker-nonce");
    let _worker_deliveries = fixture.record_agent_deliveries(&worker_id);
    let _orchestrator_deliveries =
        fixture.bind_recorded_agent_recipient(&orchestrator_id, "owner-session", "owner-nonce");
    let worker = fixture
        .service
        .mux
        .read()
        .get_tab(&worker_id)
        .unwrap()
        .clone();
    let _runtime = test_support::live_runtime(&worker);
    let session = engine::for_tab(&worker_id).unwrap();
    session.cache.lock().reconcile(ChatMsg {
        id: "ack-row".into(),
        kind: "assistant".into(),
        ..Default::default()
    });

    let result = fixture
        .service
        .submit_report(
            &worker_id,
            "opaque hierarchy report",
            ReportDetails::default(),
            None,
            None,
            None,
        )
        .unwrap();
    session.cache.lock().reconcile(ChatMsg {
        id: "later-row".into(),
        kind: "assistant".into(),
        content: "later output".into(),
        ..Default::default()
    });

    let outbound = fixture
        .service
        .report_store
        .lock()
        .outbound_for_sender(&worker_id);
    assert_eq!(outbound.len(), 1);
    assert_eq!(outbound[0].report.id, result.report_id);
    assert_eq!(
        outbound[0]
            .report
            .sender_position
            .as_ref()
            .map(|position| position.message_id.as_str()),
        Some("ack-row")
    );
    let event = fixture
        .ui
        .0
        .lock()
        .iter()
        .find(|(name, _)| name == "agent-report")
        .map(|(_, payload)| payload.clone())
        .unwrap();
    assert_eq!(event["sender_position"]["message_id"], "ack-row");
    assert_eq!(event["report"]["sender_position"]["message_id"], "ack-row");
    assert_eq!(event["report"]["kind"], serde_json::Value::Null);
}
