use super::*;
use crate::plan::{OrchestratorRef, PlanMode, TaskStatus};
use crate::service::plan::{NewTask, PlanStore};

pub(super) fn store_with_task(dir: &std::path::Path) -> (PlanStore, String, String) {
    let store = PlanStore::new_for_test(dir);
    let plan = store
        .create_plan_for_team(
            "Spawn bind".into(),
            dir.display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team-orch".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut p = store.get(&plan.id).unwrap();
    p.mode = PlanMode::Approved;
    store.save(&mut p, 2).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "bind-at-spawn".into(),
                handle: Some("spawn-bind".into()),
                personality: Some("Ada".into()),
                role_guidance: Some("You are Ada. Keep the change bounded.".into()),
                body: "Implement the task-owned behavior.".into(),
                acceptance_criteria: vec!["The focused oracle passes.".into()],
                ..Default::default()
            },
            3,
        )
        .unwrap();
    (store, plan.id, task_id)
}

#[test]
fn plan_task_binding_records_dispatched_model_and_effort() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, task_id) = store_with_task(dir.path());
    let effort_identity = crate::multiplexer::types::ReasoningEffortIdentity {
        requested: Some("xhigh".into()),
        resolved: Some("high".into()),
        ..Default::default()
    };
    let binding = bind_worker_task_to_tab(
        &store,
        Some(&plan_id),
        "spawn-bind",
        "tab-worker",
        "Ada",
        Some("session-worker"),
        Some("nonce-worker"),
        crate::plan::ExecutionIntent::ReadOnly,
        None,
        Some("codex"),
        Some("gpt-6-luna".into()),
        Some("gpt-6-luna".into()),
        resolved_worker_launch_effort(&effort_identity),
        "attempt-prepared",
        None,
        vec![crate::plan::PlanSkillManifestEntry {
            slug: "prepared".into(),
            title: "Prepared".into(),
            purpose: "Prove exact snapshot retention.".into(),
            content_digest: "sha256:prepared".into(),
        }],
        10,
    )
    .unwrap();
    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.status_label.as_deref(), Some("awaiting_worker_ack"));
    assert_eq!(task.assigned_tab_id.as_deref(), Some("tab-worker"));
    assert_eq!(task.lane.tab_id.as_deref(), Some("tab-worker"));
    assert_eq!(task.taken_by_role.as_deref(), Some("worker"));
    assert_eq!(binding.plan_id, plan_id);
    assert_eq!(binding.task_id, task_id);
    let attempt = task.execution_attempts.last().unwrap();
    assert_eq!(attempt.provider.as_deref(), Some("codex"));
    assert_eq!(attempt.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(attempt.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(
        task.execution_attempts.last().unwrap().plan_skill_manifest[0].slug,
        "prepared"
    );
    assert_eq!(
        task.execution_attempts
            .last()
            .unwrap()
            .session_id
            .as_deref(),
        Some("session-worker")
    );
}

#[test]
fn task_bound_launch_uses_the_task_ack_path_only() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, _) = store_with_task(dir.path());
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        plan_id: Some(plan_id.clone()),
        plan_task: Some("spawn-bind".into()),
        execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
        provider_reserve_pct: None,
        task: Some("Check the adjacent caller concern.".into()),
        ..Default::default()
    };

    prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

    assert!(params.task_launch_authority.is_some());
    let task_selection = crate::service::launch_delivery::select_visible_spawn_task(
        None,
        params.task.as_deref(),
        true,
        true,
    );
    let startup_handshake = crate::service::launch_delivery::startup_starter_prompt(
        true,
        params.task_launch_authority.is_some(),
    );
    assert_eq!(startup_handshake, None);
    assert_eq!(
        task_selection.provider_starter,
        Some(crate::agent::provider::StarterPrompt::AssignedTask)
    );
    assert_eq!(
        crate::service::spawn::visible_spawn_starter_prompt(
            startup_handshake,
            task_selection.provider_starter,
        ),
        Some(crate::agent::provider::StarterPrompt::AssignedTask)
    );

    let authority = params.task_launch_env();
    assert_eq!(
        authority.get("CODETERM_PLAN_ID").map(String::as_str),
        Some(plan_id.as_str())
    );
    assert_eq!(
        authority.get("CODETERM_TASK_ID").map(String::as_str),
        params.task_id.as_deref()
    );
    let prompt = params.task.unwrap();
    assert!(prompt.starts_with("You are Ada. Keep the change bounded."));
    let ack_instruction = codeterm_plan::service_plan::worker_identity::WORKER_ACK_INSTRUCTION;
    let ack = prompt.find(ack_instruction).unwrap();
    let body = prompt.find("Implement the task-owned behavior.").unwrap();
    let criteria = prompt.find("The focused oracle passes.").unwrap();
    let supplemental = prompt.find("## Orchestrator instruction").unwrap();
    assert!(ack < body);
    assert!(body < criteria && criteria < supplemental);
    assert_eq!(prompt.matches(ack_instruction).count(), 1);
    assert!(!prompt.contains("codeterm agent report"));
}

#[test]
fn direct_plan_task_spawn_records_host_owned_effort_intent() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, _) = store_with_task(dir.path());
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        plan_id: Some(plan_id),
        plan_task: Some("spawn-bind".into()),
        reasoning_effort: Some("high".into()),
        ..Default::default()
    };

    prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

    assert_eq!(params.planned_reasoning_effort.as_deref(), Some("high"));
    assert_eq!(params.reasoning_effort.as_deref(), Some("high"));
}

#[test]
fn validate_rejects_non_worker() {
    let err = validate_worker_plan_task_spawn(&SpawnAgentParams {
        role: Some("Orchestrator".into()),
        plan_task: Some("spawn-bind".into()),
        execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
        provider_reserve_pct: None,
        ..Default::default()
    })
    .unwrap_err();
    assert!(err.contains("--plan-task"));
}

#[test]
fn tab_name_contract_spawn_name_paths_reject_multi_word_task_bound_title() {
    let error = validate_worker_plan_task_spawn(&SpawnAgentParams {
        role: Some("Worker".into()),
        plan_task: Some("spawn-bind".into()),
        title: Some("Ada Lovelace".into()),
        execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
        ..Default::default()
    })
    .unwrap_err();
    assert!(error.contains("one word"), "{error}");
}

fn verifier(gates: &[&str]) -> SpawnAgentParams {
    SpawnAgentParams {
        role: Some("Worker".into()),
        execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
        authorized_gates: gates.iter().map(|gate| gate.to_string()).collect(),
        ..Default::default()
    }
}

/// The documented verification Worker: read-only, no task of its own.
#[test]
fn validate_accepts_a_read_only_worker_that_owns_no_task() {
    validate_worker_plan_task_spawn(&verifier(&[])).expect("no gates declared is still valid");
    validate_worker_plan_task_spawn(&SpawnAgentParams {
        plan_id: Some("pl_owner".into()),
        ..verifier(&["./build.sh verify"])
    })
    .expect("a read-only Worker binds no task");
}

#[test]
fn worker_kinds_support_mutating_work_with_or_without_tasks() {
    for kind in ["qa", "release_manager", "opaque-worker-kind"] {
        validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Worker".into()),
            role_profile: Some(kind.into()),
            parent_tab_id: Some("parent_tab".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            authorized_gates: vec!["./build.sh verify".into()],
            ..Default::default()
        })
        .unwrap_or_else(|error| panic!("{kind}: {error}"));

        let missing_parent = validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Worker".into()),
            role_profile: Some(kind.into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            ..Default::default()
        })
        .expect_err("taskless kinds stay under a parent");
        assert!(
            missing_parent.contains("parent"),
            "{kind}: {missing_parent}"
        );

        validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Worker".into()),
            role_profile: Some(kind.into()),
            parent_tab_id: Some("parent_tab".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
            ..Default::default()
        })
        .expect("a kind Worker may mutate without a task");
    }

    for kind in ["qa", "release_manager", "opaque-worker-kind"] {
        validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Worker".into()),
            role_profile: Some(kind.into()),
            parent_tab_id: Some("parent_tab".into()),
            plan_task: Some("task_name".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
            ..Default::default()
        })
        .expect("a kind Worker may own a normal task");

        validate_worker_plan_task_spawn(&SpawnAgentParams {
            role: Some("Worker".into()),
            role_profile: Some(kind.into()),
            parent_tab_id: Some("parent_tab".into()),
            existing_tab_id: Some("existing-tab".into()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            ..Default::default()
        })
        .expect("kind does not restrict ordinary existing-tab handling");
    }
}

/// The safeguard that stays: source-changing work owns the task it changes
/// source under, so the lane and the review gate have something to hang on.
#[test]
fn validate_rejects_a_mutating_worker_that_owns_no_task() {
    let error = validate_worker_plan_task_spawn(&SpawnAgentParams {
        execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
        ..verifier(&[])
    })
    .expect_err("a mutating Worker must own a task");
    assert!(error.contains("--plan-task"), "{error}");
    assert!(error.contains("read_only"), "{error}");

    validate_worker_plan_task_spawn(&SpawnAgentParams {
        execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
        task_id: Some("task_owned".into()),
        ..verifier(&[])
    })
    .expect("a task-bound mutating Worker is unchanged");
}

#[test]
fn mutating_worker_without_task_still_refused_without_unplanned() {
    let error = validate_worker_plan_task_spawn(&SpawnAgentParams {
        role: Some("Worker".into()),
        execution_intent: Some(crate::plan::ExecutionIntent::Mutating),
        ..Default::default()
    })
    .expect_err("plain mutating Worker spawn must not create a loose agent");
    assert!(error.contains("--plan-task"), "{error}");
}

/// The mesh Worker: it changes source on a peer, but the origin plan store
/// is on another machine, so it declares no intent and owns no task. The
/// daemon a mesh spawn lands on is where that shape is finally admitted.
#[test]
fn validate_accepts_a_worker_that_declares_no_intent_and_owns_no_task() {
    validate_worker_plan_task_spawn(&SpawnAgentParams {
        role: Some("Worker".into()),
        ..Default::default()
    })
    .expect("a Worker with no binding and no declared intent is a supported shape");
}

#[test]
fn validate_accepts_dispatch_authorized_gates_for_task_bound_workers() {
    for intent in [
        crate::plan::ExecutionIntent::ReadOnly,
        crate::plan::ExecutionIntent::Mutating,
    ] {
        let mut params = SpawnAgentParams {
            plan_task: Some("opaque-task".into()),
            task_launch_authority: Some(crate::service::TaskLaunchAuthority {
                plan_id: "opaque-plan".into(),
                task_id: "opaque-task".into(),
                attempt_id: "opaque-attempt".into(),
                lane_base: None,
            }),
            execution_intent: Some(intent),
            ..verifier(&["opaque-gate"])
        };
        validate_worker_plan_task_spawn(&params).expect("dispatch carries enforced gates");
        params.task_launch_authority = None;
        assert!(validate_worker_plan_task_spawn(&params).is_err());
    }
}

#[test]
fn validate_rejects_gates_a_spawn_cannot_enforce() {
    let roleless = validate_worker_plan_task_spawn(&SpawnAgentParams {
        role: None,
        ..verifier(&["./build.sh verify"])
    })
    .expect_err("gates are a Worker concept");
    assert!(roleless.contains("--role Worker"), "{roleless}");

    let task_bound = validate_worker_plan_task_spawn(&SpawnAgentParams {
        plan_task: Some("spawn-bind".into()),
        ..verifier(&["./build.sh verify"])
    })
    .expect_err("a task-bound spawn declares gates on the dispatch");
    assert!(task_bound.contains("dispatch"), "{task_bound}");
}

/// The gates a verifier was scoped to must reach it, through the same
/// writer the plan-task dispatch path uses.
#[test]
fn declared_gates_reach_a_task_less_worker_brief() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _plan_id, _) = store_with_task(dir.path());
    let mut params = SpawnAgentParams {
        task: Some("Verify the merged lane.".into()),
        ..verifier(&["./build.sh test-one app verifier", "./build.sh verify"])
    };

    prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

    let brief = params.task.expect("the brief survives");
    assert!(brief.starts_with("Verify the merged lane."), "{brief}");
    let rendered = crate::service::plan::task_brief::render_authorized_gates(&[
        "./build.sh test-one app verifier".to_string(),
        "./build.sh verify".to_string(),
    ]);
    assert!(brief.ends_with(&rendered), "{brief}");
}

#[test]
fn a_task_less_worker_without_gates_keeps_its_brief_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _plan_id, _) = store_with_task(dir.path());
    let mut params = SpawnAgentParams {
        task: Some("Verify the merged lane.".into()),
        ..verifier(&[])
    };

    prepare_worker_plan_task_prompt(&store, &mut params).unwrap();

    assert_eq!(params.task.as_deref(), Some("Verify the merged lane."));
}

fn block_review_wip(store: &PlanStore, plan_id: &str) {
    let mut p = store.get(plan_id).unwrap();
    p.review_policy.wip_limit = 0;
    store.save(&mut p, 4).unwrap();
}

#[test]
fn preflight_allows_dispatch_at_review_threshold() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, _task_id) = store_with_task(dir.path());
    block_review_wip(&store, &plan_id);

    preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
}

#[test]
fn preflight_allows_dispatch_when_gate_is_clear() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, _task_id) = store_with_task(dir.path());
    preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
}

#[test]
fn dispatch_refuses_statuses_that_cannot_reach_startup() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, task_id) = store_with_task(dir.path());

    for status in [
        TaskStatus::Paused,
        TaskStatus::Done,
        TaskStatus::Cancelled,
        TaskStatus::InReview,
    ] {
        store
            .with_plan(&plan_id, 4, |plan| {
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .unwrap();
                task.status = status.clone();
                Ok(())
            })
            .unwrap();
        assert!(preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").is_err());
        let task = store.show_task(&task_id).unwrap().task;
        assert_eq!(task.status, status);
        assert!(task.execution_attempts.is_empty());
    }
}

#[test]
fn spawn_preflight_allows_review_debt_without_capacity_grants() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, task_id) = store_with_task(dir.path());
    let mut plan = store.get(&plan_id).unwrap();
    plan.review_policy.credit = -(plan.review_policy.wip_limit as i64);
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.kind = "review".into();
    task.tags.push("review-capacity".into());
    store.save(&mut plan, 4).unwrap();
    preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();

    store
        .grant_review_capacity(
            &plan_id,
            &task_id,
            crate::plan::ReviewCapacityClass::Review,
            crate::plan::AgentRef::from_tab("orch"),
            5,
        )
        .unwrap();
    preflight_worker_plan_task_wip_gate(&store, Some(&plan_id), "spawn-bind").unwrap();
}

#[test]
fn review_threshold_does_not_abort_task_binding() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, task_id) = store_with_task(dir.path());
    block_review_wip(&store, &plan_id);

    let binding = bind_worker_task_to_tab(
        &store,
        Some(&plan_id),
        "spawn-bind",
        "tab-worker",
        "Ada",
        None,
        None,
        crate::plan::ExecutionIntent::ReadOnly,
        None,
        None,
        None,
        None,
        None,
        "attempt-review",
        None,
        vec![],
        10,
    )
    .unwrap();
    assert_eq!(binding.task_id, task_id);

    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.assigned_tab_id.as_deref(), Some("tab-worker"));
    assert_eq!(task.lane.tab_id.as_deref(), Some("tab-worker"));
}

#[test]
fn ack_after_direct_spawn_enters_progress() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, task_id) = store_with_task(dir.path());
    let binding = bind_worker_task_to_tab(
        &store,
        Some(&plan_id),
        "spawn-bind",
        "tab-worker",
        "Ada",
        Some("session-worker"),
        Some("nonce-worker"),
        crate::plan::ExecutionIntent::ReadOnly,
        None,
        None,
        None,
        None,
        None,
        "attempt-ack",
        None,
        vec![],
        10,
    )
    .unwrap();
    store
        .acknowledge_task_attempt(&plan_id, &task_id, "tab-worker", &binding.attempt_id, 11)
        .unwrap();

    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == binding.attempt_id)
        .unwrap();
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.status_label.as_deref(), Some("acknowledged"));
    assert_eq!(attempt.acknowledged_at, Some(11));
    assert_eq!(attempt.executor_tab_id, "tab-worker");
    assert_eq!(attempt.session_id.as_deref(), Some("session-worker"));
    assert_eq!(
        attempt.session_binding_nonce.as_deref(),
        Some("nonce-worker")
    );
}

#[test]
fn respawn_of_blocked_task_yields_an_acknowledgeable_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, task_id) = store_with_task(dir.path());
    store
        .assign_task(&plan_id, &task_id, "worker-old", None, 4)
        .unwrap();
    let block_reason = "waiting for a dependency";
    store
        .with_plan(&plan_id, 5, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::Blocked;
            task.status_label = Some(block_reason.into());
            Ok(())
        })
        .unwrap();

    let binding = bind_worker_task_to_tab(
        &store,
        Some(&plan_id),
        "spawn-bind",
        "worker-respawn",
        "Ada",
        Some("session-respawn"),
        Some("nonce-respawn"),
        crate::plan::ExecutionIntent::ReadOnly,
        None,
        None,
        None,
        None,
        None,
        "attempt-respawn",
        None,
        vec![],
        6,
    )
    .unwrap();
    let pending = store.show_task(&task_id).unwrap().task;
    assert_eq!(pending.status, TaskStatus::Pending);
    assert!(store.get(&plan_id).unwrap().strides.iter().any(|stride| {
        stride.text.contains("unblocked by dispatch") && stride.text.contains(block_reason)
    }));
    store
        .acknowledge_task_attempt(&plan_id, &task_id, "worker-respawn", &binding.attempt_id, 7)
        .unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(
        task.execution_attempts.last().unwrap().acknowledged_at,
        Some(7)
    );
}

#[test]
fn task_binding_failure_leaves_task_unassigned() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id, task_id) = store_with_task(dir.path());
    block_review_wip(&store, &plan_id);

    let err = bind_worker_task_to_tab(
        &store,
        Some(&plan_id),
        "missing-task-fixture",
        "tab-worker",
        "Ada",
        None,
        None,
        crate::plan::ExecutionIntent::ReadOnly,
        None,
        None,
        None,
        None,
        None,
        "attempt-rollback",
        None,
        vec![],
        10,
    )
    .unwrap_err();
    assert!(!err.is_empty());

    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.assigned_tab_id, None);
    assert_eq!(task.lane.tab_id, None);
}
