use super::*;
use codeterm_blueprint_types::blueprint_pipeline::{
    BlueprintResult, BlueprintSpec, IoType, Step, StepKind, Trigger,
};
use codeterm_blueprint_types::sentinel_input::{DataRef, MediaKind};

#[test]
fn probe_cadence_never_backs_off_on_unchanged_results() {
    let job: Sentinel = serde_json::from_value(serde_json::json!({
        "id":"watch", "creator_tab_id":"creator", "team_id":null, "workspace_id":"ws", "cwd":".",
        "probe":"status", "question":"status?", "buckets":["waiting"], "routes":{"waiting":[]},
        "matches":{}, "until":null, "expires_at":null, "interval_ms":60000, "next_run":1000,
        "machine":{"stopped":false,"paused":false,"last_hash":"same","last_bucket":"waiting"},
        "last_reason":"unchanged"
    }))
    .unwrap();
    let mut jobs = vec![job];
    let mut active = HashMap::new();
    let mut wakes = BTreeSet::new();
    for now in [1000, 61000, 121000] {
        active.clear();
        let (leases, _, _) = plan_due(&mut jobs, &mut active, &mut wakes, now);
        assert_eq!(leases.len(), 1);
        assert_eq!(jobs[0].next_run, now + 60000);
    }
    active.clear();
    let (leases, _, _) = plan_due(&mut jobs, &mut active, &mut wakes, 541000);
    assert_eq!(leases.len(), 1);
    assert_eq!(jobs[0].next_run, 601000);
}

#[test]
fn blueprint_input_due_is_once_per_activation_and_amendment_without_interval_runs() {
    let temp = tempfile::tempdir().unwrap();
    let clock = codeterm_cron::clock::MockClock::starting_at(1_000);
    let store = Store::new(temp.path().join("input.json"), clock);
    let mut job = store
        .prepare_pipeline(
            BlueprintSpec {
                note: None,
                name: "one-shot-input".into(),
                owner: codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner::Tab,
                trigger: Trigger::Input {
                    kind: MediaKind::Text,
                    source: DataRef::Text("hello".into()),
                },
                steps: vec![Step {
                    name: "filter".into(),
                    input: IoType::Text,
                    kind: StepKind::Filter {
                        pattern: "hello".into(),
                        bucket: None,
                    },
                    on: Default::default(),
                }],
                result: BlueprintResult::default(),
            },
            Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "workspace".into(),
                cwd: "/tmp".into(),
            },
            true,
        )
        .unwrap();
    job.next_run = 1_000;
    let id = job.id.clone();
    let mut jobs = vec![job];
    let mut active = HashMap::new();
    let mut wake_pending = BTreeSet::new();

    assert_eq!(
        plan_due(&mut jobs, &mut active, &mut wake_pending, 1_000)
            .0
            .len(),
        1
    );
    active.remove(&id);
    assert!(plan_due(&mut jobs, &mut active, &mut wake_pending, 21_000)
        .0
        .is_empty());

    jobs[0].next_run = 21_000;
    assert_eq!(
        plan_due(&mut jobs, &mut active, &mut wake_pending, 21_000)
            .0
            .len(),
        1
    );
    active.remove(&id);
    assert!(plan_due(&mut jobs, &mut active, &mut wake_pending, 41_000)
        .0
        .is_empty());
}
