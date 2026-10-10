use super::*;

const INSTANCE: &str = "a35724e2-0000-4000-8000-000000000000";

#[test]
fn registration_requires_exact_identity_and_protocol() {
    let name = format!("{INSTANCE}.lock");
    assert_eq!(
        registered_instance(&name, &registration(INSTANCE)),
        Some(INSTANCE.into())
    );
    for contents in ["", INSTANCE, "codeterm-instance-lifetime-v2\n", "unknown"] {
        assert_eq!(registered_instance(&name, contents), None);
    }
    assert_eq!(
        registered_instance("../foreign.lock", &registration(INSTANCE)),
        None
    );
    assert_eq!(registered_instance(&name, &registration("other")), None);
}

#[test]
fn foreign_alive_is_kept_and_foreign_gone_is_locked_until_sweep_ends() {
    let root = tempfile::tempdir().unwrap();
    let alive = acquire_at(root.path(), INSTANCE).unwrap();
    assert!(lock_gone_at(root.path(), "owner").is_empty());
    drop(alive);
    let gone = lock_gone_at(root.path(), "owner");
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].instance_id, INSTANCE);
    assert!(acquire_at(root.path(), INSTANCE).is_err());
    drop(gone);
    assert!(acquire_at(root.path(), INSTANCE).is_ok());
}

#[test]
fn missing_and_unknown_registrations_never_prove_gone() {
    let root = tempfile::tempdir().unwrap();
    assert!(lock_gone_at(&root.path().join("missing"), "owner").is_empty());
    std::fs::write(root.path().join(format!("{INSTANCE}.lock")), "").unwrap();
    assert!(lock_gone_at(root.path(), "owner").is_empty());
    let lifetime = acquire_at(root.path(), INSTANCE).unwrap();
    drop(lifetime);
    assert!(lock_gone_at(root.path(), INSTANCE).is_empty());
}
