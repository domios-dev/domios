use super::super::history::{Entry, HistoryReceipt, Submission};
use super::*;
use crate::{
    multiplexer::{state::MultiplexerState, types::TabBacking},
    service::test_fixture::ServiceFixture,
};
use codeterm_neighbors::wire::DeliveryStatus;

fn entry(id: usize, direction: &str, kind: &str, reply: Option<String>) -> Entry {
    serde_json::from_value(json!({"direction":direction,"author":if direction=="in"{"friend"}else{"me"},"sender_uid":"opaque-sender","body":"opaque-private-body","at":100,"message_id":format!("{id:032x}"),"kind":kind,"in_reply_to":reply,"handling_state":if direction=="in"{Some("waiting")}else{None},"handling_reason":if direction=="in"{Some("concierge_missing")}else{None}})).unwrap()
}
async fn fixture(runtime: bool) -> (ServiceFixture, Metadata, String, String) {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    let target = mux.get_tab_mut(&tab).unwrap();
    target.agent_type = Some("fixture".into());
    target.agent_session_id = Some("own-session".into());
    target.session_binding_nonce = Some("own-nonce".into());
    target.reported_session_binding_nonce = Some("own-nonce".into());
    if runtime {
        target.backing = TabBacking::ChatBackend {
            plugin_id: "fixture".into(),
            session_id: "own-session".into(),
        };
    }
    let fixture = ServiceFixture::new(mux);
    let friend = "a".repeat(64);
    fixture
        .seed_neighbor_concierge(
            &tab,
            &friend,
            serde_json::from_value(json!({"allowed":true,"scope":"opaque-scope"})).unwrap(),
        )
        .await;
    let uid = fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .neighbor_authority
        .as_ref()
        .unwrap()
        .uid
        .clone();
    let metadata = Metadata::new(fixture.service.account_data_root().unwrap(), &uid);
    for id in [1, 2] {
        fixture
            .service
            .append_neighbor_history(
                &metadata,
                &friend,
                entry(id, "in", "question", None),
                fixture.service.neighbor_history_generation(),
            )
            .await
            .unwrap();
        fixture
            .service
            .prepare_neighbor_handling(
                &metadata,
                &friend,
                &format!("{id:032x}"),
                &tab,
                fixture.service.neighbor_history_generation(),
            )
            .await
            .unwrap();
    }
    (fixture, metadata, friend, tab)
}
async fn state(metadata: &Metadata, friend: &str, id: usize) -> State {
    metadata
        .history(friend)
        .await
        .unwrap()
        .entries
        .iter()
        .find(|entry| entry.message_id == format!("{id:032x}"))
        .unwrap()
        .handling
        .handling_state
        .unwrap()
}

#[tokio::test]
async fn runtime_receipt_and_exact_turn_edges_persist_delivered_working_and_no_reply() {
    let (fixture, metadata, friend, tab) = fixture(true).await;
    let svc = &fixture.service;
    let id = format!("{:032x}", 1);
    svc.neighbor_handling_delivery(
        &metadata,
        &friend,
        &id,
        State::DeliveredToConcierge,
        None,
        svc.neighbor_history_generation(),
    )
    .await
    .unwrap();
    assert_eq!(
        state(&metadata, &friend, 1).await,
        State::DeliveredToConcierge
    );
    svc.neighbor_runtime_handling_event(
        &tab,
        "own-nonce",
        "own-session",
        "turn-a",
        Some(&id),
        State::Working,
    )
    .await
    .unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Working);
    svc.neighbor_runtime_handling_event(
        &tab,
        "own-nonce",
        "own-session",
        "foreign-turn",
        None,
        State::NoReply,
    )
    .await
    .unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Working);
    svc.neighbor_runtime_handling_event(
        &tab,
        "stale-nonce",
        "own-session",
        "turn-a",
        None,
        State::NoReply,
    )
    .await
    .unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Working);
    svc.neighbor_runtime_handling_event(
        &tab,
        "own-nonce",
        "own-session",
        "turn-a",
        None,
        State::NoReply,
    )
    .await
    .unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::NoReply);
    assert_eq!(state(&metadata, &friend, 2).await, State::Waiting);
    let page = metadata.history_page(&friend, None, 50).await.unwrap();
    assert!(!page.to_string().contains("own-nonce"));
    assert!(page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry.get("handling_owner").is_none()));
    let status = svc.neighbor_handling_status();
    assert!(!status.to_string().contains("opaque-private-body"));
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .any(|(kind, _)| kind == "neighbor-history-changed"));
}

#[tokio::test]
async fn confirmed_linked_answers_and_refusals_win_over_turn_end_and_failed_replies_do_not_answer()
{
    for (kind, expected) in [("answer", State::Answered), ("refusal", State::Refused)] {
        let (fixture, metadata, friend, tab) = fixture(true).await;
        let svc = &fixture.service;
        let incoming = format!("{:032x}", 1);
        let outgoing = format!("{:032x}", 3);
        svc.neighbor_runtime_handling_event(
            &tab,
            "own-nonce",
            "own-session",
            "turn",
            Some(&incoming),
            State::Working,
        )
        .await
        .unwrap();
        svc.append_neighbor_history(
            &metadata,
            &friend,
            entry(3, "out", kind, Some(incoming)),
            svc.neighbor_history_generation(),
        )
        .await
        .unwrap();
        assert_eq!(state(&metadata, &friend, 1).await, State::Working);
        svc.update_neighbor_history_status(
            &metadata,
            &outgoing,
            HistoryReceipt {
                delivery: DeliveryStatus::Pending.into(),
                rejection: None,
                submission: Some(Submission::Accepted),
            },
            svc.neighbor_history_generation(),
        )
        .await
        .unwrap();
        assert_eq!(state(&metadata, &friend, 1).await, expected);
        svc.neighbor_runtime_handling_event(
            &tab,
            "own-nonce",
            "own-session",
            "turn",
            None,
            State::NoReply,
        )
        .await
        .unwrap();
        assert_eq!(state(&metadata, &friend, 1).await, expected);
        svc.update_neighbor_history_status(
            &metadata,
            &outgoing,
            DeliveryStatus::NotDelivered.into(),
            svc.neighbor_history_generation(),
        )
        .await
        .unwrap();
        assert_eq!(state(&metadata, &friend, 1).await, State::Failed);
    }
}

#[tokio::test]
async fn failed_delivery_can_be_confirmed_later_and_old_runtime_bindings_are_not_claimed() {
    let (fixture, metadata, friend, tab) = fixture(true).await;
    let svc = &fixture.service;
    let id = format!("{:032x}", 1);
    svc.neighbor_handling_delivery(
        &metadata,
        &friend,
        &id,
        State::Failed,
        Some("concierge_delivery_unconfirmed"),
        svc.neighbor_history_generation(),
    )
    .await
    .unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Failed);
    svc.neighbor_runtime_handling_event(
        &tab,
        "own-nonce",
        "own-session",
        "turn",
        Some(&id),
        State::Working,
    )
    .await
    .unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Working);
    svc.mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .session_binding_nonce = Some("replacement-nonce".into());
    svc.reconcile_neighbor_handling_restore(&metadata, &friend, svc.neighbor_history_generation())
        .await
        .unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Failed);
    svc.neighbor_runtime_handling_event(
        &tab,
        "own-nonce",
        "own-session",
        "turn",
        Some(&id),
        State::Working,
    )
    .await
    .unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Failed);
}

#[tokio::test]
async fn pty_start_and_completion_use_the_exact_bound_session_and_coalesced_edges() {
    let (fixture, metadata, friend, tab) = fixture(false).await;
    let svc = &fixture.service;
    let owner = svc.neighbor_handling_owner(&tab).unwrap();
    let mut edge = PtyEdge {
        owner,
        version: 1,
        started: 1,
        ended: 0,
        working: true,
        error: false,
        generation: svc.neighbor_history_generation(),
    };
    svc.neighbor_handling_delivery(
        &metadata,
        &friend,
        &format!("{:032x}", 1),
        State::DeliveredToConcierge,
        None,
        svc.neighbor_history_generation(),
    )
    .await
    .unwrap();
    svc.apply_neighbor_pty_edge(&edge).await.unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Working);
    edge.version = 2;
    edge.ended = 2;
    edge.working = false;
    svc.apply_neighbor_pty_edge(&edge).await.unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::NoReply);
    assert_eq!(state(&metadata, &friend, 2).await, State::Waiting);
    svc.neighbor_handling_delivery(
        &metadata,
        &friend,
        &format!("{:032x}", 2),
        State::DeliveredToConcierge,
        None,
        svc.neighbor_history_generation(),
    )
    .await
    .unwrap();
    edge.error = true;
    svc.apply_neighbor_pty_edge(&edge).await.unwrap();
    assert_eq!(state(&metadata, &friend, 2).await, State::Failed);
}

#[tokio::test]
async fn owner_stop_interrupts_only_the_bound_concierge_and_never_sends_a_refusal() {
    let (fixture, metadata, friend, tab) = fixture(false).await;
    let svc = &fixture.service;
    let id = format!("{:032x}", 1);
    svc.neighbor_handling_delivery(
        &metadata,
        &friend,
        &id,
        State::DeliveredToConcierge,
        None,
        svc.neighbor_history_generation(),
    )
    .await
    .unwrap();
    let edge = PtyEdge {
        owner: svc.neighbor_handling_owner(&tab).unwrap(),
        version: 1,
        started: 1,
        ended: 0,
        working: true,
        error: false,
        generation: svc.neighbor_history_generation(),
    };
    svc.apply_neighbor_pty_edge(&edge).await.unwrap();
    let target = tab.clone();
    let response = svc
        .stop_neighbor_concierge_with(&friend, move |actual| async move {
            assert_eq!(actual, target);
            Ok(json!({"stopped":true}))
        })
        .await
        .unwrap();
    assert_eq!(response["message_ids"], json!([id]));
    assert_eq!(state(&metadata, &friend, 1).await, State::Stopped);
    assert_eq!(state(&metadata, &friend, 2).await, State::Waiting);
    let mut late = edge;
    late.version = 2;
    late.ended = 2;
    late.working = false;
    svc.apply_neighbor_pty_edge(&late).await.unwrap();
    assert_eq!(state(&metadata, &friend, 1).await, State::Stopped);
    assert!(metadata
        .history(&friend)
        .await
        .unwrap()
        .entries
        .iter()
        .all(|entry| entry.direction == "in"));
}

#[tokio::test]
async fn waiting_missing_or_paused_is_retained_without_admitting_a_turn() {
    let (fixture, metadata, friend, _) = fixture(false).await;
    let svc = &fixture.service;
    svc.append_neighbor_history(
        &metadata,
        &friend,
        entry(4, "in", "question", None),
        svc.neighbor_history_generation(),
    )
    .await
    .unwrap();
    let mut settings = metadata.settings().await.unwrap();
    settings.paused.insert(friend.clone());
    metadata.save_settings(settings).await.unwrap();
    svc.reconcile_neighbor_handling_restore(&metadata, &friend, svc.neighbor_history_generation())
        .await
        .unwrap();
    let history = metadata.history(&friend).await.unwrap();
    let waiting = history
        .entries
        .iter()
        .find(|entry| entry.message_id == format!("{:032x}", 4))
        .unwrap();
    assert_eq!(waiting.handling.handling_state, Some(State::Waiting));
    assert_eq!(waiting.handling.handling_reason.as_deref(), Some("paused"));
    assert!(waiting.handling.handling_owner.is_none());
}
