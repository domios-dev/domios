use super::*;
use crate::plan::{TaskExecutionReportSnapshot, TaskStatus};
use crate::service_plan::NewTask;
use std::sync::{Arc, Mutex as StdMutex};

fn reported_shared_commit() -> (tempfile::TempDir, PlanStore, String, String, String) {
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
    let checkout = root.path().to_str().unwrap().to_string();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = super::plan(&checkout);
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
    std::fs::write(root.path().join("owned"), "own change").unwrap();
    git(root.path(), &["add", "owned"]);
    git(root.path(), &["commit", "-m", "own"]);
    let reported = git(root.path(), &["rev-parse", "HEAD"]);
    store
        .edit_plan_fixture(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|candidate| candidate.id == task)
                .unwrap();
            task.status = TaskStatus::InReview;
            task.assigned_tab_id = Some("worker".into());
            task.execution_attempts.push(TaskExecutionAttempt {
                id: "opaque-attempt".into(),
                executor_tab_id: "worker".into(),
                placement: WorkerPlacement::Shared,
                checkout_path: Some(checkout.clone()),
                report: Some(TaskExecutionReportSnapshot {
                    id: "opaque-report".into(),
                    status: Some("in_review".into()),
                    sha: Some(reported.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    (root, store, plan.id, task, reported)
}

#[test]
fn reset_away_reported_shared_commit_records_one_incident_and_delivers_it_once() {
    let (root, store, plan, task, reported) = reported_shared_commit();
    let checkout = root.path().to_str().unwrap().to_string();
    let delivered = Arc::new(StdMutex::new(Vec::new()));
    let sink = delivered.clone();
    store.bind_shared_history_ports(SharedHistoryPorts {
        lost: Arc::new(move |incident| sink.lock().unwrap().push(incident.clone())),
        watch: Arc::new(|_| {}),
    });
    assert!(store
        .detect_lost_shared_commits(&checkout, 4)
        .unwrap()
        .is_empty());
    std::fs::write(root.path().join("foreign"), "foreign").unwrap();
    git(root.path(), &["add", "foreign"]);
    git(root.path(), &["commit", "-m", "foreign"]);
    assert!(store
        .detect_lost_shared_commits(&checkout, 5)
        .unwrap()
        .is_empty());
    let old_head = git(root.path(), &["rev-parse", "HEAD"]);
    git(root.path(), &["reset", "--hard", "HEAD~2"]);
    let new_head = git(root.path(), &["rev-parse", "HEAD"]);

    let incidents = store.detect_lost_shared_commits(&checkout, 6).unwrap();
    assert_eq!(
        incidents,
        [LostSharedCommit {
            plan_id: plan.clone(),
            task_id: task,
            attempt_id: "opaque-attempt".into(),
            commit: reported,
            checkout: checkout.clone(),
            old_head: Some(old_head),
            new_head,
            cause: "reset".into(),
        }]
    );
    assert!(store
        .detect_lost_shared_commits(&checkout, 7)
        .unwrap()
        .is_empty());
    let restarted = PlanStore::open(root.path().join(".domios").join("plans.db")).unwrap();
    assert!(restarted
        .detect_lost_shared_commits(&checkout, 8)
        .unwrap()
        .is_empty());

    assert_eq!(*delivered.lock().unwrap(), incidents);
    let strides: Vec<LostSharedCommit> = store
        .try_get(&plan)
        .unwrap()
        .strides
        .iter()
        .filter(|stride| stride.kind == LOST_COMMIT_STRIDE)
        .map(|stride| serde_json::from_str(&stride.text).unwrap())
        .collect();
    assert_eq!(strides, incidents);
}

#[test]
fn shared_report_inspection_retains_the_watch_and_detects_an_amend() {
    let (root, store, plan, task, reported) = reported_shared_commit();
    let checkout = root.path().to_str().unwrap().to_string();
    let watched = Arc::new(StdMutex::new(Vec::new()));
    let watch = watched.clone();
    store.bind_shared_history_ports(SharedHistoryPorts {
        lost: Arc::new(|_| {}),
        watch: Arc::new(move |checkout| watch.lock().unwrap().push(checkout.to_string())),
    });
    git(root.path(), &["commit", "--amend", "-m", "rewritten"]);
    let _ = store.inspect_shared_report(&plan, &task, "worker", Some(&reported), 4);
    assert_eq!(*watched.lock().unwrap(), [checkout.clone()]);
    let strides = store.try_get(&plan).unwrap().strides;
    let incident: LostSharedCommit = serde_json::from_str(
        &strides
            .iter()
            .find(|stride| stride.kind == LOST_COMMIT_STRIDE)
            .unwrap()
            .text,
    )
    .unwrap();
    assert_eq!(
        (incident.commit, incident.old_head, incident.cause.as_str()),
        (reported.clone(), Some(reported), "amend")
    );
}
