use serde_json::Value;
use tauri::Listener;

use super::{scheduler::Handle, source};
use codeterm_service_core::service::ServiceState;

const WAKE_EVENTS: [&str; 2] = ["prompt-state-changed", "tab-status-changed"];

pub(super) fn listen_any(
    svc: ServiceState,
    handle: Handle,
    app: &codeterm_host_runtime::app_runtime::AppHandle,
) {
    for event_name in WAKE_EVENTS {
        let handle = handle.clone();
        app.listen_any(event_name, move |event| {
            if let Some(tab_id) = event_tab_id(event.payload()) {
                handle.wake_tab(&tab_id);
            }
        });
    }
    let event_handle = handle.clone();
    let event_svc = svc.clone();
    app.listen_any("cron-fired", move |event| {
        if super::run_binding::queue_fired_event(&event_svc, &event_handle, event.payload()) {
            event_handle.notify();
        }
    });
    for event_name in ["cron-run-finished", "mux-state-changed"] {
        let handle = handle.clone();
        app.listen_any(event_name, move |_| handle.notify());
    }
}

fn event_tab_id(payload: &str) -> Option<String> {
    let value: Value = serde_json::from_str(payload).ok()?;
    value
        .get("tab_id")
        .and_then(Value::as_str)
        .filter(|tab_id| source::bounded_id(tab_id))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        source::TabBinding,
        store::{Create, Owner, Store},
    };
    use codeterm_service_core::cron::clock::{Clock, MockClock};
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    #[test]
    fn sentinel_observe_wake_events_cover_prompt_and_tab_status() {
        assert_eq!(WAKE_EVENTS, ["prompt-state-changed", "tab-status-changed"]);
    }

    fn observing_store() -> (tempfile::TempDir, Store, Arc<MockClock>, String) {
        let dir = tempfile::tempdir().unwrap();
        let clock = codeterm_service_core::cron::clock::MockClock::starting_at(1_000);
        let mut store = Store::new(dir.path().join("sentinels.json"), clock.clone());
        let job = store
            .create(
                Create {
                    terminal: None,
                    session: false,
                    name: Some("agent-state-watch".into()),
                    probe: None,
                    observe: Some(TabBinding::Tab("bound-agent".into())),
                    input: None,
                    question: "state?".into(),
                    buckets: vec!["waiting".into()],
                    every: "20s".into(),
                    routes: BTreeMap::new(),
                    model_classification: false,
                    matches: BTreeMap::from([("waiting".into(), ".*".into())]),
                    rearm_on_change: BTreeSet::new(),
                    until: None,
                    timeout: None,
                    paused: false,
                    handlers: None,
                    escalation_timeout_ms: None,
                    max_transitions: None,
                    max_actions: None,
                },
                Owner {
                    creator_tab_id: "creator".into(),
                    team_id: Some("team".into()),
                    workspace_id: "workspace".into(),
                    cwd: ".".into(),
                },
                false,
            )
            .unwrap();
        (dir, store, clock, job.id)
    }

    #[test]
    fn sentinel_observe_event_wakes_only_the_bound_tab_without_clock_advance() {
        assert_eq!(
            event_tab_id(r#"{"tab_id":"bound-agent"}"#),
            Some("bound-agent".into())
        );
        assert_eq!(event_tab_id(r#"{"tab":"bound-agent"}"#), None);
        let (_dir, mut store, clock, id) = observing_store();
        let first = store.due().unwrap().pop().unwrap();
        store.release(&id);
        assert!(store.due().unwrap().is_empty());
        assert!(!store.wake_tab("unrelated-agent"));
        assert!(store.due().unwrap().is_empty());
        assert!(store.wake_tab("bound-agent"));
        assert_eq!(store.next_deadline(), 0);
        assert_eq!(clock.now_ms(), 1_000);
        assert_eq!(store.due().unwrap().len(), 1);
        drop(first);
    }

    #[test]
    fn sentinel_observe_wake_during_lease_runs_once_after_release() {
        let (_dir, mut store, clock, id) = observing_store();
        let first = store.due().unwrap().pop().unwrap();
        assert!(store.wake_tab("bound-agent"));
        assert!(!store.wake_tab("bound-agent"));
        assert!(store.due().unwrap().is_empty());
        assert!(store.wake_pending.contains(&id));
        store.release(&id);
        let after_release = store.due().unwrap();
        assert_eq!(after_release.len(), 1);
        store.release(&id);
        assert!(store.due().unwrap().is_empty());
        assert_eq!(clock.now_ms(), 1_000);
        drop(first);
    }

    #[test]
    fn sentinel_observe_pending_wakes_are_unique_ids() {
        let (_dir, mut store, _clock, _id) = observing_store();
        let initial = store.due().unwrap().pop().unwrap();
        store.release(&initial.job.id);
        store.wake_tab("bound-agent");
        store.wake_tab("bound-agent");
        assert_eq!(
            store.wake_pending,
            BTreeSet::from([store.list()[0].id.clone()])
        );
    }
}
