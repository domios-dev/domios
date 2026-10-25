use super::*;
use serde_json::json;

fn job(id: &str, creator: &str, stopped: bool, until: Option<&str>) -> Sentinel {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(
        root.path().join("fixture.json"),
        codeterm_cron::clock::MockClock::starting_at(0),
    );
    let mut job=store.prepare_document(json!({"schema_version":2,"kind":"watch","name":format!("cleanup {id}"),"steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"status"}},"every":{"lit":"10s"},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["ready","done"],"matches":{"ready":{"lit":"ready"},"done":{"lit":"done"}},"until":until}]}),crate::store::Owner{creator_tab_id:creator.into(),team_id:Some("team".into()),workspace_id:"workspace".into(),cwd:"/tmp".into()},false).unwrap();
    job.id = id.into();
    job.scenario_id = Some(id.into());
    job.machine.stopped = stopped;
    job
}

#[test]
fn terminal_cleanup_requires_stopped_scenario_and_released_work() {
    let root = job("root", "worker", true, Some("ready"));
    let mut child = job("child", "worker", false, None);
    child.scenario_id = Some(root.id.clone());
    let mut jobs = vec![root, child, job("recurring", "worker", true, None)];
    assert!(terminal_roots(&jobs, &BTreeSet::new()).is_empty());
    jobs[1].machine.stopped = true;
    assert!(terminal_roots(&jobs, &BTreeSet::from(["root".into()])).is_empty());
    assert_eq!(
        terminal_roots(&jobs, &BTreeSet::new())
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["root"]
    );
    jobs[0].rearm_on_change.insert("ready".into());
    assert!(terminal_roots(&jobs, &BTreeSet::new()).is_empty());
}

#[test]
fn terminal_cleanup_retains_team_owned_agent_authored_history_records() {
    let mut blueprint = job("blueprint", "creator", true, Some("done"));
    blueprint.agent_authored = true;

    assert!(terminal_roots(&[blueprint], &BTreeSet::new()).is_empty());
}

#[test]
fn retained_stopped_agent_blueprint_is_valid_persisted_history() {
    let mut blueprint = job("blueprint", "creator", true, Some("done"));
    blueprint.agent_authored = true;

    assert!(super::super::store::validate(vec![blueprint]).is_ok());

    let mut diverged = job("diverged", "creator", true, Some("done"));
    diverged.agent_authored = true;
    diverged.until = Some("ready".into());
    assert!(super::super::store::validate(vec![diverged]).is_err());
}

#[test]
fn terminal_cleanup_removes_agent_authored_root_without_team_retention_scope() {
    let mut blueprint = job("personal-blueprint", "creator", true, Some("done"));
    blueprint.agent_authored = true;
    blueprint.team_id = None;

    assert_eq!(
        terminal_roots(&[blueprint], &BTreeSet::new())
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["personal-blueprint"]
    );
}

#[test]
fn worker_cleanup_persists_removal_and_preserves_recurring_and_foreign_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let mut store = Store::new(
        path.clone(),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    for job in [
        job("owned", "worker", false, Some("ready")),
        job("foreign", "other", true, Some("ready")),
        job("recurring", "worker", true, None),
    ] {
        store.insert(job).unwrap();
    }
    let mut team_blueprint = job("team-blueprint", "worker", true, Some("done"));
    team_blueprint.agent_authored = true;
    store.insert(team_blueprint).unwrap();
    let store = Arc::new(Mutex::new(store));
    remove_owned(&store, "worker").unwrap();
    let restored = Store::new(path, codeterm_cron::clock::MockClock::starting_at(1000));
    assert_eq!(
        restored
            .list()
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["foreign", "recurring", "team-blueprint"]
    );
    remove_terminal(&store).unwrap();
    assert_eq!(
        store
            .lock()
            .list()
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["foreign", "recurring", "team-blueprint"]
    );
}

#[test]
fn stopped_watch_waits_for_cancelled_lease_release() {
    let dir = tempfile::tempdir().unwrap();
    let mut current = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    current
        .insert(job("root", "worker", false, Some("ready")))
        .unwrap();
    let lease = current.due().unwrap().remove(0);
    current.stop("root", "stopped_by_creator").unwrap();
    current.clock = codeterm_cron::clock::MockClock::starting_at(1_801_000);
    assert!(lease.is_cancelled());
    let store = Arc::new(Mutex::new(current));
    remove_terminal(&store).unwrap();
    assert!(store.lock().get("root").is_some());
    store.lock().release("root");
    remove_terminal(&store).unwrap();
    assert!(store.lock().get("root").is_none());
}

#[test]
fn terminal_cleanup_keeps_stopped_agent_authored_blueprint_in_store() {
    let dir = tempfile::tempdir().unwrap();
    let mut blueprint = job("blueprint", "creator", true, Some("done"));
    blueprint.agent_authored = true;
    let mut current = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    current.insert(blueprint).unwrap();
    let store = Arc::new(Mutex::new(current));

    remove_terminal(&store).unwrap();

    let mut store = store.lock();
    let retained = store
        .get("blueprint")
        .expect("stopped Blueprint record remains available");
    assert_eq!(super::super::amend::history(retained).versions.len(), 1);

    store.remove_team("team").unwrap();
    assert!(store.get("blueprint").is_none());
}

#[test]
fn inserting_at_capacity_evicts_oldest_retained_stopped_agent_blueprint() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("blueprints.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    store.jobs = (0..super::super::store::MAX_SENTINELS)
        .map(|index| {
            let mut blueprint = job(
                &format!("retained-{index:03}"),
                "creator",
                true,
                Some("done"),
            );
            blueprint.agent_authored = true;
            blueprint
        })
        .collect();

    let mut next = job("new-blueprint", "creator", false, None);
    next.team_id = Some("team".into());
    store.insert(next).unwrap();

    assert!(store.get("retained-000").is_none());
    assert!(store.get("retained-001").is_some());
    assert!(store.get("new-blueprint").is_some());
    assert_eq!(store.list().len(), super::super::store::MAX_SENTINELS);
}

#[test]
fn insertion_at_capacity_refuses_when_all_blueprints_are_live() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("blueprints.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    store.jobs = (0..super::super::store::MAX_SENTINELS)
        .map(|index| {
            let mut blueprint = job(&format!("live-{index:03}"), "creator", false, None);
            blueprint.machine.paused = true;
            blueprint
        })
        .collect();

    let error = store
        .insert(job("new-blueprint", "creator", false, None))
        .unwrap_err();

    assert!(error.contains("blueprint capacity reached"));
    assert_eq!(store.list().len(), super::super::store::MAX_SENTINELS);
}

#[test]
fn stopped_cleanup_deadline_waits_for_the_whole_graph_and_keeps_team_scope() {
    let mut root = job("cleanup-root", "creator", true, None);
    root.cleanup.sync(true, 1_000);
    let mut child = job("cleanup-child", "creator", true, None);
    child.scenario_id = Some(root.id.clone());
    child.cleanup.sync(true, 2_000);
    let mut foreign = job("foreign-child", "foreign", false, None);
    foreign.scenario_id = Some(root.id.clone());
    foreign.team_id = Some("other-team".into());
    let mut jobs = vec![root.clone(), child, foreign];
    assert_eq!(
        deletion_deadline(&jobs, &root, &BTreeSet::new()),
        Some(1_802_000)
    );
    assert!(expired_roots(&jobs, &BTreeSet::new(), 1_801_000).is_empty());
    assert_eq!(
        expired_roots(&jobs, &BTreeSet::new(), 1_802_000)[0].id,
        root.id
    );
    assert!(expired_roots(&jobs, &BTreeSet::from(["cleanup-child".into()]), 1_802_000).is_empty());
    jobs[1].machine.stopped = false;
    jobs[1].cleanup.sync(false, 1_802_000);
    assert!(expired_roots(&jobs, &BTreeSet::new(), 1_802_000).is_empty());
}
