use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::startup_nudge::StartupNudge;
use crate::service::test_fixture::ServiceFixture;

fn bound_parent_fixture() -> (ServiceFixture, String) {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let parent = mux.get_tab_mut(&parent_tab_id).unwrap();
    parent.agent_type = Some("claude".into());
    parent.pty_id = Some("parent-pty".into());
    parent.agent_session_id = Some("parent-session".into());
    parent.session_binding_nonce = Some("parent-nonce".into());
    parent.reported_session_binding_nonce = Some("parent-nonce".into());
    (ServiceFixture::new(mux), parent_tab_id)
}

fn unbound_agent_fixture() -> (ServiceFixture, String) {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let parent = mux.get_tab_mut(&parent_tab_id).unwrap();
    parent.agent_type = Some("claude".into());
    parent.pty_id = Some("parent-pty".into());
    (ServiceFixture::new(mux), parent_tab_id)
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn startup_notice_to_parent_on_trust_dialog_is_withheld() {
    let (fixture, parent_tab_id) = bound_parent_fixture();
    let prompt = concat!(
        "Select an option: Do you trust this folder?\r\n",
        "  > 1. Trust this folder\r\n",
        "    2. Exit\r\n"
    );
    fixture
        .service
        .screen_state
        .lock()
        .process("parent-pty", prompt.as_bytes());
    assert_eq!(
        fixture.service.classify_startup_nudge(&parent_tab_id),
        StartupNudge::BlockedOnPrompt
    );
    fixture.service.publish_agent_startup_report(
        &parent_tab_id,
        "worker-tab",
        "Worker",
        serde_json::json!({
            "kind": "agent_startup_failed",
            "attempt_id": "trust-attempt",
        }),
    );

    fixture
        .service
        .replay_pending_reports_if_routable(&parent_tab_id);

    assert_eq!(
        fixture
            .service
            .report_store
            .lock()
            .pending_for(&parent_tab_id)
            .len(),
        1
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn report_replay_from_output_trigger_skips_unbound_target() {
    let (fixture, parent_tab_id) = unbound_agent_fixture();
    fixture.service.report_store.lock().add(
        &parent_tab_id,
        "worker-tab",
        "Worker",
        "lifecycle notice",
    );

    fixture
        .service
        .replay_pending_reports_if_routable(&parent_tab_id);

    assert_eq!(
        fixture
            .service
            .report_store
            .lock()
            .pending_for(&parent_tab_id)
            .len(),
        1
    );
}
