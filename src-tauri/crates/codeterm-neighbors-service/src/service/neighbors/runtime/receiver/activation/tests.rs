use super::*;

#[test]
fn activation_failure_classification_requires_definitive_receiver_or_key_evidence() {
    for error in [
        Error::Transport,
        Error::Unavailable,
        Error::KeyInventoryPending,
        Error::Http {
            status: 408,
            retry_after_seconds: 0,
        },
        Error::Http {
            status: 503,
            retry_after_seconds: 0,
        },
        Error::Firestore {
            code: 4,
            operation: "opaque",
        },
        Error::Firestore {
            code: 14,
            operation: "opaque",
        },
    ] {
        assert!(Failure::from(error).transient, "{error:?}");
    }
    for error in [
        Error::LocalKeyUnavailable,
        Error::Revoked,
        Error::SessionRevoked,
        Error::ReceiverElsewhere,
        Error::InvalidEnvelope,
        Error::Refused,
        Error::Http {
            status: 403,
            retry_after_seconds: 0,
        },
    ] {
        assert!(!Failure::from(error).transient, "{error:?}");
    }
    assert!(!Failure::from("transport unavailable inside arbitrary private text").transient);
}

#[test]
fn activation_diagnostics_only_change_on_failure_recovery_or_generation_transition() {
    let mut state = State::default();
    let transient = Some(Failure::from(Error::Transport));
    assert!(state.transition(7, transient));
    assert!(!state.transition(7, transient));
    assert!(state.transition(7, Some(Failure::from(Error::Revoked))));
    assert!(state.transition(7, None));
    assert!(!state.transition(7, None));
    assert!(state.transition(7, transient));
    assert!(state.transition(8, transient));
}
