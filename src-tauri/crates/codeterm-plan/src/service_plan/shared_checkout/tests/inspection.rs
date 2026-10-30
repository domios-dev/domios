use super::*;
use crate::plan::{AgentRef, ExecutionIntent, TaskStatus};
use crate::service_plan::{task_attempts::BoundWorkerReport, NewTask};
use codeterm_types::agent::ReportStatus;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

fn fixture() -> (tempfile::TempDir, Arc<PlanStore>, String, String) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init"]);
    git(root.path(), &["config", "user.name", "Fixture"]);
    git(
        root.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(root.path().join("initial"), "initial").unwrap();
    git(root.path(), &["add", "initial"]);
    git(root.path(), &["commit", "-m", "initial"]);
    let store = Arc::new(PlanStore::new_for_test(root.path()));
    let mut plan = super::plan(root.path().to_str().unwrap());
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "opaque".into(),
                files: TaskFiles {
                    edit: vec!["owned".into()],
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    (root, store, plan.id, task)
}

fn writer_is_free(store: &PlanStore) {
    let state = store.mutation_writer.state.lock();
    assert_eq!(
        state.next_ticket, state.serving_ticket,
        "Git inspection holds the plan mutation writer"
    );
}

fn begin(store: &PlanStore, plan: &str, task: &str) -> Result<String, PlanError> {
    store.begin_task_dispatch_with_reserved_attempt_and_gates(
        plan,
        task,
        "worker",
        "Fixture",
        Default::default(),
        ExecutionIntent::Mutating,
        None,
        None,
        "opaque-attempt",
        vec![],
        WorkerPlacement::Shared,
        3,
    )
}

#[test]
fn git_inspection_keeps_writer_free_and_report_refuses_files_changed_during_inspection() {
    let (root, store, plan, task) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let weak = Arc::downgrade(&store);
    let observed = calls.clone();
    *store.shared_git_observer.write() = Some(Arc::new(move |_, _| {
        writer_is_free(&weak.upgrade().unwrap());
        observed.fetch_add(1, Ordering::SeqCst);
    }));
    begin(&store, &plan, &task).unwrap();
    assert!(calls.load(Ordering::SeqCst) > 0);
    std::fs::write(root.path().join("owned"), "own change").unwrap();
    git(root.path(), &["add", "owned"]);
    git(root.path(), &["commit", "-m", "own"]);
    let sha = git(root.path(), &["rev-parse", "HEAD"]);
    store
        .with_plan(&plan, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|candidate| candidate.id == task)
                .unwrap();
            task.status = TaskStatus::InProgress;
            task.interruption.owner_epoch = 0;
            Ok(())
        })
        .unwrap();
    let changed = Arc::new(AtomicBool::new(false));
    let weak = Arc::downgrade(&store);
    let plan_id = plan.clone();
    let task_id = task.clone();
    let observed = calls.clone();
    let modified = changed.clone();
    *store.shared_git_observer.write() = Some(Arc::new(move |_, _| {
        let store = weak.upgrade().unwrap();
        writer_is_free(&store);
        observed.fetch_add(1, Ordering::SeqCst);
        if !modified.swap(true, Ordering::SeqCst) {
            store
                .extend_task_edit_files(&plan_id, &task_id, vec!["added".into()], 5)
                .unwrap();
        }
    }));
    let error = store
        .ingest_bound_worker_report(
            &plan,
            &task,
            &AgentRef::from_tab("worker"),
            BoundWorkerReport {
                status: Some(ReportStatus::InReview),
                sha: Some(sha),
                ..Default::default()
            },
            6,
        )
        .unwrap_err();
    assert!(changed.load(Ordering::SeqCst));
    assert!(calls.load(Ordering::SeqCst) > 1);
    assert!(error
        .to_string()
        .contains("context changed during Git inspection"));
    let current = store.show_task(&task).unwrap().task;
    assert_eq!(current.status, TaskStatus::InProgress);
    assert!(current.execution_attempts[0].report.is_none());
    assert_eq!(current.files.edit, ["owned", "added"]);
}

#[test]
fn dispatch_refuses_files_changed_while_reading_head_without_creating_an_attempt() {
    let (_root, store, plan, task) = fixture();
    let weak = Arc::downgrade(&store);
    let plan_id = plan.clone();
    let task_id = task.clone();
    *store.shared_git_observer.write() = Some(Arc::new(move |_, _| {
        let store = weak.upgrade().unwrap();
        writer_is_free(&store);
        store
            .update_task_files(
                &plan_id,
                &task_id,
                TaskFiles {
                    edit: vec!["changed".into()],
                    ..Default::default()
                },
                3,
            )
            .unwrap();
    }));
    let error = begin(&store, &plan, &task).unwrap_err();
    assert!(error
        .to_string()
        .contains("context changed during Git inspection"));
    assert!(store
        .show_task(&task)
        .unwrap()
        .task
        .execution_attempts
        .is_empty());
}
