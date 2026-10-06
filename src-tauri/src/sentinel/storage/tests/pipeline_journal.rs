use super::*;
use crate::storage::{BlueprintJournalWrite, SentinelId};
use codeterm_types::blueprint_pipeline::{
    BlueprintOwner, BlueprintResult, BlueprintSpec, IoType, ResultTarget, Step, StepKind, Trigger,
};

fn create_pipeline(store: &Mutex<Store>, owner: Owner) -> Sentinel {
    let spec = BlueprintSpec {
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
        step_index: 0,
        step_name: "emit".into(),
        outcome,
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
            step_index: 0,
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
            step_index: 0,
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
fn blueprint_journal_retains_only_two_hundred_newest_rows() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = Arc::new(MockClock::starting_at(1000));
    let store = typed(&storage, &clock);
    let job = create_pipeline(&store, owner(None));
    for index in 0..205 {
        done(storage.sentinels().write_blueprint_journal(
            journal_id(&job),
            decision("run_trim", 2000 + index, format!("decision_{index:03}")),
        ));
    }
    let entries = done(storage.sentinels().blueprint_log(journal_id(&job), 200)).value;
    assert_eq!(entries.len(), 200);
    assert_eq!(entries[0].outcome.as_deref(), Some("decision_204"));
    assert_eq!(entries[199].outcome.as_deref(), Some("decision_005"));
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
                step_index: 0,
                state: "done".into(),
                outcome: "done".into(),
                ended_at_ms: 1300,
                result_summary: "x".repeat(201),
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
                step_index,
                step_name: step_name.into(),
                generation: 0,
                started_at_ms: start,
            },
        ));
        done(storage.sentinels().write_blueprint_journal(
            journal_id(&job),
            BlueprintJournalWrite::Finish {
                run_id: run_id.into(),
                step_index,
                state: "done".into(),
                outcome: "done".into(),
                ended_at_ms: end,
                result_summary: format!("step {step_name} completed"),
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
