use super::validate_retained_identity;

#[test]
fn exit_between_identity_observations_completes_cleanup() {
    assert_eq!(
        validate_retained_identity(
            || Err(std::io::Error::from_raw_os_error(libc::ESRCH).to_string()),
            || Ok(true)
        ),
        Ok(())
    );
}

#[test]
fn identity_failure_without_exit_evidence_remains_a_refusal() {
    assert_eq!(
        validate_retained_identity(|| Err("changed identity".into()), || Ok(false)),
        Err("changed identity".into())
    );
}

#[test]
fn failed_exit_observation_remains_a_refusal() {
    assert_eq!(
        validate_retained_identity(
            || Err("missing identity".into()),
            || Err("invalid event".into())
        ),
        Err("invalid event".into())
    );
}
