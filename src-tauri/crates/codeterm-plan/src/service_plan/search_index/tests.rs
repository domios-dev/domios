use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use rusqlite::params;

use super::*;
use crate::plan::{AgentRef, OrchestratorRef, PlanMode, PlannerRef, TaskStatus};
use crate::service_plan::ports::PlanIndexQuery;
use crate::service_plan::{task_queries::TaskListFilter, NewTask, PlanStore};

fn store_and_root() -> (tempfile::TempDir, PlanStore) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    (root, store)
}

fn run_git(cwd: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_worktree_pair() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    let linked = tmp.path().join("linked");
    std::fs::create_dir(&main).unwrap();
    run_git(&main, &["init"]);
    run_git(&main, &["config", "user.email", "test@example.com"]);
    run_git(&main, &["config", "user.name", "Test User"]);
    std::fs::write(main.join("README.md"), "root\n").unwrap();
    run_git(&main, &["add", "README.md"]);
    run_git(&main, &["commit", "-m", "init"]);
    run_git(
        &main,
        &[
            "worktree",
            "add",
            linked.to_str().unwrap(),
            "-b",
            "linked-test",
        ],
    );
    (tmp, main, linked)
}

fn wait_for_vector_match_count(db: &Path, like_pattern: &str, min_count: i64) -> i64 {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let conn = rusqlite::Connection::open(db).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_vectors WHERE mem_id LIKE ?1",
                params![like_pattern],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if count >= min_count || std::time::Instant::now() >= deadline {
            return count;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn vector_indexing_available(_db: &Path) -> bool {
    // TestSearchPort has no-op embed; vector indexing is unavailable in unit tests
    false
}

fn hold_plan_index_lock(root: &Path) -> std::thread::JoinHandle<()> {
    let project = super::super::paths::project_root(root);
    let (ready_tx, ready_rx) = mpsc::channel();
    let locker = std::thread::spawn(move || {
        let conn = open_index(&project).unwrap();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        ready_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(225));
    });
    ready_rx.recv().unwrap();
    locker
}

#[test]
fn mirrors_plan_units_into_project_local_mem_db() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Canvas".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let section_id = store
        .add_section(
            &plan.id,
            "Soft lock",
            "plan button soft-lock regression",
            None,
            &[],
            None,
            None,
            2,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Fix soft lock".into(),
                kind: "fix".into(),
                body: "workspace tab plan button soft-lock".into(),
                section_ids: vec![section_id],
                competition: None,
                ..Default::default()
            },
            3,
        )
        .unwrap();

    flush_index_queue_for_test(root.path());

    let db = project_index_path(&super::super::paths::project_root(root.path()));
    assert!(db.exists());
    let expected_index_root = root
        .path()
        .canonicalize()
        .unwrap()
        .join(".codeterm/test-plan-index");
    assert!(
        db.starts_with(&expected_index_root),
        "index path {db:?} escaped test root {:?}",
        root.path()
    );
    assert!(!root.path().join(".codeterm/plan-index.sqlite").exists());
    let conn = rusqlite::Connection::open(&db).unwrap();
    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_fts", [], |row| row.get(0))
        .unwrap();
    assert!(fts_count > 0);
    if vector_indexing_available(&db) {
        assert!(
            wait_for_vector_match_count(&db, &format!("%{task_id}%"), 1) > 0,
            "expected vectors for task unit when embeddings are available"
        );
    }
    let hits = search_project(
        root.path(),
        &PlanIndexQuery {
            query: "plan button soft-lock".into(),
            scope: None,
            limit: Some(5),
        },
        PlanSearchStatus::Active,
    )
    .unwrap();
    assert!(!hits.is_empty());
    let task_hit = hits
        .iter()
        .find(|h| h.unit_id == task_id)
        .expect("task should appear in search results");
    assert_eq!(task_hit.source, "plan");
    assert!(task_hit
        .body
        .contains("Plan \"Canvas\" · task: Fix soft lock"));
    assert_eq!(task_hit.scope, project_scope(root.path()));
    assert_eq!(task_hit.updated_at, 3);
    assert!(task_hit.age_ms > 0);
    let encoded = serde_json::to_value(task_hit).unwrap();
    assert_eq!(encoded["updated_at"], serde_json::json!(3));
    assert!(encoded["age_ms"].as_u64().is_some());
}

#[test]
fn changing_one_task_reembeds_only_that_task_unit() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Incremental index".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Only this task changes".into(),
                kind: "fix".into(),
                body: "initial task body".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    flush_index_queue_for_test(root.path());

    let mut changed_plan = store.get(&plan.id).unwrap();
    let task = changed_plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.body = "changed task body".into();
    task.updated_at = 3;
    changed_plan.updated_at = 3;

    let changed = replace_units_once(
        root.path(),
        &format!("planunit_{}_%", changed_plan.id),
        &plan_units(&changed_plan, &project_scope(root.path())),
        false,
    )
    .unwrap();

    assert_eq!(
        changed
            .iter()
            .map(|unit| unit.id.as_str())
            .collect::<Vec<_>>(),
        vec![format!("planunit_{}_task_{task_id}", changed_plan.id)]
    );
}

#[test]
fn changing_one_section_reembeds_only_that_section_unit() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Incremental sections".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let section_id = store
        .add_section(
            &plan.id,
            "One section",
            "initial section body",
            None,
            &[],
            None,
            None,
            2,
        )
        .unwrap();
    flush_index_queue_for_test(root.path());

    let mut changed_plan = store.get(&plan.id).unwrap();
    changed_plan.sections[0].body = "changed section body".into();
    changed_plan.updated_at = 3;

    let changed = replace_units_once(
        root.path(),
        &format!("planunit_{}_%", changed_plan.id),
        &plan_units(&changed_plan, &project_scope(root.path())),
        false,
    )
    .unwrap();

    assert_eq!(
        changed
            .iter()
            .map(|unit| unit.id.as_str())
            .collect::<Vec<_>>(),
        vec![format!(
            "planunit_{}_plan_section_{section_id}",
            changed_plan.id
        )]
    );
}

#[test]
fn changing_one_spec_section_reembeds_only_that_spec_section_unit() {
    let (root, store) = store_and_root();
    let spec = store
        .create_spec(
            "Incremental spec".into(),
            root.path().display().to_string(),
            1,
        )
        .unwrap();
    let section_id = store
        .add_spec_section(
            &spec.id,
            "One spec section",
            "initial spec body",
            None,
            &[],
            None,
            None,
            2,
        )
        .unwrap();
    flush_index_queue_for_test(root.path());

    let mut changed_spec = store.get_spec(&spec.id).unwrap();
    changed_spec.sections[0].body = "changed spec body".into();
    changed_spec.updated_at = 3;

    let changed = replace_units_once(
        root.path(),
        &format!("planunit_{}_%", changed_spec.id),
        &spec_units(&changed_spec, &project_scope(root.path())),
        false,
    )
    .unwrap();

    assert_eq!(
        changed
            .iter()
            .map(|unit| unit.id.as_str())
            .collect::<Vec<_>>(),
        vec![format!(
            "planunit_{}_spec_section_{section_id}",
            changed_spec.id
        )]
    );
}

#[test]
fn index_queue_keeps_only_the_latest_snapshot_per_document() {
    let queue = IndexQueue {
        state: Mutex::new(IndexQueueState::default()),
        ready: Condvar::new(),
    };
    let mut first = crate::plan::Plan::new(
        "First title".into(),
        "project".into(),
        OrchestratorRef::default(),
    );
    first.id = "pl_queue".into();
    let mut latest = first.clone();
    latest.title = "Latest title".into();

    queue.enqueue(IndexJob::Plan(first));
    queue.enqueue(IndexJob::Plan(latest));

    let state = queue.state.lock();
    assert_eq!(state.order.len(), 1);
    assert_eq!(state.latest.len(), 1);
    let IndexJob::Plan(snapshot) = state
        .latest
        .get(&format!(
            "{}:plan:pl_queue",
            project_scope(Path::new("project"))
        ))
        .unwrap()
    else {
        panic!("expected a plan snapshot");
    };
    assert_eq!(snapshot.title, "Latest title");
}

#[test]
fn add_task_succeeds_when_plan_index_is_locked_after_verified_commit() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Concurrent writes".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let locker = hold_plan_index_lock(root.path());

    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Add under contention".into(),
                kind: "fix".into(),
                body: "task added while plan index is locked".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    assert!(task_id.starts_with("task_"));

    let visible = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .filter(|task| task.name == "Add under contention")
        .collect::<Vec<_>>();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, task_id);
    assert_eq!(visible[0].body, "task added while plan index is locked");
    locker.join().unwrap();
}

#[test]
fn close_task_immediate_readback_is_consistent() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Close readback".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Close me".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .close_task(
            &plan.id,
            &task_id,
            Some("verified close"),
            AgentRef::from_pane("worker"),
            3,
        )
        .unwrap();

    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.status, TaskStatus::Done);
    assert!(task
        .milestones
        .iter()
        .any(|milestone| milestone.text == "verified close"));
}

#[test]
fn locked_plan_index_does_not_duplicate_task_by_retrying_unsafely() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "No duplicate retry".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let locker = hold_plan_index_lock(root.path());

    let committed_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Exactly once".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    locker.join().unwrap();

    let tasks = store.get(&plan.id).unwrap().tasks;
    let committed = tasks
        .iter()
        .find(|task| task.id == committed_id)
        .expect("committed task is readable by diagnostic id");
    assert_eq!(committed.name, "Exactly once");
    let matching = tasks
        .iter()
        .filter(|task| task.name == "Exactly once")
        .count();
    assert_eq!(matching, 1);
}

#[test]
fn locked_task_update_succeeds_after_verified_commit() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Update lock".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Update me".into(),
                kind: "fix".into(),
                body: "before".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let locker = hold_plan_index_lock(root.path());

    store
        .update_task(
            &plan.id,
            &task_id,
            Some("Updated under contention"),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("after"),
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            3,
        )
        .unwrap();
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.name, "Updated under contention");
    assert_eq!(task.body, "after");
    locker.join().unwrap();
}

#[test]
fn locked_task_lane_update_succeeds_after_verified_commit() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Lane lock".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Lane task".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let locker = hold_plan_index_lock(root.path());

    store
        .update_task_lane(
            &plan.id,
            &task_id,
            crate::service_plan::TaskLaneUpdate {
                pane_id: Some(Some("pane_1".into())),
                workspace_id: Some(Some("ws_1".into())),
                worktree_path: Some(Some("/tmp/worktree".into())),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.lane.pane_id.as_deref(), Some("pane_1"));
    assert_eq!(task.lane.workspace_id.as_deref(), Some("ws_1"));
    assert_eq!(task.lane.worktree_path.as_deref(), Some("/tmp/worktree"));
    locker.join().unwrap();
}

#[test]
fn locked_task_record_succeeds_after_verified_milestone() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Record lock".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Record me".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let locker = hold_plan_index_lock(root.path());

    let milestone_id = store
        .add_milestone(
            &plan.id,
            &task_id,
            "decision",
            "recorded while plan index is locked",
            AgentRef::from_pane("worker"),
            true,
            3,
        )
        .unwrap();
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    let milestone = task
        .milestones
        .iter()
        .find(|milestone| milestone.id == milestone_id)
        .unwrap();
    assert_eq!(milestone.kind, "decision");
    assert_eq!(milestone.text, "recorded while plan index is locked");
    locker.join().unwrap();
}

#[test]
fn assign_task_readback_sets_pane_lane_and_peer() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Assign readback".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Assign me".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .assign_task(&plan.id, &task_id, "pane_2", Some("mac"), 3)
        .unwrap();

    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.assigned_pane_id.as_deref(), Some("pane_2"));
    assert_eq!(task.lane.pane_id.as_deref(), Some("pane_2"));
    assert_eq!(task.peer.as_deref(), Some("mac"));
}

#[test]
fn locked_task_take_succeeds_after_verified_assignment() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Take lock".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Take me".into(),
                kind: "fix".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let locker = hold_plan_index_lock(root.path());

    store
        .set_task_assignment(&task_id, Some("pane_take"), Some("worker"), 3)
        .unwrap();
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.assigned_pane_id.as_deref(), Some("pane_take"));
    assert_eq!(task.taken_by_role.as_deref(), Some("worker"));
    locker.join().unwrap();
}

#[test]
fn locked_heartbeat_set_succeeds_after_safe_readback() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Heartbeat lock".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 2).unwrap();
    let locker = hold_plan_index_lock(root.path());

    let result = store
        .set_heartbeat(
            Some(&plan.id),
            None,
            None,
            Some("Keep mirror failures non-fatal"),
            Some(vec!["backend cli".into()]),
            Some(vec!["read back before retry".into()]),
            false,
            false,
            3,
        )
        .unwrap();
    assert_eq!(
        result.heartbeat.current_focus.as_deref(),
        Some("Keep mirror failures non-fatal")
    );
    assert_eq!(result.heartbeat.active_lanes, vec!["backend cli"]);
    assert_eq!(result.heartbeat.rules, vec!["read back before retry"]);
    locker.join().unwrap();
}

#[test]
fn task_drop_readback_clears_assignment_role_and_peer() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Drop assignment".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Drop me".into(),
                kind: "fix".into(),
                assigned_pane_id: Some("pane_drop".into()),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "pane_drop", Some("mac"), 3)
        .unwrap();

    store.set_task_assignment(&task_id, None, None, 4).unwrap();

    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.assigned_pane_id, None);
    assert_eq!(task.taken_by_role, None);
    assert_eq!(task.peer, None);
}

#[test]
fn linked_worktree_uses_main_project_for_listing_search_and_index_scope() {
    let (tmp, main, linked) = git_worktree_pair();
    let store_root = tmp.path().join("store");
    std::fs::create_dir(&store_root).unwrap();
    let store = PlanStore::new_for_test(&store_root);
    let main_root = super::super::paths::project_root(&main);
    let linked_root = super::super::paths::project_root(&linked);

    assert_eq!(main_root, linked_root);
    assert_eq!(project_scope(&main), project_scope(&linked));
    assert_eq!(project_index_path(&main), project_index_path(&linked));

    let plan = store
        .create_plan(
            "Central canvas".into(),
            main.display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Shared worktree task".into(),
                kind: "fix".into(),
                body: "unique linked worktree recall phrase".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.project_path, main_root.display().to_string());
    assert!(store.database_path_for_test().exists());
    assert!(!main_root
        .join(format!(".codeterm/plans/active/{}/plan.yaml", plan.id))
        .exists());
    assert!(!linked.join(".codeterm/plans").exists());
    flush_index_queue_for_test(&main_root);

    let tasks = store.list_tasks(TaskListFilter {
        project_path: Some(linked.display().to_string()),
        ..Default::default()
    });
    assert!(tasks
        .iter()
        .any(|task| task.plan_id == plan.id && task.task.id == task_id));

    let hits = store
        .search_plan_index(
            &linked,
            &PlanIndexQuery {
                query: "unique linked worktree recall phrase".into(),
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap();
    assert!(hits
        .iter()
        .any(|hit| { hit.unit_id == task_id && hit.scope == project_scope(&main) }));

    delete_plan_units(&loaded).unwrap();
    let deleted_hits = store
        .search_plan_index(
            &linked,
            &PlanIndexQuery {
                query: "unique linked worktree recall phrase".into(),
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap();
    assert!(deleted_hits.is_empty());

    let reindex = store.reindex_project_index(&linked).unwrap();
    assert_eq!(reindex.project, main_root.display().to_string());
    assert_eq!(reindex.scope, project_scope(&main));
    assert_eq!(reindex.plans, 1);
}

#[test]
fn reindex_removes_deleted_units_and_old_text() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Recall".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Old".into(),
                kind: "fix".into(),
                body: "vanishing phrase".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    flush_index_queue_for_test(root.path());
    assert!(!store
        .search_plan_index(
            root.path(),
            &PlanIndexQuery {
                query: "vanishing phrase".into(),
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap()
        .is_empty());
    let db = project_index_path(&super::super::paths::project_root(root.path()));
    if vector_indexing_available(&db) {
        assert!(
            wait_for_vector_match_count(&db, &format!("%{task_id}%"), 1) > 0,
            "expected vectors for task unit before deletion when embeddings are available"
        );
    }

    store.rm_task(&plan.id, &task_id, 3).unwrap();
    flush_index_queue_for_test(root.path());
    let hits = store
        .search_plan_index(
            root.path(),
            &PlanIndexQuery {
                query: "vanishing phrase".into(),
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap();
    assert!(hits.is_empty());
    let conn = open_index(root.path()).unwrap();
    let vectors: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_vectors WHERE mem_id LIKE ?1",
            params![format!("%{task_id}%")],
            |r| r.get(0),
        )
        .unwrap_or(0);
    assert_eq!(vectors, 0);
}

#[test]
fn explicit_reindex_backfills_multiple_plans_for_project_search() {
    let (root, store) = store_and_root();
    let first = store
        .create_plan(
            "Dogfood acceptance".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    store
        .add_task(
            &first.id,
            NewTask {
                name: "Workspace plan button soft-lock".into(),
                kind: "fix".into(),
                body: "Fix the plan button soft-lock in workspace tabs".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let second = store
        .create_plan(
            "Other canvas work".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            3,
        )
        .unwrap();
    store
        .add_task(
            &second.id,
            NewTask {
                name: "Unrelated task".into(),
                kind: "fix".into(),
                body: "Keep another plan in the same project".into(),
                competition: None,
                ..Default::default()
            },
            4,
        )
        .unwrap();

    let scope = project_scope(root.path());
    flush_index_queue_for_test(root.path());
    delete_units_by_scope(root.path(), &scope).unwrap();
    assert!(store
        .search_plan_index(
            root.path(),
            &PlanIndexQuery {
                query: "plan button soft-lock".into(),
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap()
        .is_empty());

    let result = store.reindex_project_index(root.path()).unwrap();
    flush_index_queue_for_test(root.path());
    assert_eq!(result.plans, 2);
    assert!(result.units >= 4);
    let hits = store
        .search_plan_index(
            root.path(),
            &PlanIndexQuery {
                query: "plan button soft-lock".into(),
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap();

    assert!(hits.iter().any(|hit| {
        hit.source == "plan"
            && hit.kind == "task"
            && hit.unit_title == "Workspace plan button soft-lock"
    }));
}

#[test]
fn archived_plan_stays_searchable_only_when_history_is_requested() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Archive me".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    store
        .add_section(
            &plan.id,
            "Keep out",
            "archive phrase",
            None,
            &[],
            None,
            None,
            2,
        )
        .unwrap();
    flush_index_queue_for_test(root.path());
    assert!(!store
        .search_plan_index(
            root.path(),
            &PlanIndexQuery {
                query: "archive phrase".into(),
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap()
        .is_empty());

    store.archive(&plan.id, 3).unwrap();
    flush_index_queue_for_test(root.path());
    assert!(store
        .search_plan_index(
            root.path(),
            &PlanIndexQuery {
                query: "archive phrase".into(),
                scope: None,
                limit: Some(5),
            },
        )
        .unwrap()
        .is_empty());
    let archived_hits = store
        .search_plan_index_with_status(
            root.path(),
            &PlanIndexQuery {
                query: "archive phrase".into(),
                scope: None,
                limit: Some(5),
            },
            PlanSearchStatus::Archived,
        )
        .unwrap();
    assert!(archived_hits
        .iter()
        .any(|hit| hit.status.as_deref() == Some("archived")));
    let all_hits = store
        .search_plan_index_with_status(
            root.path(),
            &PlanIndexQuery {
                query: "archive phrase".into(),
                scope: None,
                limit: Some(5),
            },
            PlanSearchStatus::All,
        )
        .unwrap();
    assert!(!all_hits.is_empty());
}

#[path = "readback_lock_tests.rs"]
mod readback_lock_tests;
