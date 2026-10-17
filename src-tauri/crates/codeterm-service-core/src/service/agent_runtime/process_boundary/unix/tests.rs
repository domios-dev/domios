use super::{validate_identity_fields, IdentityClassification};

#[test]
fn changed_birth_identity_is_refused_without_exit_evidence() {
    assert!(validate_identity_fields(Ok(12), 7, 11, 7, false).is_err());
}

#[test]
fn changed_process_group_is_refused_without_exit_evidence() {
    assert!(validate_identity_fields(Ok(11), 8, 11, 7, false).is_err());
}

#[test]
fn missing_process_identity_is_refused_without_exit_evidence() {
    assert!(validate_identity_fields(Err("process vanished".into()), -1, 11, 7, false).is_err());
}

#[test]
fn confirmed_exit_allows_identity_to_disappear_during_validation() {
    assert!(validate_identity_fields(Err("process vanished".into()), -1, 11, 7, true).is_ok());
}

#[test]
fn failed_group_lookup_with_matching_birth_requires_exit_before_signaling() {
    assert_eq!(
        validate_identity_fields(Ok(11), -1, 11, 7, false),
        Ok(IdentityClassification::AwaitExit)
    );
    assert_eq!(
        validate_identity_fields(Ok(11), -1, 11, 7, true),
        Ok(IdentityClassification::Exited)
    );
}

#[test]
fn failed_group_lookup_never_overrides_changed_birth_identity() {
    assert!(validate_identity_fields(Ok(12), -1, 11, 7, false).is_err());
}

#[test]
fn matching_birth_and_group_are_valid_without_exit() {
    assert_eq!(
        validate_identity_fields(Ok(11), 7, 11, 7, false),
        Ok(IdentityClassification::Valid)
    );
}
