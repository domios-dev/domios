use super::*;

#[test]
fn local_identity_needs_persisted_credentials_and_invalidation_reaches_clones() {
    let root = tempfile::tempdir().unwrap();
    let identity = Identity {
        uid: "opaque-owner".into(),
        email: None,
        display_name: None,
        provider: "fixture".into(),
        photo_url: None,
    };
    remember(root.path(), "fixture", Some(identity.clone())).unwrap();
    assert!(load(root.path()).is_none());
    let file = SecretFile::new_at(root.path(), "domios.account");
    let credential = super::super::credentials::credential_name("fixture");
    file.set(&credential, "opaque-refresh").unwrap();
    remember(root.path(), "fixture", Some(identity.clone())).unwrap();
    let cached = load(root.path()).unwrap();
    let clone = cached.clone();
    assert_eq!(cached.status().identity, Some(identity));
    let snapshot = cached.snapshot().unwrap();
    assert!(snapshot.checking);
    assert!(snapshot.access.is_none());
    assert!(!cached.gate().summary().enforcing);
    cached.invalidate();
    assert!(clone.snapshot().is_none());
    remember(root.path(), "fixture", None).unwrap();
    assert!(load(root.path()).is_none());
}

fn paid_account() -> (StartupAccount, Arc<AtomicU64>) {
    use codeterm_entitlements::{
        Activation, Binding, CompiledKey, Enforcement, EntitlementAuthority, Mode, PinnedAuthority,
    };
    static KEYS: &[CompiledKey] = &[CompiledKey {
        key_id: "e5-rfc8032-test-only",
        spki_der_base64: "MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=",
    }];
    let authority = PinnedAuthority::new(Activation {
        project: "demo-domios",
        mode: Mode::Test,
        policy_version: 1,
        policy_epoch: "fixture-online-v1",
        enforcement: Enforcement {
            enabled: false,
            rollback: false,
        },
        keys: KEYS,
    });
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../codeterm-entitlements/vectors/grant-vectors.json"
    ))
    .unwrap();
    let vector = vectors["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|vector| vector["id"] == "valid-paid")
        .unwrap();
    let raw = serde_json::from_value(vector["response"]["grant"].clone()).unwrap();
    let time = Arc::new(AtomicU64::new(1_700_000_060));
    let grant = authority
        .verify(
            &raw,
            &Binding {
                uid: "fixture-user",
                installation_id: "fixture-installation",
            },
            None,
            time.load(Ordering::SeqCst),
        )
        .unwrap()
        .grant;
    let clock = time.clone();
    (
        StartupAccount {
            backend: "fixture".into(),
            identity: Identity {
                uid: "fixture-user".into(),
                email: None,
                display_name: None,
                provider: "fixture".into(),
                photo_url: None,
            },
            grant: Some(grant),
            valid: Arc::new(AtomicBool::new(true)),
            observed: Arc::new(AtomicU64::new(0)),
            clock: Arc::new(move || clock.load(Ordering::SeqCst)),
        },
        time,
    )
}

#[test]
fn expired_paid_startup_defers_locks_and_never_reports_free() {
    let (account, time) = paid_account();
    time.store(
        account.grant.as_ref().unwrap().payload().expires_at,
        Ordering::SeqCst,
    );
    assert!(!account.gate().summary().enforcing);
    let snapshot = account.snapshot().unwrap();
    assert_eq!(snapshot.entitlements.plan, Plan::Paid("unavailable".into()));
    assert!(snapshot.checking);
}

#[test]
fn paid_startup_gate_crossing_expiry_never_installs_free_locks() {
    let (account, time) = paid_account();
    let held_gate = account.gate();
    assert_eq!(held_gate.summary().workspaces, None);
    time.store(
        account.grant.as_ref().unwrap().payload().expires_at,
        Ordering::SeqCst,
    );
    assert_eq!(held_gate.summary().workspaces, None);
    assert!(!account.gate().summary().enforcing);
    assert_ne!(account.snapshot().unwrap().entitlements.plan, Plan::Free);
}
