use super::*;

#[test]
fn unconfirmed_general_launch_recovers_its_own_journaled_session_without_liveness() {
    use crate::multiplexer::types::PriorBindingNonce;
    use crate::session::lifecycle::LifecycleTransition;

    let mut mux = crate::multiplexer::state::MultiplexerState::empty();
    crate::service::general_agent::ensure_general_workspace(&mut mux, true);
    let id = mux
        .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
        .unwrap();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.cwd = Some("D:/fixture".into());
    tab.boot_stage = Some(BootStage::LaunchBindingUnconfirmed);
    tab.agent_booting = false;
    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        nonce: "previous-nonce".into(),
        session_id: Some("owned-session".into()),
        resume_target_session_id: None,
    });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session(
            "fixture-provider",
            "owned-session",
        ),
    ));
    fixture.service.open_lifecycle_epoch(&id).unwrap();
    fixture.service.record_lifecycle(
        &id,
        LifecycleTransition::SessionBound {
            session_id: "owned-session".into(),
            binding_nonce: "previous-nonce".into(),
            source: Default::default(),
        },
    );
    let recovery = fixture
        .service
        .failed_launch_restart_snapshot(&id)
        .unwrap()
        .unwrap();
    assert_eq!(recovery.session_id.as_deref(), Some("owned-session"));
    assert_eq!(recovery.provider, "fixture-provider");
    assert!(
        crate::service::general_agent::claim_general_agent_tab_retry(&id, &fixture.service.mux)
            .unwrap()
    );
}

#[test]
fn never_started_pty_can_restart_without_observed_agent_liveness() {
    for booting in [false, true] {
        for session in [None, Some("saved-session")] {
            let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
            let tab = mux.get_tab_mut(&id).unwrap();
            tab.protected = false;
            tab.restored_from_snapshot = true;
            tab.agent_type = Some("fixture-provider".into());
            tab.agent_session_id = session.map(str::to_owned);
            tab.cwd = Some("D:/fixture".into());
            tab.boot_stage = Some(BootStage::PtyStarting);
            tab.agent_booting = booting;
            let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
            fixture.set_agent_full_access(false);
            fixture.service.agent_registry.register_provider(Box::new(
                crate::agent::test_provider::MockProvider::with_session(
                    "fixture-provider",
                    "saved-session",
                ),
            ));
            let snap = fixture
                .service
                .failed_launch_restart_snapshot(&id)
                .unwrap()
                .unwrap();
            assert_eq!(snap.tab_id, id);
            assert_eq!(snap.provider, "fixture-provider");
            assert_eq!(snap.session_id.as_deref(), session);
            assert!(snap.pty_id.is_empty());
        }
    }
}

#[test]
fn never_started_pty_cannot_replace_a_missing_saved_session() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("missing-session".into());
    tab.cwd = Some("D:/fixture".into());
    tab.boot_stage = Some(BootStage::PtyStarting);
    tab.agent_booting = true;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider"),
    ));
    assert!(fixture.service.failed_launch_restart_snapshot(&id).is_err());
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("missing-session")
    );
}

#[test]
fn failed_launch_retains_provider_for_relaunch_after_identity_cleanup() {
    for provider in ["fixture-plain", "fixture-structured", "fixture-tool-less"] {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        assert_eq!(failed_provider(tab), Some(provider));
        assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
        assert!(tab.agent_type.is_none());
        assert_eq!(failed_provider(tab), Some(provider));
        tab.agent_booting = true;
        assert_eq!(failed_provider(tab), None);
    }
}

#[test]
fn ordinary_exited_shell_is_not_a_failed_launch() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.boot_stage = Some(BootStage::ShellReady);
    assert_eq!(failed_provider(tab), None);
}

#[test]
fn failed_launch_without_a_pty_prepares_a_fresh_restart() {
    for provider in ["fixture-plain", "fixture-structured", "fixture-tool-less"] {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider.into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.set_agent_full_access(false);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new(provider),
        ));
        let restart = fixture
            .service
            .failed_launch_restart_snapshot(&tab_id)
            .unwrap()
            .unwrap();
        assert_eq!(restart.provider, provider);
        assert_eq!(restart.tab_id, tab_id);
        assert!(restart.pty_id.is_empty());
        assert!(restart.session_id.is_none());
    }
}

fn spawn_surviving_shell_pty(svc: &ServiceContext, pty_id: &str) {
    let gate = svc.pty.lock().spawn_gate();
    crate::pty::manager::build_session(gate, None, None, 80, 24, Default::default())
        .map(|built| svc.pty.lock().register_session(Some(pty_id.into()), built))
        .expect("spawn surviving shell PTY");
}

fn deadline_tab_with_metadata(
    pty_id: &str,
    provider: &str,
    session_id: Option<&str>,
    nonce: &str,
) -> (crate::multiplexer::state::MultiplexerState, String) {
    use crate::multiplexer::types::AgentRole;
    use codeterm_types::pty::ProvisionalLaunch;

    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&tab_id, pty_id.into());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(provider.into());
    tab.agent_session_id = session_id.map(str::to_owned);
    tab.cwd = Some("D:/fixture-cwd".into());
    tab.role = Some(AgentRole::Manager);
    tab.role_profile = Some("design_lead".into());
    tab.team_id = Some("team-fixture".into());
    tab.parent_tab_id = Some("parent-fixture".into());
    tab.session_binding_nonce = Some(nonce.into());
    tab.reported_session_binding_nonce = None;
    tab.boot_stage = Some(BootStage::LaunchDeadlineElapsed);
    tab.agent_booting = true;
    tab.provisional_launch = Some(ProvisionalLaunch {
        pty_id: pty_id.into(),
        binding_nonce: nonce.into(),
        reason: "launch readiness deadline elapsed".into(),
        awaiting_binding_ack: false,
        first_turn_input_ready: false,
        first_turn_payload_written: false,
        first_turn_admitted_send_id: None,
    });
    tab.restorable_launch.captured = true;
    tab.restorable_launch.args = Some(vec!["--opaque-flag".into()]);
    tab.restorable_launch.permission_bypass_requested = true;
    (mux, tab_id)
}

#[test]
fn terminal_launch_deadline_with_stale_booting_recovers_exact_session_and_preserves_metadata() {
    use crate::agent::provider::AgentResumeParams;
    use crate::service::process_tree::{ProviderProcessTreeSnapshot, ProviderTreeWitnesses};
    use crate::session::lifecycle::{AgentTypeWriteOrigin, LifecycleTransition};
    use parking_lot::Mutex;
    use std::sync::Arc;

    for (case, session_id, record_rebind) in [
        ("journaled-binding", "exact-journal-session", false),
        ("validated-rebind", "exact-rebind-session", true),
    ] {
        let pty_id = format!("pty-deadline-{case}");
        let (mux, tab_id) =
            deadline_tab_with_metadata(&pty_id, "fixture-provider", Some(session_id), "nonce-a");
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.set_agent_full_access(true);
        let resume_log: Arc<Mutex<Vec<AgentResumeParams>>> = Arc::new(Mutex::new(Vec::new()));
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_session("fixture-provider", session_id)
                .with_launch_binary("fixture-provider")
                .with_auto_approve_flags(vec!["--opaque-bypass".into()])
                .with_resume_context_log(resume_log.clone()),
        ));
        spawn_surviving_shell_pty(&fixture.service, &pty_id);
        let tree_key = fixture
            .service
            .provider_tree_key_for_tab(&tab_id, &pty_id)
            .expect("provider tree key");
        fixture
            .service
            .ext::<Arc<ProviderTreeWitnesses>>()
            .script_snapshot(&tree_key, ProviderProcessTreeSnapshot::Absent);

        fixture.service.open_lifecycle_epoch(&tab_id).unwrap();
        if record_rebind {
            fixture.service.record_lifecycle(
                &tab_id,
                LifecycleTransition::AgentTypeWrite {
                    origin: AgentTypeWriteOrigin::Rebind,
                    previous_provider: Some("fixture-provider".into()),
                    new_provider: Some("fixture-provider".into()),
                    attempted_provider: Some("fixture-provider".into()),
                    caller_tab_id: None,
                    session_id: Some(session_id.into()),
                    nonce_verified: false,
                    applied: true,
                },
            );
        } else {
            fixture.service.record_lifecycle(
                &tab_id,
                LifecycleTransition::SessionBound {
                    session_id: session_id.into(),
                    binding_nonce: "prior-nonce".into(),
                    source: Default::default(),
                },
            );
        }

        assert_eq!(
            crate::agent::live_state::observe_tab_liveness(&fixture.service, &tab_id),
            AgentLiveness::Unobserved,
            "{case}: Unobserved liveness must not be weakened"
        );
        let snap = fixture
            .service
            .failed_launch_restart_snapshot(&tab_id)
            .expect("recovery lookup succeeds")
            .expect("terminal deadline with stale agent_booting admits exact-session recovery");
        assert_eq!(snap.tab_id, tab_id);
        assert_eq!(snap.pty_id, pty_id);
        assert_eq!(snap.provider, "fixture-provider");
        assert_eq!(snap.session_id.as_deref(), Some(session_id));
        assert_eq!(snap.cwd.as_deref(), Some("D:/fixture-cwd"));
        assert_eq!(snap.role.as_deref(), Some("Manager"));
        assert_eq!(snap.role_profile.as_deref(), Some("design_lead"));
        assert!(snap.auto_approve);

        let relaunch = super::super::relaunch::build_relaunch(&fixture.service, &snap, true)
            .expect("recorded launch builds exact resume command");
        assert!(relaunch.command.shell_command.contains("--resume"));
        assert!(relaunch.command.shell_command.contains(session_id));
        assert!(relaunch.command.shell_command.contains("--opaque-flag"));
        let logged = resume_log.lock();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].session_id, session_id);
        assert!(logged[0].skip_permissions);
        assert_eq!(logged[0].args, vec!["--opaque-flag".to_string()]);

        let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
        assert_eq!(tab.team_id.as_deref(), Some("team-fixture"));
        assert_eq!(tab.parent_tab_id.as_deref(), Some("parent-fixture"));
        assert_eq!(tab.role_profile.as_deref(), Some("design_lead"));
        fixture.service.pty.lock().kill(&pty_id).unwrap();
    }
}

#[test]
fn active_in_flight_launch_stays_fenced_from_failed_launch_recovery() {
    use crate::service::launch_diagnostics::AgentLaunchOrigin;
    use crate::service::orchestrator::launch_test_support::FakeRegisteredLaunch;
    use crate::session::lifecycle::LifecycleTransition;

    for active_stage in [
        BootStage::PtySpawned,
        BootStage::ShellReady,
        BootStage::AgentLaunching,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
        BootStage::AwaitingOwnerPrompt,
        BootStage::SpawnFailed,
    ] {
        let (mut mux, tab_id) =
            deadline_tab_with_metadata("pty-active", "fixture-provider", Some("owned"), "nonce-a");
        mux.get_tab_mut(&tab_id).unwrap().boot_stage = Some(active_stage);
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_session("fixture-provider", "owned"),
        ));
        assert!(
            fixture
                .service
                .failed_launch_restart_snapshot(&tab_id)
                .unwrap()
                .is_none(),
            "active booting stage {active_stage:?} must stay fenced"
        );
    }

    // Armed launch on the same tab/PTY/nonce must stay fenced even at LaunchDeadlineElapsed.
    let (mux, tab_id) =
        deadline_tab_with_metadata("pty-armed", "fixture-provider", Some("owned"), "nonce-old");
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("fixture-provider", "owned"),
    ));
    fixture.service.open_lifecycle_epoch(&tab_id).unwrap();
    fixture.service.record_lifecycle(
        &tab_id,
        LifecycleTransition::SessionBound {
            session_id: "owned".into(),
            binding_nonce: "prior".into(),
            source: Default::default(),
        },
    );
    let active = FakeRegisteredLaunch::register(
        &fixture.service,
        &tab_id,
        "pty-armed",
        AgentLaunchOrigin::Resume,
    )
    .expect("register active launch");
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.boot_stage = Some(BootStage::LaunchDeadlineElapsed);
        tab.agent_booting = true;
        tab.provisional_launch = Some(codeterm_types::pty::ProvisionalLaunch {
            pty_id: "pty-armed".into(),
            binding_nonce: active.attempt().binding_nonce,
            reason: "launch readiness deadline elapsed".into(),
            awaiting_binding_ack: false,
            first_turn_input_ready: false,
            first_turn_payload_written: false,
            first_turn_admitted_send_id: None,
        });
    }
    assert!(fixture
        .service
        .failed_launch_restart_snapshot(&tab_id)
        .unwrap()
        .is_none());
    drop(active);

    // A rotated nonce (newer launch started before replacing boot_stage) must stay fenced.
    let (mut mux, tab_id) = deadline_tab_with_metadata(
        "pty-rotated",
        "fixture-provider",
        Some("owned"),
        "nonce-stale",
    );
    mux.get_tab_mut(&tab_id).unwrap().session_binding_nonce = Some("nonce-newer-launch".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("fixture-provider", "owned"),
    ));
    assert!(fixture
        .service
        .failed_launch_restart_snapshot(&tab_id)
        .unwrap()
        .is_none());
}

#[test]
fn live_observed_provider_is_excluded_from_failed_launch_recovery() {
    use crate::service::process_tree::{
        ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeWitnesses,
    };
    use crate::session::lifecycle::LifecycleTransition;
    use std::collections::HashSet;
    use std::sync::Arc;

    // 1. Live provider process running under surviving PTY despite LaunchDeadlineElapsed.
    let pty_id = "pty-live-tree";
    let (mux, tab_id) =
        deadline_tab_with_metadata(pty_id, "fixture-provider", Some("owned"), "nonce-a");
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("fixture-provider", "owned")
            .with_launch_binary("fixture-provider"),
    ));
    spawn_surviving_shell_pty(&fixture.service, pty_id);
    let tree_key = fixture
        .service
        .provider_tree_key_for_tab(&tab_id, pty_id)
        .expect("provider tree key");
    fixture
        .service
        .ext::<Arc<ProviderTreeWitnesses>>()
        .script_snapshot(
            &tree_key,
            ProviderProcessTreeSnapshot::Present(HashSet::from([ProcessIdentity {
                pid: 41_410,
                start_token: "live-provider-start".into(),
            }])),
        );
    fixture.service.open_lifecycle_epoch(&tab_id).unwrap();
    fixture.service.record_lifecycle(
        &tab_id,
        LifecycleTransition::SessionBound {
            session_id: "owned".into(),
            binding_nonce: "prior".into(),
            source: Default::default(),
        },
    );
    assert!(fixture
        .service
        .failed_launch_restart_snapshot(&tab_id)
        .unwrap()
        .is_none());

    // 2. Observed AgentReady live tab must never enter failed-launch path.
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_booting = false;
        tab.boot_stage = Some(BootStage::AgentReady);
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    assert_eq!(
        crate::agent::live_state::observe_tab_liveness(&fixture.service, &tab_id),
        AgentLiveness::Live
    );
    assert!(fixture
        .service
        .failed_launch_restart_snapshot(&tab_id)
        .unwrap()
        .is_none());
    fixture.service.pty.lock().kill(pty_id).unwrap();
}

#[test]
fn terminal_launch_deadline_refuses_missing_unknown_and_conflicting_provenance() {
    use crate::session::lifecycle::{AgentTypeWriteOrigin, LifecycleTransition};

    // 1. Missing provenance on LaunchDeadlineElapsed refuses without fresh fallback.
    let (mux, tab_id) = deadline_tab_with_metadata("pty-missing", "fixture-provider", None, "n1");
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture-provider"),
    ));
    let err = fixture
        .service
        .failed_launch_restart_snapshot(&tab_id)
        .unwrap_err();
    assert!(
        matches!(err, AgentRestartError::BadRequest(msg) if msg.contains("exit_recovery_no_verified_session"))
    );

    // 2. Unknown session with only an unapplied or non-rebind AgentTypeWrite is refused.
    let (mux, tab_id) = deadline_tab_with_metadata(
        "pty-unknown",
        "fixture-provider",
        Some("unverified-session"),
        "n2",
    );
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session(
            "fixture-provider",
            "unverified-session",
        ),
    ));
    fixture.service.open_lifecycle_epoch(&tab_id).unwrap();
    fixture.service.record_lifecycle(
        &tab_id,
        LifecycleTransition::AgentTypeWrite {
            origin: AgentTypeWriteOrigin::Ui,
            previous_provider: None,
            new_provider: Some("fixture-provider".into()),
            attempted_provider: Some("fixture-provider".into()),
            caller_tab_id: None,
            session_id: Some("unverified-session".into()),
            nonce_verified: false,
            applied: true,
        },
    );
    fixture.service.record_lifecycle(
        &tab_id,
        LifecycleTransition::AgentTypeWrite {
            origin: AgentTypeWriteOrigin::Rebind,
            previous_provider: Some("fixture-provider".into()),
            new_provider: Some("fixture-provider".into()),
            attempted_provider: Some("fixture-provider".into()),
            caller_tab_id: None,
            session_id: Some("unverified-session".into()),
            nonce_verified: false,
            applied: false,
        },
    );
    assert!(matches!(
        fixture.service.failed_launch_restart_snapshot(&tab_id),
        Err(AgentRestartError::BadRequest(msg)) if msg.contains("exit_recovery_no_verified_session")
    ));

    // 3. Conflicting provider ownership (foreign owner or ambiguous multi-provider claim).
    for ambiguous in [false, true] {
        let (mux, tab_id) = deadline_tab_with_metadata(
            "pty-conflict",
            "fixture-provider",
            Some("conflicting-session"),
            "n3",
        );
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(if ambiguous {
                crate::agent::test_provider::MockProvider::with_session(
                    "fixture-provider",
                    "conflicting-session",
                )
            } else {
                crate::agent::test_provider::MockProvider::new("fixture-provider")
            }));
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_session(
                "other-provider",
                "conflicting-session",
            ),
        ));
        fixture.service.open_lifecycle_epoch(&tab_id).unwrap();
        fixture.service.record_lifecycle(
            &tab_id,
            LifecycleTransition::SessionBound {
                session_id: "conflicting-session".into(),
                binding_nonce: "prior".into(),
                source: Default::default(),
            },
        );
        assert!(matches!(
            fixture.service.failed_launch_restart_snapshot(&tab_id),
            Err(AgentRestartError::BadRequest(_))
        ));
    }
}

#[test]
fn failed_launch_recovery_enforces_one_flight_and_late_binding_fences() {
    use super::super::RestartAdmission;
    use crate::service::launch_diagnostics::AgentLaunchOrigin;
    use crate::service::orchestrator::launch_test_support::FakeRegisteredLaunch;
    use crate::session::lifecycle::LifecycleTransition;

    let (mux, tab_id) =
        deadline_tab_with_metadata("pty-fence", "fixture-provider", Some("owned"), "nonce-a");
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("fixture-provider", "owned"),
    ));
    fixture.service.open_lifecycle_epoch(&tab_id).unwrap();
    fixture.service.record_lifecycle(
        &tab_id,
        LifecycleTransition::SessionBound {
            session_id: "owned".into(),
            binding_nonce: "prior".into(),
            source: Default::default(),
        },
    );

    // One-flight fence covers both in-progress and settling restart phases.
    let mut claim = fixture
        .service
        .claim_restart(&tab_id, RestartAdmission::Standard)
        .unwrap();
    assert!(matches!(
        fixture.service.failed_launch_restart_snapshot(&tab_id),
        Err(AgentRestartError::BadRequest(msg)) if msg.contains("already in progress")
    ));
    claim.begin_settling();
    assert!(matches!(
        fixture.service.failed_launch_restart_snapshot(&tab_id),
        Err(AgentRestartError::BadRequest(msg)) if msg.contains("already in progress")
    ));
    assert!(matches!(
        fixture.service.claim_restart(&tab_id, RestartAdmission::Standard),
        Err(AgentRestartError::BadRequest(msg)) if msg.contains("already in progress")
    ));
    drop(claim);

    // Snapshot taken before teardown must refuse if late binding arrives.
    let snap = fixture
        .service
        .failed_launch_restart_snapshot(&tab_id)
        .unwrap()
        .unwrap();
    assert!(fixture
        .service
        .require_failed_launch_recovery_current(&snap)
        .is_ok());

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    assert!(matches!(
        fixture
            .service
            .require_failed_launch_recovery_current(&snap),
        Err(AgentRestartError::BadRequest(_))
    ));
    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab_id);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .boot_stage,
        Some(BootStage::AgentReady)
    );
    assert!(matches!(
        fixture
            .service
            .require_failed_launch_recovery_current(&snap),
        Err(AgentRestartError::BadRequest(_))
    ));

    // A competing launch starting before teardown must also fence the captured recovery snapshot.
    let (mux, tab_id) =
        deadline_tab_with_metadata("pty-race", "fixture-provider", Some("owned"), "nonce-a");
    let race_fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    race_fixture.set_agent_full_access(false);
    race_fixture
        .service
        .agent_registry
        .register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_session("fixture-provider", "owned"),
        ));
    race_fixture.service.open_lifecycle_epoch(&tab_id).unwrap();
    race_fixture.service.record_lifecycle(
        &tab_id,
        LifecycleTransition::SessionBound {
            session_id: "owned".into(),
            binding_nonce: "prior".into(),
            source: Default::default(),
        },
    );
    let race_snap = race_fixture
        .service
        .failed_launch_restart_snapshot(&tab_id)
        .unwrap()
        .unwrap();
    let racing = FakeRegisteredLaunch::register(
        &race_fixture.service,
        &tab_id,
        "pty-race",
        AgentLaunchOrigin::Resume,
    )
    .unwrap();
    assert!(matches!(
        race_fixture
            .service
            .require_failed_launch_recovery_current(&race_snap),
        Err(AgentRestartError::BadRequest(_))
    ));
    drop(racing);
}
