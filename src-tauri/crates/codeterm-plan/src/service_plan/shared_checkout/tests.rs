use super::*;
use crate::plan::{OrchestratorRef, TaskExecutionAttempt};
use std::process::Command;
mod candidates;
mod inspection;

fn task(id: &str, edits: &[&str], live: bool) -> Task {
    let mut task: Task = serde_json::from_value(serde_json::json!({
        "id": id, "name": id, "type": "code", "status": "pending",
        "files": {"edit": edits}
    }))
    .unwrap();
    if live {
        task.execution_attempts.push(TaskExecutionAttempt {
            id: format!("attempt-{id}"),
            placement: WorkerPlacement::Shared,
            ..Default::default()
        });
    }
    task
}

fn plan(path: &str) -> Plan {
    Plan::new(
        "fixture".into(),
        path.into(),
        OrchestratorRef::from_tab("owner"),
    )
}

#[test]
fn shared_placement_defaults_to_disjoint_files_and_reports_lane_reasons() {
    let mut plan = plan("/checkout");
    let mine = task("mine", &["src/a.rs"], false);
    plan.tasks.push(task("foreign", &["src/b.rs"], true));
    assert_eq!(
        choose_placement(&[plan.clone()], &plan, &mine, None)
            .unwrap()
            .0,
        WorkerPlacement::Shared
    );
    assert_eq!(
        choose_placement(&[plan.clone()], &plan, &mine, Some("lane"))
            .unwrap()
            .0,
        WorkerPlacement::IsolatedLane
    );
    plan.tasks[0].files.edit = vec!["src".into()];
    let decision = choose_placement(&[plan.clone()], &plan, &mine, None).unwrap();
    assert_eq!(decision.0, WorkerPlacement::IsolatedLane);
    assert!(decision.1.contains("foreign"));
    assert!(
        choose_placement(&[plan.clone()], &plan, &mine, Some("shared"))
            .unwrap_err()
            .contains("foreign")
    );
    let mut breaking = mine.clone();
    breaking.tags.push("build-breaking".into());
    assert!(choose_placement(&[], &plan, &breaking, None)
        .unwrap()
        .1
        .contains("build-breaking"));
    assert!(choose_placement(&[], &plan, &breaking, Some("shared")).is_err());
    assert_eq!(
        choose_placement(&[], &plan, &breaking, Some("lane"))
            .unwrap()
            .0,
        WorkerPlacement::IsolatedLane
    );
    assert!(choose_placement(&[], &plan, &task("empty", &[], false), None).is_err());
}

#[test]
fn missing_shared_files_provides_runnable_declaration_and_lane_commands() {
    let plan = plan("/checkout");
    let task = task("opaque-task", &[], false);
    let error = choose_placement(&[], &plan, &task, None).unwrap_err();
    assert!(error.contains(&format!(
        "domios plan task files {} {} --edit-file <path>",
        plan.id, task.id
    )));
    assert!(error.contains(&format!(
        "domios plan task dispatch {} {} --execution-intent mutating --placement lane",
        plan.id, task.id
    )));
}

#[test]
fn shared_ownership_checks_prefix_boundaries_cross_plan_and_closed_attempts() {
    let plan = plan("/checkout");
    let mine = task("mine", &["src"], false);
    let mut foreign = self::plan("/checkout");
    foreign.id = "other-plan".into();
    foreign.tasks.push(task("foreign", &["src/file.rs"], true));
    assert!(
        validate_shared_files(&[foreign.clone()], &plan, &mine, &mine.files)
            .unwrap_err()
            .contains("other-plan")
    );
    foreign.tasks[0].files.edit = vec!["src-other/file.rs".into()];
    assert!(validate_shared_files(&[foreign.clone()], &plan, &mine, &mine.files).is_ok());
    foreign.tasks[0].files.edit = vec!["src/file.rs".into()];
    foreign.tasks[0].execution_attempts[0].closed_at = Some(4);
    assert!(validate_shared_files(&[foreign], &plan, &mine, &mine.files).is_ok());
    for edit in ["../src", "/src", "C:/src", "src/../foreign", ".git/hooks"] {
        assert!(task("invalid", &[edit], false)
            .files
            .shared_edit_paths()
            .is_err());
    }
}

#[test]
#[cfg(unix)]
fn automatic_placement_refuses_linked_edit_paths_instead_of_isolating_an_external_target() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(external.path(), root.path().join("linked")).unwrap();
    let plan = self::plan(root.path().to_str().unwrap());
    let task = task("mine", &["linked/file.rs"], false);
    assert!(choose_placement(&[], &plan, &task, None)
        .unwrap_err()
        .contains("filesystem link"));
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn shared_reservation_serializes_overlap_and_extension_checks() {
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
    let store = std::sync::Arc::new(PlanStore::new_for_test(root.path()));
    let mut plan = self::plan(root.path().to_str().unwrap());
    store.save(&mut plan, 1).unwrap();
    let ids: Vec<String> = ["first", "second"]
        .into_iter()
        .map(|name| {
            store
                .add_task(
                    &plan.id,
                    super::super::NewTask {
                        name: name.into(),
                        files: TaskFiles {
                            edit: vec!["shared.rs".into()],
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    2,
                )
                .unwrap()
        })
        .collect();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ids
        .iter()
        .map(|id| {
            let store = store.clone();
            let barrier = barrier.clone();
            let plan_id = plan.id.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let result = store.begin_task_dispatch_with_reserved_attempt_and_gates(
                    &plan_id,
                    &id,
                    &id,
                    "Fixture",
                    Default::default(),
                    crate::plan::ExecutionIntent::Mutating,
                    None,
                    None,
                    &format!("attempt-{id}"),
                    vec![],
                    WorkerPlacement::Shared,
                    3,
                );
                (id, result)
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        outcomes.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    let winner = &outcomes
        .iter()
        .find(|(_, result)| result.is_ok())
        .unwrap()
        .0;
    let loser = &outcomes
        .iter()
        .find(|(_, result)| result.is_err())
        .unwrap()
        .0;
    store
        .update_task_files(
            &plan.id,
            loser,
            TaskFiles {
                edit: vec!["foreign.rs".into()],
                ..Default::default()
            },
            4,
        )
        .unwrap();
    store
        .begin_task_dispatch_with_reserved_attempt_and_gates(
            &plan.id,
            loser,
            loser,
            "Fixture",
            Default::default(),
            crate::plan::ExecutionIntent::Mutating,
            None,
            None,
            "second-attempt",
            vec![],
            WorkerPlacement::Shared,
            5,
        )
        .unwrap();
    assert!(store
        .extend_task_edit_files(&plan.id, winner, vec!["foreign.rs".into()], 6)
        .is_err());
    store
        .extend_task_edit_files(&plan.id, winner, vec!["new.rs".into()], 7)
        .unwrap();
    let task = store.show_task(winner).unwrap().task;
    assert_eq!(task.files.edit, ["shared.rs", "new.rs"]);
    assert!(store
        .update_task_files(
            &plan.id,
            winner,
            TaskFiles {
                edit: vec!["foreign.rs".into()],
                ..Default::default()
            },
            8
        )
        .is_err());
    assert_eq!(store.show_task(winner).unwrap().task.files, task.files);
    let error = store
        .update_task_files(
            &plan.id,
            winner,
            TaskFiles {
                edit: vec!["new.rs".into()],
                ..Default::default()
            },
            9,
        )
        .unwrap_err();
    assert!(error.to_string().contains("cannot release edit paths"));
    assert_eq!(store.show_task(winner).unwrap().task.files, task.files);
}

#[test]
fn shared_commit_guard_accepts_own_commit_with_foreign_dirt_and_refuses_foreign_or_undeclared_paths(
) {
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
    let mut attempt = TaskExecutionAttempt {
        checkout_path: Some(root.path().display().to_string()),
        shared_start_sha: Some(git(root.path(), &["rev-parse", "HEAD"])),
        ..Default::default()
    };
    let mine = task("mine", &["owned"], false);
    std::fs::write(root.path().join("owned"), "own change").unwrap();
    git(root.path(), &["add", "owned"]);
    git(root.path(), &["commit", "-m", "own"]);
    let own = git(root.path(), &["rev-parse", "HEAD"]);
    std::fs::write(root.path().join("foreign"), "foreign uncommitted").unwrap();
    assert_eq!(
        validate_shared_report(&mine, &attempt, Some(&own)).unwrap(),
        ["owned"]
    );
    git(root.path(), &["add", "foreign"]);
    git(root.path(), &["commit", "-m", "foreign"]);
    let foreign = git(root.path(), &["rev-parse", "HEAD"]);
    assert!(validate_shared_report(&mine, &attempt, Some(&foreign))
        .unwrap_err()
        .contains("foreign"));
    assert!(validate_shared_report(&mine, &attempt, Some(&own)).is_ok());
    std::fs::write(root.path().join("undeclared"), "undeclared").unwrap();
    git(root.path(), &["add", "undeclared"]);
    git(root.path(), &["commit", "-m", "undeclared"]);
    assert!(validate_shared_report(
        &mine,
        &attempt,
        Some(&git(root.path(), &["rev-parse", "HEAD"]))
    )
    .unwrap_err()
    .contains("undeclared"));
    std::fs::write(root.path().join("owned"), "unfinished").unwrap();
    assert!(validate_shared_report(&mine, &attempt, Some(&own))
        .unwrap_err()
        .contains("uncommitted"));
    attempt.shared_start_sha = Some(own.clone());
    assert!(validate_shared_report(&mine, &attempt, Some(&own)).is_err());
}
