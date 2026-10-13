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
        publish_statuses(
            &mut published,
            &statuses,
            |name, payload| events.publish_generic(name, payload),
            |_| {},
        );
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

fn rpc_provider() -> crate::plugin::js_runtime::JsProvider {
    use crate::plugin::js_runtime::{PluginManifest, PluginVm};
    let manifest_json = include_str!("../../../../plugins/provider-codex/plugin.json");
    let manifest: PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = PluginVm::from_source_with(
        &manifest.id,
        "module.exports.default = {};".into(),
        Some(manifest_json.into()),
    )
    .unwrap();
    crate::plugin::js_runtime::JsProvider::new(manifest, Arc::new(vm))
}

fn rpc_tab() -> (MultiplexerState, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("rpc-status-pty".into());
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some("own-rpc-thread".into());
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.structured_control_active = true;
    tab.agent_state = Some(crate::agent::state_verdict::live_state_for(
        crate::agent::provider::ProviderStateVerdict::Idle,
    ));
    (mux, tab_id)
}

#[test]
fn rpc_declared_turns_publish_once_and_reconcile_pty_idle() {
    let (state, tab_id) = rpc_tab();
    let mux = Arc::new(TimedRwLock::new("rpc-status-mux", state));
    let mut raw = TabStatusTracker::new();
    raw.set_idle("rpc-status-pty");
    let tracker = Arc::new(TimedMutex::new("rpc-status-tracker", raw));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(rpc_provider()));
    let events = EventBroadcaster::new();
    let mut receiver = events.subscribe();
    let mut published = HashMap::from([(tab_id.clone(), TabStatus::Idle)]);
    for (event, expected) in [
        ("turn/started", TabStatus::Working),
        ("turn/completed", TabStatus::Idle),
    ] {
        let verdict = registry
            .with_provider("codex", |provider| {
                crate::agent::provider::declared_state_verdict(&provider.hook_event_roster(), event)
            })
            .flatten()
            .unwrap();
        assert!(crate::agent::state_verdict::apply(
            &mux,
            &tab_id,
            "own-rpc-thread",
            verdict
        ));
        for _ in 0..2 {
            let statuses = super::super::status_reconciliation::try_reconciled_tab_statuses(
                &mux, &tracker, &registry,
            )
            .unwrap();
            assert_eq!(statuses.get(&tab_id), Some(&expected));
            let snapshot = super::super::status_reconciliation::snapshot_with_reconciled_status(
                &mux, &tracker, &registry,
            );
            assert_eq!(snapshot.tab_statuses.get(&tab_id), Some(&expected));
            publish_statuses(
                &mut published,
                &statuses,
                |name, payload| events.publish_generic(name, payload),
                |_| {},
            );
        }
        assert_status(&mut receiver, &tab_id, &format!("{expected:?}"));
        assert!(receiver.try_recv().is_err());
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn rpc_turn_status_owner_updates_api_and_publishes_once() {
    use tauri::Manager;
    let (mux, tab_id) = rpc_tab();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    fixture._app.manage(service.events.clone());
    service
        .agent_registry
        .register_provider(Box::new(rpc_provider()));
    service.agent_tracker.lock().set_idle("rpc-status-pty");
    service.publish_authoritative_tab_statuses();
    let mut receiver = service.events.subscribe();
    for (event, expected) in [("turn/started", "Working"), ("turn/completed", "Idle")] {
        super::super::codex_rpc::status::observe_event(service, &tab_id, "own-rpc-thread", event);
        let mut statuses = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            if let SystemEvent::Generic { event_type, data } = event.event {
                if event_type == "tab-status-changed" {
                    statuses.push(data);
                }
            }
        }
        assert_eq!(
            statuses,
            [serde_json::json!({"tab_id":tab_id,"status":expected})]
        );
        assert_eq!(
            service.get_tab_status(&tab_id).unwrap().status,
            expected.to_lowercase()
        );
        if event == "turn/started" {
            service.agent_tracker.lock().set_idle("rpc-status-pty");
            service.publish_authoritative_tab_statuses();
            assert_eq!(service.get_tab_status(&tab_id).unwrap().status, "working");
            assert!(receiver.try_recv().is_err());
        }
    }
}

#[test]
fn outbox_wake_follows_observable_idle_edges_only() {
    let tab = "status-tab".to_string();
    let mut published = HashMap::new();
    let mut wakes = Vec::new();
    for status in [
        TabStatus::Working,
        TabStatus::Working,
        TabStatus::Idle,
        TabStatus::Idle,
        TabStatus::Waiting,
        TabStatus::Idle,
    ] {
        publish_statuses(
            &mut published,
            &HashMap::from([(tab.clone(), status)]),
            |_, _| {},
            |tab_id| wakes.push(tab_id.to_string()),
        );
    }
    assert_eq!(wakes, [tab.clone(), tab]);
}
