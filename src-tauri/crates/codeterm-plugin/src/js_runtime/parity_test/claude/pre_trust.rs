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

#[test]
fn named_account_pre_trust_native_js_parity_at_domios_root_and_child() {
    use crate::provider_account::{with_context, InvocationContext};
    use std::collections::BTreeMap;

    let (native, js) = pair();
    let capability = js.account_capability().unwrap();
    let provider_id = native.id().to_string();
    let record_file = capability
        .isolated_paths
        .iter()
        .find(|path| {
            path.starts_with('.') && path.ends_with(".json") && path.contains(&provider_id)
        })
        .expect("provider trust record path")
        .clone();

    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".domios");
    let child = domios_root.join("general");
    let accounts_root = domios_root.join("provider-accounts");
    let account_home = accounts_root.join(&provider_id).join("work");
    let usual = temp.path().join("usual-home");
    for dir in [&domios_root, &child, &account_home, &usual] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let record = account_home.join(&record_file);
    let context = InvocationContext::new(
        provider_id,
        "work".into(),
        BTreeMap::from([(
            capability.home_environment,
            Some(account_home.to_string_lossy().into_owned()),
        )]),
        vec![account_home.clone()],
        vec![usual, accounts_root.clone()],
    )
    .unwrap();

    with_context(Some(context), || {
        // Protected-root ancestor reads remain denied for directory listing/reading.
        assert!(!crate::provider_account::read_allowed(&domios_root));
        assert!(!crate::provider_account::read_allowed(&accounts_root));

        for dir in [&domios_root, &child] {
            let path = dir.to_str().unwrap();
            std::fs::write(&record, r#"{"hasCompletedOnboarding":true}"#).unwrap();
            let native_outcome = native.pre_trust_directory(path).unwrap();
            let native_bytes = std::fs::read(&record).unwrap();

            std::fs::write(&record, r#"{"hasCompletedOnboarding":true}"#).unwrap();
            let js_outcome = js.pre_trust_directory(path).unwrap();
            let js_bytes = std::fs::read(&record).unwrap();

            assert_eq!(native_outcome, PreTrustOutcome::Granted);
            assert_eq!(js_outcome, PreTrustOutcome::Granted);
            assert_eq!(native_bytes, js_bytes);
            assert_eq!(native.is_directory_trusted(path), Ok(true));
            assert_eq!(js.is_directory_trusted(path), Ok(true));
        }
    });
}
