use super::*;

fn leaf_json(snapshot: &SessionSnapshot) -> serde_json::Value {
    serde_json::to_value(snapshot).unwrap()["workspaces"][0]["tabs"][0]["split_tree"].clone()
}

#[test]
fn permission_auto_confirm_survives_snapshot_restore_and_merge() {
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).expect("initial tab");
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("session-auto-confirm".into());
    tab.auto_approve = true;
    tab.permission_auto_confirm = true;

    let snapshot = state.to_session_snapshot();
    assert_eq!(
        leaf_json(&snapshot)["permission_auto_confirm"],
        serde_json::Value::Bool(true)
    );
    let persisted: SessionSnapshot =
        serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();

    let restored = MultiplexerState::from_session_snapshot(&persisted);
    let restored_tab = restored.get_tab(&tab_id).expect("restored tab");
    assert!(restored_tab.permission_auto_confirm);
    assert!(
        !restored_tab.auto_approve,
        "permission bypass still never restores from a snapshot"
    );

    let (mut desk, _) = MultiplexerState::new();
    desk.merge_session_snapshot(&persisted);
    assert!(
        desk.get_tab(&tab_id)
            .expect("merged tab")
            .permission_auto_confirm
    );
}

#[test]
fn a_snapshot_without_the_setting_restores_opted_out() {
    let (mut state, tab_id) = MultiplexerState::new();
    state.get_tab_mut(&tab_id).unwrap().permission_auto_confirm = true;
    let mut json = serde_json::to_value(state.to_session_snapshot()).unwrap();
    json["workspaces"][0]["tabs"][0]["split_tree"]
        .as_object_mut()
        .unwrap()
        .remove("permission_auto_confirm");
    let legacy: SessionSnapshot = serde_json::from_value(json).unwrap();

    let restored = MultiplexerState::from_session_snapshot(&legacy);
    assert!(!restored.get_tab(&tab_id).unwrap().permission_auto_confirm);

    let (opted_out, _) = MultiplexerState::new();
    assert!(leaf_json(&opted_out.to_session_snapshot())
        .get("permission_auto_confirm")
        .is_none());
}
