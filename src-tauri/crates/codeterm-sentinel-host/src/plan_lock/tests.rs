use super::*;
use crate::store::{Owner, Store};
use codeterm_cron::clock::MockClock;
use codeterm_entitlements::{compiled_free, LimitGate};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::json;

#[test]
fn plan_lock_scheduler_holds_without_observing_and_resumes_without_missed_effects() {
    let temp = tempfile::tempdir().unwrap();
    let (mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let fixture = ServiceFixture::new(mux);
    let clock = MockClock::starting_at(1000);
    codeterm_storage::test_support::seed_empty_root(&temp.path().join("storage"));
    let storage =
        codeterm_storage::Storage::open(&temp.path().join("storage"), Default::default()).unwrap();
    let store = Store::with_storage(&storage, clock.clone());
    let job = store.prepare_document(codeterm_sentinel::test_support::watch(
        json!("plan-held-watch"),
        json!({"uses":"probe","with":{"command":{"lit":"opaque"}},"every":{"lit":"10s"}}),
        json!({"with":{"question":{"lit":"opaque"}},"buckets":["ready"],"matches":{"ready":{"lit":".*"}},"rearm_on_change":["ready"]}),
        json!({"ready":[{"id":"effect","uses":"notify","with":{}}]}), json!(null)),
        Owner { creator_tab_id: creator, workspace_id: workspace.clone(), team_id: None, cwd: temp.path().to_string_lossy().into_owned() }, false).unwrap();
    let handle = crate::scheduler::fixture_handle_without_app(store);
    let job = storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
    let in_flight = storage::commit(&handle.store, Store::stage_due)
        .unwrap()
        .pop()
        .unwrap();
    let mut free = compiled_free();
    free.limits.workspaces.max = Some(0);
    fixture
        .service
        .mux
        .write()
        .set_limit_gate(LimitGate::new(free, true));
    crate::scheduler::tick_with_probe(
        &handle,
        &std::sync::Arc::new(fixture.service.clone()),
        &in_flight,
        |_, _| panic!("a held graph must not observe"),
    );
    handle.store.lock().release(&job.id);
    reconcile(&fixture.service, &handle).unwrap();
    assert_eq!(
        held_reason(handle.store.lock().get(&job.id).unwrap()),
        Some("plan_limit_locked")
    );
    assert!(refuse(&fixture.service, &job)
        .unwrap_err()
        .starts_with("plan_limit_locked:"));
    clock.set_ms(901_000);
    assert!(storage::commit(&handle.store, Store::stage_due)
        .unwrap()
        .is_empty());
    assert_eq!(
        handle
            .store
            .lock()
            .get(&job.id)
            .unwrap()
            .progress
            .last_probe_at,
        None
    );
    fixture
        .service
        .mux
        .write()
        .set_limit_gate(LimitGate::dormant());
    reconcile(&fixture.service, &handle).unwrap();
    let effects = std::cell::Cell::new(0);
    for (at, text, expected) in [
        (901_000, "missed", 0),
        (911_000, "missed", 0),
        (921_000, "new change", 1),
    ] {
        clock.set_ms(at);
        let lease = storage::commit(&handle.store, Store::stage_due)
            .unwrap()
            .pop()
            .unwrap();
        let store = &handle.store;
        crate::scheduler::advance(
            store,
            &lease,
            &crate::probe::Output {
                success: true,
                bytes: text.as_bytes().to_vec(),
                text: text.into(),
                ..Default::default()
            },
            || {
                crate::provenance::Classification::regex(crate::machine::Verdict {
                    bucket: "ready".into(),
                    confidence: 1.0,
                })
            },
            |job, transition, handler| {
                storage::commit(store, |store| {
                    store.stage_claim_handler(job, transition, handler)
                })
            },
            |_, _, _, _| {
                effects.set(effects.get() + 1);
                Ok(crate::handlers::Status::Completed.into())
            },
            |_, _, _| Ok(()),
        );
        handle.store.lock().release(&job.id);
        assert_eq!(effects.get(), expected);
    }
    let loaded = storage
        .sentinels()
        .get(codeterm_storage::SentinelId::new(job.id.clone()).unwrap())
        .wait(std::time::Duration::from_secs(10))
        .unwrap()
        .value
        .unwrap();
    let restored = crate::storage::codec::decode(&loaded);
    assert!(restored.admission.permits_steps());
    assert!(!restored.admission.baseline_pending);
    assert!(restored.machine.last_hash.is_some());
    storage
        .shutdown()
        .wait(std::time::Duration::from_secs(10))
        .unwrap();
}

#[test]
fn plan_lock_owner_inherits_workspace_team_or_tab_lock() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::new(temp.path().join("store.json"), MockClock::starting_at(1000));
    let job = store
        .create_document(
            codeterm_sentinel::test_support::watch(
                json!("ownership"),
                json!({"uses":"probe","with":{"command":{"lit":"opaque"}},"every":{"lit":"10s"}}),
                json!({"with":{"question":{"lit":"opaque"}},"buckets":["ready"]}),
                json!(null),
                json!(null),
            ),
            Owner {
                creator_tab_id: "tab".into(),
                workspace_id: "workspace".into(),
                team_id: Some("team".into()),
                cwd: ".".into(),
            },
            false,
        )
        .unwrap();
    for kind in [LimitKey::Workspaces, LimitKey::Teams, LimitKey::AgentTabs] {
        let mut access = compiled_free();
        access.limits.workspaces.max = Some(0);
        access.limits.teams.max = Some(0);
        access.limits.agent_tabs.max = Some(0);
        let gate = LimitGate::new(access, true);
        let set = |id: &str| {
            gate.over_limit(
                kind,
                &[codeterm_entitlements::LockCandidate {
                    id: id.into(),
                    created_at: 1,
                    exempt: false,
                }],
            )
        };
        let mut locks = MuxLocks::default();
        match kind {
            LimitKey::Workspaces => {
                locks.workspaces = set("workspace");
            }
            LimitKey::Teams => {
                locks.teams = set("team");
            }
            LimitKey::AgentTabs => {
                locks.agent_tabs = set("tab");
            }
        }
        assert!(locked(&locks, &job));
    }
    assert!(!locked(&MuxLocks::default(), &job));
}
