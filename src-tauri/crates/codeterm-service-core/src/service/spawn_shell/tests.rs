use crate::agent::reports::ReportDetails;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, ShellPurpose};
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::pty_ownership::OwnershipEvent;
use crate::service::spawn::SpawnTabSpec;
use crate::service::test_fixture::ServiceFixture;
use std::sync::{mpsc, Arc};
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(5);

#[test]
fn blocked_shell_builder_releases_global_locks_and_registers_once() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = fixture.service.clone();
    let gate = service.pty.lock().spawn_gate();
    let built = crate::pty::manager::build_session(gate, None, None, 120, 40, Default::default())
        .expect("prepare session before fake builder");
    let session = built.session.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    service
        .pty_ownership
        .set_test_observer(Arc::new(move |_, event| {
            if event == OwnershipEvent::AwaitingActivation {
                waiting_tx.send(()).unwrap();
            }
        }));
    let worker_service = service.clone();
    let worker_tab = tab_id.clone();
    let worker = std::thread::spawn(move || {
        worker_service.link_hidden_shell_pty_with_builder(&worker_tab, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(DEADLINE).expect("release builder");
            Ok(built)
        })
    });
    started_rx.recv_timeout(DEADLINE).expect("builder entered");
    {
        let pty = service.pty.try_lock().expect("another tab can use PTYs");
        assert!(pty.get_session("another-tab").is_none());
    }
    {
        let mux = service.mux.try_read().expect("builder releases mux");
        assert!(mux.get_tab(&tab_id).unwrap().pty_id.is_none());
    }
    let second_service = service.clone();
    let second_tab = tab_id.clone();
    let second = std::thread::spawn(move || {
        second_service.link_hidden_shell_pty_with_builder(&second_tab, |_| {
            panic!("a second activation must reuse the registered session")
        })
    });
    waiting_rx
        .recv_timeout(DEADLINE)
        .expect("second activation waits");
    release_tx.send(()).unwrap();
    let pty_id = worker.join().unwrap().expect("register session");
    assert_eq!(second.join().unwrap().unwrap(), pty_id);
    let registered = service.pty.lock().get_session(&pty_id).unwrap();
    assert!(Arc::ptr_eq(&registered, &session));
    assert_eq!(registered.generation, session.generation);
    assert_eq!(service.resolve_pty_id(&tab_id).unwrap(), pty_id);
    service.kill_ptys(&[pty_id]);
}

#[test]
fn shell_closed_during_build_reaps_the_unlinked_session() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = fixture.service.clone();
    let gate = service.pty.lock().spawn_gate();
    let built = crate::pty::manager::build_session(gate, None, None, 120, 40, Default::default())
        .expect("prepare session before fake builder");
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (candidate_tx, candidate_rx) = mpsc::channel();
    service
        .pty_ownership
        .set_test_observer(Arc::new(move |_, event| {
            if let OwnershipEvent::Linking { candidate, .. } = event {
                candidate_tx.send(candidate).unwrap();
            }
        }));
    let worker_service = service.clone();
    let worker_tab = tab_id.clone();
    let worker = std::thread::spawn(move || {
        worker_service.link_hidden_shell_pty_with_builder(&worker_tab, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(DEADLINE).expect("release builder");
            Ok(built)
        })
    });
    started_rx.recv_timeout(DEADLINE).expect("builder entered");
    assert!(service.mux.write().close_tab(&tab_id).unwrap().is_empty());
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap().unwrap_err().contains("closed"));
    let candidate = candidate_rx
        .recv_timeout(DEADLINE)
        .expect("candidate linked once");
    assert!(candidate_rx.try_recv().is_err());
    assert!(service.pty.lock().get_session(&candidate).is_none());
    assert!(service.mux.read().get_tab(&tab_id).is_none());
}

#[test]
fn failed_shell_build_leaves_no_pty_link() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let error = fixture
        .service
        .link_hidden_shell_pty_with_builder(&tab_id, |_| Err("spawn failed".into()))
        .unwrap_err();
    assert_eq!(error, "spawn failed");
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .pty_id
        .is_none());
}

#[test]
fn worker_owned_utility_shell_has_no_task_authority_and_reports_to_worker() {
    let (mut mux, orchestrator_id) = MultiplexerState::new();
    mux.set_orchestrator_role(&orchestrator_id).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator_id)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator_id).unwrap();
    let worker_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker_id, AgentRole::Worker, Some(&orchestrator_id))
        .unwrap();
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "utility shell owner".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&orchestrator_id),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker shell report".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["the report reaches the Worker".into()],
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker_id, None, 3)
        .unwrap();
    let delivered =
        fixture.bind_recorded_agent_recipient(&worker_id, "worker-session", "worker-nonce");

    let shell = fixture
        .service
        .spawn_visible_shell_impl(SpawnTabSpec {
            workspace_id: Some(workspace_id),
            team_id: Some(team_id.clone()),
            creator_tab_id: Some(worker_id.clone()),
            purpose: ShellPurpose::Build,
            ..Default::default()
        })
        .unwrap();
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&shell.tab_id)
        .unwrap()
        .clone();
    assert_eq!(tab.parent_tab_id.as_deref(), Some(worker_id.as_str()));
    assert_eq!(tab.spawned_by_tab_id.as_deref(), Some(worker_id.as_str()));
    assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(tab.role, None);

    for env in [
        fixture.service.tab_env(&shell.tab_id),
        fixture.service.frontend_pty_env(Some(&shell.tab_id)),
    ] {
        assert_eq!(
            env.get("CODETERM_PARENT_TAB_ID").map(String::as_str),
            Some(worker_id.as_str())
        );
        assert_eq!(env["CODETERM_TEAM_ID"], team_id);
        for key in [
            "CODETERM_ROLE",
            "CODETERM_PLAN_ID",
            "CODETERM_TASK_ID",
            "CODETERM_ATTEMPT_ID",
            "CODETERM_LANE_BASE",
            "CODETERM_WORKER_ID",
        ] {
            assert!(!env.contains_key(key), "{key} leaked into a utility shell");
        }
    }

    let before = serde_json::to_value(fixture.service.plan_store.get(&plan.id)).unwrap();
    for (index, (plan_hint, task_hint)) in [
        (None, None),
        (Some(plan.id.as_str()), None),
        (Some(plan.id.as_str()), Some(task_id.as_str())),
    ]
    .into_iter()
    .enumerate()
    {
        let payload = format!("utility-result-{index}");
        let report = fixture
            .service
            .submit_report(
                &shell.tab_id,
                &payload,
                ReportDetails {
                    plan_id: plan_hint.map(str::to_string),
                    task_id: task_hint.map(str::to_string),
                    attempt_id: Some("stale-parent-attempt".into()),
                    status: task_hint.map(|_| crate::agent::reports::ReportStatus::InReview),
                    caller_tab_id: Some(shell.tab_id.clone()),
                    session_binding_nonce: Some("shell-without-agent-session".into()),
                    ..Default::default()
                },
                plan_hint,
                task_hint,
                None,
            )
            .unwrap();

        assert_eq!(report.orchestrator_tab_id, worker_id);
        assert!(report.persisted_task.is_none());
        assert!(report.persisted_plan_evidence.is_none());
        assert!(report.rejected_task_transition.is_none());
        assert_eq!(
            delivered
                .texts
                .lock()
                .iter()
                .filter(|text| text.contains(&payload))
                .count(),
            1
        );
        let stored = fixture.service.report_store.lock().get(&worker_id, false);
        let entry = stored
            .iter()
            .find(|entry| entry.message == payload)
            .unwrap();
        assert!(entry.plan_id.is_none());
        assert!(entry.task_id.is_none());
        assert!(entry.attempt_id.is_none());
        assert!(entry.status.is_none());
    }
    assert_eq!(
        serde_json::to_value(fixture.service.plan_store.get(&plan.id)).unwrap(),
        before
    );
}

#[test]
fn shell_job_policy_uses_persisted_membership_and_purpose() {
    use crate::pty::manager::WindowsJobPolicy::{AllowExplicitServerBreakaway, Contained};
    let (mut mux, tab_id) = MultiplexerState::new();
    assert_eq!(super::shell_job_policy_for_mux(&mux, &tab_id), Contained);
    mux.get_tab_mut(&tab_id).unwrap().purpose = ShellPurpose::Dev;
    assert_eq!(super::shell_job_policy_for_mux(&mux, &tab_id), Contained);
    mux.get_tab_mut(&tab_id).unwrap().team_id = Some("missing-team".into());
    assert_eq!(super::shell_job_policy_for_mux(&mux, &tab_id), Contained);
    mux.get_tab_mut(&tab_id).unwrap().team_id = None;
    mux.set_orchestrator_role(&tab_id).unwrap();
    mux.resolve_or_mint_team_for_parent(&tab_id).unwrap();
    assert_eq!(
        super::shell_job_policy_for_mux(&mux, &tab_id),
        AllowExplicitServerBreakaway
    );
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("fixture".into());
    assert_eq!(super::shell_job_policy_for_mux(&mux, &tab_id), Contained);
    mux.get_tab_mut(&tab_id).unwrap().agent_type = None;
    mux.get_tab_mut(&tab_id).unwrap().purpose = ShellPurpose::Qa;
    assert_eq!(
        super::shell_job_policy_for_mux(&mux, &tab_id),
        AllowExplicitServerBreakaway
    );
    mux.get_tab_mut(&tab_id).unwrap().purpose = ShellPurpose::Build;
    assert_eq!(
        super::shell_job_policy_for_mux(&mux, &tab_id),
        AllowExplicitServerBreakaway
    );
    mux.get_tab_mut(&tab_id).unwrap().purpose = ShellPurpose::Shell;
    assert_eq!(super::shell_job_policy_for_mux(&mux, &tab_id), Contained);
    assert_eq!(
        super::shell_job_policy_for_mux(&mux, "missing-tab"),
        Contained
    );
}
