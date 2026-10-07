use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{ShellActivity, ShellCommandState, ShellPurpose};
use crate::session::store::StorageDeskAdapter;
use crate::storage::{Storage, StorageConfig};

#[test]
fn tab_purpose_desk_restore_and_transport_snapshot() {
    for purpose in [
        ShellPurpose::Build,
        ShellPurpose::Dev,
        ShellPurpose::Qa,
        ShellPurpose::Shell,
    ] {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
        let desk = StorageDeskAdapter::from_storage(storage.clone());
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.purpose = purpose;
        tab.shell_command_state = ShellCommandState {
            activity: ShellActivity::Running,
            last_exit_code: Some(3),
        };
        desk.import_snapshot(&mux.to_session_snapshot(), Vec::new())
            .unwrap();
        let restored = MultiplexerState::from_session_snapshot(&desk.snapshot().unwrap());
        let tab = restored.get_tab(&tab_id).unwrap();
        assert_eq!(tab.purpose, purpose);
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
