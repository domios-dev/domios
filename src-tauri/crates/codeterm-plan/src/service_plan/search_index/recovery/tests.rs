use super::*;
use crate::plan::OrchestratorRef;

#[test]
fn startup_snapshot_backfills_every_plan_without_mutating_it() {
    let first = Plan::new(
        "First".into(),
        "/recovery-project".into(),
        OrchestratorRef::default(),
    );
    let second = Plan::new(
        "Second".into(),
        "/recovery-project".into(),
        OrchestratorRef::default(),
    );
    let plans = vec![first, second];
    let before = plans.clone();
    let (result, jobs) = project_jobs(Path::new("/recovery-project"), &plans, &[]);
    assert_eq!(plans, before);
    assert_eq!((result.plans, result.units), (2, 2));
    assert!(matches!(&jobs[0], IndexJob::Prune(_, live) if live.len() == 2));
    assert_eq!(
        jobs.iter()
            .filter(|job| matches!(job, IndexJob::Plan(_, PlanIndexScope::Full)))
            .count(),
        2
    );
}

#[test]
fn reindex_live_set_excludes_foreign_and_deleted_units() {
    let first = Plan::new(
        "First".into(),
        "/recovery-project".into(),
        OrchestratorRef::default(),
    );
    let foreign = Plan::new(
        "Foreign".into(),
        "/foreign-project".into(),
        OrchestratorRef::default(),
    );
    let mut spec = Spec::new("Owned spec".into(), "/recovery-project".into());
    spec.plan_id = Some(first.id.clone());
    let (result, jobs) = project_jobs(
        Path::new("/recovery-project"),
        &[first.clone(), foreign.clone()],
        &[spec.clone()],
    );
    assert_eq!((result.plans, result.specs), (1, 1));
    let IndexJob::Prune(_, live) = &jobs[0] else {
        panic!("prune first")
    };
    assert!(live.contains(&stable_id(&first.id, "plan", &first.id)));
    assert!(live.contains(&stable_id(&spec.id, "spec", &spec.id)));
    assert!(!live.contains(&stable_id(&foreign.id, "plan", &foreign.id)));
    assert!(!live.contains("planunit_deleted_task_stale"));
}

#[test]
fn purge_visits_all_indexes_and_matches_only_plan_and_owned_spec() {
    let indexes = vec![
        PathBuf::from("current.db"),
        PathBuf::from("old.db"),
        PathBuf::from("foreign.db"),
    ];
    let mut visited = vec![];
    purge_indexes("pl_owned", indexes.clone(), |db| {
        visited.push(db.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert_eq!(visited, indexes);
    let entities = HashSet::from(["pl_owned", "sp_owned"]);
    for id in [
        "planunit_pl_owned_task_task",
        "planunit_pl_owned_stride_stride",
        "planunit_sp_owned_section_section",
    ] {
        assert!(owns_unit(&entities, id));
    }
    assert!(!owns_unit(&entities, "planunit_pl_other_task_task"));
    assert!(!owns_unit(&entities, "planunit_pl_ownedextra_task_task"));
}

#[test]
fn purge_failure_names_plan_index_and_storage_reason() {
    let error = purge_indexes("pl_owned", [PathBuf::from("project.db")], |_| {
        Err("vector cleanup refused".into())
    })
    .unwrap_err();
    assert_eq!(
        error,
        "delete plan pl_owned from project.db: vector cleanup refused"
    );
    let queue = IndexQueue {
        state: Mutex::new(IndexQueueState::default()),
        ready: Condvar::new(),
    };
    let plan = Plan::new(
        "Deleted".into(),
        "/recovery-project".into(),
        OrchestratorRef::default(),
    );
    queue.enqueue(IndexJob::PurgePlan(plan, vec![]));
    let (key, project, _) = queue.take();
    queue.complete(key.clone(), project, true);
    assert!(queue.state.lock().needs_full.contains(&key));
}
