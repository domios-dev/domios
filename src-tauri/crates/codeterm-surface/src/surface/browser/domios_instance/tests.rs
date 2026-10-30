use super::*;
#[test]
fn dev_instance_open_requires_explicit_nonproduction_selectors_and_reads_only_its_store() {
    assert_eq!(
        parse(
            &[
                "--domios-instance",
                "qa",
                "--api-port",
                "7790",
                "--ephemeral"
            ]
            .map(String::from)
        )
        .unwrap(),
        Target {
            name: "qa".into(),
            port: 7790,
            ephemeral: true
        }
    );
    assert!(parse(&["--domios-instance", "qa"].map(String::from)).is_err());
    assert!(parse(&["--domios-instance", "qa", "--api-port", "7685"].map(String::from)).is_err());
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing");
    assert!(read_token(&missing).is_err());
    assert!(!missing.exists());
    codeterm_util::secret_file::SecretFile::new_at(root.path(), "domios.instance").set("api_token", "opaque-local-token")
        .unwrap();
    assert_eq!(read_token(root.path()).unwrap(), "opaque-local-token");
}

#[cfg(unix)]
#[test]
fn dev_instance_credentials_refuse_linked_or_unprotected_stores() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    codeterm_util::secret_file::SecretFile::new_at(other.path(), "domios.instance").set("api_token", "opaque-other-token")
        .unwrap();
    std::os::unix::fs::symlink(other.path().join(".secrets"), root.path().join(".secrets"))
        .unwrap();
    assert!(read_token(root.path()).is_err());
    std::fs::remove_file(root.path().join(".secrets")).unwrap();
    std::fs::copy(other.path().join(".secrets"), root.path().join(".secrets")).unwrap();
    std::fs::set_permissions(
        root.path().join(".secrets"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(read_token(root.path()).is_err());
}
