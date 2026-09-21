use crate::multiplexer::state::MultiplexerState;
use crate::service::startup_nudge::StartupNudge;
use crate::service::test_fixture::{
    FailFirstHierarchyReportDelivery, RecordingHierarchyReportDelivery, ServiceFixture,
};

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
        fixture
            .service
            .classify_startup_nudge_for_hierarchy_report(&parent_tab_id),
        StartupNudge::BlockedOnPrompt
    );
    let delivery = RecordingHierarchyReportDelivery::default();
    fixture.service.publish_agent_startup_report_with_delivery(
        &parent_tab_id,
        "worker-tab",
        "Worker",
        serde_json::json!({
            "kind": "agent_startup_failed",
            "attempt_id": "trust-attempt",
        }),
        &delivery,
    );

    fixture
        .service
        .replay_pending_reports_with_delivery(&parent_tab_id, &delivery);

    assert!(delivery.writes.lock().is_empty());
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
fn failed_startup_notice_delivery_releases_claim_for_replay() {
    let (fixture, parent_tab_id) = bound_parent_fixture();
    let delivery = FailFirstHierarchyReportDelivery::default();
    fixture.service.publish_agent_startup_report_with_delivery(
        &parent_tab_id,
        "worker-tab",
        "Worker",
        serde_json::json!({
            "kind": "agent_startup_failed",
            "attempt_id": "retry-attempt",
        }),
        &delivery,
    );

    let report_id = {
        let mut store = fixture.service.report_store.lock();
        let pending = store.pending_for(&parent_tab_id);
        assert_eq!(pending.len(), 1);
        let report_id = pending[0].id.clone();
        assert!(
            store.claim(&report_id),
            "a failed delivery should release its report claim"
        );
        store.release_delivery(&report_id);
        report_id
    };

    fixture
        .service
        .replay_pending_reports_with_delivery(&parent_tab_id, &delivery);

    assert!(fixture
        .service
        .report_store
        .lock()
        .pending_for(&parent_tab_id)
        .is_empty());
    assert_eq!(
        delivery.attempts.lock().as_slice(),
        &[
            (parent_tab_id.clone(), report_id.clone()),
            (parent_tab_id.clone(), report_id.clone()),
        ]
    );
    assert_eq!(
        delivery.writes.lock().as_slice(),
        &[(parent_tab_id, report_id)]
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn report_replay_skips_unbound_agent_target() {
    let (fixture, parent_tab_id) = unbound_agent_fixture();
    let delivery = RecordingHierarchyReportDelivery::default();
    fixture.service.report_store.lock().add(
        &parent_tab_id,
        "worker-tab",
        "Worker",
        "lifecycle notice",
    );

    fixture
        .service
        .replay_pending_reports_with_delivery(&parent_tab_id, &delivery);

    assert!(delivery.writes.lock().is_empty());
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
