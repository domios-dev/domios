use super::super::LAUNCH_REGISTRY;
use super::LaunchGeneration;
use crate::multiplexer::state::MultiplexerState;
use crate::service::launch_diagnostics::AgentLaunchOrigin;

pub(super) fn retired_resume_fixture() -> (crate::service::test_fixture::ServiceFixture, String) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("resume-provider".into());
    tab.agent_session_id = Some("exact-session".into());
    tab.session_binding_nonce = Some("retired-nonce".into());
    tab.reported_session_binding_nonce = Some("retired-nonce".into());
    tab.restored_from_snapshot = true;
    tab.restorable_launch.exited_chat_binding =
        Some(crate::multiplexer::types::ExitedChatBinding {
            provider_id: "resume-provider".into(),
            session_id: "exact-session".into(),
            nonce: "retired-nonce".into(),
        });
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
    (crate::service::test_fixture::ServiceFixture::new(mux), id)
}

#[test]
fn retired_live_identity_uses_exact_resume_account_authority_without_rebinding() {
    let (fixture, id) = retired_resume_fixture();
    assert_eq!(
        crate::service::provider_accounts::for_tab(&fixture.service, &id).unwrap_err(),
        "Account tab has no harness"
    );
    for (provider, session) in [
        ("foreign", Some("exact-session")),
        ("resume-provider", Some("foreign-session")),
        ("resume-provider", None),
    ] {
        assert!(super::super::account_supervision::prepare(
            &fixture.service,
            &id,
            provider,
            session
        )
        .is_err());
    }
    let (account, reserved) = super::super::account_supervision::prepare(
        &fixture.service,
        &id,
        "resume-provider",
        Some("exact-session"),
    )
    .unwrap();
    assert!(reserved.is_some());
    assert!(account
        .resolve_bounded(&fixture.service, &id, None)
        .unwrap()
        .is_none());
    let foreign = codeterm_plugin::provider_account::InvocationContext::new(
        "foreign-provider".into(),
        "foreign-account".into(),
        Default::default(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    assert!(account
        .resolve_with(&fixture.service, &id, || Ok(Some(foreign)))
        .unwrap_err()
        .starts_with("launch_account_context_changed:"));
    let current = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    assert!(current.agent_type.is_none());
    assert!(current.agent_session_id.is_none());
    assert!(current.reported_session_binding_nonce.is_none());
    assert!(!current.launch_session_authenticated());
    assert_eq!(
        current
            .prior_session_binding_nonces
            .last()
            .unwrap()
            .resume_target_session_id
            .as_deref(),
        Some("exact-session")
    );
}

#[test]
fn abandoned_account_provisioning_retains_one_job_and_never_launches_late() {
    use crate::service::provider_accounts::{provisioning, test_support};
    for cancel in [false, true] {
        let (fixture, id) = retired_resume_fixture();
        let (account, _) = super::super::account_supervision::prepare(
            &fixture.service,
            &id,
            "resume-provider",
            Some("exact-session"),
        )
        .unwrap();
        let (entered, entry) = std::sync::mpsc::sync_channel(1);
        let (release, blocked) = std::sync::mpsc::sync_channel(1);
        let mut receipt = test_support::work_receipt(&fixture.service);
        let job = account
            .start_resolution(&fixture.service, &id, move || {
                entered.send(()).unwrap();
                blocked
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .map_err(|_| "fixture provisioning was not released".to_owned())?;
                Ok(None)
            })
            .unwrap();
        entry
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert_eq!(provisioning::retained_count(&fixture.service), 1);
        if !cancel {
            assert!(job
                .wait_blocking(std::time::Duration::ZERO)
                .unwrap_err()
                .starts_with("launch_account_supervision_unconfirmed:"));
        }
        assert!(!job.observed_completed_refusal());
        drop(job);
        assert_eq!(provisioning::retained_count(&fixture.service), 1);
        assert!(account
            .start_resolution(&fixture.service, &id, || panic!(
                "obsolete jobs must not queue"
            ))
            .is_err());
        let engine = crate::async_runtime::block_on(crate::service::agent_runtime::engine::ensure(
            &fixture.service,
            &id,
        ));
        assert!(
            matches!(engine, Err(crate::service::agent_runtime::model::RuntimeFailure::Refused(reason)) if reason.starts_with("account_provisioning_busy:"))
        );
        let replacement = fixture.service.rotate_session_binding_nonce(&id);
        let before =
            serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        release.send(()).unwrap();
        crate::async_runtime::block_on(test_support::settled(&mut receipt)).unwrap();
        assert_eq!(provisioning::retained_count(&fixture.service), 0);
        let after = fixture.service.mux.read().get_tab(&id).unwrap().clone();
        assert_eq!(serde_json::to_value(&after).unwrap(), before);
        assert_eq!(
            after.session_binding_nonce.as_deref(),
            Some(replacement.as_str())
        );
        assert!(!after.launch_session_authenticated());
        assert!(after.pty_id.is_none());
    }
}

#[test]
fn retained_startup_provisioning_grants_a_touched_chat_before_pending_pty_workers() {
    use crate::multiplexer::types::{AgentRole, TabBacking};
    use crate::service::provider_accounts::{self, provisioning};
    use crate::service::restore_admission::{notify_changed, track_startup_wave};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    const DEADLINE: Duration = Duration::from_secs(10);
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.get_active_workspace().unwrap().id.clone();
    let targets = (0..5)
        .map(|index| {
            let id = mux.create_tab(&workspace).unwrap();
            let tab = mux.get_tab_mut(&id).unwrap();
            tab.hidden = true;
            tab.restored_from_snapshot = true;
            tab.agent_type = Some("fixture-startup".to_owned());
            tab.session_binding_nonce = Some(format!("queued-{index}"));
            if index < 2 {
                tab.role = Some(AgentRole::Worker);
                tab.restorable_launch.account_id = Some("shared-fixture-account".to_owned());
            }
            if index == 2 {
                tab.backing = TabBacking::ChatBackend {
                    plugin_id: "fixture-chat".into(),
                    session_id: "restored-chat-thread".into(),
                };
            }
            id
        })
        .collect::<Vec<_>>();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    let _wave = track_startup_wave(svc, &targets);
    let (held_entered, entered) = mpsc::channel();
    let (release_owner, held) = mpsc::channel();
    let owner = provisioning::start(svc, move || {
        held_entered.send(()).unwrap();
        held.recv_timeout(DEADLINE).unwrap();
        Ok(None)
    })
    .unwrap();
    entered.recv_timeout(DEADLINE).unwrap();
    drop(owner);
    assert_eq!(provisioning::retained_count(svc), 1);
    assert!(matches!(
        provisioning::start(svc, || panic!("ordinary launch must fail fast")),
        Err(reason) if reason.starts_with("account_provisioning_busy:")
    ));
    {
        let _startup = provisioning::StartupScope::enter(svc, &targets[3]);
        assert!(provider_accounts::for_tab(svc, &targets[3])
            .unwrap()
            .is_none());
        assert_eq!(provisioning::retained_count(svc), 1);
    }

    let (granted, grants) = mpsc::channel();
    let mut waiting = Vec::new();
    for (index, id) in targets.iter().enumerate() {
        let target = id.clone();
        let service = svc.clone();
        let granted = granted.clone();
        waiting.push(std::thread::spawn(move || {
            let _startup = provisioning::StartupScope::enter(&service, &target);
            let retained_service = service.clone();
            provisioning::start(&service, move || {
                assert_eq!(provisioning::retained_count(&retained_service), 1);
                granted.send(index).unwrap();
                if index == 4 {
                    Err("definitive account refusal".into())
                } else {
                    Ok(None)
                }
            })?
            .wait_blocking(DEADLINE)
        }));
        provisioning::wait_for_startup_waiters(svc, index + 1, Instant::now() + DEADLINE);
    }
    svc.record_client_selection(Some(&workspace), Some(&targets[2]))
        .unwrap();
    svc.mux
        .write()
        .get_tab_mut(&targets[3])
        .unwrap()
        .session_binding_nonce = Some("replacement".into());
    notify_changed();
    provisioning::wait_for_startup_waiters(svc, 4, Instant::now() + DEADLINE);
    assert!(matches!(grants.try_recv(), Err(mpsc::TryRecvError::Empty)));
    release_owner.send(()).unwrap();
    let actual = (0..4)
        .map(|_| grants.recv_timeout(DEADLINE).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(actual, [2, 0, 1, 4]);
    for (index, job) in waiting.into_iter().enumerate() {
        let result = job.join().unwrap();
        match index {
            3 => assert!(result
                .unwrap_err()
                .contains("startup provisioning authority changed")),
            4 => assert_eq!(result.unwrap_err(), "definitive account refusal"),
            _ => assert!(result.unwrap().is_none()),
        }
    }
    assert_eq!(provisioning::retained_count(svc), 0);
}

#[test]
fn account_refusal_proof_requires_observed_settlement_before_cancellation() {
    for cancel in [false, true] {
        let (fixture, id) = retired_resume_fixture();
        let (account, _) = super::super::account_supervision::prepare(
            &fixture.service,
            &id,
            "resume-provider",
            Some("exact-session"),
        )
        .unwrap();
        let job = account
            .start_resolution(&fixture.service, &id, || {
                Err("definitive account refusal".into())
            })
            .unwrap();
        assert_eq!(
            job.wait_blocking(std::time::Duration::from_secs(10))
                .unwrap_err(),
            "definitive account refusal"
        );
        assert!(job.observed_completed_refusal());
        if cancel {
            let job = std::sync::Arc::new(job);
            let cancelled = job.clone();
            std::thread::spawn(move || {
                assert!(cancelled.wait_blocking(std::time::Duration::ZERO).is_err());
            })
            .join()
            .unwrap();
            assert!(!job.observed_completed_refusal());
        }
    }
}

#[test]
fn switch_launch_reserves_only_the_admitted_reset_and_observer_expiry_keeps_its_owner() {
    use crate::service::messenger::switch_handoff::with_switch_launch;
    for changed in ["team", "parent", "report", "account", "namespace", "nonce"] {
        let (fixture, id) = retired_resume_fixture();
        let expected = {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&id).unwrap();
            tab.agent_type = Some("resume-provider".into());
            tab.clone()
        };
        with_switch_launch(&fixture.service, &expected, || {
            {
                let mut mux = fixture.service.mux.write();
                let tab = mux.get_tab_mut(&id).unwrap();
                match changed {
                    "team" => tab.team_id = Some("replacement-team".into()),
                    "parent" => tab.parent_tab_id = Some("replacement-parent".into()),
                    "report" => tab.report_target_tab_id = Some("replacement-report".into()),
                    "account" => {
                        tab.restorable_launch.account_id = Some("replacement-account".into())
                    }
                    "namespace" => {
                        tab.restorable_launch.account_namespace =
                            Some("replacement-namespace".into())
                    }
                    "nonce" => tab.session_binding_nonce = Some("replacement-nonce".into()),
                    _ => unreachable!(),
                }
            }
            let before =
                serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
            assert!(super::super::account_supervision::prepare(
                &fixture.service,
                &id,
                "resume-provider",
                None
            )
            .is_err());
            assert_eq!(
                serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap(),
                before
            );
            Ok(())
        })
        .unwrap();
    }
    let (fixture, id) = retired_resume_fixture();
    let expected = {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("resume-provider".into());
        tab.clone()
    };
    let mut surviving = None;
    let result: Result<(), String> = with_switch_launch(&fixture.service, &expected, || {
        let (account, nonce) = super::super::account_supervision::prepare(
            &fixture.service,
            &id,
            "resume-provider",
            None,
        )?;
        assert!(nonce.is_some());
        surviving = Some(account);
        Err("provider_switch_readiness_unconfirmed: fixture observation expired".into())
    });
    assert!(result.is_err());
    let account = surviving.unwrap();
    account.validate(&fixture.service, &id).unwrap();
    assert!(account
        .resolve_bounded(&fixture.service, &id, None)
        .unwrap()
        .is_none());
    assert!(!fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .launch_session_authenticated());
}

#[test]
fn provisioning_cannot_authorize_a_changed_account_namespace_nonce_or_team() {
    for changed in [
        "provider",
        "account",
        "namespace",
        "nonce",
        "epoch",
        "operation",
        "team",
    ] {
        let (fixture, id) = retired_resume_fixture();
        let (account, _) = super::super::account_supervision::prepare(
            &fixture.service,
            &id,
            "resume-provider",
            Some("exact-session"),
        )
        .unwrap();
        let result = account.resolve_with(&fixture.service, &id, || {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&id).unwrap();
            match changed {
                "provider" => tab.agent_type = Some("foreign".into()),
                "account" => tab.restorable_launch.account_id = Some("foreign-account".into()),
                "namespace" => {
                    tab.restorable_launch.account_namespace = Some("foreign-namespace".into())
                }
                "nonce" => tab.session_binding_nonce = Some("foreign-nonce".into()),
                "epoch" => tab.lifecycle_epoch = Some(999),
                "operation" => tab.lifecycle_operation_id = Some("foreign-operation".into()),
                "team" => tab.team_id = Some("foreign-team".into()),
                _ => unreachable!(),
            }
            Ok(None)
        });
        assert!(
            result
                .unwrap_err()
                .starts_with("launch_account_source_changed:"),
            "{changed}"
        );
        let before =
            serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        let launch = LaunchGeneration::begin(&id, "stale-pty".into());
        assert!(!account.fail_if_current(
            &launch,
            &fixture.service,
            super::super::LaunchFailure::StartupSupervision("stale account failure".into())
        ));
        let after = serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        assert_eq!(
            after, before,
            "{changed}: stale failure must preserve replacement"
        );
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_session_id
            .is_none());
    }
}

#[test]
fn conditional_reservation_refuses_stale_source_without_touching_replacement() {
    for changed in [
        "provider",
        "session",
        "nonce",
        "account",
        "namespace",
        "operation",
        "pty",
        "team",
    ] {
        let (fixture, id) = retired_resume_fixture();
        let expected = fixture.service.mux.read().get_tab(&id).unwrap().clone();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&id).unwrap();
            match changed {
                "provider" => tab.agent_type = Some("replacement".into()),
                "session" => tab.agent_session_id = Some("replacement-session".into()),
                "nonce" => tab.session_binding_nonce = Some("replacement-nonce".into()),
                "account" => tab.restorable_launch.account_id = Some("replacement-account".into()),
                "namespace" => {
                    tab.restorable_launch.account_namespace = Some("replacement-namespace".into())
                }
                "operation" => tab.lifecycle_operation_id = Some("replacement-operation".into()),
                "pty" => tab.pty_id = Some("replacement-pty".into()),
                "team" => tab.team_id = Some("replacement-team".into()),
                _ => unreachable!(),
            }
        }
        let before =
            serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        assert!(fixture
            .service
            .reserve_agent_launch_binding_nonce_if_current(&expected, Some("exact-session"), None)
            .is_err());
        let after = serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        assert_eq!(after, before, "{changed}");
    }
}

#[test]
fn replacement_reservation_cannot_adopt_authority_changed_after_account_preflight() {
    for changed in [
        "nonce",
        "operation",
        "account",
        "namespace",
        "team",
        "generation",
        "owned-pty",
    ] {
        let (fixture, id) = retired_resume_fixture();
        let (account, reserved) = super::super::account_supervision::prepare(
            &fixture.service,
            &id,
            "resume-provider",
            Some("exact-session"),
        )
        .unwrap();
        let mut launch = LaunchGeneration::begin(&id, "old-pty".into());
        launch.binding_nonce = reserved;
        account.validate(&fixture.service, &id).unwrap();
        assert!(launch.replace("replacement-pty".into()));
        fixture.service.mux.write().get_tab_mut(&id).unwrap().pty_id =
            Some("replacement-pty".into());
        if changed == "nonce" {
            fixture.service.rotate_session_binding_nonce(&id);
        }
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&id).unwrap();
            match changed {
                "nonce" => (),
                "operation" => tab.lifecycle_operation_id = Some("B-operation".into()),
                "account" => tab.restorable_launch.account_id = Some("B-account".into()),
                "namespace" => tab.restorable_launch.account_namespace = Some("B-namespace".into()),
                "team" => tab.team_id = Some("B-team".into()),
                "generation" | "owned-pty" => (),
                _ => unreachable!(),
            }
        }
        let newer = (changed == "generation").then(|| LaunchGeneration::begin(&id, "B-pty".into()));
        let before =
            serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        let result = account.reserve_replacement(&fixture.service, &launch);
        let after = serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        if changed == "owned-pty" {
            let nonce = result.unwrap();
            assert_ne!(after, before);
            assert_eq!(
                fixture
                    .service
                    .mux
                    .read()
                    .get_tab(&id)
                    .unwrap()
                    .session_binding_nonce
                    .as_deref(),
                Some(nonce.as_str())
            );
            account.validate(&fixture.service, &id).unwrap();
        } else {
            assert!(result.is_err(), "{changed}");
            assert_eq!(after, before, "{changed}: A must not rotate or revoke B");
        }
        LAUNCH_REGISTRY
            .lock()
            .finish(newer.as_ref().unwrap_or(&launch));
    }
}

#[test]
fn cancelled_readiness_registration_retires_only_its_own_attempt() {
    use crate::service::launch_readiness::{
        arm_with_origin, attempt_is_armed, retire_attempt, LaunchAttempt,
    };
    let first = LaunchAttempt {
        tab_id: "cancel-readiness-registration".into(),
        pty_id: "owned-pty".into(),
        binding_nonce: "first-nonce".into(),
    };
    arm_with_origin(first.clone(), AgentLaunchOrigin::Resume);
    let registration = super::super::generation::ReadinessRegistration::new(first.clone());
    let second = LaunchAttempt {
        binding_nonce: "second-nonce".into(),
        ..first.clone()
    };
    arm_with_origin(second.clone(), AgentLaunchOrigin::Resume);
    drop(registration);
    assert!(attempt_is_armed(&second));
    retire_attempt(&second);
    arm_with_origin(first.clone(), AgentLaunchOrigin::Resume);
    drop(super::super::generation::ReadinessRegistration::new(
        first.clone(),
    ));
    assert!(!attempt_is_armed(&first));
    arm_with_origin(first.clone(), AgentLaunchOrigin::Resume);
    let mut handed_off = super::super::generation::ReadinessRegistration::new(first.clone());
    handed_off.handoff();
    drop(handed_off);
    assert!(attempt_is_armed(&first));
    retire_attempt(&first);
}

#[test]
fn early_send_nonce_survives_initial_launch_registration() {
    for session in [None, Some("saved-session")] {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("opaque-provider".into());
        tab.agent_session_id = session.map(str::to_string);
        tab.restored_from_snapshot = session.is_some();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        super::super::mark_tab_agent_launch_started(&fixture.service.mux, &tab_id);
        let nonce =
            super::super::generation::reserve_binding_nonce(&fixture.service, &tab_id, session);
        let mut payload = crate::service::messenger::AgentDeliveryPayload {
            durable_delivery: None,
            trace_id: None,
            transport: crate::service::messenger::Messenger::from(
                crate::service::messenger::MessageEnvelope::from_codeterm(),
            )
            .framed_delivery("opaque-payload"),
            observer: None,
            idempotency_key: None,
            expected_pty_id: None,
            expected_session_id: None,
            expected_binding_nonce: None,
        };
        let claim = fixture.service.pty_ownership.acquire(&tab_id).unwrap();
        fixture
            .service
            .pin_agent_delivery_binding(&claim, &mut payload)
            .unwrap();
        assert!(payload.expected_pty_id.is_none());
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .pty_id = Some("registered-pty".into());
        let mut launch = LaunchGeneration::begin(&tab_id, "registered-pty".into());
        launch
            .capture_binding_nonce(
                &fixture.service,
                None,
                None,
                AgentLaunchOrigin::Resume,
                session,
                Some(nonce.clone()),
            )
            .unwrap();
        assert_eq!(launch.binding_nonce.as_deref(), Some(nonce.as_str()));
        let attempt = launch.attempt().unwrap();
        assert!(crate::service::launch_readiness::attempt_is_armed(&attempt));
        let witnesses = crate::service::process_tree::ProviderTreeWitnesses::default();
        let key = crate::service::process_tree::ProviderTreeKey {
            tab_id: tab_id.clone(),
            pty_id: attempt.pty_id.clone(),
            binding_nonce: Some(nonce.clone()),
            executable_name: "opaque-provider".into(),
        };
        witnesses.record_for_test(
            &key,
            crate::service::process_tree::ProviderProcessTreeSnapshot::Absent,
        );
        let absent = witnesses.record_for_test(
            &key,
            crate::service::process_tree::ProviderProcessTreeSnapshot::Absent,
        );
        let proof = crate::service::launch_readiness::LaunchExitProof::from_finished_marker(
            &attempt,
            &absent,
            Some(0),
            false,
            Some(false),
        )
        .unwrap();
        for _ in 0..2 {
            assert!(crate::service::launch_readiness::suspect_exit(
                &tab_id,
                Some(&attempt.pty_id),
                Some(0),
                &proof,
            ));
        }
        assert!(crate::service::launch_readiness::note_acknowledged(
            &tab_id,
            "registered-pty",
            &nonce,
        ));
        assert!(!crate::service::launch_readiness::suspect_exit(
            &tab_id,
            Some(&attempt.pty_id),
            Some(0),
            &proof,
        ));
        assert!(fixture
            .service
            .agent_delivery_binding_matches(&tab_id, &payload));
        if session.is_none() {
            assert!(launch.emit_first_turn_input_ready(&fixture.service));
            let mux = fixture.service.mux.read();
            let tab = mux.get_tab(&tab_id).unwrap();
            assert_eq!(
                tab.boot_stage,
                Some(crate::service::boot_stage::BootStage::ProviderReady)
            );
            assert!(tab.boot_failure_reason.is_none());
            assert_eq!(tab.session_binding_nonce.as_deref(), Some(nonce.as_str()));
            assert!(tab.agent_session_id.is_none());
            assert!(!tab.launch_session_authenticated());
        }
        assert!(LAUNCH_REGISTRY.lock().finish(&launch));
        crate::service::launch_readiness::retire_attempt(&attempt);
    }
}

#[test]
fn launch_whose_reservation_was_reminted_cannot_replace_the_new_nonce() {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("opaque-provider".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    super::super::mark_tab_agent_launch_started(&fixture.service.mux, &tab_id);
    let reserved = super::super::generation::reserve_binding_nonce(&fixture.service, &tab_id, None);
    let replacement = fixture.service.rotate_session_binding_nonce(&tab_id);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .pty_id = Some("registered-pty".into());
    let mut launch = LaunchGeneration::begin(&tab_id, "registered-pty".into());
    assert!(launch
        .capture_binding_nonce(
            &fixture.service,
            None,
            None,
            AgentLaunchOrigin::AtomicSpawn,
            None,
            Some(reserved.clone()),
        )
        .is_err());
    assert!(launch.binding_nonce.is_none());
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone()),
        Some(replacement)
    );
    assert!(LAUNCH_REGISTRY.lock().finish(&launch));
}

#[test]
fn deferred_structured_preparation_adopts_the_restored_reservation_a_send_pinned() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("saved-session".into());
    tab.session_binding_nonce = Some("snapshot-nonce".into());
    tab.reported_session_binding_nonce = Some("snapshot-nonce".into());
    tab.restored_from_snapshot = true;
    tab.protected = false;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    crate::service::pty_io::claim_restored_launch(
        &fixture.service.mux,
        &tab_id,
        "opaque-provider",
        Some("saved-session"),
        None,
        true,
    )
    .unwrap()
    .expect("restore claims its launch");
    let reserved = super::super::reserve_launch_binding_nonce(
        &fixture.service,
        &tab_id,
        Some("saved-session"),
    )
    .expect("a restored relaunch reserves on the caller's thread");
    let mut payload = crate::service::messenger::AgentDeliveryPayload {
        durable_delivery: None,
        trace_id: None,
        transport: crate::service::messenger::Messenger::from(
            crate::service::messenger::MessageEnvelope::from_codeterm(),
        )
        .framed_delivery("opaque-payload"),
        observer: None,
        idempotency_key: None,
        expected_pty_id: None,
        expected_session_id: None,
        expected_binding_nonce: None,
    };
    let claim = fixture.service.pty_ownership.acquire(&tab_id).unwrap();
    fixture
        .service
        .pin_agent_delivery_binding(&claim, &mut payload)
        .unwrap();
    drop(claim);
    assert_eq!(
        payload.expected_binding_nonce.as_deref(),
        Some(reserved.as_str())
    );
    assert_eq!(
        fixture.service.ensure_session_binding_nonce(&tab_id),
        reserved
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .pty_id = Some("registered-pty".into());
    let mut launch = LaunchGeneration::begin(&tab_id, "registered-pty".into());
    launch
        .capture_binding_nonce(
            &fixture.service,
            None,
            None,
            AgentLaunchOrigin::RestoredSnapshot,
            Some("saved-session"),
            Some(reserved.clone()),
        )
        .unwrap();
    assert_eq!(launch.binding_nonce, payload.expected_binding_nonce);
    assert!(fixture
        .service
        .agent_delivery_binding_matches(&tab_id, &payload));
    assert!(LAUNCH_REGISTRY.lock().finish(&launch));
    assert_eq!(
        fixture.service.ensure_session_binding_nonce(&tab_id),
        reserved
    );
    let unreserved = fixture.service.rotate_session_binding_nonce(&tab_id);
    assert_ne!(unreserved, reserved);
    assert!(!fixture
        .service
        .agent_delivery_binding_matches(&tab_id, &payload));
}

#[test]
fn restore_resumes_idle_worker() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "restored-worker-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
        tab.agent_type = Some("restored-worker-fixture".into());
        tab.agent_session_id = Some("restored-worker-session".into());
        tab.pty_id = Some(pty_id.into());
        tab.restored_from_snapshot = true;
        tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some("plan-restored".into()),
            task_id: Some("task-restored".into()),
            attempt_id: Some("attempt-restored".into()),
            spawn_handle: None,
            bound_at: 1,
        });
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);

    for origin in [
        AgentLaunchOrigin::RestoredSnapshot,
        AgentLaunchOrigin::Resume,
        AgentLaunchOrigin::Restart,
        AgentLaunchOrigin::ExitRecovery,
        AgentLaunchOrigin::AutomaticExitRecovery,
    ] {
        let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
        launch
            .capture_binding_nonce(&harness.service, None, None, origin, None, None)
            .unwrap_or_else(|error| panic!("{origin:?} relaunch of a settled Worker: {error}"));
        let current = harness
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone());
        assert_eq!(current, launch.binding_nonce);
    }

    let mut dispatch = LaunchGeneration::begin(&tab_id, pty_id.into());
    assert!(dispatch
        .capture_binding_nonce(
            &harness.service,
            None,
            None,
            AgentLaunchOrigin::OrchestratedSpawn,
            None,
            None,
        )
        .is_err());
}

#[test]
fn resume_launch_keeps_its_target_session_after_identity_clear() {
    assert_resumed_launch_target(AgentLaunchOrigin::Resume);
}

#[test]
fn restored_launch_reauthenticates_its_saved_history_marker() {
    assert_resumed_launch_target(AgentLaunchOrigin::RestoredSnapshot);
}

fn assert_resumed_launch_target(origin: AgentLaunchOrigin) {
    use crate::multiplexer::types::PriorBindingNonce;

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "resume-target-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some(pty_id.into());
        tab.prior_session_binding_nonces.push(PriorBindingNonce {
            session_id: Some("resume-target-session".into()),
            resume_target_session_id: None,
            nonce: "prior-binding-nonce".into(),
        });
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
    launch
        .capture_binding_nonce(
            &harness.service,
            None,
            None,
            origin,
            Some("resume-target-session"),
            None,
        )
        .expect("resume launch records its exact target");
    assert_eq!(
        crate::service::launch_session_detection::registered_launch_marker(&tab_id).as_deref(),
        Some("resume-target-session")
    );
    {
        let mut mux = harness.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
        assert_eq!(
            tab.prior_session_binding_nonces
                .last()
                .unwrap()
                .resume_target_session_id
                .as_deref(),
            Some("resume-target-session")
        );
    }
    LAUNCH_REGISTRY.lock().finish(&launch);
}
