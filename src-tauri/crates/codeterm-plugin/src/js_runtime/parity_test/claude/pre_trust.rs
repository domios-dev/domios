use super::*;
use crate::provider::PreTrustOutcome;

#[test]
fn pre_trust_and_trusted_read_native_js_parity_with_isolated_record() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    let temp = tempfile::tempdir().unwrap();
    env.set("CLAUDE_CONFIG_DIR", temp.path());
    let cwd = temp.path().join("project");
    std::fs::create_dir(&cwd).unwrap();
    let key = crate::agent_claude::provider::pre_trust::project_key(&cwd).unwrap();
    let record = temp.path().join(".claude.json");
    let (native, js) = pair();
    let path = cwd.to_str().unwrap();
    for source in [
        "{}".to_string(),
        serde_json::json!({"other": [1, 2], "projects": {&key: {"hasTrustDialogAccepted": false, "history": []}}}).to_string(),
        serde_json::json!({"projects": {&key: {"hasTrustDialogAccepted": true}}}).to_string(),
        "invalid JSON".to_string(),
    ] {
        std::fs::write(&record, &source).unwrap();
        let native_read = native.is_directory_trusted(path);
        let native_outcome = native.pre_trust_directory(path).unwrap();
        let native_bytes = std::fs::read(&record).unwrap();
        std::fs::write(&record, &source).unwrap();
        let js_read = js.is_directory_trusted(path);
        assert_eq!(native_read, js_read);
        assert_eq!(native_outcome, js.pre_trust_directory(path).unwrap());
        assert_eq!(native_bytes, std::fs::read(&record).unwrap());
        if matches!(native_outcome, PreTrustOutcome::Granted | PreTrustOutcome::AlreadyTrusted) {
            assert_eq!(native.is_directory_trusted(path), Ok(true));
            assert_eq!(js.is_directory_trusted(path), Ok(true));
            assert_eq!(js.pre_trust_directory(path).unwrap(), PreTrustOutcome::AlreadyTrusted);
        }
    }
    std::fs::remove_file(&record).unwrap();
    assert_eq!(
        native.pre_trust_directory(path).unwrap(),
        js.pre_trust_directory(path).unwrap()
    );
    assert!(!record.exists());
    env.set("CLAUDE_CONFIG_DIR", "relative-account-root");
    assert_eq!(
        native.pre_trust_directory(path).unwrap(),
        js.pre_trust_directory(path).unwrap()
    );
    assert_eq!(
        native.is_directory_trusted(path),
        js.is_directory_trusted(path)
    );
}
