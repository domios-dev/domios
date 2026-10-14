use super::*;
use serde_json::json;

fn job(id: &str, creator: &str, stopped: bool, until: Option<&str>) -> Sentinel {
    serde_json::from_value(json!({
        "id":id,"scenario_id":id,"workspace_id":"workspace","team_id":"team","creator_tab_id":creator,
        "cwd":"/tmp","probe":"status","question":"Ready?","buckets":["ready"],
        "routes":{"ready":[]},"matches":{},"interval_ms":10000,"next_run":0,
        "expires_at":null,"until":until,"last_reason":null,
        "machine":{"paused":false,"stopped":stopped,"last_hash":null,"last_bucket":null}
    }))
    .unwrap()
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
fn worker_cleanup_persists_removal_and_preserves_recurring_and_foreign_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let mut store = Store::new(
        path.clone(),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    for job in [
        job("owned", "worker", false, Some("ready")),
        job("foreign", "other", true, Some("ready")),
        job("recurring", "worker", true, None),
    ] {
        store.insert(job).unwrap();
    }
    let store = Arc::new(Mutex::new(store));
    remove_owned(&store, "worker").unwrap();
    let restored = Store::new(path, crate::cron::clock::MockClock::starting_at(1000));
    assert_eq!(
        restored
            .list()
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["foreign", "recurring"]
    );
    remove_terminal(&store).unwrap();
    assert_eq!(
        store
            .lock()
            .list()
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["recurring"]
    );
}

#[test]
fn stopped_watch_waits_for_cancelled_lease_release() {
    let dir = tempfile::tempdir().unwrap();
    let mut current = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    current
        .insert(job("root", "worker", false, Some("ready")))
        .unwrap();
    let lease = current.due().unwrap().remove(0);
    current.stop("root", "stopped_by_creator").unwrap();
    assert!(lease.is_cancelled());
    let store = Arc::new(Mutex::new(current));
    remove_terminal(&store).unwrap();
    assert!(store.lock().get("root").is_some());
    store.lock().release("root");
    remove_terminal(&store).unwrap();
    assert!(store.lock().get("root").is_none());
}
