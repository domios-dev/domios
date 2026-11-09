// WHAT: executor retirement before local respawn, including failed and stale shutdowns.
// WHY: retiring attempt metadata does not stop a live lane writer.
// ORACLE: termination precedes tab removal and replacement; failures retain the tab and refuse reuse.
use super::*;
use crate::multiplexer::types::TaskBinding;
use crate::plan::{ExecutionIntent, TaskExecutionAttempt};
use crate::service::test_fixture::ServiceFixture;
use std::cell::Cell;

fn fixture() -> (ServiceFixture, TaskRespawnDraft) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.role = Some(AgentRole::Worker);
    tab.agent_session_id = Some("source-session".into());
    tab.session_binding_nonce = Some("source-nonce".into());
    tab.reported_session_binding_nonce = Some("source-nonce".into());
    tab.task_binding = Some(TaskBinding {
        plan_id: Some("plan".into()),
        task_id: Some("task".into()),
        attempt_id: Some("source".into()),
        ..Default::default()
    });
    let fixture = ServiceFixture::new(mux);
    let draft = TaskRespawnDraft {
        launch_profile: None,
        plan_id: "plan".into(),
        task_id: "task".into(),
        execution_intent: ExecutionIntent::Mutating,
        attempt_id: "replacement".into(),
        source_attempt: TaskExecutionAttempt {
            id: "source".into(),
            executor_tab_id: tab_id,
            session_id: Some("source-session".into()),
            session_binding_nonce: Some("source-nonce".into()),
            acknowledged_at: Some(1),
            ..Default::default()
        },
        provider: "replacement-provider".into(),
        planned_model: None,
        model: None,
        speed: None,
        reasoning_effort: None,
        parent_tab_id: None,
        workspace_id: None,
        title: None,
        explicit_title: false,
        cwd: fixture.temp.path().display().to_string(),
        task_prompt: String::new(),
        report_command: String::new(),
        plan_skill_manifest: Vec::new(),
        authorized_gates: Some(Vec::new()),
    };
    let mut plan = crate::plan::Plan::new(
        "Executor retirement".into(),
        fixture.temp.path().display().to_string(),
        crate::plan::OrchestratorRef::from_tab("owner"),
    );
    plan.id = "plan".into();
    let mut task: crate::plan::Task = serde_json::from_value(serde_json::json!({
        "id": "task", "name": "executor", "type": "code", "status": "in_progress",
        "assigned_tab_id": draft.source_attempt.executor_tab_id,
    }))
    .unwrap();
    task.execution_attempts.push(draft.source_attempt.clone());
    plan.tasks.push(task);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    (fixture, draft)
}

fn restored_fixture() -> (ServiceFixture, TaskRespawnDraft) {
    let (fixture, mut draft) = fixture();
    draft.source_attempt.provider = Some("source-provider".into());
    let tab_id = &draft.source_attempt.executor_tab_id;
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(tab_id)
        .unwrap()
        .agent_type = Some("source-provider".into());
    let nonce = fixture.service.rotate_session_binding_nonce(tab_id);
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.reported_session_binding_nonce = Some(nonce);
    tab.restored_from_snapshot = true;
    drop(mux);
    (fixture, draft)
}

#[test]
fn respawn_retires_same_attempt_after_proven_restore_nonce_rotation() {
    let (fixture, draft) = restored_fixture();
    let source = &draft.source_attempt;
    let terminations = Cell::new(0);
    fixture
        .service
        .retire_respawn_executor_with(&draft, |tabs| {
            assert_eq!(tabs.len(), 1);
            let tab = &tabs[0];
            assert_eq!(tab.id, source.executor_tab_id);
            assert_eq!(tab.agent_session_id.as_deref(), Some("source-session"));
            assert_ne!(tab.session_binding_nonce, source.session_binding_nonce);
            assert_eq!(
                tab.task_binding.as_ref().unwrap().attempt_id.as_deref(),
                Some("source")
            );
            terminations.set(terminations.get() + 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(terminations.get(), 1);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&source.executor_tab_id)
        .is_none());
    fixture
        .service
        .retire_respawn_executor_with(&draft, |_| panic!("termination replayed"))
        .unwrap();
}

#[test]
fn restored_respawn_refuses_foreign_task_attempt_session_provider_and_unproven_rotation() {
    for field in [
        "plan",
        "task",
        "attempt",
        "session",
        "provider",
        "reported_nonce",
        "lineage",
        "lineage_session",
        "source_session",
        "source_provider",
    ] {
        let (fixture, mut draft) = restored_fixture();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux
                .get_tab_mut(&draft.source_attempt.executor_tab_id)
                .unwrap();
            match field {
                "plan" => tab.task_binding.as_mut().unwrap().plan_id = Some("foreign".into()),
                "task" => tab.task_binding.as_mut().unwrap().task_id = Some("foreign".into()),
                "attempt" => tab.task_binding.as_mut().unwrap().attempt_id = Some("foreign".into()),
                "session" => tab.agent_session_id = Some("foreign".into()),
                "provider" => tab.agent_type = Some("foreign".into()),
                "reported_nonce" => tab.reported_session_binding_nonce = None,
                "lineage" => tab.prior_session_binding_nonces.clear(),
                "lineage_session" => {
                    tab.prior_session_binding_nonces[0].session_id = Some("foreign".into())
                }
                "source_session" => draft.source_attempt.session_id = None,
                "source_provider" => draft.source_attempt.provider = None,
                _ => unreachable!(),
            }
        }
        assert!(
            fixture
                .service
                .retire_respawn_executor_with(&draft, |_| panic!(
                    "foreign executor terminated: {field}"
                ))
                .is_err(),
            "{field}"
        );
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&draft.source_attempt.executor_tab_id)
            .is_some());
    }
}

#[test]
fn provider_change_retires_the_old_writer_before_replacement_and_keeps_dirty_work() {
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let tab_id = &draft.source_attempt.executor_tab_id;
    svc.mux.write().get_tab_mut(tab_id).unwrap().agent_type = Some("source-provider".into());
    let dirty = fixture.temp.path().join("uncommitted.txt");
    std::fs::write(&dirty, b"owned dirty work").unwrap();
    let live_writers = Cell::new(1);
    svc.retire_respawn_executor_with(&draft, |tabs| {
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].agent_type.as_deref(), Some("source-provider"));
        let mux = svc.mux.read();
        assert!(mux.get_tab(tab_id).is_some());
        assert!(mux.ensure_tab_membership_mutable(tab_id).is_err());
        live_writers.set(live_writers.get() - 1);
        Ok(())
    })
    .unwrap();
    assert!(svc.mux.read().get_tab(tab_id).is_none());
    assert_eq!(
        live_writers.get(),
        0,
        "replacement must not overlap its source"
    );
    live_writers.set(live_writers.get() + 1);
    assert_eq!(live_writers.get(), 1);
    assert_eq!(std::fs::read(&dirty).unwrap(), b"owned dirty work");
    svc.retire_respawn_executor_with(&draft, |_| panic!("already confirmed"))
        .unwrap();
    let mut stale = draft.clone();
    stale.source_attempt.session_binding_nonce = Some("foreign-nonce".into());
    assert!(svc.retire_respawn_executor(&stale).is_err());
}

#[test]
fn failed_termination_retains_source_and_never_authorizes_missing_tab_reuse() {
    for failure in ["kill denied", "exit acknowledgement deadline"] {
        let (fixture, draft) = fixture();
        let svc = &fixture.service;
        let tab_id = &draft.source_attempt.executor_tab_id;
        let result = svc.retire_respawn_executor_with(&draft, |_| Err(failure.into()));
        assert!(result.unwrap_err().contains(failure));
        assert!(svc.mux.read().get_tab(tab_id).is_some());
        assert!(svc.mux.read().ensure_tab_membership_mutable(tab_id).is_ok());
        svc.mux.write().close_tab(tab_id).unwrap();
        assert!(svc.retire_respawn_executor(&draft).is_err());
    }
}

#[test]
fn unknown_pty_refuses_repeated_retirement() {
    let (fixture, draft) = fixture();
    let tab_id = &draft.source_attempt.executor_tab_id;
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(tab_id)
        .unwrap()
        .pty_id = Some("unknown-pty".into());
    assert!(fixture.service.retire_respawn_executor(&draft).is_err());
    assert!(fixture.service.retire_respawn_executor(&draft).is_err());
    assert!(fixture.service.mux.read().get_tab(tab_id).is_some());
}

#[test]
fn stale_attempt_session_nonce_and_role_refuse_before_termination() {
    for field in [
        "plan",
        "task",
        "attempt",
        "session",
        "nonce",
        "reported_nonce",
        "role",
        "protected",
    ] {
        let (fixture, draft) = fixture();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux
                .get_tab_mut(&draft.source_attempt.executor_tab_id)
                .unwrap();
            match field {
                "plan" => tab.task_binding.as_mut().unwrap().plan_id = Some("foreign".into()),
                "task" => tab.task_binding.as_mut().unwrap().task_id = Some("foreign".into()),
                "attempt" => tab.task_binding.as_mut().unwrap().attempt_id = Some("foreign".into()),
                "session" => tab.agent_session_id = Some("foreign".into()),
                "nonce" => tab.session_binding_nonce = Some("foreign".into()),
                "reported_nonce" => tab.reported_session_binding_nonce = Some("foreign".into()),
                "role" => tab.role = Some(AgentRole::Manager),
                "protected" => tab.protected = true,
                _ => unreachable!(),
            }
        }
        assert!(fixture
            .service
            .retire_respawn_executor_with(&draft, |_| panic!("foreign owner"))
            .is_err());
    }
}

#[test]
fn retirement_terminates_owned_utility_ptys_and_leaves_other_children() {
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let source = &draft.source_attempt.executor_tab_id;
    let (shell, unrelated) = {
        let mut mux = svc.mux.write();
        let workspace = mux.list_workspaces()[0].id.clone();
        mux.get_tab_mut(source).unwrap().team_id = Some("team".into());
        let shell = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&shell).unwrap();
        tab.parent_tab_id = Some(source.clone());
        tab.spawned_by_tab_id = Some(source.clone());
        tab.team_id = Some("team".into());
        tab.purpose = crate::multiplexer::types::ShellPurpose::Build;
        tab.pty_id = Some("owned-shell".into());
        let unrelated = mux.create_tab(&workspace).unwrap();
        mux.get_tab_mut(&unrelated).unwrap().parent_tab_id = Some(source.clone());
        (shell, unrelated)
    };
    svc.retire_respawn_executor_with(&draft, |tabs| {
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            tabs.iter()
                .filter_map(|tab| tab.pty_id.as_deref())
                .collect::<Vec<_>>(),
            ["owned-shell"]
        );
        Ok(())
    })
    .unwrap();
    let mux = svc.mux.read();
    assert!(mux.get_tab(&shell).is_none());
    assert!(mux.get_tab(&unrelated).is_some());
}

#[test]
fn generation_change_during_termination_refuses_tab_removal_and_replacement() {
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let tab_id = &draft.source_attempt.executor_tab_id;
    let result = svc.retire_respawn_executor_with(&draft, |_| {
        svc.mux.write().get_tab_mut(tab_id).unwrap().pty_id = Some("new-generation".into());
        Ok(())
    });
    assert!(result.unwrap_err().contains("generation changed"));
    assert!(svc.mux.read().get_tab(tab_id).is_some());
    assert!(svc.retire_respawn_executor(&draft).is_err());
}

#[test]
fn missing_tab_and_remote_source_do_not_supply_termination_proof() {
    let (fixture, mut draft) = fixture();
    fixture
        .service
        .mux
        .write()
        .close_tab(&draft.source_attempt.executor_tab_id)
        .unwrap();
    assert!(fixture.service.retire_respawn_executor(&draft).is_err());
    draft.source_attempt.mesh_peer = Some("peer".into());
    assert!(fixture.service.retire_respawn_executor(&draft).is_err());
}

#[test]
fn native_pty_exit_is_observed_before_retirement_returns() {
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let gate = svc.pty.lock().spawn_gate();
    let built =
        crate::pty::manager::build_session(gate, None, None, 80, 24, Default::default()).unwrap();
    let (pty_id, session) = svc.pty.lock().register_session(None, built);
    svc.mux
        .write()
        .get_tab_mut(&draft.source_attempt.executor_tab_id)
        .unwrap()
        .pty_id = Some(pty_id.clone());
    svc.retire_respawn_executor(&draft).unwrap();
    assert!(session.child.lock().unwrap().try_wait().unwrap().is_some());
    assert!(svc.pty.lock().get_session(&pty_id).is_none());
    assert!(svc
        .mux
        .read()
        .get_tab(&draft.source_attempt.executor_tab_id)
        .is_none());
}

#[test]
fn restored_chat_worker_close_requires_absence_or_a_live_runtime_witness() {
    use crate::multiplexer::types::TabBacking;
    use crate::service::agent_runtime::{engine, model::RuntimeFailure};

    for live in [false, true] {
        let (fixture, draft) = fixture();
        let svc = &fixture.service;
        let source = &draft.source_attempt;
        let prepared = {
            let mut mux = svc.mux.write();
            let tab = mux.get_tab_mut(&source.executor_tab_id).unwrap();
            tab.agent_type = Some("source-provider".into());
            tab.backing = TabBacking::ChatBackend {
                plugin_id: "source-provider".into(),
                session_id: "source-session".into(),
            };
            tab.clone()
        };
        let snapshot = svc.mux.read().to_session_snapshot();
        *svc.mux.write() = MultiplexerState::from_session_snapshot(&snapshot);
        assert!(
            svc.mux
                .read()
                .get_tab(&source.executor_tab_id)
                .unwrap()
                .restored_from_snapshot
        );
        let _runtime = live.then(|| engine::test_support::live_runtime(&prepared));
        let process = engine::for_tab(&source.executor_tab_id)
            .and_then(|runtime| runtime.process.lock().clone());

        svc.close_tab_guarded(Some("owner"), &source.executor_tab_id, true)
            .unwrap();

        assert!(svc.mux.read().get_tab(&source.executor_tab_id).is_none());
        assert!(engine::for_tab(&source.executor_tab_id).is_none());
        if let Some(process) = process {
            assert!(matches!(
                *process.ended.borrow(),
                Some(RuntimeFailure::ProcessExited { .. })
            ));
        }
        let (mux, _) = MultiplexerState::new();
        let restarted = ServiceFixture::new_at(mux, fixture.temp.path());
        assert!(restarted
            .service
            .plan_store
            .task_executor_termination_confirmed("plan", "task", source)
            .unwrap());
    }
}

#[test]
fn runtime_provider_change_keeps_one_live_executor_after_confirmed_exit() {
    use crate::multiplexer::types::TabBacking;
    use crate::service::agent_runtime::engine;
    use crate::service::agent_runtime::model::RuntimeFailure;
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let source = &draft.source_attempt.executor_tab_id;
    let prepared = {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(source).unwrap();
        tab.agent_type = Some("source-provider".into());
        tab.backing = TabBacking::ChatBackend {
            plugin_id: "source-provider".into(),
            session_id: "source-session".into(),
        };
        tab.clone()
    };
    let _old_runtime = engine::test_support::live_runtime(&prepared);
    let old_process = engine::for_tab(source)
        .unwrap()
        .process
        .lock()
        .clone()
        .unwrap();
    svc.retire_respawn_executor(&draft).unwrap();
    assert!(matches!(
        *old_process.ended.borrow(),
        Some(RuntimeFailure::ProcessExited { .. })
    ));
    assert!(engine::for_tab(source).is_none());
    let replacement = {
        let mut mux = svc.mux.write();
        let workspace = mux.list_workspaces()[0].id.clone();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some(draft.provider.clone());
        tab.agent_session_id = Some("replacement-session".into());
        tab.session_binding_nonce = Some("replacement-nonce".into());
        tab.clone()
    };
    let _replacement_runtime = engine::test_support::live_runtime(&replacement);
    assert_eq!(
        [source.as_str(), replacement.id.as_str()]
            .into_iter()
            .filter(|id| engine::for_tab(id).is_some())
            .count(),
        1
    );
}

#[test]
fn forced_close_persists_termination_and_respawn_accepts_after_restart() {
    assert_close_persists_termination(crate::plan::TaskStatus::InProgress, true);
}

#[test]
fn forced_close_in_review_persists_termination_and_respawn_accepts_after_restart() {
    assert_close_persists_termination(crate::plan::TaskStatus::InReview, true);
}

#[test]
fn forced_close_blocked_persists_termination_and_respawn_accepts_after_restart() {
    assert_close_persists_termination(crate::plan::TaskStatus::Blocked, true);
}

#[test]
fn ordinary_close_in_review_persists_termination_and_respawn_accepts_after_restart() {
    assert_close_persists_termination(crate::plan::TaskStatus::InReview, false);
}

#[test]
fn restored_bounced_worker_force_close_records_termination_before_retirement_and_respawns() {
    let (fixture, draft) = fixture();
    let source = &draft.source_attempt;
    let mut agent = crate::service::spawn::AgentSpawnSpec::default();
    agent
        .extra_env
        .insert("CODETERM_PLAN_ID".into(), "plan".into());
    agent
        .extra_env
        .insert("CODETERM_TASK_ID".into(), "task".into());
    let binding = crate::service::spawn::task_binding_from_spawn(&agent).unwrap();
    assert!(binding.attempt_id.is_none());
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&source.executor_tab_id).unwrap();
        tab.task_binding = Some(binding.clone());
        tab.agent_type = Some("source-provider".into());
    }
    fixture
        .service
        .plan_store
        .edit_plan_fixture("plan", 2, |plan| {
            plan.tasks[0].status = crate::plan::TaskStatus::InReview;
            plan.tasks[0].execution_attempts[0].provider = Some("source-provider".into());
            Ok(())
        })
        .unwrap();
    fixture
        .service
        .plan_store
        .record_task_attempt_report(
            "plan",
            "task",
            &crate::plan::AgentRef::from_tab(&source.executor_tab_id),
            "opaque-review-report",
            Some(codeterm_types::agent::ReportStatus::InReview),
            3,
        )
        .unwrap();

    let snapshot = fixture.service.mux.read().to_session_snapshot();
    let restored_mux = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(
        restored_mux
            .get_tab(&source.executor_tab_id)
            .unwrap()
            .task_binding,
        Some(binding)
    );
    let restored = ServiceFixture::new_at(restored_mux, fixture.temp.path());
    let svc = &restored.service;
    let nonce = svc.rotate_session_binding_nonce(&source.executor_tab_id);
    svc.mux
        .write()
        .get_tab_mut(&source.executor_tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some(nonce);
    let continuation = svc
        .plan_store
        .bounce_review(
            "plan",
            "task",
            "opaque-review-defect",
            true,
            true,
            crate::plan::AgentRef::from_tab("reviewer"),
            4,
        )
        .unwrap()
        .continuation
        .unwrap();
    svc.plan_store
        .claim_review_continuation_delivery("plan", "task", &source.id, continuation.epoch, 5)
        .unwrap();
    assert!(svc
        .plan_store
        .finish_review_continuation("plan", "task", &source.id, continuation.epoch, true, 6)
        .unwrap());
    assert_eq!(
        svc.plan_store.get("plan").unwrap().tasks[0].status,
        crate::plan::TaskStatus::InProgress
    );
    assert!(svc
        .close_tab_guarded(Some("owner"), &source.executor_tab_id, false)
        .unwrap_err()
        .contains("busy worker close refused"));
    let warnings = svc
        .close_tab_guarded(Some("owner"), &source.executor_tab_id, true)
        .unwrap();
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("domios plan task respawn plan task --attempt source")));
    assert!(svc.mux.read().get_tab(&source.executor_tab_id).is_none());

    let (mux, _) = MultiplexerState::new();
    let restarted = ServiceFixture::new_at(mux, fixture.temp.path());
    let store = &restarted.service.plan_store;
    let task = &store.get("plan").unwrap().tasks[0];
    let terminated = &task.execution_attempts[0];
    assert!(
        terminated
            .executor_termination
            .as_ref()
            .unwrap()
            .confirmed_at
            <= terminated.retired_at.unwrap()
    );
    assert!(terminated.closed_at.is_some());
    assert_eq!(
        terminated.report.as_ref().unwrap().message.as_deref(),
        Some("opaque-review-report")
    );
    let replacement_draft = store
        .build_task_respawn_draft(
            "plan",
            "task",
            super::super::task_attempts::TaskRespawnRequest {
                attempt_id: source.id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    restarted
        .service
        .retire_respawn_executor(&replacement_draft)
        .unwrap();
    let replacement = store
        .record_task_respawn_attempt(
            "plan",
            "task",
            "replacement-tab",
            &replacement_draft,
            Some("worker"),
            50,
        )
        .unwrap();
    assert_ne!(replacement.attempt.id, source.id);
    assert_eq!(replacement.attempt.executor_tab_id, "replacement-tab");
    assert!(replacement.attempt.executor_termination.is_none());
    assert_eq!(
        store.get("plan").unwrap().tasks[0].execution_attempts.len(),
        2
    );
}

#[test]
fn forced_close_resolves_missing_binding_but_refuses_conflicting_or_ambiguous_attempts() {
    for field in ["missing", "plan", "task", "ambiguous"] {
        let (fixture, draft) = fixture();
        let svc = &fixture.service;
        let tab_id = &draft.source_attempt.executor_tab_id;
        {
            let mut mux = svc.mux.write();
            let tab = mux.get_tab_mut(tab_id).unwrap();
            let binding = tab.task_binding.as_mut().unwrap();
            binding.attempt_id = None;
            match field {
                "missing" => tab.task_binding = None,
                "plan" => binding.plan_id = Some("foreign".into()),
                "task" => binding.task_id = Some("foreign".into()),
                _ => {}
            }
        }
        if field == "ambiguous" {
            svc.plan_store
                .edit_plan_fixture("plan", 2, |plan| {
                    let mut second = plan.tasks[0].clone();
                    second.id = "other-task".into();
                    second.execution_attempts[0].id = "other-attempt".into();
                    plan.tasks.push(second);
                    Ok(())
                })
                .unwrap();
        }
        let result = svc.close_tab_guarded(Some("owner"), tab_id, true);
        if field == "missing" {
            result.unwrap();
            assert!(svc
                .plan_store
                .task_executor_termination_confirmed("plan", "task", &draft.source_attempt)
                .unwrap());
        } else {
            assert!(result.is_err(), "{field}");
            assert!(svc.mux.read().get_tab(tab_id).is_some(), "{field}");
            assert!(!svc
                .plan_store
                .task_executor_termination_confirmed("plan", "task", &draft.source_attempt)
                .unwrap());
        }
    }
}

#[test]
fn termination_receipt_persistence_failure_retains_tab_and_unretired_attempt() {
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let result = svc.retire_respawn_executor_with(&draft, |_| {
        svc.plan_store
            .edit_plan_fixture("plan", 2, |plan| {
                plan.tasks[0].execution_attempts[0].session_binding_nonce = Some("changed".into());
                Ok(())
            })
            .unwrap();
        Ok(())
    });
    assert!(result
        .unwrap_err()
        .contains("termination evidence could not be persisted"));
    assert!(svc
        .mux
        .read()
        .get_tab(&draft.source_attempt.executor_tab_id)
        .is_some());
    let plan = svc.plan_store.get("plan").unwrap();
    let attempt = &plan.tasks[0].execution_attempts[0];
    assert!(attempt.executor_termination.is_none());
    assert!(attempt.retired_at.is_none());
    assert!(attempt.closed_at.is_none());
}

fn assert_close_persists_termination(status: crate::plan::TaskStatus, guarded: bool) {
    let (fixture, draft) = fixture();
    fixture
        .service
        .plan_store
        .edit_plan_fixture("plan", 2, |plan| {
            plan.tasks[0].status = status.clone();
            Ok(())
        })
        .unwrap();
    let source = &draft.source_attempt;
    fixture
        .service
        .plan_store
        .record_task_attempt_report(
            "plan",
            "task",
            &crate::plan::AgentRef::from_tab(&source.executor_tab_id),
            "opaque-partial-progress",
            Some(codeterm_types::agent::ReportStatus::Partial),
            2,
        )
        .unwrap();
    if guarded {
        let warnings = fixture
            .service
            .close_tab_guarded(Some("owner"), &source.executor_tab_id, true)
            .unwrap();
        assert_eq!(
            !warnings.is_empty(),
            status == crate::plan::TaskStatus::InProgress
        );
    } else {
        fixture.service.close_tab(&source.executor_tab_id).unwrap();
    }
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&source.executor_tab_id)
        .is_none());
    let (mux, _) = MultiplexerState::new();
    let restarted = ServiceFixture::new_at(mux, fixture.temp.path());
    assert!(restarted
        .service
        .plan_store
        .task_executor_termination_confirmed("plan", "task", source)
        .unwrap());
    restarted.service.retire_respawn_executor(&draft).unwrap();
    let mut foreign = draft.clone();
    foreign.source_attempt.session_binding_nonce = Some("foreign-nonce".into());
    assert!(restarted.service.retire_respawn_executor(&foreign).is_err());
    let replacement = restarted
        .service
        .plan_store
        .record_task_respawn_attempt(
            "plan",
            "task",
            "replacement-tab",
            &draft,
            Some("worker"),
            50,
        )
        .unwrap();
    assert_eq!(replacement.attempt.executor_tab_id, "replacement-tab");
    assert_eq!(replacement.attempt.executor_termination, None);
}
#[test]
fn terminal_worker_close_resolves_missing_attempt_after_accept_or_close() {
    for accepted in [false, true] {
        let (fixture, draft) = fixture();
        let svc = &fixture.service;
        let tab_id = &draft.source_attempt.executor_tab_id;
        svc.mux
            .write()
            .get_tab_mut(tab_id)
            .unwrap()
            .task_binding
            .as_mut()
            .unwrap()
            .attempt_id = None;
        if accepted {
            svc.plan_store
                .edit_plan_fixture("plan", 2, |plan| {
                    plan.tasks[0].status = crate::plan::TaskStatus::InReview;
                    Ok(())
                })
                .unwrap();
            svc.plan_store
                .record_task_attempt_report(
                    "plan",
                    "task",
                    &crate::plan::AgentRef::from_tab(tab_id),
                    "opaque-completed-work",
                    Some(codeterm_types::agent::ReportStatus::InReview),
                    3,
                )
                .unwrap();
            svc.plan_store
                .accept_review(
                    "plan",
                    "task",
                    "opaque-acceptance",
                    true,
                    true,
                    None,
                    crate::plan::AgentRef::from_tab("owner"),
                    4,
                )
                .unwrap();
        } else {
            svc.plan_store
                .close_task(
                    "plan",
                    "task",
                    Some("opaque-owner-close"),
                    crate::plan::AgentRef::from_tab("owner"),
                    4,
                )
                .unwrap();
        }
        assert!(
            svc.plan_store.get("plan").unwrap().tasks[0].execution_attempts[0]
                .closed_at
                .is_some()
        );
        svc.close_tab_guarded(Some("owner"), tab_id, false).unwrap();
        assert!(svc.mux.read().get_tab(tab_id).is_none());
        assert!(svc
            .plan_store
            .task_executor_termination_confirmed("plan", "task", &draft.source_attempt)
            .unwrap());
    }
}

#[test]
fn terminal_worker_close_refuses_an_ambiguous_finished_attempt() {
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let tab_id = &draft.source_attempt.executor_tab_id;
    svc.mux
        .write()
        .get_tab_mut(tab_id)
        .unwrap()
        .task_binding
        .as_mut()
        .unwrap()
        .attempt_id = None;
    svc.plan_store
        .edit_plan_fixture("plan", 2, |plan| {
            plan.tasks[0].status = crate::plan::TaskStatus::Done;
            plan.tasks[0].execution_attempts[0].closed_at = Some(2);
            let mut other = plan.tasks[0].execution_attempts[0].clone();
            other.id = "other-attempt".into();
            plan.tasks[0].execution_attempts.push(other);
            Ok(())
        })
        .unwrap();
    assert!(svc.close_tab_guarded(Some("owner"), tab_id, true).is_err());
    assert!(svc.mux.read().get_tab(tab_id).is_some());
    assert!(!svc
        .plan_store
        .task_executor_termination_confirmed("plan", "task", &draft.source_attempt)
        .unwrap());
}

fn drift_executor_session(svc: &ServiceContext, tab_id: &str, drift: &str) {
    let mut mux = svc.mux.write();
    let tab = mux.get_tab_mut(tab_id).unwrap();
    match drift {
        "session" => tab.agent_session_id = Some("rebound-session".into()),
        "nonce" => tab.session_binding_nonce = Some("rebound-nonce".into()),
        "unreported" => tab.reported_session_binding_nonce = None,
        _ => unreachable!(),
    }
}

#[test]
fn finished_worker_close_succeeds_after_its_session_rebinds() {
    for (settle, drift, missing_attempt) in [
        ("done", "nonce", false),
        ("done", "session", false),
        ("done", "unreported", true),
        ("closed", "session", false),
        ("retired", "nonce", false),
        ("closed", "nonce", true),
    ] {
        let case = format!("{settle}/{drift}/{missing_attempt}");
        let (fixture, draft) = fixture();
        let svc = &fixture.service;
        let tab_id = &draft.source_attempt.executor_tab_id;
        drift_executor_session(svc, tab_id, drift);
        if missing_attempt {
            let mut mux = svc.mux.write();
            let binding = mux.get_tab_mut(tab_id).unwrap().task_binding.as_mut();
            binding.unwrap().attempt_id = None;
        }
        svc.plan_store
            .edit_plan_fixture("plan", 2, |plan| {
                let task = &mut plan.tasks[0];
                match settle {
                    "done" => {
                        task.status = crate::plan::TaskStatus::Done;
                        task.execution_attempts[0].closed_at = Some(2);
                    }
                    "closed" => task.execution_attempts[0].closed_at = Some(2),
                    "retired" => task.execution_attempts[0].retired_at = Some(2),
                    _ => unreachable!(),
                }
                Ok(())
            })
            .unwrap();
        svc.close_tab_guarded(Some("owner"), tab_id, true)
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        assert!(svc.mux.read().get_tab(tab_id).is_none(), "{case}");
        assert!(
            svc.plan_store
                .task_executor_termination_confirmed("plan", "task", &draft.source_attempt)
                .unwrap(),
            "{case}"
        );
        svc.retire_respawn_executor(&draft)
            .unwrap_or_else(|error| panic!("{case}: {error}"));
    }
}

#[test]
fn live_worker_close_with_a_rebound_session_names_its_live_attempt() {
    for drift in ["session", "nonce", "unreported"] {
        let (fixture, draft) = fixture();
        let svc = &fixture.service;
        let tab_id = &draft.source_attempt.executor_tab_id;
        drift_executor_session(svc, tab_id, drift);
        let error = svc
            .close_tab_guarded(Some("owner"), tab_id, true)
            .unwrap_err();
        assert!(
            error.contains("live attempt source of task task"),
            "{drift}: {error}"
        );
        assert!(svc.mux.read().get_tab(tab_id).is_some());
        assert!(!svc
            .plan_store
            .task_executor_termination_confirmed("plan", "task", &draft.source_attempt)
            .unwrap());
    }
}

#[test]
fn live_worker_close_refusal_classifies_the_failing_identity_proof() {
    for (drift, expected) in [
        ("session", ExecutorIdentityDrift::Session),
        ("nonce", ExecutorIdentityDrift::LaunchUnreported),
        ("unreported", ExecutorIdentityDrift::LaunchUnreported),
    ] {
        let (fixture, draft) = fixture();
        let tab_id = &draft.source_attempt.executor_tab_id;
        drift_executor_session(&fixture.service, tab_id, drift);
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(tab_id).unwrap();
        assert_eq!(
            executor_identity_drift(tab, (&draft).into()),
            Some(expected),
            "{drift}"
        );
    }
    let (fixture, draft) = fixture();
    let tab_id = &draft.source_attempt.executor_tab_id;
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.session_binding_nonce = Some("rotated-nonce".into());
        tab.reported_session_binding_nonce = Some("rotated-nonce".into());
    }
    let mux = fixture.service.mux.read();
    assert_eq!(
        executor_identity_drift(mux.get_tab(tab_id).unwrap(), (&draft).into()),
        Some(ExecutorIdentityDrift::LaunchRotated)
    );
}

#[test]
fn in_review_worker_close_refusal_names_the_review_instead_of_a_session_mismatch() {
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let tab_id = &draft.source_attempt.executor_tab_id;
    drift_executor_session(svc, tab_id, "unreported");
    svc.plan_store
        .edit_plan_fixture("plan", 2, |plan| {
            plan.tasks[0].status = crate::plan::TaskStatus::InReview;
            Ok(())
        })
        .unwrap();
    let error = svc
        .close_tab_guarded(Some("owner"), tab_id, true)
        .unwrap_err();
    assert!(error.contains("in review"), "{error}");
    assert!(!error.contains("session"), "{error}");
    assert!(svc.mux.read().get_tab(tab_id).is_some());
}

#[test]
fn finished_worker_close_refuses_while_the_tab_runs_another_live_attempt() {
    let (fixture, draft) = fixture();
    let svc = &fixture.service;
    let tab_id = &draft.source_attempt.executor_tab_id;
    drift_executor_session(svc, tab_id, "session");
    svc.plan_store
        .edit_plan_fixture("plan", 2, |plan| {
            let mut live = plan.tasks[0].clone();
            live.id = "live-task".into();
            live.execution_attempts[0].id = "live-attempt".into();
            plan.tasks[0].status = crate::plan::TaskStatus::Done;
            plan.tasks[0].execution_attempts[0].closed_at = Some(2);
            plan.tasks.push(live);
            Ok(())
        })
        .unwrap();
    let error = svc
        .close_tab_guarded(Some("owner"), tab_id, true)
        .unwrap_err();
    assert!(
        error.contains("live attempt live-attempt of task live-task"),
        "{error}"
    );
    assert!(svc.mux.read().get_tab(tab_id).is_some());
}

#[test]
fn normal_and_force_close_release_orphan_binding_after_plan_or_task_deletion() {
    for deleted_plan in [false, true] {
        for force in [false, true] {
            for incomplete_attempt in [false, true] {
                let (fixture, draft) = fixture();
                let svc = &fixture.service;
                let tab_id = &draft.source_attempt.executor_tab_id;
                let workspace = svc.mux.read().find_workspace_for_tab(tab_id).unwrap();
                {
                    let mut mux = svc.mux.write();
                    let tab = mux.get_tab_mut(tab_id).unwrap();
                    tab.worker_id = Some("orphan-worker".into());
                    if incomplete_attempt {
                        tab.task_binding.as_mut().unwrap().attempt_id = None;
                    }
                }
                if deleted_plan {
                    svc.plan_store.delete("plan").unwrap();
                } else {
                    svc.plan_store
                        .edit_plan_fixture("plan", 2, |plan| {
                            plan.tasks.clear();
                            Ok(())
                        })
                        .unwrap();
                }
                let warnings = svc.close_tab_guarded(Some("owner"), tab_id, force).unwrap();
                assert!(warnings.is_empty());
                let mux = svc.mux.read();
                assert!(mux.get_tab(tab_id).is_none());
                assert!(!mux.all_tabs().any(|tab| tab
                    .task_binding
                    .as_ref()
                    .is_some_and(|binding| binding.plan_id.as_deref() == Some("plan"))));
                let closed = mux.list_recently_closed(&workspace);
                let record = closed
                    .iter()
                    .find(|record| record.tab_id == *tab_id)
                    .unwrap();
                assert!(record.worker_id.is_none());
                assert!(record.restorable_launch.worker_restart.is_none());
                drop(mux);
                if deleted_plan {
                    assert!(svc.plan_store.get("plan").is_none());
                } else {
                    assert!(svc.plan_store.get("plan").unwrap().tasks.is_empty());
                }
            }
        }
    }
}

#[test]
fn executor_close_releases_binding_when_plan_disappears_during_termination() {
    let (fixture, draft) = fixture();
    fixture
        .service
        .retire_respawn_executor_with(&draft, |_| {
            fixture.service.plan_store.delete("plan").unwrap();
            Ok(())
        })
        .unwrap();
    let mux = fixture.service.mux.read();
    assert!(mux.get_tab(&draft.source_attempt.executor_tab_id).is_none());
    assert!(!mux.all_tabs().any(|tab| tab
        .task_binding
        .as_ref()
        .is_some_and(|binding| binding.plan_id.as_deref() == Some("plan"))));
}
