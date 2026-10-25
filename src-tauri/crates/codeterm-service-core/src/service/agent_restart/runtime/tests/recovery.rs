use super::*;
use crate::service::agent_runtime::model::RuntimeFailure;
use std::time::Duration;

fn wait_requests(fixture: &RuntimeFixture, expected: usize) {
    crate::async_runtime::block_on(async {
        let mut events = fixture.request_events.clone();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                events.borrow_and_update();
                if fixture.requests().len() >= expected {
                    return;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    });
}

fn wait_loaded(fixture: &RuntimeFixture, old_epoch: u64) {
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    crate::async_runtime::block_on(async {
        let mut changes = session.control_changes.subscribe();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                changes.borrow_and_update();
                if session.context.lock().epoch > old_epoch
                    && session
                        .thread_loaded
                        .load(std::sync::atomic::Ordering::Acquire)
                    && fixture
                        .service
                        .mux
                        .read()
                        .get_tab(&fixture.tab)
                        .unwrap()
                        .agent_state
                        .as_ref()
                        .is_some_and(|state| {
                            state.state == codeterm_types::pty::TimelineState::Idle
                        })
                {
                    return;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    });
}

#[test]
fn crashed_worker_resumes_own_thread_once_and_replacement_crash_stops() {
    let fixture = RuntimeFixture::new();
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let original = session.context.lock().clone();
    agent_runtime::transport::send(
        &fixture.service,
        &fixture.tab,
        &crate::service::messenger::AgentInput::Human(
            crate::service::messenger::HumanInput::composer("opaque interrupted input"),
        ),
        None,
    )
    .unwrap();
    assert!(session.context.lock().active_turn_id.is_some());
    let count = fixture.requests().len();
    let (notified, notification) = std::sync::mpsc::channel();
    fixture.ui.listen("notification-created", move |_| {
        let _ = notified.send(());
    });
    fixture.exits.lock().pop().unwrap().send(()).unwrap();
    wait_requests(&fixture, count + 5);
    let requests = fixture.requests();
    assert_eq!(
        requests[count..]
            .iter()
            .map(|r| r["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "initialize",
            "resumeThread",
            "readHistory",
            "control",
            "submitTurn"
        ]
    );
    assert_eq!(requests[count + 1]["params"]["threadId"], "own-thread");
    assert_eq!(requests[count + 4]["params"]["threadId"], "own-thread");
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["params"]["input"]["text"] == "opaque interrupted input")
            .count(),
        1
    );
    assert_ne!(
        requests[count - 1]["params"]["sendId"],
        requests[count + 4]["params"]["sendId"]
    );
    assert_ne!(session.context.lock().binding_nonce, original.binding_nonce);
    assert_eq!(session.context.lock().thread_id, original.thread_id);
    let records = fixture.service.lifecycle_journal.load().records;
    assert!(records.iter().any(|record| matches!(record,
        crate::session::lifecycle::JournalRecord::Transition { entry }
        if matches!(&entry.transition, crate::session::lifecycle::LifecycleTransition::RuntimeExited { diagnostics }
            if diagnostics.contains("PipeEof") && diagnostics.contains("after_compaction=false") && diagnostics.contains("requested: true")))));
    session
        .process
        .lock()
        .as_ref()
        .unwrap()
        .client
        .close(RuntimeFailure::ProcessExited {
            code: Some(1),
            signal: None,
            stderr: "fixture repeated crash".into(),
            requested: false,
        });
    notification.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(fixture.requests().len(), count + 5);
    let state = session.state.lock().snapshot();
    assert_eq!(serde_json::to_value(state).unwrap()["state"], "degraded");
    assert!(fixture.service.lifecycle_journal.load().records.iter().any(|record| matches!(record,
        crate::session::lifecycle::JournalRecord::Transition { entry }
        if matches!(&entry.transition, crate::session::lifecycle::LifecycleTransition::RuntimeExited { diagnostics }
            if diagnostics.contains("fixture repeated crash") && diagnostics.contains("code: Some(1)")))));
    assert_eq!(
        fixture
            .service
            .notif
            .lock()
            .list(false)
            .iter()
            .filter(|n| n.title == "Chat backend unavailable")
            .count(),
        1
    );
}

#[test]
fn idle_runtime_recovers_without_a_continuation_for_every_role() {
    use crate::multiplexer::types::AgentRole;
    use codeterm_types::agent::role_kind::builtins;
    for (role, profile) in [
        (None, None),
        (Some(AgentRole::Manager), Some(builtins::GENERAL_AGENT)),
        (Some(AgentRole::Worker), Some(builtins::WORKER)),
        (Some(AgentRole::Manager), Some(builtins::ORCHESTRATOR)),
        (Some(AgentRole::Manager), Some(builtins::PLANNER)),
    ] {
        let fixture = RuntimeFixture::new();
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&fixture.tab)
            .unwrap()
            .role = role;
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&fixture.tab)
            .unwrap()
            .role_profile = profile.map(str::to_string);
        let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        let old_epoch = session.context.lock().epoch;
        let count = fixture.requests().len();
        session
            .process
            .lock()
            .as_ref()
            .unwrap()
            .client
            .close(RuntimeFailure::PipeEof);
        wait_requests(&fixture, count + 3);
        wait_loaded(&fixture, old_epoch);
        assert_eq!(
            fixture.requests()[count + 1]["params"]["threadId"],
            "own-thread"
        );
        assert!(fixture.requests()[count..]
            .iter()
            .all(|r| r["method"] != "submitTurn"));
    }
}
