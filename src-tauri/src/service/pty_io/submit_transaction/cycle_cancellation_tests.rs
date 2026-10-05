use super::*;
use crate::service::pty_io::submit_transaction::cycle::submit_cycle_registered;

#[test]
fn submit_cycle_cancellation_is_scoped_to_one_pty() {
    let first = claim_submit_cycle("cycle-scope-first");
    let second = claim_submit_cycle("cycle-scope-second");
    let newer_first = claim_submit_cycle("cycle-scope-first");

    assert!(!first.is_current());
    assert!(newer_first.is_current());
    assert!(second.is_current());
    assert!(Arc::ptr_eq(&first.cycle, &newer_first.cycle));
    assert!(!Arc::ptr_eq(&first.cycle, &second.cycle));
}

#[test]
fn submit_cycle_registry_reclaims_pty_key_after_last_claim() {
    let pty_id = "cycle-registry-reclamation";
    assert!(!submit_cycle_registered(pty_id));
    {
        let claim = claim_submit_cycle(pty_id);
        let superseded = claim_submit_cycle(pty_id);
        drop(claim);
        assert!(superseded.is_current());
        assert!(submit_cycle_registered(pty_id));
    }
    assert!(!submit_cycle_registered(pty_id));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the ServiceContext harness requires an interactive desktop runtime"
)]
fn superseded_claim_is_refused_before_any_write_or_parser_side_effect() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::delivery_error::DeliveryError;
    use crate::service::test_fixture::ServiceFixture;

    let pty_id = "superseded-claim-zero-write";
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.link_pty(&tab_id, pty_id.to_string());
    let fixture = ServiceFixture::new(mux);
    let superseded = claim_submit_cycle(pty_id);
    let _current = claim_submit_cycle(pty_id);

    let error = submit_text_with_key_claimed_and_follow_up(
        &fixture.service,
        pty_id,
        "payload",
        None,
        true,
        b"\r",
        VerifySubmitStrategy::None,
        VerifyOpts::default(),
        None,
        false,
        superseded,
    )
    .unwrap_err();

    assert!(matches!(
        &error,
        InjectionError::AgentInputUnavailable { pty_id: actual } if actual == pty_id
    ));
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert!(!tab.agent_type_locked);
    assert!(tab.agent_type.is_none());
    let delivery = DeliveryError::from_injection(pty_id, error);
    assert!(matches!(delivery, DeliveryError::TabBusyRefused { .. }));
    assert!(!delivery.bytes_written());
}
