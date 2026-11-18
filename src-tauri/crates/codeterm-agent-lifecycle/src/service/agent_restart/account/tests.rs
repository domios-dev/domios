use super::*;
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::multiplexer::state::MultiplexerState;
use crate::service::agent_lifecycle::AccountRestartBinding;
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::agent::chat_types::{ChatMsg, StructuredChat};

struct ConversationProvider;

impl AgentProvider for ConversationProvider {
    fn id(&self) -> &str {
        "opaque-provider"
    }

    fn display_name(&self) -> &str {
        "Fixture"
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        unreachable!("this test only resumes an exact session")
    }

    fn build_resume_command(&self, session: &str, _: bool) -> Option<String> {
        Some(format!("fixture --resume {session}"))
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }

    fn uses_structured_chat(&self) -> bool {
        true
    }

    fn read_structured_chat(&self, _: &str, session: &str, _: Option<&str>) -> StructuredChat {
        assert_eq!(session, "exact-session");
        StructuredChat {
            messages: vec![ChatMsg {
                id: "earlier-reply".into(),
                kind: "assistant".into(),
                content: "Retained conversation reply".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

#[test]
fn account_selection_waits_for_exact_pty_cleanup_and_retirement() {
    let (fixture, snap, target) = fixture();
    let id = snap.tab_id.clone();
    assert!(commit_pty(&fixture.service, &snap, &target).is_err());
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.remember_superseded_binding_nonce();
        tab.agent_type = None;
        tab.agent_session_id = None;
        tab.session_binding_nonce = None;
        tab.reported_session_binding_nonce = None;
        tab.lifecycle_epoch = None;
    }
    assert!(commit_pty(&fixture.service, &snap, &target).is_err());
    fixture.service.mux.write().get_tab_mut(&id).unwrap().pty_id = None;
    commit_pty(&fixture.service, &snap, &target).unwrap();
    assert!(selected(
        fixture.service.mux.read().get_tab(&id).unwrap(),
        &target
    ));
    assert!(commit_pty(&fixture.service, &snap, &target).is_err());
}

fn fixture() -> (ServiceFixture, TabRestartSnapshot, AccountRestartTarget) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("exact-session".into());
    tab.session_binding_nonce = Some("old-nonce".into());
    tab.reported_session_binding_nonce = Some("old-nonce".into());
    tab.pty_id = Some("old-pty".into());
    tab.lifecycle_epoch = Some(7);
    tab.boot_stage = Some(BootStage::AgentReady);
    let target = AccountRestartTarget {
        terminal_turn: None,
        expected: AccountRestartBinding::capture(tab, None).unwrap(),
        account_id: Some("next-account".into()),
        account_namespace: Some("next-namespace".into()),
        invocation: Some(
            codeterm_plugin::provider_account::InvocationContext::new(
                "opaque-provider".into(),
                "next-account".into(),
                Default::default(),
                vec![],
                vec![],
            )
            .unwrap(),
        ),
    };
    let snap = TabRestartSnapshot {
        tab_id: id.clone(),
        pty_id: "old-pty".into(),
        provider: "opaque-provider".into(),
        target_provider: None,
        session_id: Some("exact-session".into()),
        cwd: None,
        auto_approve: false,
        role: None,
        role_profile: None,
        handoff_history: None,
    };
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(ConversationProvider));
    fixture.service.agent_tracker.lock().set_idle(&snap.pty_id);
    (fixture, snap, target)
}

#[test]
fn pty_account_switch_success_keeps_the_exact_session() {
    exercise_switch(false);
}

#[test]
fn pty_account_switch_launch_failure_resumes_the_original_account_and_session() {
    exercise_switch(true);
}

fn exercise_switch(fail_target: bool) {
    let (fixture, snap, target) = fixture();
    let svc = &fixture.service;
    let source = AccountRestartTarget {
        expected: target.expected.clone(),
        account_id: None,
        account_namespace: None,
        invocation: None,
        terminal_turn: None,
    };
    let resume = |account: &str| Relaunch {
        command: format!("{account} --resume exact-session").into(),
        task: None,
    };
    let read_history =
        || crate::service::tab_chat::tab_current_transcript_snapshot(svc, &snap.tab_id, 0).unwrap();
    let history = read_history();
    assert_eq!(history.1, 1);
    assert_eq!(history.0[0].content, "Retained conversation reply");
    let prepared = PtyAccountSwitch {
        target: &target,
        target_launch: resume("target"),
        source,
        source_launch: resume("source"),
    };
    let mut launches = Vec::new();
    let result = prepared.execute_with_launch(
        svc,
        &snap,
        RestartOutcome::accepted(&snap, false, false),
        svc.pty_ownership.acquire(&snap.tab_id).unwrap(),
        None,
        |svc, snap, leg, account| {
            let mut mux = svc.mux.write();
            let tab = mux.get_tab_mut(&snap.tab_id).unwrap();
            assert!(selected(tab, account));
            assert!(tab.pty_id.is_none());
            assert!(tab.agent_session_id.is_none());
            let destination = if account.account_id.is_some() {
                "target"
            } else {
                "source"
            };
            assert!(leg
                .command
                .contains(&format!("{destination} --resume exact-session")));
            launches.push(destination);
            let nonce = format!("{destination}-nonce");
            tab.agent_type = Some(snap.provider.clone());
            tab.pty_id = Some(format!("{destination}-pty"));
            tab.session_binding_nonce = Some(nonce.clone());
            tab.remember_resume_binding_nonce("exact-session");
            if fail_target && destination == "target" {
                tab.boot_stage = Some(BootStage::SpawnFailed);
                tab.boot_failure_reason = Some("Fixture target launch failed".into());
            } else {
                tab.boot_stage = Some(BootStage::AgentReady);
                tab.agent_session_id = snap.session_id.clone();
                tab.reported_session_binding_nonce = Some(nonce.clone());
            }
            Some(nonce)
        },
    );
    let (outcome, account, destination) = if fail_target {
        let Err(RestartAttemptError::Failed {
            outcome,
            reason,
            timed_out: false,
        }) = result
        else {
            panic!("expected a restored switch failure, got {result:?}");
        };
        assert_eq!(reason, "Fixture target launch failed");
        assert!(outcome.source_account_restored);
        assert!(!outcome.account_target_refused);
        assert_eq!(launches, ["target", "source"]);
        (outcome, None, "source")
    } else {
        let outcome = result.unwrap();
        assert!(!outcome.source_account_restored);
        assert_eq!(launches, ["target"]);
        (outcome, Some("next-account"), "target")
    };
    assert!(outcome.old_pty_torn_down);
    assert!(outcome.agent_ready);
    assert!(outcome.binding_restored);
    let binding =
        AccountRestartBinding::capture(svc.mux.read().get_tab(&snap.tab_id).unwrap(), None)
            .unwrap();
    assert_eq!(binding.account_id.as_deref(), account);
    assert_eq!(binding.session_id, "exact-session");
    assert_eq!(binding.binding_nonce, format!("{destination}-nonce"));
    assert_eq!(read_history(), history);
}

#[test]
fn pty_account_switch_requires_a_bound_session_before_the_deadline() {
    let (fixture, snap, target) = fixture();
    let svc = &fixture.service;
    {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&snap.tab_id).unwrap();
        target.commit_selection(tab).unwrap();
        tab.pty_id = Some("target-pty".into());
        tab.session_binding_nonce = Some("target-nonce".into());
        tab.reported_session_binding_nonce = None;
        tab.agent_session_id = None;
        tab.boot_stage = Some(BootStage::ProviderReady);
    }
    let error = wait_for_resume_until(svc, &snap, &target, Some("target-nonce"), Instant::now())
        .unwrap_err();
    assert!(error.timed_out);
    assert!(failed_launch_pty(svc, &snap, &target, Some("other-nonce")).is_err());
}
