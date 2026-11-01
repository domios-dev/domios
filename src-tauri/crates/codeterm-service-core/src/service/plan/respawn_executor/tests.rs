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
        plan_id: "plan".into(),
        task_id: "task".into(),
        execution_intent: ExecutionIntent::Mutating,
        attempt_id: "replacement".into(),
        source_attempt: TaskExecutionAttempt {
            id: "source".into(),
            executor_tab_id: tab_id,
            session_id: Some("source-session".into()),
            session_binding_nonce: Some("source-nonce".into()),
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
