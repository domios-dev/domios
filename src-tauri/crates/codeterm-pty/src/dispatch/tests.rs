use super::*;

#[test]
fn test_health_returns_ok() {
    let result = cmd_health().unwrap();
    let json = result.into_json();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["name"], "CodeTerm");
    assert!(json["version"].is_string());
    assert!(json["platform"].is_string());
}

#[test]
fn test_command_deserialize_health() {
    let json = r#"{"cmd": "health"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    assert!(matches!(cmd, Command::Health));
}

#[test]
fn test_command_deserialize_send_text() {
    let json = r#"{"cmd": "send_text", "tab_id": "abc", "text": "hello"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::SendText { tab_id, text } => {
            assert_eq!(tab_id, "abc");
            assert_eq!(text, "hello");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_command_deserialize_get_output() {
    let json = r#"{"cmd": "get_output", "tab_id": "abc", "strip_ansi": true}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::GetOutput {
            tab_id,
            strip_ansi,
            full,
            offset,
            max_bytes,
        } => {
            assert_eq!(tab_id, "abc");
            assert!(strip_ansi);
            assert!(!full); // default false
            assert!(offset.is_none());
            assert!(max_bytes.is_none());
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_command_deserialize_get_text() {
    let json = r#"{"cmd": "get_text", "tab_id": "abc", "formatted": true}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::GetText { tab_id, formatted } => {
            assert_eq!(tab_id, "abc");
            assert!(formatted);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_command_deserialize_tab_list() {
    let json = r#"{"cmd": "tab_list"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    assert!(matches!(cmd, Command::TabList));
}

#[test]
fn tab_inspect_defaults_to_present_state_only() {
    let cmd: Command =
        serde_json::from_str(r#"{"cmd":"tab_inspect","tab_ids":["tab-1"]}"#).unwrap();
    assert!(matches!(
        cmd,
        Command::TabInspect {
            lifecycle: false,
            ..
        }
    ));
}

#[test]
fn test_command_result_into_json() {
    let result = CommandResult::Json(serde_json::json!({"test": true}));
    let json = result.into_json();
    assert_eq!(json["test"], true);
}

/// A startup/lifecycle lock must affect only the tab snapshot request. Health
/// has no state dependency and must remain answerable while the tab path
/// reports its bounded busy condition.
struct StartupBlockedPort;

impl CommandPort for StartupBlockedPort {
    fn mark_user_activity(&self) {}

    fn mux_snapshot(&self) -> crate::multiplexer::state::MultiplexerSnapshot {
        panic!("the blocking snapshot path must not be used by this API seam")
    }

    fn send_text(&self, _: &str, _: &str) -> Result<(), String> {
        unreachable!()
    }

    fn read_tab_output(
        &self,
        _: &str,
        _: Option<usize>,
        _: Option<usize>,
        _: bool,
        _: bool,
    ) -> Result<serde_json::Value, String> {
        unreachable!()
    }

    fn read_tab_screen(&self, _: &str, _: bool) -> Result<serde_json::Value, String> {
        unreachable!()
    }

    fn close_tab_guarded(&self, _: Option<&str>, _: &str) -> Result<(), String> {
        unreachable!()
    }

    fn focus_tab(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }

    fn rename_tab(&self, _: &str, _: &str) -> Result<(), String> {
        unreachable!()
    }

    fn set_tab_description(&self, _: &str, _: &str) -> Result<(), String> {
        unreachable!()
    }

    fn create_workspace(
        &self,
        _: String,
        _: Option<String>,
        _: bool,
    ) -> crate::multiplexer::state::WorkspaceInfo {
        unreachable!()
    }

    fn create_provisioned_workspace(
        &self,
        _: String,
        _: bool,
    ) -> Result<crate::multiplexer::state::WorkspaceInfo, String> {
        unreachable!()
    }

    fn rename_workspace(&self, _: &str, _: &str) -> Result<(), String> {
        unreachable!()
    }

    fn close_workspace_guarded(&self, _: Option<&str>, _: &str, _: bool) -> Result<(), String> {
        unreachable!()
    }

    fn list_workspaces(&self) -> Vec<crate::multiplexer::state::WorkspaceInfo> {
        unreachable!()
    }

    fn list_recently_closed(&self, _: &str) -> Vec<crate::multiplexer::types::ClosedTab> {
        unreachable!()
    }

    fn reopen_closed_tab(&self, _: &str, _: Option<usize>) -> Result<String, String> {
        unreachable!()
    }

    fn reopen_closed_group(&self, _: &str) -> Result<(String, String), String> {
        unreachable!()
    }

    fn resize_tab(&self, _: &str, _: u16, _: u16) -> Result<(), String> {
        unreachable!()
    }

    fn kill_tab_pty(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }

    fn list_agent_providers(&self) -> Vec<serde_json::Value> {
        unreachable!()
    }
}

#[test]
fn health_and_tabs_do_not_share_a_blocking_startup_wait() {
    let port = StartupBlockedPort;
    let health = dispatch(&Command::Health, &port).unwrap().into_json();
    assert_eq!(health["status"], "ok");

    let tabs = dispatch_tab_summary_if_ready(
        &Command::TabSummary {
            workspace_id: None,
            caller_tab_id: None,
            focused_workspace: false,
            agents_only: false,
        },
        &port,
    )
    .unwrap_err();
    assert_eq!(tabs, TAB_SUMMARY_BUSY_ERROR);
}

// ── Command enum round-trip tests ───────────────────────────────

#[test]
fn tab_close_deserialize() {
    let cmd: Command = serde_json::from_str(r#"{"cmd": "tab_close", "tab_id": "abc"}"#).unwrap();
    assert!(matches!(cmd, Command::TabClose { .. }));
}

#[test]
fn tab_focus_deserialize() {
    let cmd: Command = serde_json::from_str(r#"{"cmd": "tab_focus", "tab_id": "abc"}"#).unwrap();
    assert!(matches!(cmd, Command::TabFocus { .. }));
}

#[test]
fn tab_rename_deserialize() {
    let json = r#"{"cmd": "tab_rename", "tab_id": "abc", "title": "My Tab"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::TabRename { tab_id, title } => {
            assert_eq!(tab_id, "abc");
            assert_eq!(title, "My Tab");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn get_output_deserialize_full_params() {
    let json = r#"{"cmd":"get_output","tab_id":"p","offset":100,"max_bytes":4096,"strip_ansi":true,"full":true}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::GetOutput {
            tab_id,
            offset,
            max_bytes,
            strip_ansi,
            full,
        } => {
            assert_eq!(tab_id, "p");
            assert_eq!(offset, Some(100));
            assert_eq!(max_bytes, Some(4096));
            assert!(strip_ansi);
            assert!(full);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn command_round_trips_through_json() {
    // Validates that every variant we migrated survives a JSON round trip with tag-based dispatch.
    let commands = vec![
        Command::Health,
        Command::TabList,
        Command::TabInspect {
            tab_ids: vec!["p".into()],
            lifecycle: true,
        },
        Command::SendText {
            tab_id: "p".into(),
            text: "hi".into(),
        },
        Command::GetOutput {
            tab_id: "p".into(),
            offset: None,
            max_bytes: None,
            strip_ansi: false,
            full: false,
        },
        Command::GetText {
            tab_id: "p".into(),
            formatted: false,
        },
        Command::TabClose {
            tab_id: "p".into(),
            caller_tab_id: None,
        },
        Command::TabFocus { tab_id: "p".into() },
        Command::TabRename {
            tab_id: "p".into(),
            title: "t".into(),
        },
        Command::ListAgentProviders,
        Command::WorkspaceCreate {
            name: "w".into(),
            cwd: None,
            activate: None,
            provision: true,
        },
        Command::WorkspaceRename {
            workspace_id: "w".into(),
            name: "n".into(),
        },
        Command::WorkspaceClose {
            workspace_id: "w".into(),
            caller_tab_id: None,
            force: false,
        },
        Command::WorkspaceList,
        Command::PtyResize {
            tab_id: "p".into(),
            cols: 80,
            rows: 24,
        },
        Command::PtyKill { tab_id: "p".into() },
    ];
    for cmd in commands {
        let json = serde_json::to_value(&cmd).unwrap();
        let roundtrip: Command = serde_json::from_value(json).unwrap();
        // Same variant tag after roundtrip.
        assert_eq!(
            std::mem::discriminant(&cmd),
            std::mem::discriminant(&roundtrip),
        );
    }
}

#[test]
fn workspace_create_deserialize() {
    let json = r#"{"cmd":"workspace_create","name":"proj","cwd":"/tmp"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::WorkspaceCreate {
            name,
            cwd,
            activate,
            provision,
        } => {
            assert_eq!(name, "proj");
            assert_eq!(cwd.as_deref(), Some("/tmp"));
            assert!(activate.is_none());
            assert!(!provision);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn workspace_create_deserialize_no_cwd() {
    let json = r#"{"cmd":"workspace_create","name":"proj"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::WorkspaceCreate {
            name,
            cwd,
            activate,
            provision,
        } => {
            assert_eq!(name, "proj");
            assert!(cwd.is_none());
            assert!(activate.is_none());
            assert!(!provision);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn workspace_create_deserialize_with_activate() {
    let json = r#"{"cmd":"workspace_create","name":"proj","activate":true}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::WorkspaceCreate { activate, .. } => assert_eq!(activate, Some(true)),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn workspace_create_deserialize_with_provision() {
    let json = r#"{"cmd":"workspace_create","name":"bench","provision":true}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::WorkspaceCreate { cwd, provision, .. } => {
            assert!(cwd.is_none());
            assert!(provision);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn workspace_provision_with_cwd_is_refused_before_any_workspace_is_created() {
    let error = dispatch(
        &Command::WorkspaceCreate {
            name: "bench".into(),
            cwd: Some("/home/user/project".into()),
            activate: None,
            provision: true,
        },
        &StartupBlockedPort,
    )
    .unwrap_err();
    assert_eq!(error, WORKSPACE_PROVISION_CWD_CONFLICT);
}

#[test]
fn workspace_create_wire_cannot_inject_provenance() {
    let json = r#"{"cmd":"workspace_create","name":"proj","cwd":"/home/user/project","provisioned_cwd":"/home/user/project"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    let echoed = serde_json::to_value(&cmd).unwrap();
    assert!(echoed.get("provisioned_cwd").is_none());
    assert!(matches!(
        cmd,
        Command::WorkspaceCreate {
            provision: false,
            ..
        }
    ));
}

#[test]
fn workspace_rename_deserialize() {
    let json = r#"{"cmd":"workspace_rename","workspace_id":"w1","name":"new"}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::WorkspaceRename { workspace_id, name } => {
            assert_eq!(workspace_id, "w1");
            assert_eq!(name, "new");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn workspace_close_deserialize() {
    let cmd: Command =
        serde_json::from_str(r#"{"cmd":"workspace_close","workspace_id":"w1"}"#).unwrap();
    assert!(matches!(cmd, Command::WorkspaceClose { .. }));
}

#[test]
fn workspace_list_deserialize() {
    let cmd: Command = serde_json::from_str(r#"{"cmd":"workspace_list"}"#).unwrap();
    assert!(matches!(cmd, Command::WorkspaceList));
}

#[test]
fn pty_resize_deserialize() {
    let json = r#"{"cmd":"pty_resize","tab_id":"p","cols":120,"rows":40}"#;
    let cmd: Command = serde_json::from_str(json).unwrap();
    match cmd {
        Command::PtyResize { tab_id, cols, rows } => {
            assert_eq!(tab_id, "p");
            assert_eq!(cols, 120);
            assert_eq!(rows, 40);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn pty_kill_deserialize() {
    let cmd: Command = serde_json::from_str(r#"{"cmd":"pty_kill","tab_id":"p"}"#).unwrap();
    assert!(matches!(cmd, Command::PtyKill { .. }));
}

#[test]
fn list_agent_providers_deserialize() {
    let cmd: Command = serde_json::from_str(r#"{"cmd": "list_agent_providers"}"#).unwrap();
    assert!(matches!(cmd, Command::ListAgentProviders));
}

// `cmd_tab_list` and `build_tab_list_json` enrich tabs with sidebar labels.
// Tests stay on the pure helper so both `Tab list` and `Tab find` keep full workspace context.

#[test]
fn tab_list_exposes_one_name_and_workspace_context() {
    use crate::multiplexer::state::MultiplexerState;
    let (mut state, initial_tab) = MultiplexerState::new();
    let snapshot = state.snapshot();
    let ws = snapshot.workspaces.first().expect("default ws").clone();
    let tab_id = state.find_tab_for_tab(&initial_tab).expect("default tab");
    state.rename_tab(&tab_id, "Dalton").unwrap();
    state
        .set_tab_description(&tab_id, Some("Tracks inventory and party filters".into()))
        .unwrap();
    let snapshot = state.snapshot();
    let tabs = build_tab_list_json(&snapshot);
    let tab = tabs
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(initial_tab.as_str()))
        .expect("default Tab present in list");
    assert_eq!(
        tab.get("title").and_then(|v| v.as_str()),
        Some("Dalton"),
        "the tab name is serialized in title",
    );
    assert!(
        tab.get("tab_title").is_none(),
        "duplicate name field remains"
    );
    assert_eq!(
        tab.get("description").and_then(|v| v.as_str()),
        Some("Tracks inventory and party filters"),
    );
    assert_eq!(
        tab.get("tab_id").and_then(|v| v.as_str()),
        Some(tab_id.as_str()),
    );
    assert_eq!(
        tab.get("workspace_id").and_then(|v| v.as_str()),
        Some(ws.id.as_str()),
    );
    assert!(
        tab.get("workspace_name").and_then(|v| v.as_str()).is_some(),
        "workspace_name must be attached so user can disambiguate same-titled tabs across workspaces",
    );
}

#[test]
fn tab_summary_is_workspace_scoped_and_excludes_full_payload_fields() {
    use crate::multiplexer::state::MultiplexerState;
    let (mut state, initial_tab) = MultiplexerState::new();
    let snapshot = state.snapshot();
    let workspace = snapshot.workspaces.first().expect("default workspace");
    let summaries = build_tab_summary_json(&snapshot, Some(&workspace.id), None, false, false);
    assert_eq!(summaries.len(), 1);
    let tab = summaries.first().expect("summary Tab");
    assert_eq!(tab["id"].as_str(), Some(initial_tab.as_str()));
    assert_eq!(tab["workspace_id"].as_str(), Some(workspace.id.as_str()));
    assert!(tab.get("agent_state").is_none());
    assert!(tab.get("model_fallback").is_none());
    for identity in [
        "agent_session_id",
        "provider_context_session_id",
        "session_binding_nonce",
        "reported_session_binding_nonce",
    ] {
        assert!(
            tab.get(identity).is_some(),
            "{identity} must survive into the summary",
        );
    }
    assert!(tab["reported_session_binding_nonce"].is_null());

    let record = state.get_tab_mut(&initial_tab).expect("default Tab");
    record.session_binding_nonce = Some("nonce-recorded".into());
    record.reported_session_binding_nonce = Some("nonce-reported".into());
    let summaries =
        build_tab_summary_json(&state.snapshot(), Some(&workspace.id), None, false, false);
    assert_eq!(summaries[0]["session_binding_nonce"], "nonce-recorded");
    assert_eq!(
        summaries[0]["reported_session_binding_nonce"],
        "nonce-reported"
    );
}

#[test]
fn tab_list_defaults_to_all_workspaces() {
    use crate::multiplexer::state::MultiplexerState;

    let (mut state, first_tab) = MultiplexerState::new();
    let second_workspace = state.create_workspace("second".into(), None);
    let second_tab = state.create_tab(&second_workspace.id).unwrap();

    let summaries = build_tab_summary_json(&state.snapshot(), None, Some(&first_tab), false, false);
    let ids = summaries
        .iter()
        .filter_map(|tab| tab["id"].as_str())
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(
        ids,
        [first_tab.as_str(), second_tab.as_str()]
            .into_iter()
            .collect()
    );
    assert!(summaries.iter().all(|tab| tab["scope"] == "all_workspaces"));
}

#[test]
fn tab_list_explicit_workspace_scope() {
    use crate::multiplexer::state::MultiplexerState;

    let (mut state, first_tab) = MultiplexerState::new();
    let first_workspace = state.snapshot().workspaces[0].id.clone();
    let second_workspace = state.create_workspace("second".into(), None);
    let second_tab = state.create_tab(&second_workspace.id).unwrap();

    let summaries = build_tab_summary_json(
        &state.snapshot(),
        Some(&first_workspace),
        None,
        false,
        false,
    );

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0]["id"], first_tab);
    assert_ne!(summaries[0]["id"], second_tab);
    assert_eq!(summaries[0]["scope"], "workspace");
    assert_eq!(summaries[0]["scope_workspace_id"], first_workspace);
    assert_eq!(summaries[0]["other_workspace_tab_count"], 1);
}

#[test]
fn tab_list_agents_filter_remains_global() {
    use crate::multiplexer::state::MultiplexerState;

    let (mut state, first_tab) = MultiplexerState::new();
    let second_workspace = state.create_workspace("second".into(), None);
    let second_tab = state.create_tab(&second_workspace.id).unwrap();
    state.get_tab_mut(&second_tab).unwrap().agent_type = Some("claude".into());

    let summaries = build_tab_summary_json(&state.snapshot(), None, None, false, true);

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0]["id"], second_tab);
    assert_ne!(summaries[0]["id"], first_tab);
    assert_eq!(summaries[0]["scope"], "all_workspaces");
}

#[test]
fn tab_summary_keeps_team_terminal_provenance() {
    use crate::multiplexer::state::MultiplexerState;

    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).expect("default Tab");
    tab.team_id = Some("team_probe".into());
    tab.spawned_by_tab_id = Some("creator_probe".into());

    let snapshot = state.snapshot();
    let workspace = snapshot.workspaces.first().expect("default workspace");
    let summaries = build_tab_summary_json(&snapshot, Some(&workspace.id), None, false, false);
    let summary = summaries.first().expect("summary Tab");

    assert_eq!(summary["team_id"], "team_probe");
    assert_eq!(summary["spawned_by_tab_id"], "creator_probe");
}

#[test]
fn tab_summary_exposes_task_binding_and_roleless_flag() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::TaskBinding;

    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).expect("default Tab");
    tab.task_binding = Some(TaskBinding {
        plan_id: Some("pl_1".into()),
        task_id: Some("task_9".into()),
        attempt_id: None,
        spawn_handle: Some("M3 IV-gate".into()),
        bound_at: 5,
    });
    tab.roleless_tracked = true;

    let snapshot = state.snapshot();
    let workspace = snapshot.workspaces.first().expect("default workspace");
    let summaries = build_tab_summary_json(&snapshot, Some(&workspace.id), None, false, false);
    let summary = summaries.first().expect("summary Tab");

    assert_eq!(summary["task_binding"]["task_id"], "task_9");
    assert_eq!(summary["task_binding"]["plan_id"], "pl_1");
    assert_eq!(summary["roleless_tracked"], true);
}

#[test]
fn tab_list_default_scope_keeps_requesting_tab_visible_during_focus_churn() {
    use crate::multiplexer::state::MultiplexerState;

    let (mut state, requesting_tab) = MultiplexerState::new();
    let other_workspace = state.create_workspace("other".into(), None);
    let focused_tab = state.create_tab(&other_workspace.id).unwrap();
    state.set_focused_tab(&focused_tab).unwrap();

    let summaries =
        build_tab_summary_json(&state.snapshot(), None, Some(&requesting_tab), true, false);

    assert!(
        summaries
            .iter()
            .any(|tab| tab["id"].as_str() == Some(requesting_tab.as_str())),
        "requesting live Tab {requesting_tab} was omitted after focus moved to {focused_tab}"
    );
}

#[test]
fn pending_delivery_survives_persistence_and_compact_projection() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryRecord};
    use crate::session::store::SessionStore;

    let (mut state, tab_id) = MultiplexerState::new();
    let mut pending = PendingDeliveryRecord::new(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        PendingDeliveryChannel::InteractiveInput,
    );
    pending.mark_unconfirmed();
    state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(pending.clone());

    let snapshot = state.to_session_snapshot();
    let temp = tempfile::tempdir().unwrap();
    let store = SessionStore::for_test_dir(temp.path().to_path_buf());
    store.save(&snapshot).unwrap();
    let persisted = store.load_last().unwrap().unwrap();
    let restored = MultiplexerState::from_session_snapshot(&persisted);
    let restored_snapshot = restored.snapshot();
    let workspace_id = restored_snapshot.workspaces[0].id.as_str();
    let projection =
        build_tab_summary_json(&restored_snapshot, Some(workspace_id), None, false, false);
    let projected = projection
        .iter()
        .find(|tab| tab["id"].as_str() == Some(tab_id.as_str()))
        .unwrap();

    assert_eq!(
        restored.get_tab(&tab_id).unwrap().pending_delivery,
        Some(pending.clone())
    );
    assert_eq!(
        projected.get("pending_delivery"),
        Some(&serde_json::to_value(pending).unwrap())
    );
}

#[test]
fn tab_summary_command_deserializes_compact_scope_options() {
    let command: Command = serde_json::from_str(
        r#"{"cmd":"tab_summary","workspace_id":"ws_1","caller_tab_id":"tab_1","focused_workspace":false,"agents_only":true}"#,
    )
    .expect("Tab summary command");
    assert!(matches!(
        command,
        Command::TabSummary {
            workspace_id: Some(workspace_id),
            caller_tab_id: Some(caller_tab_id),
            focused_workspace: false,
            agents_only: true,
        } if workspace_id == "ws_1" && caller_tab_id == "tab_1"
    ));
}

#[test]
fn tab_list_focused_field_tracks_mux_focused_tab_id() {
    use crate::multiplexer::state::MultiplexerState;
    let (mut state, tab_a) = MultiplexerState::new();
    // Create a second tab.
    let tab_b = state
        .create_tab(&state.list_workspaces()[0].id.clone())
        .unwrap();
    // Focus tab_a explicitly.
    state.set_focused_tab(&tab_a).unwrap();
    let tabs = build_tab_list_json(&state.snapshot());
    let get_focused = |tabs: &[serde_json::Value], id: &str| {
        tabs.iter()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
            .and_then(|p| p.get("focused"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    assert!(
        get_focused(&tabs, &tab_a),
        "focused Tab must have focused=true"
    );
    assert!(
        !get_focused(&tabs, &tab_b),
        "non-focused Tab must have focused=false"
    );
    // Switch focus.
    state.set_focused_tab(&tab_b).unwrap();
    let tabs = build_tab_list_json(&state.snapshot());
    assert!(
        !get_focused(&tabs, &tab_a),
        "tab_a must be unfocused after switch"
    );
    assert!(
        get_focused(&tabs, &tab_b),
        "tab_b must be focused after switch"
    );
}

#[test]
fn tab_list_marks_exactly_one_focused_tab_from_each_snapshot() {
    use crate::multiplexer::state::MultiplexerState;
    let (mut state, tab_a) = MultiplexerState::new();
    let tab_b = state
        .create_tab(&state.list_workspaces()[0].id.clone())
        .unwrap();

    for focused in [&tab_a, &tab_b] {
        state.set_focused_tab(focused).unwrap();
        let tabs = build_tab_list_json(&state.snapshot());
        let focused_ids: Vec<&str> = tabs
            .iter()
            .filter(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
            .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
            .collect();

        assert_eq!(
            focused_ids,
            vec![focused.as_str()],
            "Tab list must derive focused=true from the snapshot being projected"
        );
    }
}

#[test]
fn tab_list_keeps_tab_info_after_rename_to_fibonacci() {
    use crate::multiplexer::state::MultiplexerState;
    let (mut state, visible) = MultiplexerState::new();
    let tab_id = state.find_tab_for_tab(&visible).unwrap();
    state.rename_tab(&tab_id, "Fibonacci").unwrap();
    let snapshot = state.snapshot();
    let tabs = build_tab_list_json(&snapshot);
    assert_eq!(tabs.len(), 1);
    assert_eq!(
        tabs[0].get("title").and_then(|v| v.as_str()),
        Some("Fibonacci"),
    );
    assert!(tabs[0].get("tab_title").is_none());
}
