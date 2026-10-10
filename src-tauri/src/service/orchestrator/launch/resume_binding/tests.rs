use super::*;

#[test]
fn authenticated_rebind_retires_only_the_binding_bootstrap() {
    assert!(!fallback_still_required(true, true));
    assert!(fallback_still_required(true, false));
    assert!(fallback_still_required(false, true));
    assert!(fallback_still_required(false, false));
}

#[test]
fn restored_lazy_session_without_resume_target_gets_binding_turn() {
    let turn = binding_turn(true, true, false, None, None).unwrap();
    assert_eq!(turn.channel, PendingDeliveryChannel::FallbackBootstrap);
    assert!(!turn.payload.is_empty());
    assert_eq!(binding_turn(true, true, true, None, None), None);
}

#[test]
fn fresh_or_eager_session_does_not_gain_a_binding_turn() {
    for (relaunch, lazy_session, planned) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, false, true),
    ] {
        assert_eq!(
            binding_turn(relaunch, lazy_session, planned, None, None),
            None
        );
    }
}

#[test]
fn only_current_unauthenticated_relaunch_requires_recovery() {
    for relaunch in [false, true] {
        for authenticated in [false, true] {
            for current in [false, true] {
                assert_eq!(
                    recovery_required(relaunch, authenticated, current),
                    (relaunch, authenticated, current) == (true, false, true),
                );
            }
        }
    }
}

#[test]
fn unresolved_delivery_is_preserved_without_a_new_binding_turn() {
    for state in [
        PendingDeliveryState::Pending,
        PendingDeliveryState::Ingested,
        PendingDeliveryState::Unconfirmed,
    ] {
        assert_eq!(binding_turn(true, true, false, None, Some(state)), None);
    }
    for state in [
        PendingDeliveryState::AcceptedStrong,
        PendingDeliveryState::AcceptedWeak,
    ] {
        assert!(binding_turn(true, true, false, None, Some(state)).is_some());
    }
}

#[test]
fn binding_turn_replaces_only_a_settled_record_and_consumes_once() {
    use crate::service::boot_stage::BootStageSink;
    use crate::service::launch_delivery::{LaunchConsumptionOutcome, LaunchDeliveryGate};
    use std::cell::Cell;

    struct Sink;
    impl BootStageSink for Sink {
        fn record(&self, _: &str, _: BootStage, _: Option<&str>) {}
    }

    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let spec = binding_turn(true, true, false, None, None).unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    prepare_binding_turn(tab, &spec);
    let original = tab.pending_delivery.clone().unwrap();
    prepare_binding_turn(tab, &spec);
    assert_eq!(tab.pending_delivery.as_ref(), Some(&original));
    tab.pending_delivery
        .as_mut()
        .unwrap()
        .confirm_strong()
        .unwrap();
    prepare_binding_turn(tab, &spec);
    let replacement = tab.pending_delivery.as_ref().unwrap();
    assert_ne!(replacement.record_id(), original.record_id());
    assert_eq!(replacement.state(), PendingDeliveryState::Pending);

    let mux = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
        "binding-turn-test",
        state,
    ));
    let writes = Cell::new(0);
    let mut gate = LaunchDeliveryGate::new();
    gate.mark_provider_ready();
    for expected in [
        LaunchConsumptionOutcome::Confirmed,
        LaunchConsumptionOutcome::AlreadyConfirmed,
    ] {
        let outcome = gate
            .consume(&mux, &tab_id, &Sink, || {
                writes.set(writes.get() + 1);
                Ok(true)
            })
            .unwrap();
        assert_eq!(outcome, expected);
    }
    assert_eq!(writes.get(), 1);
}

#[test]
fn preparing_binding_turn_never_replaces_written_uncertainty() {
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let spec = binding_turn(true, true, false, None, None).unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    prepare_binding_turn(tab, &spec);
    tab.pending_delivery.as_mut().unwrap().mark_ingested();
    let written = tab.pending_delivery.clone();
    prepare_binding_turn(tab, &spec);
    assert_eq!(tab.pending_delivery, written);
    tab.pending_delivery.as_mut().unwrap().mark_unconfirmed();
    let uncertain = tab.pending_delivery.clone();
    prepare_binding_turn(tab, &spec);
    assert_eq!(tab.pending_delivery, uncertain);
}

#[test]
fn restored_lazy_session_with_resume_target_has_no_binding_turn() {
    for pending in [
        None,
        Some(PendingDeliveryState::AcceptedStrong),
        Some(PendingDeliveryState::AcceptedWeak),
    ] {
        assert_eq!(
            binding_turn(true, true, false, Some("owned-session"), pending),
            None
        );
    }
}

#[test]
fn binding_turn_decision_requires_new_restored_lazy_session_without_planned_turn() {
    for relaunch in [false, true] {
        for lazy_session in [false, true] {
            for planned in [false, true] {
                for resume_target in [None, Some("owned-session")] {
                    assert_eq!(
                        binding_turn_required(relaunch, lazy_session, planned, resume_target),
                        (relaunch, lazy_session, planned, resume_target)
                            == (true, true, false, None),
                    );
                }
            }
        }
    }
}

#[test]
fn binding_observed_before_deadline_suppresses_binding_turn() {
    use super::super::{LaunchRegistry, ProviderSessionBinding};
    use std::time::{Duration, Instant};

    let mut registry = LaunchRegistry::default();
    let launch = registry.begin("binding-wait", "binding-wait-pty".into());
    assert!(registry.bind_provider_session(&launch, "binding-wait-nonce".into()));
    let (started, waiting) = std::sync::mpsc::sync_channel(0);
    let deadline = Instant::now() + Duration::from_secs(1);
    std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            started.send(()).unwrap();
            await_fallback_required(&launch, true, deadline)
        });
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(registry.record_provider_session(
            "binding-wait",
            &ProviderSessionBinding {
                pty_id: "binding-wait-pty".into(),
                binding_nonce: "binding-wait-nonce".into(),
            },
        ));
        assert!(!waiter.join().unwrap());
    });
}

#[test]
fn binding_fallback_requires_absent_evidence_at_deadline() {
    use super::super::{LaunchRegistry, ProviderSessionBinding};
    use std::time::Instant;

    let mut registry = LaunchRegistry::default();
    let launch = registry.begin("binding-deadline", "binding-deadline-pty".into());
    assert!(registry.bind_provider_session(&launch, "binding-deadline-nonce".into()));
    assert!(await_fallback_required(&launch, true, Instant::now()));
    assert!(registry.record_provider_session(
        "binding-deadline",
        &ProviderSessionBinding {
            pty_id: "binding-deadline-pty".into(),
            binding_nonce: "binding-deadline-nonce".into(),
        },
    ));
    assert!(!await_fallback_required(&launch, true, Instant::now()));
}

#[test]
fn task_fallback_does_not_wait_for_binding() {
    use super::super::LaunchRegistry;
    use std::time::{Duration, Instant};

    let mut registry = LaunchRegistry::default();
    let launch = registry.begin("task-fallback", "task-fallback-pty".into());
    assert!(await_fallback_required(
        &launch,
        false,
        Instant::now() + Duration::from_secs(60)
    ));
}

#[test]
fn restored_unknown_store_resumes_own_target_and_hook_binds_without_user_turn() {
    use crate::multiplexer::types::PriorBindingNonce;
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("lazy-fixture".into());
    tab.restored_from_snapshot = true;
    tab.session_binding_nonce = Some("new-launch".into());
    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        session_id: None,
        resume_target_session_id: Some("own-resume-target".into()),
        nonce: "previous-launch".into(),
    });
    let mux = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
        "restore-test",
        state,
    ));
    assert!(crate::service::restored_launch::restored_session_resumable(
        None
    ));
    let launch = crate::service::pty_io::claim_restored_pty_boot(&mux, &tab_id)
        .unwrap()
        .unwrap();
    assert_eq!(launch.session_id.as_deref(), Some("own-resume-target"));
    assert_eq!(
        binding_turn(true, true, false, launch.session_id.as_deref(), None),
        None
    );
    assert!(mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .agent_session_id
        .is_none());
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("lazy-fixture")
            .with_session_starts_on_first_user_turn(),
    ));
    let confirmation = crate::api::hook_routes::bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "lazy-fixture",
        "own-resume-target",
        Some("new-launch"),
    )
    .unwrap();
    assert!(confirmation.nonce_verified);
    let state = mux.read();
    let tab = state.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("own-resume-target"));
    assert_eq!(
        tab.reported_session_binding_nonce.as_deref(),
        Some("new-launch")
    );
    assert!(tab.pending_delivery.is_none());
}

#[test]
fn restored_target_without_current_provider_confirmation_stays_unbound() {
    let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("lazy-fixture".into());
    tab.session_binding_nonce = Some("new-launch".into());
    let mux = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
        "restore-refusal-test",
        state,
    ));
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "lazy-fixture",
    )));
    for (session, nonce) in [("foreign", Some("foreign-launch")), ("unconfirmed", None)] {
        assert!(crate::api::hook_routes::bind_reported_session(
            &mux,
            &registry,
            &tab_id,
            "lazy-fixture",
            session,
            nonce,
        )
        .is_err());
        assert!(mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .is_none());
    }
    assert!(!crate::service::restored_launch::restored_session_resumable(Some(false)));
}
