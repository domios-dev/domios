use super::*;
use codeterm_blueprint_types::blueprint_journal::{JournalMarker, JournalState};
use codeterm_storage::{BlueprintJournalWrite, SentinelId};

fn create_pipeline(store: &Mutex<Store>, owner: Owner) -> Sentinel {
    create_pipeline_with_steps(store, owner, 2)
}

fn create_pipeline_with_steps(store: &Mutex<Store>, owner: Owner, count: usize) -> Sentinel {
    let steps = if count == 2 {
        json!([
            {"id":"emit","run":{"lit":"printf ready"}},
            {"id":"match","uses":"filter","with":{"pattern":{"lit":"^ready$"},"bucket":{"lit":"matched"}}}
        ])
    } else {
        serde_json::Value::Array((0..count).map(|index|json!({"id":format!("step-{index}"),"uses":"filter","with":{"pattern":{"lit":"ready"}}})).collect())
    };
    let document = json!({"schema_version":2,"kind":"pipeline","name":"journal pipeline","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":steps,"result":{"to":"owner"}});
    let job = store
        .lock()
        .prepare_document(document, owner, false)
        .expect("pipeline definition");
    commit(store, |current| current.stage_insert(job)).expect("pipeline creation")
}

fn journal_id(job: &Sentinel) -> SentinelId {
    SentinelId::new(job.id.clone()).expect("journal sentinel id")
}

#[test]
fn journal_evidence_round_trip_large_marker_size_refusal_and_legacy_rows() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(None));
    done(storage.sentinels().write_blueprint_journal(
        journal_id(&job),
        BlueprintJournalWrite::Claim {
            definition_version: Some(1),
            parent: None,
            run_id: "evidence".into(),
            marker: JournalMarker::Step(70000),
            step_name: "emit".into(),
            generation: 0,
            started_at_ms: 1000,
        },
    ));
    let evidence = json!({"ports":{"inputs":{"in":{"preview":"7","type":"number","bytes":1,"truncated":false,"redacted":false}},"outputs":{}}}).as_object().unwrap().clone();
    let write = |evidence| BlueprintJournalWrite::Finish {
        evidence: Some(evidence),
        run_id: "evidence".into(),
        marker: JournalMarker::Step(70000),
        state: JournalState::Done,
        outcome: "done".into(),
        ended_at_ms: 1100,
        result_summary: "done".into(),
        body_ref: None,
        reason: None,
    };
    let oversized = json!({"padding":"λ".repeat(16384)})
        .as_object()
        .unwrap()
        .clone();
    assert!(storage
        .sentinels()
        .write_blueprint_journal(journal_id(&job), write(oversized))
        .wait(Duration::from_secs(10))
        .is_err());
    done(
        storage
            .sentinels()
            .write_blueprint_journal(journal_id(&job), write(evidence.clone())),
    );
    let mut followup = decision("evidence", 1200, "unchanged".into());
    if let BlueprintJournalWrite::Decision { marker, .. } = &mut followup {
        *marker = JournalMarker::Step(70000);
    }
    done(
        storage
            .sentinels()
            .write_blueprint_journal(journal_id(&job), followup),
    );
    close(storage);
    let storage = open(root.path());
    let entries = done(storage.sentinels().blueprint_log(journal_id(&job), 1)).value;
    assert_eq!(entries.len(), 3);
    for entry in &entries[..2] {
        assert_eq!(entry.marker(), JournalMarker::Step(70000));
        assert_eq!(entry.evidence, evidence);
        assert_eq!(
            serde_json::to_value(entry).unwrap()["ports"],
            evidence["ports"]
        );
    }
    assert!(entries[2].evidence.is_empty());
    let old: codeterm_storage::BlueprintJournalEntry = serde_json::from_value(json!({"entry_id":1,"run_id":"legacy","step_index":0,"step_name":"old","state":"done","outcome":"done","started_at_ms":1,"ended_at_ms":2})).unwrap();
    assert!(old.evidence.is_empty());
    assert!(serde_json::to_value(old).unwrap().get("ports").is_none());
    close(storage);
}

fn decision(run_id: &str, at_ms: u64, outcome: String) -> BlueprintJournalWrite {
    BlueprintJournalWrite::Decision {
        run_id: run_id.into(),
        marker: JournalMarker::Step(0),
        step_name: "emit".into(),
        outcome: outcome.into(),
        at_ms,
        result_summary: Some("bounded summary".into()),
        reason: None,
    }
}

#[test]
fn blueprint_journal_claim_refuses_replay_after_reopen() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(None));
    let first = done(storage.sentinels().write_blueprint_journal(
        journal_id(&job),
        BlueprintJournalWrite::Claim {
            definition_version: None,
            parent: None,
            run_id: "run_first".into(),
            marker: JournalMarker::Step(0),
            step_name: "emit".into(),
            generation: 0,
            started_at_ms: 1100,
        },
    ));
    assert_eq!(first.value.outcome, MutationOutcome::Applied);
    close(storage);

    let storage = open(root.path());
    let second = done(storage.sentinels().write_blueprint_journal(
        journal_id(&job),
        BlueprintJournalWrite::Claim {
            definition_version: None,
            parent: None,
            run_id: "run_reopen".into(),
            marker: JournalMarker::Step(0),
            step_name: "emit".into(),
            generation: 0,
            started_at_ms: 1200,
        },
    ));
    assert_eq!(
        second.value.outcome,
        MutationOutcome::Fenced(Fence::Claimed)
    );
    let entries = done(storage.sentinels().blueprint_log(journal_id(&job), 200)).value;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].outcome.as_deref(), Some("skip:uncertain_claim"));
    assert_eq!(entries[1].state, "running");
    close(storage);
}

#[test]
fn blueprint_journal_delete_with_blueprint_cascades_rows() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(Some("team")));
    done(storage.sentinels().write_blueprint_journal(
        journal_id(&job),
        decision("run_delete", 2000, "classified".into()),
    ));
    done(storage.sentinels().remove_team("team".into()));
    let entries = done(storage.sentinels().blueprint_log(journal_id(&job), 200)).value;
    assert!(entries.is_empty());
    close(storage);
}

#[test]
fn blueprint_journal_refuses_summaries_over_two_hundred_characters() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(None));
    let result = storage
        .sentinels()
        .write_blueprint_journal(
            journal_id(&job),
            BlueprintJournalWrite::Finish {
                evidence: None,
                run_id: "run_summary".into(),
                marker: JournalMarker::Step(0),
                state: "done".into(),
                outcome: "done".into(),
                ended_at_ms: 1300,
                result_summary: "x".repeat(201),
                body_ref: None,
                reason: None,
            },
        )
        .wait(DEADLINE);
    assert!(result.is_err());
    close(storage);
}

#[test]
fn blueprint_pipeline_journal_records_each_step_boundary_in_order() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(None));
    for (step_index, step_name, start, end) in [(0, "emit", 1100, 1200), (1, "match", 1201, 1300)] {
        let run_id = "run_steps";
        done(storage.sentinels().write_blueprint_journal(
            journal_id(&job),
            BlueprintJournalWrite::Claim {
                definition_version: None,
                parent: None,
                run_id: run_id.into(),
                marker: JournalMarker::Step(step_index as u32),
                step_name: step_name.into(),
                generation: 0,
                started_at_ms: start,
            },
        ));
        done(storage.sentinels().write_blueprint_journal(
            journal_id(&job),
            BlueprintJournalWrite::Finish {
                evidence: None,
                run_id: run_id.into(),
                marker: JournalMarker::Step(step_index as u32),
                state: "done".into(),
                outcome: "done".into(),
                ended_at_ms: end,
                result_summary: format!("step {step_name} completed"),
                body_ref: None,
                reason: None,
            },
        ));
    }
    let entries = done(storage.sentinels().blueprint_log(journal_id(&job), 200)).value;
    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.step_index, entry.state.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "done"), (1, "running"), (0, "done"), (0, "running")]
    );
    assert!(entries.iter().all(|entry| {
        entry
            .result_summary
            .as_ref()
            .is_none_or(|summary| summary.len() <= 200 && !summary.contains("full command output"))
    }));
    close(storage);
}

#[test]
fn blueprint_journal_retains_a_complete_sixty_four_step_run_and_derives_its_close() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline_with_steps(&store, owner(None), 64);
    for index in 0..64 {
        let at = 2000 + u64::from(index) * 4;
        let marker = JournalMarker::Step(index);
        let name = format!("step-{index}");
        for write in [
            BlueprintJournalWrite::Claim {
                definition_version: None,
                parent: None,
                run_id: "long-run".into(),
                marker,
                step_name: name.clone(),
                generation: 0,
                started_at_ms: at,
            },
            BlueprintJournalWrite::Finish {
                evidence: None,
                run_id: "long-run".into(),
                marker,
                state: JournalState::Done,
                outcome: "done".into(),
                ended_at_ms: at + 1,
                result_summary: "filter matched".into(),
                body_ref: None,
                reason: None,
            },
            BlueprintJournalWrite::Decision {
                run_id: "long-run".into(),
                marker,
                step_name: name.clone(),
                outcome: "filter_match".into(),
                at_ms: at + 2,
                result_summary: None,
                reason: None,
            },
            BlueprintJournalWrite::Decision {
                run_id: "long-run".into(),
                marker,
                step_name: name,
                outcome: "unchanged".into(),
                at_ms: at + 3,
                result_summary: None,
                reason: None,
            },
        ] {
            assert_eq!(
                done(
                    storage
                        .sentinels()
                        .write_blueprint_journal(journal_id(&job), write)
                )
                .value
                .outcome,
                MutationOutcome::Applied
            );
        }
    }
    done(storage.sentinels().write_blueprint_journal(
        journal_id(&job),
        BlueprintJournalWrite::CloseRun {
            run_id: "long-run".into(),
            state: JournalState::Done,
            outcome: "done".into(),
            at_ms: 2300,
            reason: None,
        },
    ));
    let entries = done(storage.sentinels().blueprint_log(journal_id(&job), 1)).value;
    assert_eq!(entries.len(), 257);
    for index in 0..64 {
        assert_eq!(
            entries
                .iter()
                .filter(|row| row.marker() == JournalMarker::Step(index))
                .count(),
            4
        );
    }
    assert_eq!(
        codeterm_storage::BlueprintJournalEntry::derive_run(&entries.iter().collect::<Vec<_>>()),
        (Some(JournalState::Done), None)
    );
    for index in 0..32 {
        done(storage.sentinels().write_blueprint_journal(
            journal_id(&job),
            BlueprintJournalWrite::CloseRun {
                run_id: format!("later-{index}"),
                state: JournalState::Done,
                outcome: "done".into(),
                at_ms: 3000 + index,
                reason: None,
            },
        ));
    }
    let entries = done(storage.sentinels().blueprint_log(journal_id(&job), 200)).value;
    assert_eq!(entries.len(), 32);
    assert!(entries.iter().all(|entry| entry.run_id != "long-run"));
    close(storage);
}

#[test]
fn blueprint_journal_body_reference_round_trips_outside_summary_and_held_close_is_typed() {
    use codeterm_blueprint_types::blueprint_journal::JournalReason;
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(None));
    let reference = format!("inbox/{}.md", "x".repeat(300));
    done(storage.sentinels().write_blueprint_journal(
        journal_id(&job),
        BlueprintJournalWrite::Claim {
            definition_version: None,
            parent: None,
            run_id: "result-run".into(),
            marker: JournalMarker::Result,
            step_name: "$result".into(),
            generation: 0,
            started_at_ms: 1100,
        },
    ));
    done(storage.sentinels().write_blueprint_journal(
        journal_id(&job),
        BlueprintJournalWrite::Finish {
            evidence: None,
            run_id: "result-run".into(),
            marker: JournalMarker::Result,
            state: JournalState::Failed,
            outcome: "handler_unconfirmed".into(),
            ended_at_ms: 1200,
            result_summary: "delivery unconfirmed".into(),
            body_ref: Some(reference.clone()),
            reason: Some(JournalReason::UncertainResult),
        },
    ));
    done(storage.sentinels().write_blueprint_journal(
        journal_id(&job),
        BlueprintJournalWrite::CloseRun {
            run_id: "result-run".into(),
            state: JournalState::Held,
            outcome: "held".into(),
            at_ms: 1201,
            reason: Some(JournalReason::UncertainResult),
        },
    ));
    let persisted = done(storage.sentinels().get(journal_id(&job)))
        .value
        .unwrap();
    assert_eq!(
        persisted.record.runtime.held.as_deref(),
        Some("uncertain_pipeline_result")
    );
    let entries = done(storage.sentinels().blueprint_log(journal_id(&job), 1)).value;
    assert_eq!(entries[1].body_ref.as_deref(), Some(reference.as_str()));
    assert_eq!(
        entries[1].result_summary.as_deref(),
        Some("delivery unconfirmed")
    );
    assert_eq!(entries[0].reason, Some(JournalReason::UncertainResult));
    assert_eq!(
        codeterm_storage::BlueprintJournalEntry::derive_run(&entries.iter().collect::<Vec<_>>()),
        (Some(JournalState::Held), None)
    );
    close(storage);
}

#[test]
fn blueprint_journal_commits_coalesce_scoped_changes_with_new_revisions() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(Some("team")));
    store.lock().take_changes();
    let initial = store
        .lock()
        .committed_revision("workspace", Some("team"))
        .unwrap()
        .to_owned();
    commit(&store, |store| {
        store.stage_pipeline_journal(&job.id, decision("run_events", 1100, "done".into()))
    })
    .unwrap();
    let first = store.lock().take_changes();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["workspace_id"], "workspace");
    assert_eq!(first[0]["team_id"], "team");
    assert_ne!(first[0]["revision"], initial);
    assert!(first[0]["revision"]
        .as_str()
        .unwrap()
        .starts_with("journal:r"));
    for at in [1200, 1300] {
        commit(&store, |store| {
            store.stage_pipeline_journal(&job.id, decision("run_events", at, "done".into()))
        })
        .unwrap();
    }
    let batch = store.lock().take_changes();
    assert_eq!(batch.len(), 1);
    assert_ne!(batch[0]["revision"], first[0]["revision"]);
    let log = store.lock().typed_pipeline_log(&job.id, 200).unwrap();
    assert_eq!(done(log).value.len(), 3);
    assert!(store.lock().take_changes().is_empty());
    close(storage);
}

#[test]
fn blueprint_runtime_commit_invalidates_checkpoint_state_once() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(Some("team")));
    store.lock().active.insert(
        job.id.clone(),
        Arc::new(crate::probe::cancellation::Cancellation::new(false)),
    );
    store.lock().take_changes();
    let initial = store
        .lock()
        .committed_revision("workspace", Some("team"))
        .unwrap()
        .to_owned();
    let mut admission = job.admission.clone();
    admission.continuation = Some(json!({"run_id":"run_checkpoint"}));
    assert!(commit(&store, |store| store
        .stage_admission(&job.id, admission.clone()))
    .unwrap());
    assert_eq!(store.lock().get(&job.id).unwrap().revision, job.revision);
    let changes = store.lock().take_changes();
    assert_eq!(changes.len(), 1);
    assert_ne!(changes[0]["revision"], initial);
    assert!(!commit(&store, |store| store.stage_admission(&job.id, admission)).unwrap());
    assert!(store.lock().take_changes().is_empty());
    close(storage);
}

#[test]
fn blueprint_journal_emits_once_after_commit_without_the_store_lock() {
    struct Ui {
        store: Mutex<Option<std::sync::Weak<Mutex<Store>>>>,
        events: Mutex<Vec<(String, Value)>>,
    }
    impl codeterm_service_core::ports::host::UiEvents for Ui {
        fn emit(&self, event: &str, payload: Value) {
            let store = self.store.lock().as_ref().unwrap().upgrade().unwrap();
            assert!(
                store.try_lock().is_some(),
                "store guard released before emit"
            );
            self.events.lock().push((event.into(), payload));
        }
        fn emit_to(&self, _: &str, event: &str, payload: Value) {
            self.emit(event, payload);
        }
    }
    let root = temp_root();
    let storage = open(root.path());
    let ui = Arc::new(Ui {
        store: Mutex::new(None),
        events: Mutex::new(Vec::new()),
    });
    let handle = crate::scheduler::fixture_handle_with_ui(
        Store::with_storage(&storage, MockClock::starting_at(1000)),
        ui.clone(),
    );
    *ui.store.lock() = Some(Arc::downgrade(&handle.store));
    let job = create_pipeline(&handle.store, owner(Some("team")));
    handle.store.lock().take_changes();
    assert!(crate::pipeline::write_journal(
        &handle,
        &job,
        decision("run_ui", 1100, "done".into())
    ));
    let log = handle
        .store
        .lock()
        .typed_pipeline_log(&job.id, 200)
        .unwrap();
    assert_eq!(done(log).value.len(), 1);
    let events = ui.events.lock();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "automation-graph-changed");
    assert_eq!(events[0].1["workspace_id"], "workspace");
    assert_eq!(events[0].1["team_id"], "team");
    close(storage);
}

#[test]
fn blueprint_journal_one_event_for_a_storage_commit_batch_even_with_separate_notifies() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1000);
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(Some("team")));
    store.lock().take_changes();
    let hold = storage.hold_writer().unwrap();
    hold.wait_held(DEADLINE).unwrap();
    let first = store
        .lock()
        .stage_pipeline_journal(&job.id, decision("run_batch", 1100, "done".into()));
    let second = store
        .lock()
        .stage_pipeline_journal(&job.id, decision("run_batch", 1200, "done".into()));
    hold.release();
    first.settle(&store).unwrap();
    let events = store.lock().take_changes();
    assert_eq!(events.len(), 1);
    assert_eq!(
        done(store.lock().typed_pipeline_log(&job.id, 200).unwrap())
            .value
            .len(),
        2
    );
    second.settle(&store).unwrap();
    assert!(store.lock().take_changes().is_empty());
    close(storage);
}
