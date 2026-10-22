use super::*;
use crate::plan::OrchestratorRef;

#[test]
fn task_scope_selects_listed_tasks_and_tracks_all_live_ids() {
    let mut plan = super::super::tests::decisions::sample_plan();
    plan.tasks.remove(2);
    let (units, live) = task_scope_units(&plan, &HashSet::from(["beta".into()]));
    assert_eq!(
        units
            .iter()
            .filter(|unit| unit.kind == "task")
            .map(|unit| unit.unit_id.as_str())
            .collect::<Vec<_>>(),
        ["beta"]
    );
    assert!(live.contains("planunit_pl_decisions_task_alpha"));
    assert!(live.contains("planunit_pl_decisions_task_beta"));
    assert!(!live.contains("planunit_pl_decisions_task_gamma"));
}

#[test]
fn persisted_row_changes_select_only_changed_tasks() {
    use crate::service_plan::store_persist::index_scope;
    use crate::service_plan::store_sync::RowWrites;
    assert_eq!(
        index_scope(RowWrites {
            changed_tasks: vec!["beta".into()],
            ..Default::default()
        }),
        PlanIndexScope::Tasks(HashSet::from(["beta".into()]))
    );
    assert_eq!(
        index_scope(RowWrites {
            header_identity_changed: true,
            changed_tasks: vec!["beta".into()],
            ..Default::default()
        }),
        PlanIndexScope::Full
    );
}

#[test]
fn moving_project_selects_a_full_rebuild_in_the_new_scope() {
    let mut plan = super::super::tests::decisions::sample_plan();
    let old = IndexJob::DeletePlan(plan.clone());
    plan.canonical_project_path = "/moved-project".into();
    let new = IndexJob::Plan(plan.clone(), PlanIndexScope::Full);
    assert_ne!(old.project_key(), new.project_key());
    let units = plan_units(&plan, &new.project_key());
    assert!(units.iter().all(|unit| unit.scope == new.project_key()));
    assert_eq!(units.iter().filter(|unit| unit.kind == "task").count(), 3);
}

#[test]
fn a_failed_plan_job_makes_the_next_job_for_that_plan_full() {
    let queue = IndexQueue {
        state: Mutex::new(IndexQueueState::default()),
        ready: Condvar::new(),
    };
    let plan = Plan::new(
        "Healing".into(),
        "D:/project".into(),
        OrchestratorRef::default(),
    );
    let tasks = || PlanIndexScope::Tasks(HashSet::from(["t".to_string()]));
    let scope_of = |job: IndexJob| match job {
        IndexJob::Plan(_, scope) => scope,
        _ => unreachable!(),
    };

    queue.enqueue(IndexJob::Plan(plan.clone(), tasks()));
    let (key, project, _) = queue.take();
    queue.enqueue(IndexJob::Plan(plan.clone(), tasks()));
    queue.complete(key, project, true);
    let (key, project, job) = queue.take();
    assert_eq!(scope_of(job), PlanIndexScope::Full);
    queue.complete(key, project, true);

    queue.enqueue(IndexJob::Plan(plan.clone(), tasks()));
    let (key, project, job) = queue.take();
    assert_eq!(scope_of(job), PlanIndexScope::Full);
    queue.complete(key, project, false);

    queue.enqueue(IndexJob::Plan(plan, tasks()));
    assert_eq!(scope_of(queue.take().2), tasks());
}

#[test]
fn queued_task_scopes_merge_and_full_wins() {
    let alpha = PlanIndexScope::Tasks(HashSet::from(["a".to_string()]));
    let beta = PlanIndexScope::Tasks(HashSet::from(["b".to_string()]));
    assert_eq!(
        alpha.clone().union(beta),
        PlanIndexScope::Tasks(HashSet::from(["a".to_string(), "b".to_string()]))
    );
    assert_eq!(alpha.union(PlanIndexScope::Full), PlanIndexScope::Full);
}
