use super::*;

#[test]
fn startup_drops_orphan_teams_and_requests_durable_resync() {
    let (mux, _) = MultiplexerState::new();
    let mut snapshot = mux.to_session_snapshot();
    let workspace_id = snapshot.workspaces[0].id.clone();
    let valid_team = codeterm_types::team::Team::new(workspace_id, 1);
    let orphan_team = codeterm_types::team::Team::new("removed-workspace", 2);
    let orphan_id = orphan_team.id.clone();
    let valid_id = valid_team.id.clone();
    snapshot.teams = vec![orphan_team, valid_team];
    let mut boot_resync = false;

    reconcile_startup_teams(&mut snapshot, &mut boot_resync);
    let restored = MultiplexerState::from_session_snapshot(&snapshot);

    assert!(boot_resync);
    assert_eq!(
        snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec![valid_id.as_str()]
    );
    assert!(restored.get_team(&orphan_id).is_none());
    assert!(restored.get_team(&valid_id).is_some());
}

#[test]
fn storage_cutover_unavailable_startup_skips_default_workspace_and_rejects_persistence() {
    let storage = StorageAuthority::Unavailable {
        storage: None,
        reason: "test storage failure".into(),
    };
    let mut boot_resync = false;

    let state = initial_empty_desk_state(&storage, true, &mut boot_resync);

    assert!(state.list_workspaces().is_empty());
    assert!(!boot_resync);
    codeterm_service_core::persistence_test_support::
            assert_unavailable_rejects_persistence_without_a_commit();
}

#[test]
fn restore_contract_startup_flags_an_exited_member_and_journals_its_decision_once() {
    use codeterm_service_core::crash::restore_report::{build_restore_report, DropReason};
    use codeterm_service_core::session::lifecycle::{
        JournalRecord, LifecycleTransition, TabLifecycleJournal,
    };
    use codeterm_types::pty::AgentLiveness;

    let (mut state, _) = MultiplexerState::new();
    let workspace = state.create_workspace("startup-exited".into(), None).id;
    let tab_id = state.create_tab(&workspace).unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("exited-session".into());
    tab.session_binding_nonce = Some("exited-nonce".into());
    tab.role = Some(codeterm_service_core::multiplexer::types::AgentRole::Worker);
    assert!(codeterm_service_core::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
    let raw = serde_json::to_string(&state.to_session_snapshot()).unwrap();
    let journal_dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(journal_dir.path().to_path_buf());

    for boot in 1..=2 {
        let snapshot: codeterm_service_core::session::store::SessionSnapshot =
            serde_json::from_str(&raw).unwrap();
        let mut panes = snapshot_agent_panes(&snapshot);
        let restored = MultiplexerState::from_session_snapshot(&snapshot);
        codeterm_service_core::service::apply_restore_decisions(&journal, &restored, &mut panes);
        let liveness: std::collections::BTreeMap<_, _> = [(tab_id.clone(), AgentLiveness::Absent)]
            .into_iter()
            .collect();
        let report = build_restore_report(&panes, &restored_agent_pane_ids(&restored), &liveness);

        let pane = report
            .panes
            .iter()
            .find(|pane| pane.pane.tab_id == tab_id)
            .expect("the exited member is a bound pane");
        assert_eq!(
            pane.drop_reason,
            Some(DropReason::AgentExitedBeforeShutdown),
            "boot {boot}"
        );
        assert_eq!(
            pane.pane.exited_session_id.as_deref(),
            Some("exited-session"),
            "boot {boot}"
        );
        let decisions = journal
            .load()
            .records
            .iter()
            .filter(|record| record.tab_id() == tab_id)
            .filter(|record| {
                matches!(
                    record,
                    JournalRecord::Transition { entry } if matches!(
                        &entry.transition,
                        LifecycleTransition::RestoreAwaitingAgentRestart { session_id }
                            if session_id == "exited-session"
                    )
                )
            })
            .count();
        assert_eq!(decisions, 1, "boot {boot}");
    }
}

fn isolated_startup_env() -> codeterm_util::test_env::EnvGuard {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    for name in INHERITED_IDENTITY_ENV_VARS.iter().copied().chain([
        "CODETERM_API_URL",
        "CODETERM_API_PORT",
        "CODETERM_API_TOKEN",
        "CODETERM_SOCKET_PATH",
        "CODETERM_INSTANCE",
        "CODETERM_DATA_DIR",
        "CODETERM_DEV",
    ]) {
        env.remove(name);
    }
    env
}

/// Dev launched inside prod must scrub inherited API env before PTYs spawn.
#[test]
fn scrub_overwrites_inherited_prod_api_env_with_instance_values() {
    let mut env = isolated_startup_env();
    for name in INHERITED_IDENTITY_ENV_VARS {
        env.set(name, format!("inherited-{name}"));
    }
    env.set("CODETERM_API_URL", "http://127.0.0.1:7685");
    env.set("CODETERM_API_PORT", "7685");
    env.set("CODETERM_API_TOKEN", "prod-token-inherited");
    env.set("CODETERM_SOCKET_PATH", "prod-inherited.sock");

    let mut cfg = codeterm_service_core::config::schema::AppConfig::default();
    cfg.api.host = "0.0.0.0".to_string();
    cfg.api.port = 7686;
    cfg.api.token = Some("dev-token".to_string());
    let config_state: ConfigState = Arc::new(Mutex::new(cfg));

    scrub_inherited_api_env(&config_state);

    assert_eq!(
        std::env::var("CODETERM_API_URL").ok().as_deref(),
        Some("http://127.0.0.1:7686"),
        "inherited prod URL must be replaced by the running instance URL"
    );
    assert_eq!(
        std::env::var("CODETERM_API_PORT").ok().as_deref(),
        Some("7686")
    );
    assert_eq!(
        std::env::var("CODETERM_API_TOKEN").ok().as_deref(),
        Some("dev-token"),
        "inherited prod token must be replaced by the running instance token"
    );
    let expected_socket_path = codeterm_service_core::paths::socket_display_path();
    assert_eq!(
        std::env::var("CODETERM_SOCKET_PATH").ok().as_deref(),
        Some(expected_socket_path.as_str())
    );
    for name in INHERITED_IDENTITY_ENV_VARS {
        assert!(
            std::env::var(name).is_err(),
            "inherited identity variable {name} must be removed"
        );
    }
}

#[test]
fn scrub_preserves_instance_env_and_later_daemon_identity() {
    let mut env = isolated_startup_env();
    env.set("CODETERM_TAB_ID", "inherited-tab");
    env.set("CODETERM_INSTANCE", "dev-test");
    env.set("CODETERM_DATA_DIR", "/tmp/codeterm-dev-test");
    env.set("CODETERM_DEV", "1");

    let mut cfg = codeterm_service_core::config::schema::AppConfig::default();
    cfg.api.port = 7686;
    let config_state: ConfigState = Arc::new(Mutex::new(cfg));

    scrub_inherited_api_env(&config_state);

    assert!(std::env::var("CODETERM_TAB_ID").is_err());
    assert_eq!(
        std::env::var("CODETERM_INSTANCE").ok().as_deref(),
        Some("dev-test")
    );
    assert_eq!(
        std::env::var("CODETERM_DATA_DIR").ok().as_deref(),
        Some("/tmp/codeterm-dev-test")
    );
    assert_eq!(std::env::var("CODETERM_DEV").ok().as_deref(), Some("1"));

    env.set("CODETERM_TAB_ID", "daemon-owned-tab");
    assert_eq!(
        std::env::var("CODETERM_TAB_ID").ok().as_deref(),
        Some("daemon-owned-tab"),
        "identity assigned after the startup scrub must remain intact"
    );
}

/// A tokenless instance must not leave a stale inherited prod token in env —
/// otherwise a tab would present a foreign bearer that 401s everywhere.
#[test]
fn scrub_removes_inherited_token_when_instance_has_none() {
    let mut env = isolated_startup_env();
    env.set("CODETERM_API_TOKEN", "prod-token-inherited");

    let mut cfg = codeterm_service_core::config::schema::AppConfig::default();
    cfg.api.port = 7686;
    cfg.api.token = None;
    let config_state: ConfigState = Arc::new(Mutex::new(cfg));

    scrub_inherited_api_env(&config_state);

    assert!(
        std::env::var("CODETERM_API_TOKEN").is_err(),
        "tokenless instance must clear an inherited token, not keep prod's"
    );
}
