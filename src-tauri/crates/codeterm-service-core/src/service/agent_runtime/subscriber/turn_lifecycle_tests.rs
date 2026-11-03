use super::*;
use codeterm_types::agent::chat_types::ChatMsg;
use codeterm_types::pty::TimelineState;

fn turn(id: Option<&str>, state: RuntimeTurnState) -> AgentRuntimeEvent {
    AgentRuntimeEvent::TurnState {
        thread_id: "thread".into(),
        turn_id: id.map(str::to_string),
        parent_thread_id: None,
        state,
        detail: None,
        status_line: Some("opaque command".into()),
        started_at: None,
        completed_at: None,
    }
}

fn command(status: &str) -> AgentRuntimeEvent {
    AgentRuntimeEvent::ChatItem {
        thread_id: "thread".into(),
        item: ChatMsg {
            id: "command".into(),
            kind: "tool_call".into(),
            tool_flow_id: Some("first".into()),
            tool_status: Some(status.into()),
            ..Default::default()
        },
    }
}

async fn publish(scene: &fixtures::Scene, events: Vec<AgentRuntimeEvent>, at: u64) {
    let epoch = scene.session.context.lock().epoch;
    publish_events(&scene.fixture.service, &scene.session, epoch, events, at)
        .await
        .unwrap();
}

fn assert_idle(scene: &fixtures::Scene) {
    assert!(scene.session.context.lock().active_turn_id.is_none());
    assert!(scene.session.context.lock().active_turns.is_empty());
    assert!(scene
        .session
        .state
        .lock()
        .active_turn_started_at_ms
        .is_none());
    let tab = scene.session.context.lock().tab_id.clone();
    let mux = scene.fixture.service.mux.read();
    let target = mux.get_tab(&tab).unwrap();
    let live = target.agent_state.as_ref().unwrap();
    assert_eq!(live.state, TimelineState::Idle);
    assert!(live.status_line.is_none());
    assert!(target.agent_runtime_state.as_ref().unwrap()["active_turn_started_at_ms"].is_null());
}

#[tokio::test]
async fn runtime_turn_start_and_completion_publish_tab_status() {
    use crate::agent::status::TabStatus;
    use crate::multiplexer::types::TabBacking;

    let scene = fixtures::scene();
    let tab_id = scene.session.context.lock().tab_id.clone();
    scene
        .fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .backing = TabBacking::ChatBackend {
        plugin_id: scene.session.provider.clone(),
        session_id: "thread".into(),
    };
    for (state, expected, at) in [
        (RuntimeTurnState::Working, TabStatus::Working, 10),
        (RuntimeTurnState::Idle, TabStatus::Idle, 20),
    ] {
        publish(&scene, vec![turn(Some("first"), state)], at).await;
        let svc = &scene.fixture.service;
        let snapshot = svc.snapshot_with_reconciled_status();
        assert_eq!(snapshot.tab_statuses.get(&tab_id), Some(&expected));
        {
            let events = scene.fixture.ui.0.lock();
            let (_, payload) = events
                .iter()
                .rev()
                .find(|(name, payload)| name == "tab-status-changed" && payload["tab_id"] == tab_id)
                .unwrap();
            assert_eq!(payload["status"], format!("{expected:?}"));
        }
        let mux = svc.mux.read();
        assert_eq!(
            svc.resolved_tab_status(mux.get_tab(&tab_id).unwrap()),
            Some(expected)
        );
    }
}

#[tokio::test]
async fn command_completion_after_a_new_turn_cannot_resurrect_the_previous_turn() {
    let scene = fixtures::scene();
    publish(
        &scene,
        vec![
            turn(Some("first"), RuntimeTurnState::Working),
            command("running"),
        ],
        10,
    )
    .await;
    publish(
        &scene,
        vec![turn(Some("first"), RuntimeTurnState::Idle)],
        20,
    )
    .await;
    publish(
        &scene,
        vec![turn(Some("next"), RuntimeTurnState::Working)],
        30,
    )
    .await;
    publish(
        &scene,
        vec![
            command("completed"),
            turn(Some("first"), RuntimeTurnState::Working),
        ],
        40,
    )
    .await;
    assert_eq!(
        scene.session.context.lock().active_turn_id.as_deref(),
        Some("next")
    );
    assert_eq!(
        scene.session.state.lock().active_turn_started_at_ms,
        Some(30)
    );
    publish(&scene, vec![turn(Some("next"), RuntimeTurnState::Idle)], 50).await;
    assert_idle(&scene);
    let row = scene.session.cache.lock().snapshot().pop().unwrap();
    assert_eq!(row.tool_status.as_deref(), Some("completed"));
}

#[tokio::test]
async fn a_later_turn_settles_after_a_lost_completion_and_late_command_update() {
    let scene = fixtures::scene();
    publish(
        &scene,
        vec![
            turn(Some("first"), RuntimeTurnState::Working),
            command("running"),
        ],
        10,
    )
    .await;
    publish(
        &scene,
        vec![turn(Some("next"), RuntimeTurnState::Working)],
        20,
    )
    .await;
    publish(
        &scene,
        vec![turn(Some("first"), RuntimeTurnState::Working)],
        30,
    )
    .await;
    publish(&scene, vec![turn(Some("next"), RuntimeTurnState::Idle)], 40).await;
    assert_idle(&scene);
    assert!(scene.session.completions.lock().contains("thread", "first"));
    let row = scene.session.cache.lock().snapshot().pop().unwrap();
    assert_eq!(row.tool_status.as_deref(), Some("abandoned"));
}

#[tokio::test]
async fn status_without_a_turn_preserves_live_identity_and_cannot_restart_idle() {
    let scene = fixtures::scene();
    publish(
        &scene,
        vec![turn(Some("first"), RuntimeTurnState::Working)],
        10,
    )
    .await;
    publish(&scene, vec![turn(None, RuntimeTurnState::Working)], 20).await;
    assert_eq!(
        scene.session.context.lock().active_turn_id.as_deref(),
        Some("first")
    );
    assert_eq!(
        scene.session.state.lock().active_turn_started_at_ms,
        Some(10)
    );
    publish(&scene, vec![turn(None, RuntimeTurnState::Idle)], 30).await;
    publish(
        &scene,
        vec![
            turn(None, RuntimeTurnState::Working),
            turn(Some("first"), RuntimeTurnState::Working),
        ],
        40,
    )
    .await;
    assert_idle(&scene);
}

#[tokio::test]
async fn terminal_turn_outcomes_settle_the_transcript_live_row() {
    for detail in [
        None,
        Some(RuntimeTurnDetail::Interrupted),
        Some(RuntimeTurnDetail::Failed),
    ] {
        let scene = fixtures::scene();
        publish(
            &scene,
            vec![
                turn(Some("first"), RuntimeTurnState::Working),
                command("running"),
            ],
            10,
        )
        .await;
        let mut completed = turn(Some("first"), RuntimeTurnState::Idle);
        if let AgentRuntimeEvent::TurnState {
            detail: outcome, ..
        } = &mut completed
        {
            *outcome = detail;
        }
        publish(&scene, vec![completed], 20).await;
        assert_idle(&scene);
        let row = scene.session.cache.lock().snapshot().pop().unwrap();
        assert_eq!(row.tool_status.as_deref(), Some("interrupted"));
        assert_eq!(row.tool_completed_at_ms, Some(20));
        let events = scene.fixture.ui.0.lock();
        let tab = scene.session.context.lock().tab_id.clone();
        let last = events
            .iter()
            .rev()
            .find(|(name, _)| name == &format!("tab-chat-msg-{tab}"))
            .unwrap();
        assert_eq!(last.1["messages"][0]["toolStatus"], "interrupted");
    }
}

#[tokio::test]
async fn replayed_running_items_cannot_reopen_a_completed_command_or_turn() {
    let scene = fixtures::scene();
    publish(
        &scene,
        vec![
            turn(Some("first"), RuntimeTurnState::Working),
            command("completed"),
        ],
        10,
    )
    .await;
    publish(
        &scene,
        vec![turn(Some("first"), RuntimeTurnState::Idle)],
        20,
    )
    .await;
    publish(
        &scene,
        vec![
            command("running"),
            turn(Some("first"), RuntimeTurnState::Working),
        ],
        30,
    )
    .await;
    assert_idle(&scene);
    let row = scene.session.cache.lock().snapshot().pop().unwrap();
    assert_eq!(row.tool_status.as_deref(), Some("completed"));
}
