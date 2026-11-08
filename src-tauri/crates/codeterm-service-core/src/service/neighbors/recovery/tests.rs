use super::*;

#[test]
fn startup_string_boundary_keeps_permanent_refusals_and_retries_setup_outages() {
    for error in [
        Error::Locked,
        Error::Unavailable,
        Error::PermissionDenied,
        Error::SessionRevoked,
        Error::Disabled,
        Error::Revoked,
        Error::ReceiverNotChosen,
        Error::ReceiverStatusUnknown,
    ] {
        assert_eq!(startup_error(&error.to_string()), error);
        assert!(!retryable(startup_error(&error.to_string())));
    }
    for error in [
        "installation temporarily unavailable",
        "metadata temporarily unavailable",
        "Firestore control.listen failed (gRPC code 16)",
    ] {
        assert_eq!(startup_error(error), Error::Transport);
    }
    assert_eq!(
        startup_error("Firestore control.listen failed (gRPC code 7)"),
        Error::PermissionDenied
    );
}

#[test]
fn entitlement_hold_logs_only_reason_transitions() {
    use super::hold::{entitlement_refusal, EntitlementHold, HoldTransition};
    assert!(entitlement_refusal(Error::Locked));
    for error in [Error::Transport, Error::PermissionDenied, Error::Revoked] {
        assert!(!entitlement_refusal(error));
    }
    let hold = EntitlementHold::default();
    let locked: Result<()> = Err(Error::Locked);
    assert_eq!(
        hold.settle(hold.epoch(), &locked),
        HoldTransition::Held(Error::Locked)
    );
    hold.changed();
    assert_eq!(
        hold.settle(hold.epoch(), &locked),
        HoldTransition::Unchanged
    );
    assert_eq!(
        hold.settle(hold.epoch(), &Ok(())),
        HoldTransition::Released(Error::Locked)
    );
    assert_eq!(
        hold.settle(hold.epoch(), &Ok(())),
        HoldTransition::Unchanged
    );
    let transport: Result<()> = Err(Error::Transport);
    assert_eq!(
        hold.settle(hold.epoch(), &transport),
        HoldTransition::Unchanged
    );
}

#[test]
fn entitlement_hold_waits_only_until_an_entitlement_change_after_the_attempt_began() {
    use futures_util::FutureExt;
    let hold = super::hold::EntitlementHold::default();
    let locked: Result<()> = Err(Error::Locked);
    assert!(Box::pin(hold.released()).now_or_never().is_some());

    let began = hold.epoch();
    hold.changed();
    hold.settle(began, &locked);
    assert!(Box::pin(hold.released()).now_or_never().is_some());

    hold.settle(hold.epoch(), &locked);
    let mut waiting = Box::pin(hold.released());
    assert!(waiting.as_mut().now_or_never().is_none());
    hold.changed();
    assert!(waiting.as_mut().now_or_never().is_some());
}

#[test]
fn named_no_receiver_setup_refusal_does_not_become_a_transport_retry() {
    assert_eq!(
        startup_error(
            "receiver_not_chosen: No machine receives messages from Anya. Receive them here?"
        ),
        Error::ReceiverNotChosen
    );
    assert!(!retryable(startup_error(
        "receiver_not_chosen: Choose receiving explicitly"
    )));
}
