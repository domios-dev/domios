use super::*;
#[test]
fn probe_cadence_never_backs_off_on_unchanged_results() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(
        root.path().join("fixture.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let mut job=store.prepare_document(serde_json::json!({"schema_version":2,"kind":"watch","name":"cadence fixture","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"status"}},"every":{"lit":"60s"},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"status?"}},"buckets":["waiting"],"matches":{"waiting":{"lit":"waiting"}}}]}),Owner{creator_tab_id:"creator".into(),team_id:None,workspace_id:"ws".into(),cwd:".".into()},false).unwrap();
    job.id = "watch".into();
    job.machine.last_hash = Some("same".into());
    job.machine.last_bucket = Some("waiting".into());
    job.last_reason = Some("unchanged".into());
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
        .prepare_document(
            serde_json::json!({"schema_version":2,"kind":"pipeline","name":"one-shot-input","owner":"tab","trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":"hello"}}},"steps":[{"id":"filter","uses":"filter","with":{"pattern":{"lit":"hello"}}}]}),
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
