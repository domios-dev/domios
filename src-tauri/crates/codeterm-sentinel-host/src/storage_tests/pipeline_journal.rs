use super::*;
use codeterm_blueprint_types::blueprint_journal::{JournalMarker, JournalState};
use codeterm_blueprint_types::blueprint_pipeline::{
    BlueprintOwner, BlueprintResult, BlueprintSpec, IoType, ResultTarget, Step, StepKind, Trigger,
};
use codeterm_storage::{BlueprintJournalWrite, SentinelId};

fn create_pipeline(store: &Mutex<Store>, owner: Owner) -> Sentinel {
    create_pipeline_with_steps(store, owner, 2)
}

fn create_pipeline_with_steps(store: &Mutex<Store>, owner: Owner, count: usize) -> Sentinel {
    let mut spec = BlueprintSpec {
        name: "journal pipeline".into(),
        owner: BlueprintOwner::Tab,
        trigger: Trigger::Schedule {
            every: "20s".into(),
        },
        steps: vec![
            Step {
                name: "emit".into(),
                input: IoType::Text,
                kind: StepKind::Command {
                    command: "printf ready".into(),
                },
                on: Default::default(),
            },
            Step {
                name: "match".into(),
                input: IoType::Text,
                kind: StepKind::Filter {
                    pattern: "^ready$".into(),
                    bucket: Some("matched".into()),
                },
                on: Default::default(),
            },
        ],
        result: BlueprintResult {
            to: ResultTarget::Owner,
        },
    };
    if count != 2 {
        spec.steps = (0..count)
            .map(|index| Step {
                name: format!("step-{index}"),
                input: IoType::Text,
                kind: StepKind::Filter {
                    pattern: "ready".into(),
                    bucket: None,
                },
                on: Default::default(),
            })
            .collect();
    }
    let job = store
        .lock()
        .prepare_pipeline(spec, owner, false)
        .expect("pipeline definition");
    commit(store, |current| current.stage_insert(job)).expect("pipeline creation")
}

fn journal_id(job: &Sentinel) -> SentinelId {
    SentinelId::new(job.id.clone()).expect("journal sentinel id")
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
                run_id: run_id.into(),
                marker: JournalMarker::Step(step_index as u16),
                step_name: step_name.into(),
                generation: 0,
                started_at_ms: start,
            },
        ));
        done(storage.sentinels().write_blueprint_journal(
            journal_id(&job),
            BlueprintJournalWrite::Finish {
                run_id: run_id.into(),
                marker: JournalMarker::Step(step_index as u16),
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
                run_id: "long-run".into(),
                marker,
                step_name: name.clone(),
                generation: 0,
                started_at_ms: at,
            },
            BlueprintJournalWrite::Finish {
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
