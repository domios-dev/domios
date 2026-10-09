use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{ShellActivity, ShellCommandState, ShellPurpose};
use crate::session::store::StorageDeskAdapter;
use crate::storage::{Storage, StorageConfig};

#[test]
fn tab_purpose_desk_restore_and_transport_snapshot() {
    for purpose in ShellPurpose::ALL {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
        let desk = StorageDeskAdapter::from_storage(storage.clone());
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.purpose = purpose;
        tab.icon = Some("flask-conical".into());
        tab.utility_created_at = 42;
        tab.shell_command_state = ShellCommandState {
            activity: ShellActivity::Running,
            last_exit_code: Some(3),
        };
        desk.import_snapshot(&mux.to_session_snapshot(), Vec::new())
            .unwrap();
        let restored = MultiplexerState::from_session_snapshot(&desk.snapshot().unwrap());
        let tab = restored.get_tab(&tab_id).unwrap();
        assert_eq!(tab.purpose, purpose);
        assert_eq!(tab.icon.as_deref(), Some("flask-conical"));
        assert_eq!(tab.utility_created_at, 42);
        assert_eq!(tab.shell_command_state, ShellCommandState::default());
        let snapshot = restored.snapshot();
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(
            json["tabs"][&tab_id]["purpose"],
            serde_json::to_value(purpose).unwrap()
        );
        let summary = crate::dispatch::build_tab_summary_json(&snapshot, None, None, false, false);
        assert_eq!(
            summary[0]["purpose"],
            serde_json::to_value(purpose).unwrap()
        );
        assert_eq!(summary[0]["icon"], "flask-conical");
        storage
            .shutdown()
            .wait(std::time::Duration::from_secs(30))
            .unwrap();
    }
}

#[test]
fn tab_purpose_survives_pty_replacement() {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().purpose = ShellPurpose::Build;
    mux.link_pty(&tab_id, "old".into());
    mux.get_tab_mut(&tab_id)
        .unwrap()
        .shell_command_state
        .activity = ShellActivity::Running;
    assert_eq!(
        mux.link_pty(&tab_id, "replacement".into()),
        Some("old".into())
    );
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.purpose, ShellPurpose::Build);
    assert_eq!(tab.shell_command_state, ShellCommandState::default());
}

#[test]
fn tab_purpose_rest_create_defaults_and_refuses_unknown() {
    use crate::api::tab_routes::CreateShellTabBody;
    let body: CreateShellTabBody = serde_json::from_str("{}").unwrap();
    assert_eq!(body.purpose, ShellPurpose::Shell);
    let body: CreateShellTabBody = serde_json::from_str(r#"{"purpose":"qa"}"#).unwrap();
    assert_eq!(body.purpose, ShellPurpose::Qa);
    assert!(serde_json::from_str::<CreateShellTabBody>(r#"{"purpose":"guess"}"#).is_err());
}

#[test]
fn shell_purpose_serde_matches_as_str_for_every_variant() {
    for purpose in ShellPurpose::ALL {
        assert_eq!(serde_json::to_value(purpose).unwrap(), purpose.as_str());
        let parsed: ShellPurpose =
            serde_json::from_value(serde_json::json!(purpose.as_str())).unwrap();
        assert_eq!(parsed, purpose);
    }
    for (raw, purpose) in [
        ("deploy", ShellPurpose::Deploy),
        ("logs", ShellPurpose::Logs),
        ("db", ShellPurpose::Db),
    ] {
        assert_eq!(
            serde_json::from_value::<ShellPurpose>(raw.into()).unwrap(),
            purpose
        );
    }
}

#[test]
fn tab_purpose_rest_description_icon_and_unknown_icon_refusal() {
    use super::super::{SpawnTabSpec, TabCommand};
    use crate::api::tab_routes::CreateShellTabBody;
    let body: CreateShellTabBody = serde_json::from_value(serde_json::json!({
        "purpose": "qa", "icon": "flask-conical", "description": "  Runs tests  "
    }))
    .unwrap();
    let mut spec = SpawnTabSpec {
        shell: None,
        purpose: body.purpose,
        icon: body.icon,
        description: body.description,
        ..Default::default()
    };
    super::prepare_tab_identity(&mut spec).unwrap();
    assert_eq!(spec.description.as_deref(), Some("Runs tests"));
    assert_eq!(spec.icon.as_deref(), Some("flask-conical"));
    spec.icon = Some("missing-icon".into());
    assert!(super::prepare_tab_identity(&mut spec)
        .unwrap_err()
        .starts_with("Unknown Lucide icon"));
    spec.icon = Some("bug".into());
    spec.command = TabCommand::ChatBackend {
        plugin_id: "local".into(),
        config: serde_json::Value::Null,
        role: None,
        role_profile: None,
        can_spawn: None,
        watcher: None,
    };
    assert!(super::prepare_tab_identity(&mut spec).is_err());
}

#[test]
fn shell_profile_selection_refuses_agent_backing() {
    use super::super::{AgentSpawnSpec, SpawnTabSpec, TabCommand};
    let mut spec = SpawnTabSpec {
        shell: Some("wsl:Ubuntu".into()),
        command: TabCommand::Agent(AgentSpawnSpec::default()),
        ..Default::default()
    };
    assert!(super::prepare_tab_identity(&mut spec).is_err());
}
