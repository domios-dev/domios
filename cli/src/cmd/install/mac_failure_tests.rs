#[test]
fn missing_installer_returns_failure_before_the_caller_withdraws_resume() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-installer.app");

    let result = super::run_install_from_file(missing.to_str().unwrap(), false);

    assert!(result.is_err());
}
