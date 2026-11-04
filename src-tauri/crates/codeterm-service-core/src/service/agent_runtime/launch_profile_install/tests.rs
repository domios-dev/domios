use super::*;
use serde_json::json;
fn profile() -> RuntimeLaunchProfile {
    serde_json::from_value(json!({"binary":"npx","args":["@deepseek-ai/dsh@0.2.1-rc.5","--profile","acp"],"install":{"package":"@deepseek-ai/dsh@0.2.1-rc.5"},"secretEnv":{"DEEPSEEK_API_KEY":"domios:provider:acp:deepseek-api-key"}})).unwrap()
}
#[test]
fn missing_secret_refuses_launch_with_sign_in_required() {
    assert_eq!(
        compose_secret_environment(&profile(), |_| None).unwrap_err(),
        super::super::model::RuntimeFailure::SignInRequired
    );
    for value in ["", " ", "invalid\0key"] {
        assert_eq!(
            compose_secret_environment(&profile(), |_| Some(value.into())).unwrap_err(),
            super::super::model::RuntimeFailure::SignInRequired
        );
    }
}
#[test]
fn secret_is_only_in_launch_environment_and_never_in_argv_receipt_or_config() {
    let profile = profile();
    let sentinel = "opaque-test-secret-never-persist";
    let env = compose_secret_environment(&profile, |key| {
        assert_eq!(key, "domios:provider:acp:deepseek-api-key");
        Some(sentinel.into())
    })
    .unwrap();
    assert_eq!(env["DEEPSEEK_API_KEY"], sentinel);
    let config = json!({"launchProfile":"deepseek-harness"});
    let receipt = InstallReceipt {
        profile_id: "deepseek-harness".into(),
        declaration_sha256: fingerprint(&profile).unwrap(),
        package: "@deepseek-ai/dsh@0.2.1-rc.5".into(),
        version: "0.2.1-rc.5".into(),
        sha256: None,
        command: AgentRuntimeCommand {
            binary: "/fake/node".into(),
            args: profile.args.clone(),
        },
        timestamp_ms: 1,
    };
    for public in [
        serde_json::to_string(&profile).unwrap(),
        serde_json::to_string(&receipt).unwrap(),
        config.to_string(),
        serde_json::to_string(&profile.args).unwrap(),
        super::super::model::public_provider_settings(&config).to_string(),
    ] {
        assert!(!public.contains(sentinel));
    }
}
#[test]
fn receipt_is_durable_outside_workspace_and_declaration_bound() {
    let app = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let binary = app.path().join("agent");
    std::fs::write(&binary, b"fixture").unwrap();
    let root = app.path().join("providers/fixture/profiles");
    let mut profile = profile();
    let script = app.path().join("agent.js");
    std::fs::write(&script, b"fixture script").unwrap();
    let saved = receipt(
        &root,
        "fixture",
        &profile,
        "opaque@1.2.3".into(),
        "1.2.3".into(),
        None,
        AgentRuntimeCommand {
            binary: binary.to_str().unwrap().into(),
            args: vec![script.to_str().unwrap().into(), "--acp".into()],
        },
    )
    .unwrap();
    assert_eq!(
        read_receipt(&root, "fixture", &profile).unwrap(),
        Some(saved)
    );
    assert_eq!(detect(&root, "fixture", &profile), InstallState::Installed);
    assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
    profile.args.push("changed".into());
    assert!(read_receipt(&root, "fixture", &profile).unwrap().is_none());
    assert_eq!(
        detect(&root, "fixture", &profile),
        InstallState::NotInstalled
    );
}
#[test]
fn bad_urls_checksums_and_oversize_archives_are_refused() {
    for url in [
        "http://registry.npmjs.org/agent.tgz",
        "file:///tmp/agent",
        "https://attacker.invalid/agent",
        "https://user@registry.npmjs.org/agent",
    ] {
        assert!(verify_download(url, b"fixture", None).is_err());
    }
    assert!(verify_download(
        "https://registry.npmjs.org/agent.tgz",
        b"fixture",
        Some(&"0".repeat(64))
    )
    .is_err());
    assert!(verify_download(
        "https://registry.npmjs.org/agent.tgz",
        &vec![0; MAX_ARCHIVE_BYTES + 1],
        None
    )
    .is_err());
    assert!(verify_download("https://registry.npmjs.org/agent.tgz", b"fixture", None).is_ok());
}
#[test]
fn exact_version_and_explicit_local_confirmation_are_required() {
    for spec in [
        "name",
        "name@latest",
        "name@^1.2.3",
        "@scope/name@*",
        "../name@1.2.3",
        "https://host@1.2.3",
    ] {
        assert!(exact_package(spec).is_err());
    }
    assert_eq!(
        exact_package("@deepseek-ai/dsh@0.2.1-rc.5").unwrap(),
        ("@deepseek-ai/dsh", "0.2.1-rc.5")
    );
    let root = tempfile::tempdir().unwrap();
    let local = serde_json::from_value(
        json!({"binary":"local-acp-command","args":[],"ownerConfigured":true}),
    )
    .unwrap();
    assert!(confirm_local(
        root.path(),
        "local",
        &local,
        AgentRuntimeCommand {
            binary: "sh".into(),
            args: vec![]
        },
        true
    )
    .is_err());
    assert!(confirm_local(
        root.path(),
        "local",
        &local,
        AgentRuntimeCommand {
            binary: "/fake/agent".into(),
            args: vec![]
        },
        false
    )
    .is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
#[tokio::test]
async fn unconfirmed_install_never_starts_download_or_package_manager() {
    let root = tempfile::tempdir().unwrap();
    assert!(install(root.path(), "fixture", &profile(), false)
        .await
        .is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
