use super::*;
use crate::agent::registry::AgentRegistry;
use crate::api::events::{EventBroadcaster, SystemEvent};
use crate::lock_hold::{TimedMutex, TimedRwLock};
use crate::multiplexer::state::MultiplexerState;
use crate::plugin::agent_claude::ClaudeProvider;
use codeterm_types::pty::{BootStage, TabStatusTracker, TabVerdict, TimelineState};

#[test]
fn transition_decision_emits_initial_and_changed_statuses_only() {
    for status in [
        TabStatus::Idle,
        TabStatus::Working,
        TabStatus::Waiting,
        TabStatus::Error,
        TabStatus::Dead,
    ] {
        assert_eq!(status_transition(None, status), Some(status));
        assert_eq!(status_transition(Some(status), status), None);
    }
    assert_eq!(
        status_transition(Some(TabStatus::Working), TabStatus::Idle),
        Some(TabStatus::Idle)
    );
    assert_eq!(
        status_transition(Some(TabStatus::Idle), TabStatus::Working),
        Some(TabStatus::Working)
    );
}

#[test]
fn reconciled_completion_is_observable_once_on_shared_event_channel() {
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("completion-pty".into());
    tab.agent_type = Some("claude".into());
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_session_id = Some("own-session".into());
    tab.agent_state = Some(
        TabVerdict {
            state: TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    let mux = Arc::new(TimedRwLock::new("completion-mux", state));
    let mut raw = TabStatusTracker::new();
    raw.set_working("completion-pty");
    let tracker = Arc::new(TimedMutex::new("completion-tracker", raw));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(ClaudeProvider::new()));
    let events = EventBroadcaster::new();
    let mut receiver = events.subscribe();
    let mut published = HashMap::new();
    let mut emit_snapshot = || {
        let statuses = super::super::status_reconciliation::try_reconciled_tab_statuses(
            &mux, &tracker, &registry,
        )
        .unwrap();
        publish_statuses(&mut published, &statuses, |name, payload| {
            events.publish_generic(name, payload)
        });
    };
    emit_snapshot();
    assert_status(&mut receiver, &tab_id, "Working");
    assert!(crate::agent::state_verdict::apply(
        &mux,
        &tab_id,
        "own-session",
        crate::agent::provider::ProviderStateVerdict::Idle
    ));
    assert_eq!(
        tracker.lock().get_status("completion-pty"),
        TabStatus::Working
    );
    emit_snapshot();
    assert_status(&mut receiver, &tab_id, "Idle");
    emit_snapshot();
    assert!(receiver.try_recv().is_err());
}

fn assert_status(
    receiver: &mut tokio::sync::broadcast::Receiver<crate::api::events::SeqEvent>,
    tab_id: &str,
    status: &str,
) {
    match receiver
        .try_recv()
        .expect("status event must be published")
        .event
    {
        SystemEvent::Generic { event_type, data } => {
            assert_eq!(event_type, "tab-status-changed");
            assert_eq!(
                data,
                serde_json::json!({"tab_id": tab_id, "status": status})
            );
        }
        event => panic!("unexpected event: {event:?}"),
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn service_completion_publishes_idle_without_duplicate_events() {
    use tauri::Manager;
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().pty_id = Some("service-completion-pty".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    fixture._app.manage(service.events.clone());
    let mut receiver = service.events.subscribe();
    assert!(service.mark_agent_turn_started(&tab_id));
    assert_status(&mut receiver, &tab_id, "Working");
    drain_mux_events(&mut receiver);
    assert!(service.mark_agent_turn_complete(&tab_id));
    assert_status(&mut receiver, &tab_id, "Idle");
    assert_eq!(service.get_tab_status(&tab_id).unwrap().status, "idle");
    assert!(service.mark_agent_turn_complete(&tab_id));
    drain_mux_events(&mut receiver);
}

fn drain_mux_events(receiver: &mut tokio::sync::broadcast::Receiver<crate::api::events::SeqEvent>) {
    while let Ok(event) = receiver.try_recv() {
        assert!(matches!(event.event, SystemEvent::MuxStateChanged));
    }
}
