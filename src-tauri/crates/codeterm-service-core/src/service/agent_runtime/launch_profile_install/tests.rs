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
    let readiness_before = crate::provider::profile_readiness_revision(&root);
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
    assert!(crate::provider::profile_readiness_revision(&root) > readiness_before);
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

#[test]
fn binary_receipt_rechecks_executable_and_ignores_host_icon_projection() {
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("agent");
    std::fs::write(&binary, b"fixture executable").unwrap();
    let mut profile: RuntimeLaunchProfile = serde_json::from_value(json!({
        "binary":"agent","args":[],"ownerConfigured":true,
        "install":{"distribution":"binary","package":"fixture@1.2.3","url":"https://example.test/agent","sha256":"a".repeat(64),"executable":"agent"}
    })).unwrap();
    receipt(
        root.path(),
        "fixture",
        &profile,
        "fixture@1.2.3".into(),
        "1.2.3".into(),
        Some(format!("{:x}", Sha256::digest(b"fixture executable"))),
        AgentRuntimeCommand {
            binary: binary.to_string_lossy().into_owned(),
            args: vec![],
        },
    )
    .unwrap();
    assert!(read_receipt(root.path(), "fixture", &profile)
        .unwrap()
        .is_some());
    profile.icon_svg = Some("host projection".into());
    assert!(read_receipt(root.path(), "fixture", &profile)
        .unwrap()
        .is_some());
    std::fs::write(&binary, b"tampered executable").unwrap();
    assert!(read_receipt(root.path(), "fixture", &profile).is_err());
}
/// A registry install exactly as the owner profile store recorded it before declaration overlays.
const CLAUDE_ACP_DECLARATION: &str = r#"{"registryId":"claude-acp","iconUrl":"https://cdn.agentclientprotocol.com/registry/v1/latest/claude-acp.svg","binary":"npx","args":["@agentclientprotocol/claude-agent-acp@0.101.0"],"install":{"distribution":"npx","package":"@agentclientprotocol/claude-agent-acp@0.101.0"},"ownerConfigured":true}"#;
/// The declaration hash the build that installed it wrote into its receipt.
const CLAUDE_ACP_EARLIER_RECEIPT_SHA256: &str =
    "b6739036e49f035d0494a742bc2b215448c087a30e899d463a6c4903ab2c701b";

fn earlier_declaration() -> RuntimeLaunchProfile {
    serde_json::from_str(CLAUDE_ACP_DECLARATION).unwrap()
}

/// The profile a launch reads: the shipped ACP runtime with the owner profile overlaid.
fn resolved_at_launch(declaration: RuntimeLaunchProfile) -> RuntimeLaunchProfile {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../../plugins/provider-acp/plugin.json"
    ))
    .unwrap();
    let shipped = serde_json::from_value(manifest["capabilities"]["agentRuntime"].clone()).unwrap();
    let owner = BTreeMap::from([("claude-acp".to_string(), declaration)]);
    let mut resolved = codeterm_plugin::agent_runtime::owner_profiles::overlay_with(shipped, owner)
        .unwrap()
        .launch_profiles
        .unwrap()
        .remove("claude-acp")
        .unwrap();
    assert!(resolved.full_access_mode.is_some() && resolved.limits_source.is_some());
    resolved.icon_svg = Some("<svg/>".into());
    resolved
}

fn installed_command(dir: &Path) -> AgentRuntimeCommand {
    let node = dir.join("node");
    let script = dir.join("index.js");
    std::fs::write(&node, b"fixture runtime").unwrap();
    std::fs::write(&script, b"fixture script").unwrap();
    AgentRuntimeCommand {
        binary: node.to_string_lossy().into_owned(),
        args: vec![script.to_string_lossy().into_owned()],
    }
}

fn declared_later(profile: &mut RuntimeLaunchProfile) {
    profile.icon_url = Some("https://example.test/refreshed.svg".into());
    profile.client_capabilities = serde_json::from_value(json!({"terminal":false})).unwrap();
    profile.host_working_directory = true;
}

#[test]
fn an_install_from_an_earlier_declaration_starts_under_todays_resolved_declaration() {
    let app = tempfile::tempdir().unwrap();
    let root = app.path().join("profiles");
    let saved = receipt(
        &root,
        "claude-acp",
        &earlier_declaration(),
        "@agentclientprotocol/claude-agent-acp@0.101.0".into(),
        "0.101.0".into(),
        Some("a".repeat(64)),
        installed_command(app.path()),
    )
    .unwrap();
    let mut today = resolved_at_launch(earlier_declaration());
    declared_later(&mut today);
    assert_eq!(
        read_receipt(&root, "claude-acp", &today).unwrap(),
        Some(saved)
    );
    assert_eq!(detect(&root, "claude-acp", &today), InstallState::Installed);
    today.install.as_mut().unwrap().package =
        "@agentclientprotocol/claude-agent-acp@0.102.0".into();
    assert!(read_receipt(&root, "claude-acp", &today).unwrap().is_none());
}

#[test]
fn a_receipt_written_by_an_earlier_build_starts_without_reinstall_and_is_rebound() {
    let app = tempfile::tempdir().unwrap();
    let root = app.path().join("profiles");
    std::fs::create_dir_all(&root).unwrap();
    let command = installed_command(app.path());
    let earlier = InstallReceipt {
        profile_id: "claude-acp".into(),
        declaration_sha256: CLAUDE_ACP_EARLIER_RECEIPT_SHA256.into(),
        package: "@agentclientprotocol/claude-agent-acp@0.101.0".into(),
        version: "0.101.0".into(),
        sha256: Some("a".repeat(64)),
        command: command.clone(),
        timestamp_ms: 1,
    };
    let path = root.join("claude-acp.json");
    std::fs::write(&path, serde_json::to_vec(&earlier).unwrap()).unwrap();
    let mut today = resolved_at_launch(earlier_declaration());
    std::fs::remove_file(&command.args[0]).unwrap();
    assert!(read_receipt(&root, "claude-acp", &today).unwrap().is_none());
    assert_eq!(
        serde_json::from_slice::<InstallReceipt>(&std::fs::read(&path).unwrap()).unwrap(),
        earlier
    );
    std::fs::write(&command.args[0], b"fixture script").unwrap();
    let rebound = InstallReceipt {
        declaration_sha256: fingerprint(&today).unwrap(),
        ..earlier.clone()
    };
    assert_eq!(
        read_receipt(&root, "claude-acp", &today).unwrap(),
        Some(rebound.clone())
    );
    assert_eq!(
        serde_json::from_slice::<InstallReceipt>(&std::fs::read(&path).unwrap()).unwrap(),
        rebound
    );
    declared_later(&mut today);
    assert_eq!(
        read_receipt(&root, "claude-acp", &today).unwrap(),
        Some(rebound)
    );
    today.args.push("--other".into());
    assert!(read_receipt(&root, "claude-acp", &today).unwrap().is_none());
}
