use super::*;
use crate::plan::{OrchestratorRef, Plan, PlannerRef};

#[test]
fn post_open_binding_retargets_pre_send_lease_and_accepts_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: Some("preflight-session".into()),
        title: Some("Planner".into()),
    });
    store.save(&mut plan, 1).unwrap();
    store
        .queue_planner_request(&plan.id, "req".into(), "audit".into(), None, None, 2)
        .unwrap();
    store
        .claim_planner_requests(
            &plan.id,
            "planner",
            "preflight-session",
            "preflight-nonce",
            3,
        )
        .unwrap();

    assert!(store
        .rebind_planner_session(&plan.id, "planner", "opened-session", "opened-nonce", 4,)
        .unwrap());
    let rebound = store.get(&plan.id).unwrap();
    assert!(matches!(
        rebound.planner,
        Some(PlannerRef::Tab { session_id: Some(ref id), .. }) if id == "opened-session"
    ));
    let request = &rebound.planner_requests[0];
    assert_eq!(
        request.delivered_to_session_id.as_deref(),
        Some("opened-session")
    );
    assert_eq!(
        request.delivered_with_binding_nonce.as_deref(),
        Some("opened-nonce")
    );

    store
        .start_planner_request_delivery(
            &plan.id,
            "req",
            "opened-session",
            "opened-nonce",
            PlannerDeliveryTransport::ChatBackend,
            5,
        )
        .unwrap();
    store
        .accept_planner_request(&plan.id, "req", "preflight-session", "preflight-nonce", 6)
        .unwrap();
    assert_eq!(
        store.get(&plan.id).unwrap().planner_requests[0].state,
        PlannerRequestState::InProgress
    );
    store
        .accept_planner_request(&plan.id, "req", "opened-session", "opened-nonce", 7)
        .unwrap();
    assert!(store
        .claim_planner_requests(
            &plan.id,
            "replacement",
            "replacement-session",
            "replacement-nonce",
            DELIVERY_LEASE_MS + 8,
        )
        .unwrap()
        .is_empty());
}

#[test]
fn rotation_after_send_start_records_observed_identity_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .queue_planner_request(&plan.id, "req".into(), "audit".into(), None, None, 2)
        .unwrap();
    store
        .claim_planner_requests(&plan.id, "planner", "sent-session", "sent-nonce", 3)
        .unwrap();
    store
        .start_planner_request_delivery(
            &plan.id,
            "req",
            "sent-session",
            "sent-nonce",
            PlannerDeliveryTransport::ChatBackend,
            4,
        )
        .unwrap();
    store
        .mark_planner_delivery_uncertain(
            &plan.id,
            "req",
            "provider binding rotated after send started",
            Some("observed-session"),
            Some("observed-nonce"),
            5,
        )
        .unwrap();

    let persisted = store.get(&plan.id).unwrap();
    let request = &persisted.planner_requests[0];
    let inbox = &persisted.planner_inbox[0];
    assert_eq!(
        request.delivered_to_session_id.as_deref(),
        Some("sent-session")
    );
    assert_eq!(inbox.session_id.as_deref(), Some("observed-session"));
    assert_eq!(inbox.binding_nonce.as_deref(), Some("observed-nonce"));
    assert_eq!(inbox.phase, PlannerDeliveryPhase::DeliveryUncertain);
    assert!(store
        .claim_planner_requests(
            &plan.id,
            "planner",
            "observed-session",
            "observed-nonce",
            DELIVERY_LEASE_MS + 6,
        )
        .unwrap()
        .is_empty());
}

#[test]
fn requests_are_durable_and_claimed_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_request".into();
    store.save(&mut plan, 1).unwrap();
    store
        .queue_planner_request(
            &plan.id,
            "req_1".into(),
            "one\n<none>".into(),
            Some("caller"),
            Some("sid"),
            2,
        )
        .unwrap();
    assert_eq!(
        store
            .claim_planner_requests(&plan.id, "planner", "sid-p", "nonce-p", 3)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .claim_planner_requests(&plan.id, "planner", "sid-p", "nonce-p", 4)
        .unwrap()
        .is_empty());
    store
        .accept_planner_request(&plan.id, "req_1", "sid-p", "nonce-p", 4)
        .unwrap();
    let completed = store
        .complete_planner_request(&plan.id, "req_1", 5)
        .unwrap();
    let committed = store.get(&plan.id).unwrap().planner_requests.remove(0);
    assert_eq!(committed.state, PlannerRequestState::Completed);
    assert_eq!(completed, committed);
    assert_eq!(completed.completed_at, Some(5));
    assert_eq!(completed.lease_expires_at, None);
}

#[test]
fn replacement_claims_an_abandoned_lease_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_replacement".into();
    store.save(&mut plan, 1).unwrap();
    store
        .queue_planner_request(&plan.id, "req_1".into(), "audit".into(), None, None, 2)
        .unwrap();
    assert_eq!(
        store
            .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 3)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 4)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .claim_planner_requests(&plan.id, "new", "sid-new", "nonce-new", 5)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .claim_planner_requests(&plan.id, "new", "sid-new", "nonce-new", 6)
        .unwrap()
        .is_empty());
}

#[test]
fn provider_accepted_delivery_waits_for_ack_without_automatic_replay() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_ack".into();
    store.save(&mut plan, 1).unwrap();
    store
        .queue_planner_request(&plan.id, "req_1".into(), "audit".into(), None, None, 2)
        .unwrap();
    store
        .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 3)
        .unwrap();
    store
        .accept_planner_request(&plan.id, "req_1", "sid-old", "nonce-old", 4)
        .unwrap();

    assert!(store
        .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 5)
        .unwrap()
        .is_empty());
    assert!(store
        .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-new", 6)
        .unwrap()
        .is_empty());
}

#[test]
fn expired_in_progress_lease_replays_once_to_same_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_lease".into();
    store.save(&mut plan, 1).unwrap();
    store
        .queue_planner_request(&plan.id, "req_1".into(), "audit".into(), None, None, 2)
        .unwrap();
    store
        .claim_planner_requests(&plan.id, "planner", "sid", "nonce", 3)
        .unwrap();

    assert!(store
        .claim_planner_requests(&plan.id, "planner", "sid", "nonce", DELIVERY_LEASE_MS + 2,)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .claim_planner_requests(&plan.id, "planner", "sid", "nonce", DELIVERY_LEASE_MS + 3,)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn provider_accepted_delivery_never_replays_after_lease_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_accepted_lease".into();
    store.save(&mut plan, 1).unwrap();
    store
        .queue_planner_request(&plan.id, "req_1".into(), "audit".into(), None, None, 2)
        .unwrap();
    store
        .claim_planner_requests(&plan.id, "old", "sid-old", "nonce-old", 3)
        .unwrap();
    store
        .accept_planner_request(&plan.id, "req_1", "sid-old", "nonce-old", 4)
        .unwrap();

    let replacement = store
        .claim_planner_requests(
            &plan.id,
            "new",
            "sid-new",
            "nonce-new",
            DELIVERY_LEASE_MS + 5,
        )
        .unwrap();
    assert!(replacement.is_empty());
    store
        .release_planner_request(&plan.id, "req_1", "sid-old", "nonce-old", 6)
        .unwrap();
    let request = &store.get(&plan.id).unwrap().planner_requests[0];
    assert_eq!(request.state, PlannerRequestState::Accepted);
    assert_eq!(request.delivered_to_session_id.as_deref(), Some("sid-old"));
}

#[test]
fn powershell_and_posix_payload_is_stored_as_data_without_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_shell_data".into();
    store.save(&mut plan, 1).unwrap();
    let payload = "- audit `model`\n- <nonexistent>\n$env:BAD='x'; echo $(whoami)\n$(false) && :";
    let request = store
        .queue_planner_request(
            &plan.id,
            "req_shell".into(),
            payload.into(),
            Some("caller"),
            Some("caller-sid"),
            2,
        )
        .unwrap();
    assert_eq!(request.payload, payload);
    assert_eq!(
        store.get(&plan.id).unwrap().planner_requests[0].payload,
        payload
    );
}

#[test]
fn pty_and_chatbackend_crash_windows_never_resend_uncertain_delivery() {
    for transport in [
        PlannerDeliveryTransport::PtyAgent,
        PlannerDeliveryTransport::ChatBackend,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "P".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = format!("pl_{transport:?}");
        store.save(&mut plan, 1).unwrap();
        store
            .queue_planner_request(&plan.id, "req".into(), "audit".into(), None, None, 2)
            .unwrap();

        store
            .claim_planner_requests(&plan.id, "planner", "sid", "nonce", 3)
            .unwrap();
        store
            .start_planner_request_delivery(&plan.id, "req", "sid", "nonce", transport, 4)
            .unwrap();
        assert!(store
            .claim_planner_requests(
                &plan.id,
                "replacement",
                "sid-new",
                "nonce-new",
                DELIVERY_LEASE_MS + 5,
            )
            .unwrap()
            .is_empty());
        let plan = store.get(&plan.id).unwrap();
        let inbox = &plan.planner_inbox[0];
        assert_eq!(inbox.phase, PlannerDeliveryPhase::DeliveryUncertain);
        assert_eq!(inbox.transport, Some(transport));
        assert!(inbox
            .last_error
            .as_deref()
            .unwrap()
            .contains("send started"));
    }
}

#[test]
fn provider_accept_and_planner_ack_are_durable_inbox_phases() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "P".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .queue_planner_request(&plan.id, "req".into(), "audit".into(), None, None, 2)
        .unwrap();
    store
        .claim_planner_requests(&plan.id, "planner", "sid", "nonce", 3)
        .unwrap();
    store
        .start_planner_request_delivery(
            &plan.id,
            "req",
            "sid",
            "nonce",
            PlannerDeliveryTransport::ChatBackend,
            4,
        )
        .unwrap();
    store
        .accept_planner_request(&plan.id, "req", "sid", "nonce", 5)
        .unwrap();
    assert_eq!(
        store.get(&plan.id).unwrap().planner_inbox[0].phase,
        PlannerDeliveryPhase::ProviderAccepted
    );
    store.complete_planner_request(&plan.id, "req", 6).unwrap();
    assert_eq!(
        store.get(&plan.id).unwrap().planner_inbox[0].phase,
        PlannerDeliveryPhase::PlannerAcked
    );
}
