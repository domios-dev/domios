use std::path::Path;

use rusqlite::params;

use super::*;
use crate::plan::{OrchestratorRef, TaskStatus};
use crate::service::plan::{task_queries::TaskListFilter, NewTask, PlanStore};

fn store_and_root() -> (tempfile::TempDir, PlanStore) {
    let root = tempfile::tempdir().unwrap();
    (root, PlanStore::new_for_test(Path::new("/unused")))
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
                ..Default::default()
            },
            3,
        )
        .unwrap();

    let db = project_index_path(&super::super::paths::project_root(root.path()));
    assert!(db.exists());
    let hits = store
        .search_plan_index(
            root.path(),
            &search::SearchQuery {
                query: "plan button soft-lock".into(),
                kind: Some("task".into()),
                limit: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(hits[0].source, "plan");
    assert_eq!(hits[0].unit_id, task_id);
    assert!(hits[0]
        .body
        .contains("Plan \"Canvas\" · task: Fix soft lock"));
    assert_eq!(hits[0].scope, project_scope(root.path()));
}

#[test]
fn linked_worktree_uses_main_project_for_listing_search_and_index_scope() {
    let (_tmp, main, linked) = git_worktree_pair();
    let store = PlanStore::new_for_test(Path::new("/unused"));
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
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let loaded = store.get(&plan.id).unwrap();
    assert_eq!(loaded.project_path, main_root.display().to_string());
    assert!(main_root
        .join(".codeterm/plans")
        .join(format!("{}.json", plan.id))
        .exists());
    assert!(!linked.join(".codeterm/plans").exists());

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
            &search::SearchQuery {
                query: "unique linked worktree recall phrase".into(),
                kind: Some("task".into()),
                limit: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(hits
        .iter()
        .any(|hit| { hit.unit_id == task_id && hit.scope == project_scope(&main) }));

    let reindex = store.reindex_project_index(&linked).unwrap();
    assert_eq!(reindex.project, main_root.display().to_string());
    assert_eq!(reindex.scope, project_scope(&main));
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
                ..Default::default()
            },
            2,
        )
        .unwrap();
    assert!(!store
        .search_plan_index(
            root.path(),
            &search::SearchQuery {
                query: "vanishing phrase".into(),
                limit: Some(5),
                ..Default::default()
            },
        )
        .unwrap()
        .is_empty());

    store.rm_task(&plan.id, &task_id, 3).unwrap();
    let hits = store
        .search_plan_index(
            root.path(),
            &search::SearchQuery {
                query: "vanishing phrase".into(),
                limit: Some(5),
                ..Default::default()
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
                ..Default::default()
            },
            4,
        )
        .unwrap();

    let scope = project_scope(root.path());
    delete_units_by_scope(root.path(), &scope).unwrap();
    assert!(store
        .search_plan_index(
            root.path(),
            &search::SearchQuery {
                query: "plan button soft-lock".into(),
                limit: Some(5),
                ..Default::default()
            },
        )
        .unwrap()
        .is_empty());

    let result = store.reindex_project_index(root.path()).unwrap();
    assert_eq!(result.plans, 2);
    assert!(result.units >= 4);
    let hits = store
        .search_plan_index(
            root.path(),
            &search::SearchQuery {
                query: "plan button soft-lock".into(),
                limit: Some(5),
                ..Default::default()
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
fn archived_plan_removes_units_from_index() {
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
    assert!(!store
        .search_plan_index(
            root.path(),
            &search::SearchQuery {
                query: "archive phrase".into(),
                limit: Some(5),
                ..Default::default()
            },
        )
        .unwrap()
        .is_empty());

    store.archive(&plan.id, 3).unwrap();
    assert!(store
        .search_plan_index(
            root.path(),
            &search::SearchQuery {
                query: "archive phrase".into(),
                limit: Some(5),
                ..Default::default()
            },
        )
        .unwrap()
        .is_empty());
}

#[test]
fn related_traversal_is_dangling_safe() {
    let (root, store) = store_and_root();
    let plan = store
        .create_plan(
            "Related".into(),
            root.path().display().to_string(),
            OrchestratorRef::default(),
            vec![],
            1,
        )
        .unwrap();
    let section_id = store
        .add_section(&plan.id, "Section", "", None, &[], None, None, 2)
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Task".into(),
                kind: "fix".into(),
                section_ids: vec![section_id.clone(), "deleted_section".into()],
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .update_task(
            &plan.id,
            &task_id,
            None,
            None,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            4,
        )
        .unwrap();

    let groups = store.related_units(&task_id);
    let section_group = groups
        .iter()
        .find(|group| group.edge_type == "task_sections")
        .unwrap();
    assert!(section_group
        .units
        .iter()
        .any(|unit| unit.id == section_id && !unit.removed));
    assert!(section_group
        .units
        .iter()
        .any(|unit| unit.id == "deleted_section" && unit.removed));
    assert_eq!(
        store.get(&plan.id).unwrap().tasks[0].status,
        TaskStatus::InProgress
    );
}
