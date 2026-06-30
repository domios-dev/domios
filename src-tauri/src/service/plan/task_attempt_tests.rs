use super::*;
use crate::agent::reports::ReportStatus;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskLaneMetadata};

#[test]
fn task_attempt_history_builds_respawn_prompt_from_prior_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Respawn".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "retry lane".into(),
                kind: "implement".into(),
                model: Some("codex/gpt-5.5".into()),
                body: "Fix the task history respawn CLI.".into(),
                lane: TaskLaneMetadata {
                    worktree_path: Some("/tmp/worktree".into()),
                    branch: Some("lane/retry".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "pane_old", None, 3)
        .unwrap();
    let attempt = store.list_task_attempts(&p.id, &task_id).unwrap().remove(0);
    assert_ne!(attempt.attempt_id, "pane_old");
    assert!(attempt.attempt.executor_title.is_none());
    store
        .record_task_attempt_report(
            &p.id,
            &task_id,
            &AgentRef::from_pane("pane_old"),
            "STATUS: blocked\nFOLLOWUP: respawn needed",
            Some(ReportStatus::Blocked),
            4,
        )
        .unwrap();

    let draft = store
        .build_task_respawn_draft(
            &p.id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: attempt.attempt_id.clone(),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(draft.provider, "codex");
    assert_eq!(draft.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(draft.cwd, "/tmp/worktree");
    assert!(draft
        .task_prompt
        .contains("Fix the task history respawn CLI."));
    assert!(draft.task_prompt.contains("STATUS: blocked"));
    assert!(draft.task_prompt.contains("branch: lane/retry"));
    assert!(draft
        .task_prompt
        .contains(&format!("attempt_id: {}", attempt.attempt_id)));
    assert!(draft.task_prompt.contains(&format!(
        "codeterm agent report --plan {} --task {}",
        p.id, task_id
    )));
}

#[test]
fn task_attempt_ids_are_stable_distinct_from_executor_pane() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Attempts".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "same pane".into(),
                kind: "implement".into(),
                body: "Run twice in same pane.".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .assign_task(&p.id, &task_id, "pane_same", None, 3)
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "pane_same", None, 4)
        .unwrap();

    let attempts = store.list_task_attempts(&p.id, &task_id).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_ne!(attempts[0].attempt_id, attempts[1].attempt_id);
    assert_ne!(attempts[0].attempt_id, "pane_same");
    assert_eq!(attempts[0].attempt.executor_pane_id, "pane_same");
    assert_eq!(attempts[1].attempt.executor_pane_id, "pane_same");
    assert!(attempts[0].attempt.executor_title.is_none());
    assert!(attempts[1].attempt.executor_title.is_none());

    let shown = store
        .show_task_attempt(&p.id, &task_id, &attempts[0].attempt_id)
        .unwrap();
    assert_eq!(shown.attempt_id, attempts[0].attempt_id);
    assert!(store
        .show_task_attempt(&p.id, &task_id, "pane_same")
        .is_err());
}
