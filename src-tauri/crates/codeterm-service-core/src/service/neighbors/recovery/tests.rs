use super::*;

#[test]
fn startup_string_boundary_keeps_permanent_refusals_and_retries_setup_outages() {
    for error in [Error::Locked, Error::PermissionDenied, Error::SessionRevoked, Error::Disabled, Error::Revoked] {
        assert_eq!(startup_error(&error.to_string()), error);
        assert!(!retryable(startup_error(&error.to_string())));
    }
    for error in ["installation temporarily unavailable", "metadata temporarily unavailable", "Firestore control.listen failed (gRPC code 16)"] {
        assert_eq!(startup_error(error), Error::Transport);
    }
    assert_eq!(startup_error("Firestore control.listen failed (gRPC code 7)"), Error::PermissionDenied);
}
