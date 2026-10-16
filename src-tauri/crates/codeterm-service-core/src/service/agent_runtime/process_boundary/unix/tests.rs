use super::validate_identity_fields;

#[test]
fn changed_birth_identity_is_refused_without_exit_evidence() {
    assert!(validate_identity_fields(42, Ok(12), 7, 11, 7, || Ok(false)).is_err());
}

#[test]
fn changed_process_group_is_refused_without_exit_evidence() {
    assert!(validate_identity_fields(42, Ok(11), 8, 11, 7, || Ok(false)).is_err());
}

#[test]
fn missing_process_identity_is_refused_without_exit_evidence() {
    assert!(
        validate_identity_fields(42, Err("process vanished".into()), -1, 11, 7, || Ok(false))
            .is_err()
    );
}

#[test]
fn confirmed_exit_allows_identity_to_disappear_during_validation() {
    assert!(
        validate_identity_fields(42, Err("process vanished".into()), -1, 11, 7, || Ok(true))
            .is_ok()
    );
}
