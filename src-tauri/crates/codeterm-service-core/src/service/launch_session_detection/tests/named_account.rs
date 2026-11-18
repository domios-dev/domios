use super::*;
use crate::agent::provider::LaunchProcess;
use crate::multiplexer::state::MultiplexerState;
use crate::service::boot_stage::BootStage;
use crate::service::{provider_accounts, test_fixture::ServiceFixture};
use codeterm_types::config::provider_accounts::ProviderAccountConfig;
use serde_json::{json, Value};
use std::fs;

#[tokio::test]
async fn named_claude_home_skips_onboarding_installs_consented_hooks_and_binds_exact_launch() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.pty_id = Some("own-pty".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.boot_stage = Some(BootStage::SpawnFailed);
    tab.restorable_launch.account_id = Some("second".into());
    tab.restorable_launch.account_namespace = Some("second-home".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let root = fixture.temp.path();
    let usual = root.join("usual");
    let override_registry = root.join("override");
    let home = root.join("data/provider-accounts/claude/second-home");
    fs::create_dir(&usual).unwrap();
    fs::create_dir(&override_registry).unwrap();
    let default_record = r#"{"projects":{"foreign":{}},"mcpServers":{"foreign":{}},"oauthAccount":{"secret":"opaque"}}"#;
    fs::write(usual.join(".claude.json"), default_record).unwrap();
    fs::write(usual.join(".credentials.json"), "opaque-credentials").unwrap();
    fs::write(
        usual.join("settings.json"),
        r#"{"env":{"DEFAULT_ONLY":"opaque"}}"#,
    )
    .unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set("CLAUDE_CONFIG_DIR", &usual);
    env.set("CODETERM_CLAUDE_PROCESS_REGISTRY_DIR", &override_registry);
    provider_accounts::test_support::configure(svc, root.join("data"), root.join("config.toml"))
        .unwrap();
    svc.agent_registry.register_provider(Box::new(
        codeterm_plugin::agent_claude::ClaudeProvider::new(),
    ));
    let config = svc.ext::<crate::config::ConfigState>();
    {
        let mut config = config.lock();
        let accounts = config
            .agents
            .entry("claude".into())
            .or_default()
            .accounts
            .get_or_insert_with(Default::default);
        accounts.order.push("second".into());
        accounts.profiles.insert(
            "second".into(),
            ProviderAccountConfig {
                credential_namespace: Some("second-home".into()),
                ..Default::default()
            },
        );
    }
    let read_json =
        |name| serde_json::from_slice::<Value>(&fs::read(home.join(name)).unwrap()).unwrap();
    provider_accounts::for_tab(svc, &tab_id).unwrap().unwrap();
    assert_eq!(
        read_json(".claude.json"),
        json!({"hasCompletedOnboarding": true})
    );
    assert!(!home.join(".credentials.json").exists());
    assert!(!home.join("settings.json").exists());
    assert_eq!(
        provider_accounts::named_session_hook_for_tab(svc, &tab_id).unwrap(),
        Some(false)
    );

    let named_settings = json!({"theme": "dark", "hooks":{"SessionStart":[{"matcher":"","hooks":[
        {"type":"command","command":"foreign-command","_source":"codeterm-hook-v3"},
        {"type":"command","command":"stale --header \"X-CodeTerm-Hook: codeterm-hook-v3\""}
    ]}]}});
    codeterm_util::private_file::write_atomic(
        &home.join("settings.json"),
        &serde_json::to_vec(&named_settings).unwrap(),
    )
    .unwrap();
    fs::write(
        home.join(".claude.json"),
        r#"{"hasCompletedOnboarding":false,"theme":"light"}"#,
    )
    .unwrap();
    config
        .lock()
        .agents
        .get_mut("claude")
        .unwrap()
        .extra
        .insert(
            crate::config::schema::INTEGRATION_CONSENT_GRANTED.into(),
            toml::Value::Boolean(true),
        );
    assert_eq!(
        provider_accounts::named_session_hook_for_tab(svc, &tab_id).unwrap(),
        Some(true)
    );
    let settings = read_json("settings.json");
    assert_eq!(settings["theme"], "dark");
    assert!(settings.get("env").is_none());
    let session_start = settings["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(session_start.len(), 2);
    assert_eq!(session_start[0]["hooks"][0]["command"], "foreign-command");
    let command = session_start[1]["hooks"][0]["command"].as_str().unwrap();
    for needle in [
        "X-CodeTerm-Hook: codeterm-hook-v3",
        "$CODETERM_API_URL",
        "$CODETERM_API_TOKEN",
        "$CODETERM_TAB_ID",
        "$CODETERM_SESSION_BINDING_NONCE",
    ] {
        assert!(command.contains(needle));
    }
    assert_eq!(
        read_json(".claude.json"),
        json!({"hasCompletedOnboarding":true, "theme":"light"})
    );
    assert_eq!(
        fs::read_to_string(usual.join(".claude.json")).unwrap(),
        default_record
    );

    let session = "00000000-0000-4000-8000-000000000002";
    let record = |session_id, start| {
        json!({"pid":2860,"sessionId":session_id,"procStart":start,"kind":"interactive"})
            .to_string()
    };
    fs::write(
        override_registry.join("2860.json"),
        record("foreign-session", "1000"),
    )
    .unwrap();
    let named_registry = home.join("sessions/2860.json");
    let probe = || {
        svc.detect_launch_from_evidence(
            &tab_id,
            "claude".into(),
            "own-pty".into(),
            "own-nonce".into(),
            LaunchSessionEvidence {
                processes: vec![LaunchProcess {
                    pid: 2860,
                    depth: 1,
                    start_token: "1000".into(),
                }],
                ..Default::default()
            },
        )
    };
    fs::write(&named_registry, record(session, "900")).unwrap();
    assert!(probe().is_none());
    fs::write(&named_registry, record(session, "1000")).unwrap();
    let detected = probe().expect("only the named home's matching live process proves a launch");
    assert_eq!(detected.detection.session_id, session);
    drop(env);
    svc.commit_detected_launch_session(&tab_id, detected).await;
    let mux = svc.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some(session));
    assert!(tab.launch_session_authenticated());
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
}
