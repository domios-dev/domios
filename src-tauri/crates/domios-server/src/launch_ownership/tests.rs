use super::retire_unattached;

#[test]
fn development_ownership_retires_only_an_unattached_orphan() {
    assert!(!retire_unattached(false, false));
    assert!(!retire_unattached(false, true));
    assert!(retire_unattached(true, false));
    assert!(!retire_unattached(true, true));
}
