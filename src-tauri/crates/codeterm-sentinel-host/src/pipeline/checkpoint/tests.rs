use super::*;
use crate::store::{Owner, Store};
use codeterm_cron::clock::MockClock;
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::json;

#[test]
fn plan_lock_checkpoint_resumes_next_step_with_the_same_run_without_replay() {
    let temp = tempfile::tempdir().unwrap();
    let (mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .mux
        .write()
        .set_limit_gate(codeterm_entitlements::LimitGate::dormant());
    let clock = MockClock::starting_at(1000);
    codeterm_storage::test_support::seed_empty_root(&temp.path().join("storage"));
    let storage =
        codeterm_storage::Storage::open(&temp.path().join("storage"), Default::default()).unwrap();
    let store = Store::with_storage(&storage, clock.clone());
    let job = store.prepare_document(json!({"schema_version":2,"kind":"pipeline","name":"checkpoint-proof","owner":"tab",
        "trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":"opaque"}}},
        "steps":[{"id":"first","uses":"filter","with":{"pattern":{"lit":".*"}}},{"id":"second","uses":"filter","with":{"pattern":{"lit":".*"}}}],"result":{"to":"notify"}}),
        Owner { creator_tab_id:creator, workspace_id:workspace, team_id:None, cwd:temp.path().to_string_lossy().into_owned() }, false).unwrap();
    let handle = crate::scheduler::fixture_handle_without_app(store);
    let job = storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
    let lease = storage::commit(&handle.store, Store::stage_due)
        .unwrap()
        .pop()
        .unwrap();
    let spec = job.pipeline().unwrap();
    let mut ctx = RunCtx::new(&handle, &fixture.service, &lease, "held-run");
    let first = execute_step(
        &mut ctx,
        &spec.steps[0],
        ExecutionEnvelope::from_text("opaque"),
    )
    .unwrap();
    let state =
        blueprint_pipeline::advance(&RunnerState::start(spec).unwrap(), first.outcome).unwrap();
    assert_eq!(state.current_step, Some(1));
    hold(&ctx, &state, &first.output, "opaque", None).unwrap();
    drop(ctx);
    super::super::finish_run(
        &handle,
        &lease,
        "held-run",
        &RunCompletion::held("plan_limit_locked".into()),
        false,
    );
    let persisted = storage
        .sentinels()
        .get(codeterm_storage::SentinelId::new(job.id.clone()).unwrap())
        .wait(Duration::from_secs(10))
        .unwrap()
        .value
        .unwrap();
    assert_eq!(persisted.record.runtime.generation, lease.job.revision);
    handle.store.lock().release(&job.id);
    let restored = crate::storage::codec::decode(&persisted);
    let checkpoint = load(&restored).unwrap().unwrap();
    assert_eq!(checkpoint.current_step, Some(1));
    assert_eq!(checkpoint.next_marker, 1);
    assert_eq!(checkpoint.run_id, "held-run");
    clock.set_ms(901_000);
    crate::plan_lock::reconcile(&fixture.service, &handle).unwrap();
    let resumed = storage::commit(&handle.store, Store::stage_due)
        .unwrap()
        .pop()
        .unwrap();
    super::super::tick(&handle, &fixture.service, &resumed);
    let rows = handle
        .store
        .lock()
        .typed_pipeline_log(&job.id, 100)
        .unwrap()
        .wait(Duration::from_secs(10))
        .unwrap()
        .value;
    assert_eq!(
        rows.iter()
            .filter(|row| row.step_name == "first" && row.state == JournalState::Running)
            .count(),
        1
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.step_name == "second" && row.state == JournalState::Running)
            .count(),
        1
    );
    assert!(rows.iter().all(|row| row.run_id == "held-run"));
    assert_eq!(
        codeterm_storage::BlueprintJournalEntry::derive_run(&rows.iter().collect::<Vec<_>>()).0,
        Some(JournalState::Done)
    );
    assert!(handle
        .store
        .lock()
        .get(&job.id)
        .unwrap()
        .admission
        .continuation
        .is_none());
    storage.shutdown().wait(Duration::from_secs(10)).unwrap();
}
