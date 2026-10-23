//! Respawn, competition and the prompts they compose: attempt history feeding a
//! respawn brief, draft titles, preflight revalidation, and the canonical identity
//! prefix. Report handling and attempt identity stay in the parent module.

use std::sync::{Arc, Barrier};

use super::super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan, TaskLaneMetadata, TaskStatus};
use codeterm_types::agent::ReportStatus;

#[test]
fn attempt_model_fields_round_trip() {
    let (dir, store, plan_id, task_id) = super::bound_report_store();
    let attempt_id = store.list_task_attempts(&plan_id, &task_id).unwrap()[0]
        .attempt_id
        .clone();
    store
        .with_plan(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .unwrap();
            attempt.provider = Some("fixture-agent".into());
            attempt.model = Some("gpt-6-luna".into());
            attempt.reasoning_effort = Some("xhigh".into());
            attempt.session_model = Some("gpt-runtime".into());
            Ok(())
        })
        .unwrap();

    store
        .update_task_speed(&plan_id, &task_id, Some("opaque-tier"), 4)
        .unwrap();
    let reloaded = PlanStore::new_for_test(dir.path());
    let saved = reloaded
        .list_task_attempts(&plan_id, &task_id)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let json = serde_json::to_value(&saved).unwrap();
    assert_eq!(json["provider"], "fixture-agent");
    assert_eq!(json["model"], "gpt-6-luna");
    assert_eq!(json["reasoning_effort"], "xhigh");
    assert_eq!(json["session_model"], "gpt-runtime");

    let draft = reloaded
        .build_task_respawn_draft(
            &plan_id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        draft.execution_intent,
        crate::plan::ExecutionIntent::Mutating
    );
    assert_eq!(
        reloaded.show_task(&task_id).unwrap().task.speed.as_deref(),
        Some("opaque-tier")
    );
    assert_eq!(draft.speed.as_deref(), Some("opaque-tier"));
    assert_eq!(draft.provider, "fixture-agent");
    assert_eq!(draft.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(draft.reasoning_effort.as_deref(), Some("xhigh"));

    let respawned = reloaded
        .record_task_respawn_attempt(&plan_id, &task_id, "worker-next", &draft, Some("worker"), 5)
        .unwrap();
    assert_eq!(respawned.attempt.id, draft.attempt_id);
    assert_eq!(respawned.attempt.provider.as_deref(), Some("fixture-agent"));
    assert_eq!(respawned.attempt.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(respawned.attempt.reasoning_effort.as_deref(), Some("xhigh"));
    assert_eq!(respawned.attempt.session_model, None);
}

#[test]
fn respawn_explicit_reasoning_effort_overrides_the_source_attempt() {
    let (_dir, store, plan_id, task_id) = super::bound_report_store();
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    let draft = store
        .build_task_respawn_draft(
            &plan_id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id,
                reasoning_effort: Some("opaque-effort".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(draft.reasoning_effort.as_deref(), Some("opaque-effort"));
    let replacement = store
        .record_task_respawn_attempt(&plan_id, &task_id, "replacement", &draft, Some("worker"), 5)
        .unwrap();
    assert_eq!(
        replacement.attempt.reasoning_effort.as_deref(),
        Some("opaque-effort")
    );
}

#[test]
fn respawn_uses_source_resolved_model_instead_of_conflicting_task_model() {
    let (_dir, store, plan_id, task_id) = super::bound_report_store();
    let source_attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    store
        .with_plan(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.model = Some("fixture-agent:model".into());
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == source_attempt_id)
                .unwrap();
            attempt.provider = Some("fixture-agent".into());
            attempt.agent_type = Some("fixture-agent".into());
            attempt.model = None;
            attempt.provider_resolved_model = Some("haiku".into());
            attempt.requested_model = Some("haiku".into());
            Ok(())
        })
        .unwrap();

    let draft = store
        .build_task_respawn_draft(
            &plan_id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: source_attempt_id,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(draft.provider, "fixture-agent");
    assert_eq!(draft.model.as_deref(), Some("haiku"));
}

#[test]
fn respawn_from_a_source_attempt_is_single_use_while_replacement_is_live() {
    let (_dir, store, plan_id, task_id) = super::bound_report_store();
    let source_attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    let request = || task_attempts::TaskRespawnRequest {
        attempt_id: source_attempt_id.clone(),
        ..Default::default()
    };

    let first = store
        .build_task_respawn_draft(&plan_id, &task_id, request())
        .unwrap();
    let first_reservation = store
        .reserve_task_respawn_startup(&plan_id, &task_id, &source_attempt_id, &first.attempt_id)
        .unwrap();
    let first_created = store
        .record_task_respawn_attempt(
            &plan_id,
            &task_id,
            "worker-first-respawn",
            &first,
            Some("worker"),
            4,
        )
        .unwrap();
    drop(first_reservation);

    let second = store
        .build_task_respawn_draft(&plan_id, &task_id, request())
        .unwrap();
    let result = store.reserve_task_respawn_startup(
        &plan_id,
        &task_id,
        &source_attempt_id,
        &second.attempt_id,
    );

    assert!(matches!(
        result,
        Err(PlanError::RespawnAttemptNotCurrent { current_attempt_id, tab_id })
            if current_attempt_id == first_created.attempt.id && tab_id == "worker-first-respawn"
    ));
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(
        task.execution_attempts
            .iter()
            .filter(|attempt| attempt.closed_at.is_none())
            .count(),
        1
    );
    assert_eq!(
        task.assigned_tab_id.as_deref(),
        Some("worker-first-respawn")
    );

    store
        .with_plan(&plan_id, 6, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == first_created.attempt.id)
                .unwrap()
                .closed_at = Some(6);
            Ok(())
        })
        .unwrap();
    let after_loss = store
        .build_task_respawn_draft(
            &plan_id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: first_created.attempt.id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let recovered_reservation = store
        .reserve_task_respawn_startup(
            &plan_id,
            &task_id,
            &first_created.attempt.id,
            &after_loss.attempt_id,
        )
        .unwrap();
    drop(recovered_reservation);
}

#[test]
fn replay_after_a_respawn_closes_refuses_the_retired_source_without_mutation() {
    let (_dir, store, plan_id, task_id) = super::bound_report_store();
    let source_attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    let first = store
        .build_task_respawn_draft(
            &plan_id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: source_attempt_id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let reservation = store
        .reserve_task_respawn_startup(&plan_id, &task_id, &source_attempt_id, &first.attempt_id)
        .unwrap();
    let created = store
        .record_task_respawn_attempt(
            &plan_id,
            &task_id,
            "worker-first-respawn",
            &first,
            Some("worker"),
            4,
        )
        .unwrap();
    drop(reservation);
    store
        .with_plan(&plan_id, 5, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, &task_id).unwrap();
            task.execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == created.attempt.id)
                .unwrap()
                .closed_at = Some(5);
            Ok(())
        })
        .unwrap();
    let before = store.show_task(&task_id).unwrap().task;
    let replay = store
        .build_task_respawn_draft(
            &plan_id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: source_attempt_id.clone(),
                ..Default::default()
            },
        )
        .unwrap();

    let refusal = match store.reserve_task_respawn_startup(
        &plan_id,
        &task_id,
        &source_attempt_id,
        &replay.attempt_id,
    ) {
        Ok(_) => panic!("a printed command cannot reuse its retired source attempt"),
        Err(error) => error,
    };
    assert!(matches!(
        &refusal,
        PlanError::RespawnAttemptNotCurrent { current_attempt_id, tab_id }
            if current_attempt_id == &created.attempt.id && tab_id == "worker-first-respawn"
    ));
    assert_ne!(refusal.rpc_code(), 0);
    let refusal_data = refusal.to_json();
    assert_eq!(refusal_data["type"], "respawn_attempt_not_current");
    assert_eq!(refusal_data["current_attempt_id"], created.attempt.id);
    assert_eq!(refusal_data["tab_id"], "worker-first-respawn");
    let after = store.show_task(&task_id).unwrap().task;
    assert_eq!(
        after.execution_attempts.len(),
        before.execution_attempts.len()
    );
    assert_eq!(after.assigned_tab_id, before.assigned_tab_id);
    assert_eq!(
        after.execution_attempts.last().unwrap().id,
        created.attempt.id
    );
}

#[test]
fn concurrent_respawns_of_one_source_create_only_one_replacement() {
    let (_dir, store, plan_id, task_id) = super::bound_report_store();
    let store = Arc::new(store);
    let source_attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    let barrier = Arc::new(Barrier::new(2));
    let workers = ["worker-respawn-a", "worker-respawn-b"].map(|tab_id| {
        let store = store.clone();
        let plan_id = plan_id.clone();
        let task_id = task_id.clone();
        let source_attempt_id = source_attempt_id.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let draft = store
                .build_task_respawn_draft(
                    &plan_id,
                    &task_id,
                    task_attempts::TaskRespawnRequest {
                        attempt_id: source_attempt_id.clone(),
                        ..Default::default()
                    },
                )
                .unwrap();
            barrier.wait();
            let reservation = store.reserve_task_respawn_startup(
                &plan_id,
                &task_id,
                &source_attempt_id,
                &draft.attempt_id,
            );
            match reservation {
                Ok(_reservation) => store
                    .record_task_respawn_attempt(
                        &plan_id,
                        &task_id,
                        tab_id,
                        &draft,
                        Some("worker"),
                        4,
                    )
                    .map(|created| Some(created.attempt.id)),
                Err(error) => Err(error),
            }
        })
    });
    let outcomes = workers.map(|worker| worker.join().unwrap());
    let created = outcomes
        .iter()
        .find_map(|outcome| outcome.as_ref().ok().and_then(|created| created.clone()))
        .expect("one concurrent respawn creates a replacement");
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert!(outcomes.iter().any(|outcome| matches!(
        outcome,
        Err(PlanError::DispatchInFlight { .. }) | Err(PlanError::RespawnAttemptNotCurrent { .. })
    )));
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.execution_attempts.len(), 2);
    assert_eq!(
        task.execution_attempts
            .iter()
            .filter(|attempt| attempt.closed_at.is_none())
            .count(),
        1
    );
    assert_eq!(task.execution_attempts.last().unwrap().id, created);
}

#[test]
fn respawn_keeps_persona() {
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
                personality: Some("Ranker".into()),
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
    assert_eq!(draft.title.as_deref(), Some("Ranker"));
    assert_eq!(draft.cwd, "/tmp/worktree");
    let task = store.show_task(&task_id).unwrap().task;
    let expected_identity =
        crate::service_plan::worker_identity::compose_task_worker_identity(&task, Some("Ranker"));
    let (identity, _) =
        crate::service_plan::worker_identity::split_worker_identity(&draft.task_prompt);
    assert_eq!(identity, Some(expected_identity.as_str()));
    let identity = identity.unwrap();
    assert!(identity.contains("Ranker"));
    assert!(identity.contains("Act as planner; do not implement."));
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
        "domios agent report --plan {} --task {}",
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

    assert!(draft.report_command.contains("domios agent report"));
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
fn compete_commit_preserves_admission_when_review_debt_changes() {
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

    commit.join().unwrap().unwrap();
    let task = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(store.get(&plan.id).unwrap().review_policy.credit, -5);
}

#[test]
fn respawn_commit_preserves_admission_when_review_backlog_changes() {
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

    commit.join().unwrap().unwrap();
    let after = store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(after.status, before.status);
    assert_eq!(after.assigned_tab_id.as_deref(), Some("spawned-tab"));
    assert_eq!(after.lane.tab_id.as_deref(), Some("spawned-tab"));
    assert_eq!(
        after.execution_attempts.len(),
        before.execution_attempts.len() + 1
    );
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
    let expected_prefix =
        format!("<codeterm-worker-charter task=\"{task_id}\">\nYou are Hopper. Review only.\n");
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
    let ack = draft.task_prompt.find("domios agent ack").unwrap();
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
    let ack = prompt.find("domios agent ack").unwrap();
    let report = prompt.find("When done, run exactly:").unwrap();
    assert!(ack < prompt.find("Task body:").unwrap());
    assert!(ack < report);
}
