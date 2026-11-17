use super::*;
use codeterm_plugin::provider_account::{self, storage::AccountContinuation};

fn capability() -> AccountCapability {
    AccountCapability {
        home_environment: "ACCOUNT_STORAGE_TEST_HOME".into(),
        default_home: "~/.fixture".into(),
        secret_environment: vec!["ACCOUNT_STORAGE_TEST_TOKEN".into()],
        excluded_environment: vec!["ACCOUNT_STORAGE_TEST_SESSION".into()],
        isolated_paths: vec!["auth.json".into(), "config.toml".into()],
        shared_paths: vec![SharedStoragePath {
            path: "state_".into(),
            kind: StoragePathKind::VersionedSqlite,
            root_environment: Some("ACCOUNT_STORAGE_TEST_SQLITE".into()),
            configuration: Some(StorageConfiguration {
                file: "config.toml".into(),
                format: StorageConfigurationFormat::Toml,
                key: "sqlite_home".into(),
            }),
        }],
        shared_index_paths: vec![],
        continuation: AccountContinuation::Unverified,
    }
}

#[test]
fn selected_home_pins_existing_sqlite_root_without_copying_credentials_or_history() {
    let data = tempfile::tempdir().unwrap();
    let usual = tempfile::tempdir().unwrap();
    let shared = tempfile::tempdir().unwrap();
    let original = format!(
        "sqlite_home = {:?}\nmodel = 'opaque-default'\n",
        shared.path().to_string_lossy()
    );
    std::fs::write(usual.path().join("config.toml"), &original).unwrap();
    std::fs::write(usual.path().join("auth.json"), "opaque-usual-credential").unwrap();
    std::fs::write(shared.path().join("state_12.sqlite"), "opaque-shared-index").unwrap();
    let account = provision(
        data.path(),
        "fixture",
        "account-b",
        &capability(),
        usual.path(),
    )
    .unwrap();
    let home = data.path().join("provider-accounts/fixture/account-b");
    assert!(!home.join("auth.json").exists());
    assert!(!home.join("state_12.sqlite").exists());
    assert_eq!(
        configuration_value(
            &home,
            capability().shared_paths[0].configuration.as_ref().unwrap()
        )
        .unwrap(),
        Some(shared.path().to_string_lossy().into_owned())
    );
    provider_account::with_context(Some(account.clone()), || {
        assert_eq!(
            provider_account::environment("ACCOUNT_STORAGE_TEST_HOME"),
            Some(home.clone().into_os_string())
        );
        assert_eq!(
            provider_account::environment("ACCOUNT_STORAGE_TEST_SQLITE"),
            Some(shared.path().as_os_str().to_owned())
        );
        assert!(provider_account::environment_is_selected(
            "ACCOUNT_STORAGE_TEST_TOKEN"
        ));
        assert!(provider_account::environment("ACCOUNT_STORAGE_TEST_TOKEN").is_none());
        assert!(provider_account::environment_is_selected(
            "ACCOUNT_STORAGE_TEST_SESSION"
        ));
    });
    let restored = provision(
        data.path(),
        "fixture",
        "account-b",
        &capability(),
        usual.path(),
    )
    .unwrap();
    assert_eq!(account.cache_key(), restored.cache_key());
    assert_eq!(
        std::fs::read_to_string(usual.path().join("config.toml")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read_to_string(usual.path().join("auth.json")).unwrap(),
        "opaque-usual-credential"
    );
    assert_eq!(
        std::fs::read_to_string(shared.path().join("state_12.sqlite")).unwrap(),
        "opaque-shared-index"
    );
}

#[test]
fn provisioning_rejects_host_environment_authority_and_foreign_history() {
    let data = tempfile::tempdir().unwrap();
    let usual = tempfile::tempdir().unwrap();
    for name in [
        "HOME",
        "USERPROFILE",
        "CODETERM_API_TOKEN",
        "codeterm_tab_id",
        "PATH",
    ] {
        let mut capability = capability();
        capability.home_environment = name.into();
        assert!(provision(
            data.path(),
            "fixture",
            "account-b",
            &capability,
            usual.path()
        )
        .is_err());
    }
    let target = usual.path().join("history.jsonl");
    let foreign = data.path().join("foreign-history.jsonl");
    std::fs::write(&target, "own-session").unwrap();
    std::fs::write(&foreign, "foreign-session").unwrap();
    assert!(link_shared(&target, &foreign, false).is_err());
    assert_eq!(
        std::fs::read_to_string(&foreign).unwrap(),
        "foreign-session"
    );
    let mut shared_secret = capability();
    shared_secret.shared_paths[0] = SharedStoragePath {
        path: "auth.json".into(),
        kind: StoragePathKind::File,
        root_environment: None,
        configuration: None,
    };
    assert!(provision(
        data.path(),
        "fixture",
        "account-b",
        &shared_secret,
        usual.path()
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn isolated_credential_symlink_cannot_escape_to_the_usual_account() {
    let data = tempfile::tempdir().unwrap();
    let usual = tempfile::tempdir().unwrap();
    let home = data.path().join("provider-accounts/fixture/account-b");
    std::fs::create_dir_all(&home).unwrap();
    use std::os::unix::fs::PermissionsExt;
    for path in [
        home.clone(),
        home.parent().unwrap().into(),
        home.parent().unwrap().parent().unwrap().into(),
    ] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(usual.path().join("auth.json"), "opaque-usual").unwrap();
    std::os::unix::fs::symlink(usual.path().join("auth.json"), home.join("auth.json")).unwrap();
    assert!(provision(
        data.path(),
        "fixture",
        "account-b",
        &capability(),
        usual.path()
    )
    .is_err());
}
