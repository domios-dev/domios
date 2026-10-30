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
