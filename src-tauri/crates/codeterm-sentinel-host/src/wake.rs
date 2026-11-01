use serde_json::Value;

use super::{scheduler::Handle, source};
use codeterm_service_core::service::ServiceState;

const WAKE_EVENTS: [&str; 2] = ["prompt-state-changed", "tab-status-changed"];

pub fn subscribe(svc: &ServiceState, handle: Handle) -> crate::event_subscription::Subscription {
    let owner = std::sync::Arc::downgrade(svc);
    let failed_handle = handle.clone();
    crate::event_subscription::subscribe(
        svc.events.clone(),
        move |event| {
            let Some(svc) = owner.upgrade() else {
                return;
            };
            let name = event.frame.event_type.as_str();
            let payload = crate::event_subscription::payload(event);
            if WAKE_EVENTS.contains(&name) {
                if let Some(tab_id) = event_tab_id(&payload.to_string()) {
                    handle.wake_tab(&tab_id);
                }
            } else if name == "cron-fired" {
                if super::run_binding::queue_fired_event(&svc, &handle, &payload.to_string()) {
                    handle.notify();
                }
            } else if matches!(
                name,
                "cron-run-finished" | "mux-state-changed" | "entitlement-changed"
            ) {
                handle.notify();
            }
            crate::service_adapter::graph::invalidate(name, &handle.store, svc.ui.as_ref());
        },
        move |error| {
            log::error!("[blueprint] {error}");
            failed_handle.store.lock().close_admission();
            failed_handle.probe_owner.close_admission();
            failed_handle.notify();
        },
    )
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
    use crate::store::{Owner, Store};
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
            .create_document(codeterm_sentinel::test_support::watch(serde_json::to_value::<Option<String>>(Some("agent-state-watch".into())).unwrap(),serde_json::json!({"uses":"observe","with":serde_json::json!({"kind":{"lit":"tab"},"id":{"lit":serde_json::to_value::<String>("bound-agent".into()).unwrap()}}),"every":serde_json::json!({"lit":serde_json::to_value::<String>("20s".into()).unwrap()}),"paused":false,"session":false}),serde_json::json!({"with":serde_json::json!({"question":{"lit":serde_json::to_value::<String>("state?".into()).unwrap()}}),"buckets":serde_json::to_value::<Vec<String>>(vec!["waiting".into()]).unwrap(),"matches":codeterm_sentinel::test_support::literal_params(serde_json::to_value::<std::collections::BTreeMap<String,String>>(BTreeMap::from([("waiting".into(), ".*".into())])).unwrap()),"until":serde_json::to_value::<Option<String>>(None).unwrap(),"rearm_on_change":serde_json::to_value::<std::collections::BTreeSet<String>>(BTreeSet::new()).unwrap(),"escalation_timeout_ms":serde_json::to_value::<Option<u64>>(None).unwrap(),"model_classification":false}),serde_json::Value::Null,serde_json::to_value::<Option<String>>(None).unwrap()),
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

    #[tokio::test]
    async fn process_split_contract_service_events_wake_exact_tab_and_scheduler() {
        let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
        let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
        let svc = Arc::new(fixture.service.clone());
        let (_dir, store, clock, id) = observing_store();
        let handle = crate::scheduler::fixture_handle_with_ui(store, svc.ui.clone());
        handle.store.lock().due().unwrap();
        handle.store.lock().release(&id);
        let subscription = subscribe(&svc, handle.clone());
        for event in WAKE_EVENTS {
            for (tab, expected) in [("foreign-agent", false), ("bound-agent", true)] {
                svc.ui.emit(event, serde_json::json!({"tab_id":tab}));
                let seq = svc.events.current_seq() - 1;
                tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    subscription.acknowledged(seq),
                )
                .await
                .unwrap()
                .unwrap();
                assert_eq!(
                    handle.store.lock().due().unwrap().len(),
                    usize::from(expected)
                );
                handle.store.lock().release(&id);
            }
        }
        for event in ["cron-run-finished", "mux-state-changed"] {
            let before = handle.wake_generation();
            svc.ui.emit(event, serde_json::Value::Null);
            let seq = svc.events.current_seq() - 1;
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                subscription.acknowledged(seq),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(handle.wake_generation(), before + 1);
        }
        assert_eq!(clock.now_ms(), 1_000);
    }

    #[tokio::test]
    async fn process_split_contract_real_scheduler_installs_service_subscription() {
        let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
        let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
        let svc = Arc::new(fixture.service.clone());
        codeterm_storage::test_support::seed_empty_root(&fixture.temp.path().join("automation"));
        let storage = codeterm_storage::Storage::open(
            fixture.temp.path().join("automation"),
            Default::default(),
        )
        .unwrap();
        let ports = crate::scheduler::start(
            svc.clone(),
            MockClock::starting_at(1_000),
            &storage,
            fixture.temp.path(),
        )
        .unwrap();
        assert!(Arc::ptr_eq(
            &ports.wake,
            &svc.ext::<codeterm_service_core::ports::sentinel::SentinelPorts>()
                .wake
        ));
        let subscription = svc
            .host_extension::<crate::event_subscription::Subscription>()
            .unwrap();
        svc.events
            .publish(codeterm_events::SystemEvent::MuxStateChanged);
        let seq = svc.events.current_seq() - 1;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            subscription.acknowledged(seq),
        )
        .await
        .unwrap()
        .unwrap();
        let handle = svc.host_extension::<Handle>().unwrap();
        assert!(handle.wake_generation() > 0);
        handle.probe_owner.close_admission();
        handle.notify();
        storage
            .shutdown()
            .wait(std::time::Duration::from_secs(2))
            .unwrap();
    }

    #[tokio::test]
    async fn process_split_contract_cron_events_bind_only_the_verified_run() {
        use codeterm_service_core::cron::store::{CronRunInfo, CronRunStatus, CronStore};
        let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
        let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
        let svc = Arc::new(fixture.service.clone());
        let mut cron = CronStore::new_with_path(fixture.temp.path().join("cron.json"));
        let cron_id = cron
            .create(
                serde_json::from_value(serde_json::json!({
                    "name":"fixture", "workspace_id":"workspace", "team_id":"team",
                    "schedule":"0 * * * *", "task":"opaque-fixture", "enabled":false
                }))
                .unwrap(),
            )
            .unwrap()
            .id;
        cron.record_run(
            &cron_id,
            CronRunInfo {
                cancel_reason: None,
                run_id: "own-run".into(),
                started_at: 1_000,
                finished_at: None,
                status: CronRunStatus::Running,
                tab_id: Some("own-tab".into()),
                session_id: None,
                cwd: None,
                summary: None,
                report_delivery: None,
            },
        )
        .unwrap();
        let cron: codeterm_service_core::CronState = Arc::new(parking_lot::Mutex::new(cron));
        assert!(svc.set_ext(cron).is_ok());
        let mut store = Store::new(
            fixture.temp.path().join("watchers.json"),
            MockClock::starting_at(1_000),
        );
        let params = serde_json::json!({"schema_version":2,"kind":"watch","name":"cron-run-watch","steps":[{"id":"source","uses":"observe","with":{"kind":{"lit":"cron_run"},"id":{"lit":cron_id}},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"state"}},"buckets":["waiting"],"matches":{"waiting":{"lit":".*"}},"routes":{"waiting":"notice"}},{"id":"notice","uses":"notify"}]});
        let own = store
            .create_document(
                params.clone(),
                Owner {
                    creator_tab_id: "creator".into(),
                    team_id: Some("team".into()),
                    workspace_id: "workspace".into(),
                    cwd: ".".into(),
                },
                false,
            )
            .unwrap();
        store
            .create_document(
                params,
                Owner {
                    creator_tab_id: "foreign".into(),
                    team_id: Some("other-team".into()),
                    workspace_id: "workspace".into(),
                    cwd: ".".into(),
                },
                false,
            )
            .unwrap();
        let handle = crate::scheduler::fixture_handle_with_ui(store, svc.ui.clone());
        let subscription = subscribe(&svc, handle.clone());
        for (tab, expected) in [("forged-tab", 0), ("own-tab", 1)] {
            codeterm_service_core::service::cron::emit_cron_fired(&svc, &cron_id, "own-run", tab);
            assert_eq!(
                fixture
                    .ui
                    .0
                    .lock()
                    .iter()
                    .filter(|(name, payload)| { name == "cron-fired" && payload["tab_id"] == tab })
                    .count(),
                1
            );
            let seq = svc.events.current_seq() - 1;
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                subscription.acknowledged(seq),
            )
            .await
            .unwrap()
            .unwrap();
            let bindings = handle.take_run_bindings();
            assert_eq!(bindings.len(), expected);
            if let Some((id, binding)) = bindings.first() {
                assert_eq!(id, &own.id);
                assert_eq!(binding.run_id, "own-run");
                assert_eq!(binding.tab_id, "own-tab");
            }
        }
    }
}
