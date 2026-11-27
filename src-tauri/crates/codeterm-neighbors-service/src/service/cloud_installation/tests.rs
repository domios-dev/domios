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

fn enrolled_receipt(installation: &CloudInstallation) -> Stored {
    let invalid = DetachedGrant {
        alg: "Ed25519".into(),
        key_id: "invalid".into(),
        payload: String::new(),
        signature: String::new(),
    };
    Stored::Enrolled {
        account_id: "foreign".into(),
        discovery: installation.0.discovery.clone(),
        membership: invalid.clone(),
        signed_registry: invalid,
    }
}

#[test]
fn resupplied_identical_binding_is_dropped_unread_after_record_exists() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = tempfile::tempdir().unwrap();
    let key = InstallationKey::generate().public_bytes();
    let nonce = URL_SAFE_NO_PAD.encode([7; 32]);
    let pair = || Some((URL_SAFE_NO_PAD.encode(key), Zeroizing::new(nonce.clone())));

    let initial_reads = AtomicUsize::new(0);
    let initial_discards = AtomicUsize::new(0);
    let bound = CloudInstallation::restore_with(
        root.path(),
        "mesh".into(),
        [4; 32],
        || {
            initial_reads.fetch_add(1, Ordering::SeqCst);
            Ok(pair())
        },
        || {
            initial_discards.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    assert_eq!(initial_reads.load(Ordering::SeqCst), 1);
    assert_eq!(initial_discards.load(Ordering::SeqCst), 0);
    let id = bound.installation_id().to_owned();

    let bound_reads = AtomicUsize::new(0);
    let bound_discards = AtomicUsize::new(0);
    let bound_restart = CloudInstallation::restore_with(
        root.path(),
        "mesh".into(),
        [4; 32],
        || {
            bound_reads.fetch_add(1, Ordering::SeqCst);
            Ok(pair())
        },
        || {
            bound_discards.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    assert_eq!(bound_reads.load(Ordering::SeqCst), 0);
    assert_eq!(bound_discards.load(Ordering::SeqCst), 1);
    assert_eq!(bound_restart.installation_id(), id);
    assert!(bound_restart
        .authorize_bootstrap_controller(&key, &[1; 32])
        .is_ok());

    CloudInstallation::persist(&bound.0.store, root.path(), &enrolled_receipt(&bound)).unwrap();
    drop((bound, bound_restart));

    let enrolled_reads = AtomicUsize::new(0);
    let enrolled_discards = AtomicUsize::new(0);
    let enrolled_restart = CloudInstallation::restore_with(
        root.path(),
        "mesh".into(),
        [4; 32],
        || {
            enrolled_reads.fetch_add(1, Ordering::SeqCst);
            Ok(pair())
        },
        || {
            enrolled_discards.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    assert_eq!(enrolled_reads.load(Ordering::SeqCst), 0);
    assert_eq!(enrolled_discards.load(Ordering::SeqCst), 1);
    assert_eq!(enrolled_restart.installation_id(), id);
    assert!(enrolled_restart.bootstrap_responder().is_err());
    assert!(enrolled_restart
        .authorize_bootstrap_controller(&key, &[1; 32])
        .is_err());
    let record = enrolled_restart.0.store.load(RECORD).unwrap().unwrap();
    assert!(!record.expose().contains(&nonce));
    assert!(matches!(
        serde_json::from_str(record.expose()).unwrap(),
        Stored::Enrolled { .. }
    ));
}

#[test]
fn resupplied_different_binding_is_dropped_unread_after_record_exists() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = tempfile::tempdir().unwrap();
    let original_key = InstallationKey::generate().public_bytes();
    let original_nonce = URL_SAFE_NO_PAD.encode([8; 32]);
    let other_key = InstallationKey::generate().public_bytes();
    let other_nonce = URL_SAFE_NO_PAD.encode([9; 32]);

    let bound = CloudInstallation::restore(
        root.path(),
        "mesh".into(),
        [4; 32],
        Some((
            URL_SAFE_NO_PAD.encode(original_key),
            Zeroizing::new(original_nonce),
        )),
    )
    .unwrap();
    let id = bound.installation_id().to_owned();

    let bound_reads = AtomicUsize::new(0);
    let bound_discards = AtomicUsize::new(0);
    let bound_restart = CloudInstallation::restore_with(
        root.path(),
        "mesh".into(),
        [4; 32],
        || {
            bound_reads.fetch_add(1, Ordering::SeqCst);
            Ok(Some((
                URL_SAFE_NO_PAD.encode(other_key),
                Zeroizing::new(other_nonce.clone()),
            )))
        },
        || {
            bound_discards.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    assert_eq!(bound_reads.load(Ordering::SeqCst), 0);
    assert_eq!(bound_discards.load(Ordering::SeqCst), 1);
    assert_eq!(bound_restart.installation_id(), id);
    assert!(bound_restart
        .authorize_bootstrap_controller(&original_key, &[1; 32])
        .is_ok());
    assert!(bound_restart
        .authorize_bootstrap_controller(&other_key, &[1; 32])
        .is_err());
    let bound_record = bound_restart.0.store.load(RECORD).unwrap().unwrap();
    assert!(!bound_record.expose().contains(&other_nonce));

    CloudInstallation::persist(&bound.0.store, root.path(), &enrolled_receipt(&bound)).unwrap();
    drop((bound, bound_restart));

    let enrolled_reads = AtomicUsize::new(0);
    let enrolled_discards = AtomicUsize::new(0);
    let enrolled_restart = CloudInstallation::restore_with(
        root.path(),
        "mesh".into(),
        [4; 32],
        || {
            enrolled_reads.fetch_add(1, Ordering::SeqCst);
            Ok(Some((
                URL_SAFE_NO_PAD.encode(other_key),
                Zeroizing::new(other_nonce.clone()),
            )))
        },
        || {
            enrolled_discards.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    assert_eq!(enrolled_reads.load(Ordering::SeqCst), 0);
    assert_eq!(enrolled_discards.load(Ordering::SeqCst), 1);
    assert_eq!(enrolled_restart.installation_id(), id);
    assert!(enrolled_restart.bootstrap_responder().is_err());
    assert!(enrolled_restart
        .authorize_bootstrap_controller(&original_key, &[1; 32])
        .is_err());
    assert!(enrolled_restart
        .authorize_bootstrap_controller(&other_key, &[1; 32])
        .is_err());
    let enrolled_record = enrolled_restart.0.store.load(RECORD).unwrap().unwrap();
    assert!(!enrolled_record.expose().contains(&other_nonce));
    assert!(matches!(
        serde_json::from_str(enrolled_record.expose()).unwrap(),
        Stored::Enrolled { .. }
    ));
}

#[test]
fn resupplied_malformed_binding_is_dropped_unread_after_record_exists() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fresh = tempfile::tempdir().unwrap();
    assert!(CloudInstallation::restore_with(
        fresh.path(),
        "mesh".into(),
        [4; 32],
        || Err("Invalid cloud binding".into()),
        || {},
    )
    .is_err());

    let root = tempfile::tempdir().unwrap();
    let key = InstallationKey::generate().public_bytes();
    let nonce = URL_SAFE_NO_PAD.encode([10; 32]);
    let bound = CloudInstallation::restore(
        root.path(),
        "mesh".into(),
        [4; 32],
        Some((URL_SAFE_NO_PAD.encode(key), Zeroizing::new(nonce))),
    )
    .unwrap();
    let id = bound.installation_id().to_owned();

    let bound_reads = AtomicUsize::new(0);
    let bound_discards = AtomicUsize::new(0);
    let bound_restart = CloudInstallation::restore_with(
        root.path(),
        "mesh".into(),
        [4; 32],
        || {
            bound_reads.fetch_add(1, Ordering::SeqCst);
            Err("Invalid cloud binding".into())
        },
        || {
            bound_discards.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    assert_eq!(bound_reads.load(Ordering::SeqCst), 0);
    assert_eq!(bound_discards.load(Ordering::SeqCst), 1);
    assert_eq!(bound_restart.installation_id(), id);
    assert!(bound_restart
        .authorize_bootstrap_controller(&key, &[1; 32])
        .is_ok());

    CloudInstallation::persist(&bound.0.store, root.path(), &enrolled_receipt(&bound)).unwrap();
    drop((bound, bound_restart));

    let enrolled_reads = AtomicUsize::new(0);
    let enrolled_discards = AtomicUsize::new(0);
    let enrolled_restart = CloudInstallation::restore_with(
        root.path(),
        "mesh".into(),
        [4; 32],
        || {
            enrolled_reads.fetch_add(1, Ordering::SeqCst);
            Err("Invalid cloud binding".into())
        },
        || {
            enrolled_discards.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    assert_eq!(enrolled_reads.load(Ordering::SeqCst), 0);
    assert_eq!(enrolled_discards.load(Ordering::SeqCst), 1);
    assert_eq!(enrolled_restart.installation_id(), id);
    assert!(enrolled_restart.bootstrap_responder().is_err());
    let record = enrolled_restart.0.store.load(RECORD).unwrap().unwrap();
    assert!(matches!(
        serde_json::from_str(record.expose()).unwrap(),
        Stored::Enrolled { .. }
    ));
}

#[tokio::test(start_paused = true)]
async fn enrollment_lifecycle_wait_bound_covers_cold_boot_and_times_out_at_bound() {
    assert!(
        ENROLLMENT_LIFECYCLE_WAIT_TIMEOUT >= std::time::Duration::from_secs(180),
        "guest lifecycle wait failure bound must be at least 180 s"
    );
    let root = tempfile::tempdir().unwrap();
    let controller_key = InstallationKey::generate().public_bytes();
    let nonce = URL_SAFE_NO_PAD.encode([11; 32]);
    let bound = CloudInstallation::restore(
        root.path(),
        "mesh".into(),
        [4; 32],
        Some((
            URL_SAFE_NO_PAD.encode(controller_key),
            Zeroizing::new(nonce),
        )),
    )
    .unwrap();
    let mut lifecycle = bound.0.lifecycle.subscribe();
    let wait = tokio::time::timeout(
        ENROLLMENT_LIFECYCLE_WAIT_TIMEOUT,
        lifecycle.wait_for(Option::is_some),
    );
    tokio::pin!(wait);

    // Must still be pending well past the old 30 s bound (e.g. at 90 s of a cold boot).
    tokio::time::advance(std::time::Duration::from_secs(90)).await;
    tokio::task::yield_now().await;
    assert!(
        tokio::select! {
            biased;
            _ = &mut wait => false,
            _ = std::future::ready(()) => true,
        },
        "lifecycle wait must not time out at 90 s"
    );

    // Must expire cleanly once the full failure bound elapses without lifecycle readiness.
    tokio::time::advance(ENROLLMENT_LIFECYCLE_WAIT_TIMEOUT).await;
    assert!(wait.await.is_err());
}
