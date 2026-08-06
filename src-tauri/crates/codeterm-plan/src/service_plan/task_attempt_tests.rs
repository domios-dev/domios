use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskLaneMetadata, TaskStatus};
use codeterm_types::agent::ReportStatus;
use std::sync::{Arc, Barrier};

mod plan_skill_evidence;

fn bound_report_store() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Worker reports".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "bound task".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["works".into()],
                test_plan: Some("cargo test -p codeterm-plan worker_report".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "worker", None, 3)
        .unwrap();
    (dir, store, plan.id, task_id)
}

fn worker_report(status: ReportStatus, sha: Option<&str>) -> task_attempts::BoundWorkerReport {
    task_attempts::BoundWorkerReport {
        message: "CHANGES: report ingestion\nTESTS RUN: cargo test".into(),
        status: Some(status),
        branch: Some("lane/worker".into()),
        sha: sha.map(str::to_string),
        changed_files: vec!["src/report.rs".into()],
        test_cmd: Some("cargo test".into()),
        test_result: Some("ok".into()),
        skipped: None,
        authority: None,
    }
}

#[test]
fn dispatched_attempt_records_worktree_path_and_base() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "dispatch lane".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "mutate".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let lane = TaskLaneMetadata {
        worktree_path: Some("/lanes/task-a".into()),
        branch: Some("lane/task-a".into()),
        base: Some("basecommit".into()),
        ..Default::default()
    };

    let attempt_id = store
        .begin_task_dispatch(&plan.id, &task_id, "worker-tab", "Worker", lane, 3)
        .unwrap();

    let task = store.get(&plan.id).unwrap().tasks.remove(0);
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
        .expect("dispatch must create the reserved attempt");
    assert_eq!(attempt.worktree_path.as_deref(), Some("/lanes/task-a"));
    assert_eq!(attempt.base.as_deref(), Some("basecommit"));
    assert_eq!(attempt.branch.as_deref(), Some("lane/task-a"));
}

#[test]
fn bound_in_review_report_advances_atomically() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    assert!(store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, Some("abc123")),
            4,
        )
        .unwrap());

    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.lane.branch.as_deref(), Some("lane/worker"));
    let report = task.execution_attempts[0].report.as_ref().unwrap();
    assert_eq!(report.sha.as_deref(), Some("abc123"));
    assert_eq!(task.execution_attempts[0].executor_tab_id, "worker");
    assert_eq!(report.changed_files, vec!["src/report.rs"]);
}

#[test]
fn in_review_report_without_sha_still_advances_bound_task() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    assert!(store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, None),
            4,
        )
        .unwrap());
    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InReview);
    assert!(task.execution_attempts[0].report.is_some());
    assert_eq!(task.milestones.len(), 1);
}

#[test]
fn partial_report_keeps_bound_task_actionable() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    assert!(!store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::Partial, Some("abc123")),
            4,
        )
        .unwrap());
    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.execution_attempts[0].report.is_some());
    assert_eq!(task.milestones.len(), 1);
}

#[test]
fn duplicate_stale_or_unbound_reports_never_regress_task() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::InReview, Some("abc123")),
            4,
        )
        .unwrap();
    assert!(!store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            worker_report(ReportStatus::Partial, Some("old")),
            3,
        )
        .unwrap());
    assert!(!store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("unbound"),
            worker_report(ReportStatus::InReview, Some("other")),
            5,
        )
        .unwrap());
    let task = store.get(&plan_id).unwrap().tasks.remove(0);
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(
        task.execution_attempts[0]
            .report
            .as_ref()
            .unwrap()
            .sha
            .as_deref(),
        Some("abc123")
    );
}

#[test]
fn task_attempt_history_builds_respawn_prompt_from_prior_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Respawn".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
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
                role_guidance: Some("Act as planner; do not implement.".into()),
                acceptance_criteria: vec!["preserve task authority".into()],
                correctness: Some("The final provider prompt keeps every task field.".into()),
                test_plan: Some("cargo test respawn_prompt".into()),
                qa_plan: Some("independent hidden oracle".into()),
                intent: Some("Prefer shared prompt composition.".into()),
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
        .assign_task(&p.id, &task_id, "tab_old", None, 3)
        .unwrap();
    let attempt = store.list_task_attempts(&p.id, &task_id).unwrap().remove(0);
    assert_ne!(attempt.attempt_id, "tab_old");
    assert!(attempt.attempt.executor_title.is_none());
    store
        .record_task_attempt_report(
            &p.id,
            &task_id,
            &AgentRef::from_tab("tab_old"),
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
                model: Some("gpt-override".into()),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(draft.provider, "codex");
    assert_eq!(draft.planned_model.as_deref(), Some("codex/gpt-5.5"));
    assert_eq!(draft.model.as_deref(), Some("gpt-override"));
    assert_eq!(draft.cwd, "/tmp/worktree");
    assert!(draft.task_prompt.starts_with(
        "Act as planner; do not implement.\n\
         Your assigned task is: retry lane"
    ));
    assert!(draft
        .task_prompt
        .contains(crate::service_plan::worker_identity::WORKER_IDENTITY_END));
    assert!(
        draft
            .task_prompt
            .find(crate::service_plan::worker_identity::WORKER_IDENTITY_END)
            .unwrap()
            < draft
                .task_prompt
                .find("You are a CodeTerm Worker respawned")
                .unwrap()
    );
    assert!(draft.task_prompt.contains("STATUS: blocked"));
    assert!(draft.task_prompt.contains("branch: lane/retry"));
    assert!(draft.task_prompt.contains("preserve task authority"));
    assert!(draft
        .task_prompt
        .contains("The final provider prompt keeps every task field."));
    assert!(draft.task_prompt.contains("cargo test respawn_prompt"));
    assert!(draft
        .task_prompt
        .contains("Prefer shared prompt composition."));
    assert!(!draft.task_prompt.contains("independent hidden oracle"));
    assert!(draft
        .task_prompt
        .contains(&format!("attempt_id: {}", attempt.attempt_id)));
    assert!(draft.task_prompt.contains(&format!(
        "codeterm agent report --plan {} --task {}",
        p.id, task_id
    )));
    assert!(!draft.task_prompt.contains("--base "));
}

#[test]
fn respawn_report_command_uses_attempt_base_not_task_lane_base() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Respawn".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "retry lane".into(),
                kind: "implement".into(),
                lane: TaskLaneMetadata {
                    base: Some("task-base".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_old", None, 3)
        .unwrap();
    store
        .with_plan(&p.id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.execution_attempts[0].base = Some("attempt-base".into());
            Ok(())
        })
        .unwrap();

    let draft = store
        .build_task_respawn_draft(
            &p.id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: store.list_task_attempts(&p.id, &task_id).unwrap()[0]
                    .attempt_id
                    .clone(),
                ..Default::default()
            },
        )
        .unwrap();

    assert!(draft.report_command.contains("codeterm agent report"));
    assert!(!draft.report_command.contains("attempt-base"));
    assert!(!draft.report_command.contains("task-base"));
}

#[test]
fn task_respawn_draft_preserves_explicit_request_title() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Respawn title".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "retry lane".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_old", None, 3)
        .unwrap();
    let attempt = store.list_task_attempts(&p.id, &task_id).unwrap().remove(0);

    let draft = store
        .build_task_respawn_draft(
            &p.id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: attempt.attempt_id,
                title: Some("Exact User Title".into()),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(draft.title.as_deref(), Some("Exact User Title"));
    assert!(draft.explicit_title);
}

#[test]
fn task_respawn_draft_omitted_title_stays_provisional() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Respawn inherited title".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "retry lane".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_old", None, 3)
        .unwrap();
    let attempt = store.list_task_attempts(&p.id, &task_id).unwrap().remove(0);

    let draft = store
        .build_task_respawn_draft(
            &p.id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: attempt.attempt_id,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(draft.title, None);
    assert!(!draft.explicit_title);
}

#[test]
fn competition_keeps_planned_model_separate_from_lane_requests() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Competition".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "compare models".into(),
                kind: "implement".into(),
                model: Some("codex/model-planned".into()),
                competition: Some(crate::plan::TaskCompetitionMetadata {
                    enabled: true,
                    attempts: Some(2),
                    models: vec!["model-a".into(), "model-b".into()],
                    judge_hint: None,
                }),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let drafts = store
        .build_task_compete_drafts(
            &plan.id,
            &task_id,
            task_attempts::TaskCompeteRequest::default(),
        )
        .unwrap();

    assert_eq!(drafts.len(), 2);
    assert!(drafts
        .iter()
        .all(|draft| draft.planned_model.as_deref() == Some("codex/model-planned")));
    assert_eq!(drafts[0].model.as_deref(), Some("model-a"));
    assert_eq!(drafts[1].model.as_deref(), Some("model-b"));
}

#[test]
fn compete_commit_revalidates_debt_after_preflight_interleaving() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let mut plan = Plan::new(
        "Compete capacity race".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "race target".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let draft = store
        .build_task_compete_drafts(
            &plan.id,
            &task_id,
            task_attempts::TaskCompeteRequest::default(),
        )
        .unwrap()
        .remove(0);
    let phase = Arc::new(Barrier::new(2));
    let worker_store = Arc::clone(&store);
    let worker_phase = Arc::clone(&phase);
    let worker_plan_id = plan.id.clone();
    let worker_task_id = task_id.clone();
    let commit = std::thread::spawn(move || {
        let snapshot = worker_store.get(&worker_plan_id).unwrap();
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.id == worker_task_id)
            .unwrap();
        review_gate::validate_dispatch_review_capacity(&snapshot, task).unwrap();
        worker_phase.wait();
        worker_phase.wait();
        worker_store.record_task_compete_attempt(
            &worker_plan_id,
            &worker_task_id,
            "spawned-tab",
            &draft,
            Some("worker"),
            4,
        )
    });

    phase.wait();
    store
        .with_plan(&plan.id, 3, |plan| {
            plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
            Ok(())
        })
        .unwrap();
    phase.wait();

    assert!(matches!(
        commit.join().unwrap(),
        Err(PlanError::ReviewWipExceeded { .. })
    ));
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.assigned_tab_id, None);
    assert_eq!(task.lane.tab_id, None);
    assert!(task.execution_attempts.is_empty());
}

#[test]
fn respawn_commit_revalidates_wip_after_preflight_interleaving() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let mut plan = Plan::new(
        "Respawn capacity race".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.review_policy.wip_limit = 1;
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "respawn target".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let blocker_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "review blocker".into(),
                kind: "review".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "source-tab", None, 2)
        .unwrap();
    let source_attempt = store.list_task_attempts(&plan.id, &task_id).unwrap()[0]
        .attempt_id
        .clone();
    let draft = store
        .build_task_respawn_draft(
            &plan.id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: source_attempt,
                ..Default::default()
            },
        )
        .unwrap();
    let before = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    let phase = Arc::new(Barrier::new(2));
    let worker_store = Arc::clone(&store);
    let worker_phase = Arc::clone(&phase);
    let worker_plan_id = plan.id.clone();
    let worker_task_id = task_id.clone();
    let commit = std::thread::spawn(move || {
        let snapshot = worker_store.get(&worker_plan_id).unwrap();
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.id == worker_task_id)
            .unwrap();
        review_gate::validate_dispatch_review_capacity(&snapshot, task).unwrap();
        worker_phase.wait();
        worker_phase.wait();
        worker_store.record_task_respawn_attempt(
            &worker_plan_id,
            &worker_task_id,
            "spawned-tab",
            &draft,
            Some("worker"),
            4,
        )
    });

    phase.wait();
    store
        .with_plan(&plan.id, 3, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == blocker_id)
                .unwrap()
                .status = TaskStatus::InReview;
            Ok(())
        })
        .unwrap();
    phase.wait();

    assert!(matches!(
        commit.join().unwrap(),
        Err(PlanError::ReviewWipExceeded { .. })
    ));
    let after = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(after.status, before.status);
    assert_eq!(after.assigned_tab_id, before.assigned_tab_id);
    assert_eq!(after.lane.tab_id, before.lane.tab_id);
    assert_eq!(after.execution_attempts, before.execution_attempts);
}

#[test]
fn role_guidance_round_trips_and_worker_charter_prepends_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Guidance".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    p.mode = crate::plan::PlanMode::Execution;
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "guided".into(),
                kind: "implement".into(),
                personality: Some("Hopper".into()),
                role_guidance: Some("You are Hopper. Review only.".into()),
                assigned_tab_id: Some("tab_guided".into()),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let plan = store.get(&p.id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(
        task.role_guidance.as_deref(),
        Some("You are Hopper. Review only.")
    );

    let brief = store.prepend_worker_charter_for_tab("tab_guided", "continue");
    let expected_prefix = format!(
        "<codeterm-worker-charter task=\"{task_id}\">\nYou are Hopper. Review only.\n\nYou are a Worker on task {task_id}."
    );
    assert_eq!(&brief[..expected_prefix.len()], expected_prefix);
    assert_eq!(brief.matches("You are Hopper.").count(), 1);
}

#[test]
fn respawn_prompt_starts_with_canonical_identity_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Respawn identity".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "retry lane".into(),
                kind: "implement".into(),
                personality: Some("Hopper".into()),
                role_guidance: Some("You are Hopper. Audit reconnect semantics.".into()),
                body: "Fix the reconnect prompt.".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_old", None, 3)
        .unwrap();
    let attempt = store.list_task_attempts(&p.id, &task_id).unwrap().remove(0);

    let draft = store
        .build_task_respawn_draft(
            &p.id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: attempt.attempt_id,
                ..Default::default()
            },
        )
        .unwrap();

    let expected_prefix = format!(
        "You are Hopper. Audit reconnect semantics.\n\
         Your assigned task is: retry lane\n\n{}\n\n\
         {}\n\n\
         You are a CodeTerm Worker respawned from a prior task attempt.\n\
         Work only on this one task. Do not reactivate or reuse the old historical attempt.\n\n",
        crate::service_plan::worker_identity::WORKER_IDENTITY_END,
        crate::service_plan::worker_identity::WORKER_ACK_INSTRUCTION
    );
    assert_eq!(&draft.task_prompt[..expected_prefix.len()], expected_prefix);
    assert_eq!(draft.task_prompt.matches("You are Hopper.").count(), 1);
    let ack = draft.task_prompt.find("codeterm agent ack").unwrap();
    let report = draft.task_prompt.find("When done, run exactly:").unwrap();
    assert!(ack < draft.task_prompt.find("Original task body:").unwrap());
    assert!(ack < report);
}

#[test]
fn competition_prompt_starts_with_same_canonical_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Competition identity".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "judgeable".into(),
                kind: "implement".into(),
                personality: Some("Hopper".into()),
                role_guidance: Some("You are Hopper. Audit reconnect semantics.".into()),
                competition: Some(crate::plan::TaskCompetitionMetadata {
                    enabled: true,
                    attempts: Some(2),
                    models: vec![],
                    judge_hint: None,
                }),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    let drafts = store
        .build_task_compete_drafts(
            &p.id,
            &task_id,
            task_attempts::TaskCompeteRequest::default(),
        )
        .unwrap();
    let prompt = &drafts[0].task_prompt;

    let expected_prefix = format!(
        "You are Hopper. Audit reconnect semantics.\n\
         Your assigned task is: judgeable\n\n{}\n\n\
         {}\n\n\
         You are one isolated competitive attempt.",
        crate::service_plan::worker_identity::WORKER_IDENTITY_END,
        crate::service_plan::worker_identity::WORKER_ACK_INSTRUCTION
    );
    assert_eq!(&prompt[..expected_prefix.len()], expected_prefix);
    assert_eq!(prompt.matches("You are Hopper.").count(), 1);
    let ack = prompt.find("codeterm agent ack").unwrap();
    let report = prompt.find("When done, run exactly:").unwrap();
    assert!(ack < prompt.find("Task body:").unwrap());
    assert!(ack < report);
}

#[test]
fn assignment_attempt_records_planner_role() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Planner attempt".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "plan".into(),
                kind: "plan".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_role(&p.id, &task_id, "planner-tab", None, Some("planner"), 3)
        .unwrap();

    let attempt = store.list_task_attempts(&p.id, &task_id).unwrap().remove(0);
    assert_eq!(attempt.attempt.role.as_deref(), Some("planner"));
}

#[test]
fn task_attempt_ids_are_stable_distinct_from_executor_tab() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Attempts".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "same tab".into(),
                kind: "implement".into(),
                body: "Run twice in same tab.".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .assign_task(&p.id, &task_id, "tab_same", None, 3)
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_same", None, 4)
        .unwrap();

    let attempts = store.list_task_attempts(&p.id, &task_id).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_ne!(attempts[0].attempt_id, attempts[1].attempt_id);
    assert_ne!(attempts[0].attempt_id, "tab_same");
    assert_eq!(attempts[0].attempt.executor_tab_id, "tab_same");
    assert_eq!(attempts[1].attempt.executor_tab_id, "tab_same");
    assert!(attempts[0].attempt.executor_title.is_none());
    assert!(attempts[1].attempt.executor_title.is_none());

    let shown = store
        .show_task_attempt(&p.id, &task_id, &attempts[0].attempt_id)
        .unwrap();
    assert_eq!(shown.attempt_id, attempts[0].attempt_id);
    assert!(store
        .show_task_attempt(&p.id, &task_id, "tab_same")
        .is_err());
}

#[test]
fn terminal_status_update_leaves_history_but_no_active_presence() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Attempts".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "terminal".into(),
                kind: "implement".into(),
                body: "Close me.".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_live", None, 3)
        .unwrap();
    store
        .update_task(
            &p.id,
            &task_id,
            None,
            None,
            None,
            None,
            Some("cancelled"),
            None,
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

    let p = store.get(&p.id).unwrap();
    let task = p.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Cancelled);
    assert!(task.assigned_tab_id.is_none());
    assert!(task.lane.tab_id.is_none());
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].executor_tab_id, "tab_live");
    assert_eq!(task.execution_attempts[0].closed_at, Some(4));
    assert!(store.task_binding_for_tab("tab_live").is_none());
    assert!(store.task_bindings_for_tab("tab_live").is_empty());
}

#[test]
fn recent_worker_attempt_titles_filters_workspace_session_and_limit() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Recent titles".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "task".into(),
                kind: "implement".into(),
                lane: TaskLaneMetadata {
                    workspace_id: Some("ws_target".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_role(&plan.id, &task_id, "tab-1", None, Some("worker"), 3)
        .unwrap();
    store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            "tab-1",
            Some("Ada"),
            Some("sess-1"),
            None,
            4,
        )
        .unwrap();
    store
        .assign_task_with_role(&plan.id, &task_id, "tab-2", None, Some("worker"), 5)
        .unwrap();
    store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            "tab-2",
            Some("Bea"),
            Some("sess-2"),
            None,
            6,
        )
        .unwrap();

    assert_eq!(
        store.recent_worker_attempt_titles("ws_target", Some("sess-1"), 10),
        vec!["Ada".to_string()]
    );
    assert_eq!(
        store.recent_worker_attempt_titles("ws_target", None, 1),
        vec!["Bea".to_string()]
    );
}

#[test]
fn reconcile_assignment_cleanup_closes_live_attempt_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Attempts".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "reconcile".into(),
                kind: "implement".into(),
                body: "Tab vanished.".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&p.id, &task_id, "tab_live", None, 3)
        .unwrap();
    let drift = [crate::plan::reconcile::DriftItem {
        plan_id: p.id.clone(),
        task_id: task_id.clone(),
        kind: crate::plan::reconcile::DriftKind::BoundToClosedTab,
        detail: "closed".into(),
        suggested_fix: "clear".into(),
        not_mine: false,
    }];

    store.apply_reconcile_fixes(&p.id, &drift, 4).unwrap();

    let p = store.get(&p.id).unwrap();
    let task = p.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert!(task.assigned_tab_id.is_none());
    assert!(task.lane.tab_id.is_none());
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].executor_tab_id, "tab_live");
    assert_eq!(task.execution_attempts[0].closed_at, Some(4));
}

// Reproduces a proxy reporter attempting to replace the assigned executor.
#[test]
fn report_does_not_rebind_executor() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Masquerade".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "owned by A".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    // Explicit assignment path binds the real executor A.
    store
        .assign_task(&p.id, &task_id, "tab_A", None, 3)
        .unwrap();

    // Exercise the report path: lane truth, milestone, then attempt report.
    let reporter_b = AgentRef::from_tab("tab_B");
    store
        .record_report_lane_truth(&p.id, &task_id, Some("lane/b"), None, 4)
        .unwrap();
    store
        .add_milestone(
            &p.id,
            &task_id,
            "note",
            "STATUS: in_review from B",
            reporter_b.clone(),
            true,
            4,
        )
        .unwrap();
    store
        .record_task_attempt_report(
            &p.id,
            &task_id,
            &reporter_b,
            "STATUS: in_review\nCHANGES: done",
            Some(ReportStatus::InReview),
            4,
        )
        .unwrap();

    let p = store.get(&p.id).unwrap();
    let task = p.tasks.iter().find(|t| t.id == task_id).unwrap();

    // Executor binding is untouched by the report.
    assert_eq!(task.assigned_tab_id.as_deref(), Some("tab_A"));
    assert_eq!(task.lane.tab_id.as_deref(), Some("tab_A"));

    // No masquerade attempt is created.
    assert_eq!(task.execution_attempts.len(), 1);
    let attempt = &task.execution_attempts[0];
    assert_eq!(attempt.executor_tab_id, "tab_A");

    // The stride/milestone carries reporter B.
    let stride = task
        .milestones
        .iter()
        .find(|m| m.kind == "note")
        .expect("reporter stride");
    assert_eq!(stride.from.tab_id.as_deref(), Some("tab_B"));

    // The executor's attempt retains distinct reporter attribution.
    let report = attempt.report.as_ref().expect("attempt report recorded");
    assert_eq!(report.reporter_tab_id.as_deref(), Some("tab_B"));
    assert!(report.message.as_deref().unwrap().contains("CHANGES: done"));

    // Lane truth (branch) from the report was captured.
    assert_eq!(task.lane.branch.as_deref(), Some("lane/b"));
}

// A report cannot use an empty task assignment to capture executor ownership.
#[test]
fn report_on_unassigned_task_never_binds_reporter() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut p = Plan::new(
        "Unbound".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut p, 1).unwrap();
    let task_id = store
        .add_task(
            &p.id,
            NewTask {
                name: "unowned".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();

    store
        .record_report_lane_truth(&p.id, &task_id, Some("lane/b"), None, 3)
        .unwrap();

    let p = store.get(&p.id).unwrap();
    let task = p.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert!(
        task.assigned_tab_id.is_none(),
        "a report must not bind an executor onto an unassigned task"
    );
    assert!(task.lane.tab_id.is_none());
    assert_eq!(task.lane.branch.as_deref(), Some("lane/b"));
}
