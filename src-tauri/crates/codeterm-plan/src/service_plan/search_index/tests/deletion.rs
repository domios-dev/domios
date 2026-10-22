use super::*;

#[test]
fn busy_project_accepts_deletion_without_executing_index_work() {
    let queue = IndexQueue {
        state: Mutex::new(IndexQueueState::default()),
        ready: Condvar::new(),
    };
    let plan = Plan::new(
        "Delete".into(),
        "/decision-project".into(),
        OrchestratorRef::default(),
    );
    let project = project_scope(plan_project_root(&plan));
    queue
        .state
        .lock()
        .in_flight_projects
        .insert(project.clone());
    queue.enqueue(IndexJob::PurgePlan(plan.clone(), vec![]));
    let state = queue.state.lock();
    assert_eq!(state.in_flight, 0);
    assert_eq!(state.order.len(), 1);
    assert!(
        matches!(state.latest.get(&format!("{project}:purge:{}", plan.id)), Some(IndexJob::PurgePlan(snapshot, _)) if snapshot.id == plan.id)
    );
}
