use super::*;
use crate::multiplexer::types::TaskBinding;
use crate::plan::{OrchestratorRef, Plan, PlanMode};
use crate::service::plan::{task_assignment::AssignmentRuntimeBinding, NewTask};

#[test]
fn task_worker_switch_from_pty_carries_attempt_and_delivers_one_handoff() {
    let (fixture, task, attempt) = switch_worker_fixture();
    let store = &fixture.service.plan_store;
    assert!(fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("supplied-model".into()),
            None,
            false
        )
        .err()
        .unwrap()
        .starts_with("provider_switch_busy:"));
    let outcome = fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("supplied-model".into()),
            None,
            true,
        )
        .unwrap();
    assert!(outcome.binding_restored && outcome.handoff_delivered);
    let persisted = store.show_task(&task).unwrap().task;
    assert_eq!(persisted.execution_attempts.len(), 1);
    let carried = &persisted.execution_attempts[0];
    assert_eq!(carried.id, attempt);
    assert_eq!(carried.provider.as_deref(), Some(PROVIDER));
    assert_eq!(
        carried.session_id.as_deref(),
        Some(outcome.session_id.as_str())
    );
    assert_eq!(
        carried.provider_resolved_model.as_deref(),
        Some("supplied-model")
    );
    assert_eq!(carried.acknowledged_at, Some(4));
    assert!(carried.closed_at.is_none());
    assert!(!persisted.interruption.handoff_required);
    let requests = fixture.requests();
    let submissions: Vec<_> = requests
        .iter()
        .filter(|frame| frame["method"] == "submitTurn")
        .collect();
    assert_eq!(submissions.len(), 1);
    let payload = serde_json::to_string(&submissions[0]["params"]).unwrap();
    assert_eq!(payload.matches("provider-handoff").count(), 1);
    assert!(payload.contains(&attempt));
    let opens: Vec<_> = requests
        .iter()
        .filter(|frame| frame["method"] == "openThread")
        .collect();
    assert_eq!(opens.len(), 1);
    let prompt = opens[0]["params"]["instructions"].as_str().unwrap();
    let preset = crate::service::persona::resolve_ambient(
        Some(crate::multiplexer::types::AgentRole::Worker),
        None,
        None,
    )
    .unwrap();
    let contract = preset.contract_guidance();
    for fragment in [
        crate::prompts::AGENT_CODETERM_PRIMER.trim(),
        crate::prompts::AGENT_ROUTING_CONTRACT.trim(),
        crate::prompts::WORKER_CODETERM_CONTEXT.trim(),
        crate::prompts::AGENT_PAD_RULE.trim(),
        crate::prompts::RICH_RENDERING_GUIDANCE.trim(),
        crate::service::orchestrator::ROLE_AUTHORITY,
        contract.as_str(),
    ] {
        assert!(!fragment.is_empty());
        assert_eq!(prompt.matches(fragment).count(), 1, "{fragment}");
    }
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    assert_eq!(
        tab.agent_runtime_config.as_ref().unwrap()["systemPrompt"],
        prompt
    );
    assert_eq!(
        tab.restorable_launch.identity_instructions.as_deref(),
        Some(prompt)
    );
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .boot_failure_reason
        .is_none());
}

#[test]
fn failed_target_launch_fences_the_worker_and_releases_carry_authority() {
    let (fixture, task, attempt) = switch_worker_fixture();
    let (tx, rx) = std::sync::mpsc::channel();
    let listener = fixture.ui.listen("coordination-incident", move |payload| {
        if payload["kind"] == "worker_interruption_checkpoint_failed" {
            tx.send(()).unwrap();
        }
    });
    let error = fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("refused-model".into()),
            None,
            true,
        )
        .err()
        .unwrap();
    assert!(error.contains("fixture load refused"), "{error}");
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    fixture.ui.unlisten(listener);
    let fenced = fixture.service.plan_store.show_task(&task).unwrap().task;
    assert_eq!(fenced.execution_attempts.len(), 1);
    assert_eq!(fenced.execution_attempts[0].id, attempt);
    assert!(fenced.execution_attempts[0].closed_at.is_some());
    assert_eq!(
        fenced.execution_attempts[0].interruption.reason.as_deref(),
        Some("forced_context_reset")
    );
    assert!(fenced.interruption.handoff_required);
    assert!(fenced.assigned_tab_id.is_none());
    assert!(!fixture
        .service
        .restart_carries_worker_interruption(&fixture.tab));
    assert!(!fixture
        .service
        .carry_authorized_runtime_restart_binding(&fixture.tab, "foreign-thread", "foreign-nonce")
        .unwrap());
    assert!(fixture
        .requests()
        .iter()
        .all(|frame| frame["method"] != "submitTurn"));
}

#[test]
fn failed_runtime_restart_releases_the_task_attempt_authority() {
    let (fixture, task, attempt) = switch_worker_fixture();
    fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("supplied-model".into()),
            None,
            true,
        )
        .unwrap();
    fixture
        .service
        .ext::<crate::service::agent_restart::RestartTracker>()
        .lock()
        .last_restart
        .remove(&fixture.tab);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .agent_runtime_config = Some(json!({"systemPrompt":"opaque refused identity"}));
    let error = fixture
        .service
        .agent_restart_observed(&fixture.tab, true, 0)
        .unwrap_err();
    assert!(matches!(error, RestartAttemptError::Failed { .. }));
    assert!(!fixture
        .service
        .carry_authorized_runtime_restart_binding(&fixture.tab, "foreign-thread", "foreign-nonce")
        .unwrap());
    let persisted = fixture.service.plan_store.show_task(&task).unwrap().task;
    assert_eq!(persisted.execution_attempts.len(), 1);
    assert_eq!(persisted.execution_attempts[0].id, attempt);
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|frame| frame["method"] == "submitTurn")
            .count(),
        1
    );
}

fn switch_worker_fixture() -> (RuntimeFixture, String, String) {
    let fixture = RuntimeFixture::new();
    agent_runtime::engine::stop_blocking(&fixture.service, &fixture.tab).unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("source-provider"),
    ));
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.agent_type = Some("source-provider".into());
        tab.agent_session_id = Some("source-thread".into());
        tab.session_binding_nonce = Some("source-nonce".into());
        tab.reported_session_binding_nonce = Some("source-nonce".into());
        tab.backing = TabBacking::Pty;
        tab.pty_id = None;
        tab.agent_booting = false;
    }
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "switch fixture".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "opaque work".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task,
            &fixture.tab,
            None,
            Some("worker"),
            Some(&AssignmentRuntimeBinding {
                session_id: "source-thread".into(),
                session_binding_nonce: "source-nonce".into(),
                provider: Some("source-provider".into()),
                model: None,
                session_model: None,
                reasoning_effort: None,
            }),
            None,
            3,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|entry| entry.id == task)
                .unwrap();
            task.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
            task.lane.worktree_path = Some(fixture.temp.path().display().to_string());
            Ok(())
        })
        .unwrap();
    let attempt = store.show_task(&task).unwrap().task.execution_attempts[0]
        .id
        .clone();
    store
        .acknowledge_task_attempt(&plan.id, &task, &fixture.tab, &attempt, 4)
        .unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .task_binding = Some(TaskBinding {
        plan_id: Some(plan.id.clone()),
        task_id: Some(task.clone()),
        attempt_id: Some(attempt.clone()),
        ..Default::default()
    });
    (fixture, task, attempt)
}

#[test]
fn loaded_runtime_supersedes_stale_boot_failure_and_can_be_repaired() {
    let fixture = RuntimeFixture::new();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        crate::service::boot_stage::apply_boot_stage(
            tab,
            codeterm_types::pty::BootStage::SpawnFailed,
            Some("provider_switch_binding_failed: opaque"),
        );
    }
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    agent_runtime::engine::publish(&fixture.service, &fixture.tab, &session);
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .cloned()
        .unwrap();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
    assert!(tab.boot_failure_reason.is_none());
    let caller = crate::service::AuthenticatedPlanCaller {
        tab_id: Some(fixture.tab.clone()),
        session_id: tab.agent_session_id.clone(),
        binding_nonce: tab.session_binding_nonce.clone(),
    };
    let target = fixture
        .service
        .authorize_tab_repair(&fixture.tab, &caller)
        .unwrap();
    let repaired = crate::async_runtime::block_on(
        fixture
            .service
            .repair_tab_from_live_evidence(target, &caller, None),
    )
    .unwrap();
    assert_eq!(repaired.tab_id, fixture.tab);
    assert!(agent_runtime::engine::has_live_bound_session(
        fixture.service.mux.read().get_tab(&fixture.tab).unwrap()
    ));
}
