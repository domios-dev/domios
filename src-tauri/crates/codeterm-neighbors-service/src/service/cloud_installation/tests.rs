use super::*;

#[test]
fn persisted_enrollment_disables_reinjected_creation_secret_even_if_membership_expires_or_is_invalid(
) {
    let root = tempfile::tempdir().unwrap();
    let key = InstallationKey::generate().public_bytes();
    let nonce = URL_SAFE_NO_PAD.encode([3; 32]);
    let supplied = || Some((URL_SAFE_NO_PAD.encode(key), Zeroizing::new(nonce.clone())));
    let open =
        || CloudInstallation::restore(root.path(), "mesh".into(), [4; 32], supplied()).unwrap();
    let bound = open();
    assert!(bound.bootstrap_responder().is_ok());
    assert!(bound.authorize_bootstrap_controller(&key, &[1; 32]).is_ok());
    assert!(bound
        .authorize_bootstrap_controller(&[0; 32], &[1; 32])
        .is_err());
    let invalid = DetachedGrant {
        alg: "Ed25519".into(),
        key_id: "invalid".into(),
        payload: String::new(),
        signature: String::new(),
    };
    let enrolled = Stored::Enrolled {
        account_id: "foreign".into(),
        discovery: bound.0.discovery.clone(),
        membership: invalid.clone(),
        signed_registry: invalid,
    };
    CloudInstallation::persist(&bound.0.store, root.path(), &enrolled).unwrap();
    drop(bound);
    let restored = open();
    assert!(restored.bootstrap_responder().is_err());
    assert!(restored
        .authorize_bootstrap_controller(&key, &[1; 32])
        .is_err());
    assert!(restored.identity().is_none());
    let record = restored.0.store.load(RECORD).unwrap().unwrap();
    assert!(!record.expose().contains(&nonce));
    assert!(matches!(
        serde_json::from_str(record.expose()).unwrap(),
        Stored::Enrolled { .. }
    ));
}
