// WHAT: host credential disclosure through CLI token and web-link consumers.
// WHY: tab callers must not upgrade their launch credential to owner authority.
// ORACLE: tab and mesh requests refuse before opening even an unreadable store.
use super::*;

#[test]
fn tab_and_mesh_callers_cannot_read_the_host_store() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".secrets"), "unreadable fixture").unwrap();
    assert!(read(root.path(), false, Some("tab"))
        .unwrap_err()
        .contains("host-only"));
    assert!(read(root.path(), true, None)
        .unwrap_err()
        .contains("local-only"));
}

#[test]
fn owner_can_read_the_local_instance_credential() {
    let root = tempfile::tempdir().unwrap();
    codeterm_util::secret_file::SecretFile::new_at(root.path(), "domios.instance")
        .set("api_token", "owner-fixture")
        .unwrap();
    assert_eq!(
        read(root.path(), false, None).unwrap().as_deref(),
        Some("owner-fixture")
    );
}
