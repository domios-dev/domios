use super::*;

#[test]
fn instance_token_command_uses_local_store_without_daemon_or_environment() {
    let dir = tempfile::tempdir().unwrap();
    codeterm_util::instance_token::initialize_at(dir.path(), Some("local-authority"), false)
        .unwrap();
    assert_eq!(
        instance_api_token(dir.path(), false).unwrap(),
        "local-authority"
    );
}

#[test]
fn instance_token_command_refuses_remote_before_accessing_store() {
    let dir = tempfile::tempdir().unwrap();
    assert!(instance_api_token(&dir.path().join("absent"), true)
        .unwrap_err()
        .contains("local-only"));
    assert!(!dir.path().join("absent").exists());
}

#[test]
fn instance_token_command_refuses_missing_authority() {
    let dir = tempfile::tempdir().unwrap();
    assert!(instance_api_token(dir.path(), false)
        .unwrap_err()
        .contains("No local instance API credential"));
}
