use super::*;

#[test]
fn snapshots_exclude_orphaned_and_duplicate_live_tabs() {
    let mut state = MultiplexerState::empty();
    let workspace_id = state.create_workspace("Clean".to_string(), None).id;
    let tab_id = state.create_tab(&workspace_id).unwrap();
    let workspace = state
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == workspace_id)
        .unwrap();
    workspace.tabs.push(tab_id.clone());
    workspace.tabs.push("closed-tab".to_string());
    workspace.active_tab_id = Some("closed-tab".to_string());

    let live = state.snapshot();
    assert_eq!(live.workspaces[0].tabs.len(), 1);
    assert_eq!(live.workspaces[0].tabs[0], tab_id);
    assert_eq!(
        live.workspaces[0].active_tab_id.as_deref(),
        Some(tab_id.as_str())
    );

    let persisted = state.to_session_snapshot();
    assert_eq!(persisted.workspaces[0].tabs.len(), 1);
    assert_eq!(persisted.workspaces[0].tabs[0].id, tab_id);
}
