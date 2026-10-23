use super::*;
use crate::multiplexer::types::TaskBinding;
use crate::plan::{AgentRef, OrchestratorRef, Plan, PlanMode};
use crate::service::plan::{
    interruption::TaskMutationAuthority, task_assignment::AssignmentRuntimeBinding,
    task_attempts::BoundWorkerReport, NewTask,
};
use crate::service::test_fixture::ServiceFixture;

#[test]
fn provider_switch_keeps_roleless_spawn_metadata_without_plan_authority() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let binding = crate::multiplexer::types::TaskBinding {
        spawn_handle: Some("opaque-spawn-task".into()),
        bound_at: 17,
        ..Default::default()
    };
    mux.get_tab_mut(&id).unwrap().task_binding = Some(binding.clone());
    let fixture = ServiceFixture::new(mux);
    assert!(fixture
        .service
        .authorize_provider_switch_binding(&id, "source", "target", "nonce")
        .unwrap()
        .is_none());
    fixture
        .service
        .finalize_runtime_worker_session(&id, "new-thread", "new-nonce")
        .unwrap();
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .task_binding
            .as_ref(),
        Some(&binding)
    );
}

#[test]
fn unavailable_target_refuses_before_retiring_or_mutating_source() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("source-fixture".into());
    tab.agent_session_id = Some("source-thread".into());
    tab.session_binding_nonce = Some("source-nonce".into());
    tab.reported_session_binding_nonce = Some("source-nonce".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("source-fixture"),
    ));
    assert!(fixture
        .service
        .switch_provider_in_place(&id, "missing-target", None, None, false)
        .is_err());
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("source-fixture"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("source-thread"));
    assert_eq!(tab.session_binding_nonce.as_deref(), Some("source-nonce"));
}

#[test]
fn host_handoff_carries_identity_and_archive_reference_as_data() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.title = "Opaque".into();
    tab.description = Some("description-token".into());
    tab.cwd = Some("cwd-token".into());
    tab.agent_type = Some("source-token".into());
    tab.agent_session_id = Some("session-token".into());
    let fixture = ServiceFixture::new(mux);
    let tab = fixture.service.mux.read().get_tab(&id).cloned().unwrap();
    let input = fixture
        .service
        .provider_switch_input(&tab, &[], std::path::Path::new("archive-token"))
        .unwrap();
    assert_eq!(prepare_handoff(&input).unwrap().text(), input);
    assert!(prepare_handoff("opaque invalid frame")
        .err()
        .unwrap()
        .starts_with("provider_switch_handoff_invalid:"));
    let frame = codeterm_wire::message_markers::parse_host_injected(&input).unwrap();
    assert_eq!(frame.kind, "system");
    assert_eq!(
        frame.attrs.get("context").map(String::as_str),
        Some(provider_handoff::input::HANDOFF_CONTEXT)
    );
    let context: serde_json::Value =
        serde_json::from_str(frame.payload.split("\n\n").nth(1).unwrap()).unwrap();
    assert_eq!(context["tab_id"], id);
    assert_eq!(context["title"], tab.title);
    assert_eq!(context["description"], "description-token");
    assert_eq!(context["previous_provider"], "source-token");
    assert_eq!(context["previous_session_id"], "session-token");
    assert_eq!(context["previous_transcript_path"], "archive-token");
    assert_eq!(context["cwd"], "cwd-token");
    assert!(!context["pad_path"].as_str().unwrap().is_empty());
    assert!(input.len() <= provider_handoff::input::MAX_INPUT_BYTES);
}

#[test]
fn source_archive_failure_releases_authority_and_fences_the_attempt() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("source-provider".into());
    tab.agent_session_id = Some("source-thread".into());
    tab.session_binding_nonce = Some("source-nonce".into());
    tab.reported_session_binding_nonce = Some("source-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "archive failure".into(),
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
            &id,
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
        .with_plan(&plan.id, 4, |plan| {
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
    let carry = fixture
        .service
        .authorize_provider_switch_binding(
            &id,
            "source-provider",
            "target-provider",
            "source-nonce",
        )
        .unwrap();
    assert!(carry.is_some());
    let source = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    let _claim = fixture.service.claim_restart(&id, false).unwrap();
    fixture.service.stage_runtime_restart_binding(&id, carry);
    let (tx, rx) = std::sync::mpsc::channel();
    let listener = fixture.ui.listen("coordination-incident", move |payload| {
        if payload["kind"] == "worker_interruption_checkpoint_failed" {
            tx.send(()).unwrap();
        }
    });
    let retire = || -> Result<(), String> {
        let _cleanup = SwitchBindingCleanup {
            service: &fixture.service,
            tab_id: &id,
            interrupt_on_failure: true,
        };
        assert!(!fixture.service.signal_worker_interruption(
            &id,
            crate::service::worker_interruption::WorkerInterruptionEvent::ForcedContextReset
        ));
        assert!(fixture.service.mux.write().remove_tab(&id).is_some());
        fixture
            .service
            .archive_provider_switch_source(&id, &source)?;
        Ok(())
    };
    assert!(retire()
        .unwrap_err()
        .contains("tab closed after source retirement"));
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    fixture.ui.unlisten(listener);
    let fenced = store.show_task(&task).unwrap().task;
    assert!(fenced.interruption.handoff_required);
    assert!(fenced.execution_attempts[0].closed_at.is_some());
    assert_eq!(
        fenced.execution_attempts[0].interruption.reason.as_deref(),
        Some("forced_context_reset")
    );
    assert!(!fixture
        .service
        .carry_authorized_runtime_restart_binding(&id, "foreign-thread", "foreign-nonce")
        .unwrap());
}

#[test]
fn provider_history_reads_only_the_tabs_own_archive() {
    let (mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let pad = fixture.service.ensure_agent_pad(&id).unwrap();
    let message = crate::agent::chat_types::ChatMsg {
        kind: "user".into(),
        content: "opaque-history".into(),
        ..Default::default()
    };
    let path = pad
        .write_inbox_body(&serde_json::to_vec(&vec![message]).unwrap())
        .unwrap();
    let messages = fixture
        .service
        .provider_switch_history(&id, path.to_str().unwrap())
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "opaque-history");
    assert!(fixture
        .service
        .provider_switch_history(&id, "../other/inbox/archive.md")
        .is_err());
    assert!(fixture
        .service
        .provider_switch_history(&id, "state.md")
        .is_err());
}

#[test]
fn provider_switch_retires_source_bootstrap_without_replaying_it() {
    let (mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .create_pending_delivery(
            &id,
            crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
            "opaque-source-bootstrap",
        )
        .unwrap();
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pending_delivery
        .is_some());
    fixture.service.retire_provider_switch_input(&id).unwrap();
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pending_delivery
        .is_none());
}

#[test]
fn provider_switch_restores_plan_attempt_for_acknowledgement_and_report() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("source-provider".into());
    tab.agent_session_id = Some("source-thread".into());
    tab.session_binding_nonce = Some("source-nonce".into());
    tab.reported_session_binding_nonce = Some("source-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "provider switch carry".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker task".into(),
                acceptance_criteria: vec!["the attempt retains the target session".into()],
                test_plan: Some("verify the rebound task attempt and report".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task_id,
            &tab_id,
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
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .task_binding = Some(TaskBinding {
        spawn_handle: Some("opaque-spawn-metadata".into()),
        bound_at: 3,
        ..Default::default()
    });
    let acknowledgement = store
        .acknowledge_live_task_attempt_for_tab_with_runtime_binding(
            &tab_id,
            Some(&plan.id),
            Some(&task_id),
            Some(("source-thread", "source-nonce")),
            4,
        )
        .unwrap();
    let acknowledged_at = acknowledgement.attempt.acknowledged_at;
    let original_lane = store.show_task(&task_id).unwrap().task.lane.clone();

    let authority = fixture
        .service
        .authorize_provider_switch_binding(
            &tab_id,
            "source-provider",
            "target-provider",
            "source-nonce",
        )
        .unwrap()
        .expect("the exact plan attempt is carried even when tab metadata is incomplete");
    let restored_tab_binding = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .task_binding
        .clone()
        .unwrap();
    assert_eq!(
        restored_tab_binding.plan_id.as_deref(),
        Some(plan.id.as_str())
    );
    assert_eq!(
        restored_tab_binding.task_id.as_deref(),
        Some(task_id.as_str())
    );
    assert_eq!(
        restored_tab_binding.attempt_id.as_deref(),
        Some(attempt_id.as_str())
    );
    assert_eq!(
        restored_tab_binding.spawn_handle.as_deref(),
        Some("opaque-spawn-metadata")
    );
    fixture
        .service
        .stage_runtime_restart_binding(&tab_id, Some(authority));
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("target-provider".into());
        tab.agent_session_id = Some("target-thread".into());
        tab.session_binding_nonce = Some("target-nonce".into());
        tab.reported_session_binding_nonce = Some("target-nonce".into());
    }
    fixture
        .service
        .finalize_runtime_worker_session(&tab_id, "target-thread", "target-nonce")
        .unwrap();
    assert!(fixture
        .service
        .finalize_authorized_runtime_restart_binding(&tab_id, "target-thread", "target-nonce")
        .unwrap());

    let rebound = store.show_task(&task_id).unwrap().task;
    assert_eq!(rebound.execution_attempts.len(), 1);
    assert_eq!(rebound.execution_attempts[0].id, attempt_id);
    assert_eq!(rebound.lane, original_lane);
    assert_eq!(
        rebound.execution_attempts[0].provider.as_deref(),
        Some("target-provider")
    );
    assert_eq!(
        rebound.execution_attempts[0].session_id.as_deref(),
        Some("target-thread")
    );
    assert_eq!(
        rebound.execution_attempts[0]
            .session_binding_nonce
            .as_deref(),
        Some("target-nonce")
    );
    assert_eq!(
        rebound.execution_attempts[0].acknowledged_at,
        acknowledged_at
    );

    let repeated_ack = store
        .acknowledge_live_task_attempt_for_tab_with_runtime_binding(
            &tab_id,
            Some(&plan.id),
            Some(&task_id),
            Some(("target-thread", "target-nonce")),
            5,
        )
        .unwrap();
    assert!(repeated_ack.already_acknowledged);
    let accepted_report = store
        .ingest_bound_worker_report(
            &plan.id,
            &task_id,
            &AgentRef::from_tab(&tab_id),
            BoundWorkerReport {
                message: "provider switch report".into(),
                status: Some(codeterm_types::agent::ReportStatus::InReview),
                authority: Some(TaskMutationAuthority {
                    tab_id: tab_id.clone(),
                    session_id: "target-thread".into(),
                    session_binding_nonce: "target-nonce".into(),
                    owner_epoch: rebound.interruption.owner_epoch,
                }),
                ..Default::default()
            },
            6,
        )
        .unwrap();
    assert!(accepted_report);
    let reported = store.show_task(&task_id).unwrap().task;
    assert_eq!(reported.execution_attempts[0].id, attempt_id);
    assert_eq!(
        reported.execution_attempts[0]
            .report
            .as_ref()
            .and_then(|report| report.message.as_deref()),
        Some("provider switch report")
    );
}

#[test]
fn provider_switch_refuses_an_attempt_owned_by_another_tab_before_mutation() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("provider-switch-fixture".into(), None);
    let tab_id = mux.create_tab(&workspace.id).unwrap();
    let foreign_tab_id = mux.create_tab(&workspace.id).unwrap();
    for (id, session, nonce) in [
        (&tab_id, "source-thread", "source-nonce"),
        (&foreign_tab_id, "foreign-thread", "foreign-nonce"),
    ] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("source-provider".into());
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
    }
    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "foreign provider switch".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task_id,
            &tab_id,
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
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&foreign_tab_id)
        .unwrap()
        .task_binding = Some(TaskBinding {
        plan_id: Some(plan.id.clone()),
        task_id: Some(task_id.clone()),
        attempt_id: Some(attempt_id),
        bound_at: 3,
        ..Default::default()
    });

    let result = fixture.service.authorize_provider_switch_binding(
        &foreign_tab_id,
        "source-provider",
        "target-provider",
        "foreign-nonce",
    );
    assert!(result.is_err());
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&foreign_tab_id)
        .cloned()
        .unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("source-provider"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("foreign-thread"));
    assert_eq!(tab.session_binding_nonce.as_deref(), Some("foreign-nonce"));
    let attempt = &store.show_task(&task_id).unwrap().task.execution_attempts[0];
    assert_eq!(attempt.session_id.as_deref(), Some("source-thread"));
    assert_eq!(
        attempt.session_binding_nonce.as_deref(),
        Some("source-nonce")
    );
}

#[test]
fn unavailable_switch_details_omit_the_executable_search_path() {
    let error = crate::service::spawn_cli::ProviderPreflightError::BinaryUnavailable {
        provider_id: "target".into(),
        display_name: "Target".into(),
        binary: "target-cli".into(),
        searched: vec![std::path::PathBuf::from("opaque-private-path".repeat(1000))],
        install_hint: None,
    };
    let message = super::switch_preflight_error(error);
    assert!(message.starts_with("provider_switch_unavailable:"));
    assert!(message.contains("target-cli"));
    assert!(!message.contains("opaque-private-path"));
    assert!(message.len() < 256);
}
