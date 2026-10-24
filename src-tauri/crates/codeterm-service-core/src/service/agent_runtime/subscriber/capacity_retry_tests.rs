use super::*;
use crate::delayed_send::DelayedSendQueue;
use codeterm_types::agent::chat_types::ChatMsg;
use serde_json::json;

fn capacity_error(turn: &str, metadata: serde_json::Value) -> AgentRuntimeEvent {
    AgentRuntimeEvent::ChatItem {
        thread_id: "thread".into(),
        item: ChatMsg {
            id: format!("{turn}:error"),
            kind: "system".into(),
            content: "opaque diagnostic".into(),
            tool_flow_id: Some(turn.into()),
            system_subtype: Some("api_error".into()),
            tool_input: Some(metadata),
            ..Default::default()
        },
    }
}

fn failed(turn: &str) -> AgentRuntimeEvent {
    AgentRuntimeEvent::TurnState {
        thread_id: "thread".into(),
        turn_id: Some(turn.into()),
        state: RuntimeTurnState::Idle,
        detail: Some(RuntimeTurnDetail::Failed),
        status_line: None,
        started_at: None,
        completed_at: None,
        parent_thread_id: None,
    }
}

#[tokio::test]
async fn capacity_retry_runtime_fixture_schedules_one_visible_exact_turn_message() {
    let scene = fixtures::scene();
    let context = scene.session.context.lock().clone();
    scene
        .session
        .capacity_retry
        .lock()
        .begin("original", "opaque user payload");
    scene.session.capacity_retry.lock().bind("original", "turn");
    let error = capacity_error(
        "turn",
        json!({"retryableKind":"transient","retryable":false}),
    );
    publish_events(
        &scene.fixture.service,
        &scene.session,
        context.epoch,
        vec![error.clone(), error, failed("turn"), failed("turn")],
        123,
    )
    .await
    .unwrap();
    let messages =
        crate::delayed_send::list_views(&scene.fixture.service, Some(context.tab_id.clone()))
            .unwrap();
    assert_eq!(messages.len(), 1);
    let msg = &messages[0].message;
    assert_eq!(msg.text, "opaque user payload");
    assert_eq!(msg.scheduled_at_ms, 10_123);
    assert_eq!(msg.turn_retry.as_ref().unwrap().attempt, 1);
    assert!(scene
        .fixture
        .service
        .ext::<DelayedSendQueue>()
        .claim_due(10_122)
        .unwrap()
        .is_empty());
    assert!(scene
        .session
        .cache
        .lock()
        .snapshot()
        .iter()
        .any(|item| item.id == "turn:error"));
    crate::delayed_send::delete_message(&scene.fixture.service, msg.id.clone()).unwrap();
    assert!(scene
        .fixture
        .service
        .ext::<DelayedSendQueue>()
        .claim_due(10_123)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn capacity_retry_ignores_unknown_transient_foreign_and_unbound_errors() {
    for (bound_turn, error_turn, metadata) in [
        (
            Some("turn"),
            "turn",
            json!({"retryableKind":"rate_limited_until","retryable":false}),
        ),
        (
            Some("turn"),
            "turn",
            json!({"retryableKind":"rate_limited_until","retryAtMs":0,"retryable":false}),
        ),
        (
            Some("turn"),
            "turn",
            json!({"class":"server_overloaded","retryable":false}),
        ),
        (
            Some("turn"),
            "turn",
            json!({"retryableKind":"transient","retryable":true}),
        ),
        (
            Some("turn"),
            "foreign",
            json!({"retryableKind":"transient","retryable":false}),
        ),
        (
            None,
            "turn",
            json!({"retryableKind":"transient","retryable":false}),
        ),
    ] {
        let scene = fixtures::scene();
        let context = scene.session.context.lock().clone();
        scene
            .session
            .capacity_retry
            .lock()
            .begin("original", "opaque user payload");
        if let Some(turn) = bound_turn {
            scene.session.capacity_retry.lock().bind("original", turn);
        }
        publish_events(
            &scene.fixture.service,
            &scene.session,
            context.epoch,
            vec![capacity_error(error_turn, metadata), failed("turn")],
            1,
        )
        .await
        .unwrap();
        assert!(scene
            .fixture
            .service
            .ext::<DelayedSendQueue>()
            .list(Some(&context.tab_id))
            .is_empty());
    }
}

#[tokio::test]
async fn capacity_retry_limit_publishes_final_error_and_has_no_eleventh_message() {
    let scene = fixtures::scene();
    let context = scene.session.context.lock().clone();
    scene
        .session
        .capacity_retry
        .lock()
        .begin("original", "opaque payload");
    for attempt in 0..=10 {
        let turn = format!("turn-{attempt}");
        let send = scene.session.capacity_retry.lock().send_id.clone();
        scene.session.capacity_retry.lock().bind(&send, &turn);
        publish_events(
            &scene.fixture.service,
            &scene.session,
            context.epoch,
            vec![
                capacity_error(
                    &turn,
                    json!({"retryableKind":"transient","retryable":false}),
                ),
                failed(&turn),
            ],
            123,
        )
        .await
        .unwrap();
        let queue = scene.fixture.service.ext::<DelayedSendQueue>();
        let messages = queue.list(Some(&context.tab_id));
        if attempt == 10 {
            assert!(messages.is_empty());
            let rows = scene.session.cache.lock().snapshot();
            let final_error = rows
                .iter()
                .find(|row| row.id.ends_with(":exhausted"))
                .expect("visible final error");
            assert_eq!(final_error.system_subtype.as_deref(), Some("api_error"));
            assert!(final_error.content.contains("10 attempts"));
        } else {
            assert_eq!(messages.len(), 1);
            let msg = queue.delete(&messages[0].id).unwrap();
            assert!(scene
                .session
                .capacity_retry
                .lock()
                .claim(&msg.id, &format!("retry-{}", attempt + 1))
                .is_some());
        }
    }
}

#[tokio::test]
async fn capacity_retry_rate_limit_uses_one_visible_reset_time_message() {
    let scene = fixtures::scene();
    let context = scene.session.context.lock().clone();
    scene
        .session
        .capacity_retry
        .lock()
        .begin("original", "opaque payload");
    scene.session.capacity_retry.lock().bind("original", "turn");
    publish_events(
        &scene.fixture.service,
        &scene.session,
        context.epoch,
        vec![
            capacity_error(
                "turn",
                json!({"retryableKind":"rate_limited_until","retryAtMs":900_000,"retryable":false}),
            ),
            failed("turn"),
        ],
        123,
    )
    .await
    .unwrap();
    let queue = scene.fixture.service.ext::<DelayedSendQueue>();
    let messages =
        crate::delayed_send::list_views(&scene.fixture.service, Some(context.tab_id.clone()))
            .unwrap();
    assert_eq!(messages.len(), 1);
    let msg = &messages[0].message;
    assert_eq!(msg.scheduled_at_ms, 900_000);
    assert_eq!(
        msg.turn_retry.as_ref().unwrap().kind,
        RuntimeRetryableErrorKind::RateLimitedUntil
    );
    assert!(queue.claim_due(899_999).unwrap().is_empty());
    crate::delayed_send::delete_message(&scene.fixture.service, msg.id.clone()).unwrap();
    assert!(queue.claim_due(900_000).unwrap().is_empty());
}
