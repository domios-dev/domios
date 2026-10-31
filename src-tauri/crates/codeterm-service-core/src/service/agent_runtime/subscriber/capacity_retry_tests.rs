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

fn finished(turn: &str, detail: Option<RuntimeTurnDetail>) -> AgentRuntimeEvent {
    AgentRuntimeEvent::TurnState {
        thread_id: "thread".into(),
        turn_id: Some(turn.into()),
        state: RuntimeTurnState::Idle,
        detail,
        status_line: None,
        started_at: None,
        completed_at: None,
        parent_thread_id: None,
    }
}

async fn fail(scene: &fixtures::Scene, turn: &str, metadata: serde_json::Value, at: u64) {
    let epoch = scene.session.context.lock().epoch;
    publish_events(
        &scene.fixture.service,
        &scene.session,
        epoch,
        vec![
            capacity_error(turn, metadata),
            finished(turn, Some(RuntimeTurnDetail::Failed)),
        ],
        at,
    )
    .await
    .unwrap();
}

fn transient() -> serde_json::Value {
    json!({"retryableKind":"transient","retryable":false})
}

#[tokio::test]
async fn capacity_retry_schedules_one_ordinary_continue_at_ten_seconds() {
    let scene = fixtures::scene();
    fail(&scene, "turn", transient(), 123).await;
    let epoch = scene.session.context.lock().epoch;
    publish_events(
        &scene.fixture.service,
        &scene.session,
        epoch,
        vec![
            capacity_error("turn", transient()),
            finished("turn", Some(RuntimeTurnDetail::Failed)),
        ],
        123,
    )
    .await
    .unwrap();
    let queue = scene.fixture.service.ext::<DelayedSendQueue>();
    let messages = queue.list(None);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "continue");
    assert_eq!(messages[0].scheduled_at_ms, 10_123);
    assert!(queue.claim_due(10_122).unwrap().is_empty());
    assert_eq!(queue.claim_due(10_123).unwrap().len(), 1);
    assert!(queue.claim_due(10_123).unwrap().is_empty());
    assert_eq!(
        scene
            .session
            .cache
            .lock()
            .snapshot()
            .iter()
            .filter(|item| item.content == "Automatic retry: attempt 1/10")
            .count(),
        1
    );
}

#[tokio::test]
async fn capacity_retry_rate_limit_schedules_at_reset_and_can_repeat() {
    let scene = fixtures::scene();
    for (turn, reset) in [("first", 900_000), ("second", 1_800_000)] {
        fail(
            &scene,
            turn,
            json!({"retryableKind":"rate_limited_until","retryAtMs":reset,"retryable":false}),
            123,
        )
        .await;
        let queue = scene.fixture.service.ext::<DelayedSendQueue>();
        let messages = queue.list(None);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "continue");
        assert_eq!(messages[0].scheduled_at_ms, reset);
        queue.delete(&messages[0].id).unwrap();
    }
    assert!(scene
        .session
        .cache
        .lock()
        .snapshot()
        .iter()
        .any(|item| item.content == "Automatic retry: attempt 2/10"));
}

#[tokio::test]
async fn capacity_retry_repeat_failure_increments_and_success_resets_per_tab() {
    let scene = fixtures::scene();
    let other = fixtures::scene();
    fail(&scene, "first", transient(), 1).await;
    fail(&scene, "second", transient(), 2).await;
    fail(&other, "other", transient(), 3).await;
    assert!(scene
        .session
        .cache
        .lock()
        .snapshot()
        .iter()
        .any(|item| item.content == "Automatic retry: attempt 2/10"));
    assert!(other
        .session
        .cache
        .lock()
        .snapshot()
        .iter()
        .any(|item| item.content == "Automatic retry: attempt 1/10"));
    let epoch = scene.session.context.lock().epoch;
    publish_events(
        &scene.fixture.service,
        &scene.session,
        epoch,
        vec![finished("success", None)],
        4,
    )
    .await
    .unwrap();
    assert!(scene
        .fixture
        .service
        .ext::<DelayedSendQueue>()
        .list(None)
        .is_empty());
    fail(&scene, "after-success", transient(), 5).await;
    assert_eq!(
        scene
            .session
            .cache
            .lock()
            .snapshot()
            .iter()
            .filter(|item| item.content == "Automatic retry: attempt 1/10")
            .count(),
        2
    );
}

#[tokio::test]
async fn capacity_retry_tenth_failure_stops_with_one_system_line() {
    let scene = fixtures::scene();
    for attempt in 1..=12 {
        fail(&scene, &format!("turn-{attempt}"), transient(), attempt).await;
        let messages = scene.fixture.service.ext::<DelayedSendQueue>().list(None);
        assert_eq!(messages.len(), usize::from(attempt < 10));
    }
    assert_eq!(scene.session.cache.lock().snapshot().iter().filter(|item| {
        item.content == "Automatic retry stopped after 10 attempts. Choose another model or send a new turn."
    }).count(), 1);
}

#[tokio::test]
async fn capacity_retry_ignores_unclassified_foreign_and_provider_managed_errors() {
    for metadata in [
        json!({"retryableKind":"rate_limited_until","retryable":false}),
        json!({"retryableKind":"rate_limited_until","retryAtMs":0,"retryable":false}),
        json!({"class":"server_overloaded","retryable":false}),
        json!({"retryableKind":"transient","retryable":true}),
    ] {
        let scene = fixtures::scene();
        fail(&scene, "turn", metadata, 1).await;
        assert!(scene
            .fixture
            .service
            .ext::<DelayedSendQueue>()
            .list(None)
            .is_empty());
    }
    let scene = fixtures::scene();
    let epoch = scene.session.context.lock().epoch;
    publish_events(
        &scene.fixture.service,
        &scene.session,
        epoch,
        vec![
            capacity_error("foreign", transient()),
            finished("own", Some(RuntimeTurnDetail::Failed)),
        ],
        1,
    )
    .await
    .unwrap();
    assert!(scene
        .fixture
        .service
        .ext::<DelayedSendQueue>()
        .list(None)
        .is_empty());
}

#[tokio::test]
async fn capacity_retry_persistence_refusal_still_records_completion_once() {
    let scene = fixtures::scene();
    let epoch = scene.session.context.lock().epoch;
    let before = scene.session.completions.lock().sequence();
    let changes = scene.session.control_changes.subscribe();
    let queue = scene.fixture.service.ext::<DelayedSendQueue>();
    queue.refuse_persistence(Some("injected queue persistence refusal"));
    let events = vec![
        capacity_error("turn", transient()),
        finished("turn", Some(RuntimeTurnDetail::Failed)),
    ];
    let result = publish_events(
        &scene.fixture.service,
        &scene.session,
        epoch,
        events.clone(),
        123,
    )
    .await;
    assert!(
        matches!(result, Err(super::super::model::RuntimeFailure::HookFailed(reason))
        if reason == "injected queue persistence refusal")
    );
    assert!(scene.session.completions.lock().contains("thread", "turn"));
    assert_eq!(scene.session.completions.lock().sequence(), before + 1);
    assert!(changes.has_changed().unwrap());
    let queued = queue.list(None);
    assert_eq!(queued.len(), 1);
    queue.refuse_persistence(None);
    publish_events(&scene.fixture.service, &scene.session, epoch, events, 124)
        .await
        .unwrap();
    assert_eq!(queue.list(None).len(), 1);
    assert_eq!(queue.list(None)[0].id, queued[0].id);
}
