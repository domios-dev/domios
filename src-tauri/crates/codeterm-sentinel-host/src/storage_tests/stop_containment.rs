//! The live QA sequence: apply, re-apply changed, delete, recreate, run a long command, stop it.

use super::*;
use crate::amend::PreparedUpdate;
use codeterm_blueprint_types::blueprint_journal::{JournalMarker, JournalReason, JournalState};
use codeterm_storage::{BlueprintJournalWrite, Fault, SentinelId};

fn document(name: &str, command: &str) -> Value {
    json!({"schema_version":2,"kind":"pipeline","name":name,"owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"wait","run":{"lit":command}}],"result":{"to":"owner"}})
}

fn apply(store: &Mutex<Store>, name: &str, command: &str) -> Sentinel {
    let mut candidate = store
        .lock()
        .prepare_document(document(name, command), owner(Some("team")), false)
        .expect("admitted definition");
    let current = store
        .lock()
        .list()
        .iter()
        .find(|job| job.name.as_deref() == Some(name))
        .cloned();
    let Some(current) = current else {
        return commit(store, |s| s.stage_insert(candidate)).expect("created");
    };
    candidate.id = current.id.clone();
    let version = current.definition_version;
    commit(store, |s| {
        s.stage_apply_definition(
            version,
            "creator",
            1000,
            PreparedUpdate {
                job: candidate,
                cause_transition_id: None,
                note: None,
            },
            None,
        )
    })
    .expect("applied changed definition")
}

fn journal(storage: &Storage, job: &Sentinel, write: BlueprintJournalWrite) {
    done(
        storage
            .sentinels()
            .write_blueprint_journal(SentinelId::new(job.id.clone()).unwrap(), write),
    );
}

fn write(
    store: &Mutex<Store>,
    job: &Sentinel,
    write: BlueprintJournalWrite,
) -> Result<bool, String> {
    commit(store, |s| s.stage_pipeline_journal(&job.id, write))
}

#[test]
fn blueprint_stop_after_delete_recreate_never_takes_the_scheduler_down() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let bystander = create(&store, Some("team"), false, json!({}));

    apply(&store, "qa-long-run", "sleep 60");
    apply(&store, "qa-long-run", "sleep 61");
    let third = apply(&store, "qa-long-run", "sleep 62");
    assert_eq!(
        third.definition_version, 3,
        "a third apply keeps a lossless head lineage"
    );
    commit(&store, |s| {
        s.stage_delete_scenario(&third.id, "workspace", Some("team"))
    })
    .expect("deleted");
    assert!(store.lock().get(&third.id).is_none());
    let job = apply(&store, "qa-long-run", "sleep 600");
    assert_ne!(job.id, third.id);

    journal(
        &storage,
        &job,
        BlueprintJournalWrite::Claim {
            definition_version: None,
            parent: None,
            run_id: "run_long".into(),
            marker: JournalMarker::Step(0),
            step_name: "wait".into(),
            generation: job.revision,
            started_at_ms: 1100,
        },
    );
    let stopped = commit(&store, |s| s.stage_stop(&job.id, "stopped_by_host")).expect("stopped");
    assert!(stopped.machine.stopped);

    let cancelled = JournalReason::Other("skip:lease_cancelled".into());
    assert_eq!(
        write(
            &store,
            &job,
            BlueprintJournalWrite::Finish {
                evidence: None,
                run_id: "run_long".into(),
                marker: JournalMarker::Step(0),
                state: JournalState::Skipped,
                outcome: "failed".into(),
                ended_at_ms: 1200,
                result_summary: "node stopped: skip:lease_cancelled".into(),
                body_ref: None,
                reason: Some(cancelled.clone()),
            },
        ),
        Ok(true),
        "a cancelled step resolves durably"
    );
    write(
        &store,
        &job,
        BlueprintJournalWrite::CloseRun {
            run_id: "run_long".into(),
            state: JournalState::Skipped,
            outcome: "skipped".into(),
            at_ms: 1200,
            reason: Some(cancelled),
        },
    )
    .expect("run closed");

    store
        .lock()
        .ensure_available()
        .expect("scheduler stays available");
    assert!(commit(&store, Store::stage_due)
        .expect("other blueprints keep scheduling")
        .iter()
        .any(|lease| lease.job.id == bystander.id));
    let entries = done(
        storage
            .sentinels()
            .blueprint_log(SentinelId::new(job.id.clone()).unwrap(), 20),
    )
    .value;
    assert!(entries
        .iter()
        .any(|entry| entry.step_index == 0 && entry.state == JournalState::Skipped));
    close(storage);
}

#[test]
fn blueprint_invalid_journal_write_is_refused_without_closing_the_store() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let job = apply(&store, "qa-refused", "sleep 1");
    let error = write(
        &store,
        &job,
        BlueprintJournalWrite::Finish {
            evidence: None,
            run_id: "run_refused".into(),
            marker: JournalMarker::Step(0),
            state: JournalState::Done,
            outcome: "done".into(),
            ended_at_ms: 1200,
            result_summary: "x".repeat(201),
            body_ref: None,
            reason: None,
        },
    )
    .expect_err("an oversized summary is refused");
    assert!(error.starts_with("blueprint_write_refused:"), "{error}");
    store
        .lock()
        .ensure_available()
        .expect("one refused write stays contained");
    close(storage);
}

#[test]
fn blueprint_runtime_hold_survives_reload_and_sibling_reapply() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let mut held = apply(&store, "http-health", "true");
    held.machine.held = Some(JournalReason::UncertainResult.as_str().into());
    assert_eq!(commit(&store, |s| s.stage_finish(&held)), Ok(true));

    apply(&store, "qa-long-run", "sleep 60");
    apply(&store, "qa-long-run", "sleep 61");
    let reloaded = Store::with_storage(&storage, MockClock::starting_at(1000));
    reloaded.ensure_available().expect("a runtime hold reloads");
    assert_eq!(
        reloaded.get(&held.id).unwrap().machine.held.as_deref(),
        Some("uncertain_pipeline_result")
    );
    close(storage);
}

#[test]
fn blueprint_persistence_failure_recovers_from_committed_state_without_restart() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let job = apply(&store, "qa-recover", "sleep 1");
    done(storage.inject_fault(Fault::FailNextCommit));
    let error = commit(&store, |s| s.stage_stop(&job.id, "stopped_by_host"))
        .expect_err("the injected commit fails");
    assert!(
        error.starts_with("blueprint_store_persistence_failed:"),
        "{error}"
    );
    assert!(store.lock().ensure_available().is_err());

    assert_eq!(crate::storage::reconcile(&store), Ok(true));
    store
        .lock()
        .ensure_available()
        .expect("recovered admission");
    assert!(
        !store.lock().get(&job.id).unwrap().machine.stopped,
        "the cache matches committed state, not the failed intent"
    );
    commit(&store, |s| s.stage_stop(&job.id, "stopped_by_host")).expect("stop after recovery");
    close(storage);
}
