use super::*;

#[test]
fn authenticated_rebind_retires_only_the_binding_bootstrap() {
    assert!(!fallback_still_required(true, true));
    assert!(fallback_still_required(true, false));
    assert!(fallback_still_required(false, true));
    assert!(fallback_still_required(false, false));
}

#[test]
fn resumed_lazy_session_gets_one_binding_turn() {
    let turn = binding_turn(true, true, false, None).unwrap();
    assert_eq!(turn.channel, PendingDeliveryChannel::FallbackBootstrap);
    assert!(!turn.payload.is_empty());
    assert_eq!(binding_turn(true, true, true, None), None);
}

#[test]
fn fresh_or_eager_session_does_not_gain_a_binding_turn() {
    for (relaunch, lazy_session, planned) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, false, true),
    ] {
        assert_eq!(binding_turn(relaunch, lazy_session, planned, None), None);
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
        assert_eq!(binding_turn(true, true, false, Some(state)), None);
    }
    for state in [
        PendingDeliveryState::AcceptedStrong,
        PendingDeliveryState::AcceptedWeak,
    ] {
        assert!(binding_turn(true, true, false, Some(state)).is_some());
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
    let spec = binding_turn(true, true, false, None).unwrap();
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
    let spec = binding_turn(true, true, false, None).unwrap();
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
