use super::*;

#[test]
fn foreign_owner_is_refused_even_with_private_mode() {
    assert_eq!(private(0o600, 1001, 1000, 0o600), Err(Error::SecretStore));
    assert_eq!(private(0o600, 1000, 1000, 0o600), Ok(()));
    assert_eq!(private(0o640, 1000, 1000, 0o600), Err(Error::SecretStore));
    assert_eq!(private(0o644, 1000, 1000, 0o600), Err(Error::SecretStore));
}
