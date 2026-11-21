use crate::agent::provider::{LaunchProcess, LaunchSessionEvidence};
use crate::multiplexer::state::MultiplexerState;
use crate::service::boot_stage::BootStage;
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::orchestrator::launch_test_support::FakeRegisteredLaunch;
use crate::service::{
    provider_accounts, provider_hooks, test_fixture::ServiceFixture, AgentLaunchOrigin,
};
use codeterm_types::config::provider_accounts::ProviderAccountConfig;
use serde_json::{json, Value};
use std::fs;
use std::time::{Duration, Instant};

#[test]
fn named_claude_without_hooks_binds_the_shell_child_on_input_attach() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.pty_id = Some("named-pty".into());
    tab.session_binding_nonce = Some("named-nonce".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    tab.restorable_launch.account_id = Some("second".into());
    tab.restorable_launch.account_namespace = Some("second-home".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let root = fixture.temp.path();
    let usual = root.join("usual");
    let home = root.join("data/provider-accounts/claude/second-home");
    fs::create_dir(&usual).unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set("CLAUDE_CONFIG_DIR", &usual);
    provider_accounts::test_support::configure(svc, root.join("data"), root.join("config.toml"))
        .unwrap();
    svc.agent_registry.register_provider(Box::new(
        codeterm_plugin::agent_claude::ClaudeProvider::new(),
    ));
    {
        let mut config = svc.ext::<crate::config::ConfigState>().lock();
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
    let invocation = provider_accounts::for_tab(svc, &tab_id).unwrap().unwrap();
    let settings = r#"{"hooks":{}}"#;
    fs::write(home.join("settings.json"), settings).unwrap();
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (release, blocked) = std::sync::mpsc::sync_channel(1);
    let mut receipt = provider_accounts::test_support::work_receipt(svc);
    let job = provider_accounts::provisioning::start(svc, move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(10)).map_err(|_| "fixture provisioning was not released".to_owned())?;
        Ok(None)
    }).unwrap();
    observed.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(svc.prepare_launch_session_hook(&tab_id, "named-nonce", &invocation).unwrap_err().starts_with("account_provisioning_busy:"));
    assert_eq!(fs::read_to_string(home.join("settings.json")).unwrap(), settings);
    drop(job);
    assert_eq!(provider_accounts::provisioning::retained_count(svc), 1);
    release.send(()).unwrap();
    crate::async_runtime::block_on(provider_accounts::test_support::settled(&mut receipt)).unwrap();
    svc.prepare_launch_session_hook(&tab_id, "named-nonce", &invocation).unwrap();

    let start = chrono::DateTime::parse_from_rfc3339("2026-11-18T14:01:51.000789Z")
        .unwrap()
        .timestamp_micros();
    let start_token = start.to_string();
    let recorded = if cfg!(target_os = "macos") {
        "Wed Nov 18 14:01:51 2026"
    } else {
        &start_token
    };
    let session = "88000e56-6afd-4363-8344-07b48fd61b61";
    let registry = home.join("sessions");
    assert!(!fs::symlink_metadata(&registry).unwrap().is_symlink());
    let file = fs::File::create(registry.join("75610.json")).unwrap();
    serde_json::to_writer(
        &file,
        &json!({
            "pid": 75610, "sessionId": session, "procStart": recorded, "kind": "interactive",
        }),
    )
    .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + Duration::from_millis(start as u64 / 1000 + 2089))
        .unwrap();
    svc.ext_or_init(|| super::super::probe::FixtureProcessTree {
        pty_id: "named-pty".into(),
        processes: vec![
            LaunchProcess {
                pid: 74609,
                depth: 0,
                start_token: (start - 1_000_000).to_string(),
            },
            LaunchProcess {
                pid: 75610,
                depth: 1,
                start_token,
            },
        ],
    });
    let attempt = LaunchAttempt::current(&svc.mux, &tab_id, "named-pty").unwrap();
    svc.emit_boot_stage(&tab_id, BootStage::AgentReady, None);
    assert!(!svc.hookless_launch_has_input(&attempt));
    assert_eq!(
        svc.mux.read().get_tab(&tab_id).unwrap().agent_session_id,
        None
    );
    svc.screen_state.process("named-pty", b"\x1b[?2004h");
    assert!(crate::service::boot_wait::wait_until(
        Instant::now() + Duration::from_secs(5),
        || { svc.mux.read().get_tab(&tab_id).unwrap().boot_stage == Some(BootStage::AgentReady) }
    ));
    let mux = svc.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some(session));
    assert!(tab.launch_session_authenticated());
    assert_eq!(
        fs::read_to_string(home.join("settings.json")).unwrap(),
        settings
    );
}

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
    let typical_registry = usual.join("sessions");
    fs::create_dir(&typical_registry).unwrap();
    fs::remove_dir(home.join("sessions")).unwrap();
    codeterm_util::fs_links::directory_link(&typical_registry, &home.join("sessions")).unwrap();

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
    let invocation = provider_accounts::for_tab(svc, &tab_id).unwrap().unwrap();
    assert_eq!(read_json("settings.json"), named_settings);
    let launch = codeterm_plugin::provider_account::with_context(Some(invocation.clone()), || {
        FakeRegisteredLaunch::register(svc, &tab_id, "own-pty", AgentLaunchOrigin::RestoredSnapshot)
    })
    .unwrap();
    let nonce = launch.attempt().binding_nonce;
    svc.mux.write().get_tab_mut(&tab_id).unwrap().boot_stage = Some(BootStage::SpawnFailed);
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

    let mut disabled = settings.clone();
    disabled["disableAllHooks"] = json!(true);
    let disabled_bytes = serde_json::to_vec_pretty(&disabled).unwrap();
    let settings_path = home.join("settings.json");
    fs::write(&settings_path, &disabled_bytes).unwrap();
    assert_eq!(
        provider_accounts::prepare_named_session_hook(svc, &invocation).unwrap(),
        Some(false)
    );
    assert_eq!(fs::read(&settings_path).unwrap(), disabled_bytes);

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
    fs::write(
        typical_registry.join("2860.json"),
        record("foreign-default-session", "1000"),
    )
    .unwrap();
    let named_registry = home.join("sessions/2860.json");
    let probe = || {
        svc.detect_launch_from_evidence(
            &tab_id,
            "claude".into(),
            "own-pty".into(),
            nonce.clone(),
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
    assert_ne!(
        fs::canonicalize(home.join("sessions")).unwrap(),
        fs::canonicalize(&typical_registry).unwrap(),
    );
    assert!(
        probe().is_none(),
        "the default home's valid PID record carries no named-account authority"
    );
    fs::write(&named_registry, record(session, "900")).unwrap();
    assert!(probe().is_none());
    fs::write(&named_registry, record(session, "1000")).unwrap();
    let detected = probe().expect("only the named home's matching live process proves a launch");
    assert_eq!(detected.detection.session_id, session);
    drop(env);
    svc.screen_state.process("other-pty", b"\x1b[?2004h");
    svc.commit_detected_launch_session(&tab_id, detected).await;
    {
        let mux = svc.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some(session));
        assert!(tab.launch_session_authenticated());
        assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
    }
    svc.screen_state.process("own-pty", b"\x1b[?2004h");
    assert!(
        crate::service::boot_wait::wait_until(Instant::now() + Duration::from_secs(10), || {
            svc.mux.read().get_tab(&tab_id).unwrap().boot_stage == Some(BootStage::AgentReady)
        }),
        "authenticated registry binding and the same PTY's input attachment recover readiness"
    );

    let retired = home.with_file_name("retired-namespace");
    codeterm_util::private_file::create_dir_owner_only(&retired).unwrap();
    fs::write(retired.join("settings.json"), settings.to_string()).unwrap();
    provider_hooks::uninstall_integrations(svc, "claude", || {
        assert_eq!(
            provider_hooks::integration_consent(svc, "claude"),
            Some(false)
        );
        Ok(())
    })
    .unwrap();
    let saved_config = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(saved_config.contains("integration_consent_granted = false"));
    for path in [home.join("settings.json"), retired.join("settings.json")] {
        let cleaned: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(cleaned["theme"], "dark");
        assert_eq!(
            cleaned["hooks"]["SessionStart"],
            json!([{"matcher":"","hooks":[
                {"type":"command","command":"foreign-command","_source":"codeterm-hook-v3"}
            ]}])
        );
        assert!(!cleaned.to_string().contains("X-CodeTerm-Hook"));
    }
    let cleaned = fs::read(home.join("settings.json")).unwrap();
    assert_eq!(read_json("settings.json")["disableAllHooks"], true);
    assert_eq!(
        provider_accounts::prepare_named_session_hook(svc, &invocation).unwrap(),
        Some(false)
    );
    assert_eq!(fs::read(home.join("settings.json")).unwrap(), cleaned);
    assert_eq!(
        fs::read_to_string(typical_registry.join("2860.json")).unwrap(),
        record("foreign-default-session", "1000")
    );
}
