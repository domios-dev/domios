use super::*;

fn closed_workspace() -> (MultiplexerState, String, Vec<ClosedTab>) {
    let mut state = MultiplexerState::empty();
    let workspace = state
        .create_workspace("Project".into(), Some("/restore/project".into()))
        .id;
    for (title, session) in [("Manager", "session-manager"), ("Worker", "session-worker")] {
        let id = state.create_tab(&workspace).unwrap();
        let tab = state.get_tab_mut(&id).unwrap();
        tab.title = title.into();
        tab.description = Some(format!("{title} description"));
        tab.cwd = Some("/restore/project".into());
        tab.agent_type = Some("provider".into());
        tab.agent_session_id = Some(session.into());
        tab.chat_view = title == "Worker";
    }
    let ids = state.get_workspace(&workspace).unwrap().tabs.clone();
    state.get_tab_mut(&ids[1]).unwrap().parent_tab_id = Some(ids[0].clone());
    state.close_workspace(&workspace).unwrap();
    let group_id = state.recent_closed_view()[0].group_id.clone();
    let (_, records) = state.closed_store.peek_group(&group_id).unwrap();
    (state, group_id, records)
}

#[test]
fn workspace_restore_preserves_captured_order_sessions_and_parent_links() {
    let (mut state, group_id, records) = closed_workspace();
    assert_eq!(records[0].title, "Manager");
    assert_eq!(records[1].title, "Worker");
    let (workspace_id, active_id) = state.reopen_closed_group(&group_id).unwrap();
    let workspace = state.get_workspace(&workspace_id).unwrap();
    assert_eq!(workspace.name, "Project");
    assert_eq!(workspace.cwd.as_deref(), Some("/restore/project"));
    assert_eq!(workspace.tabs[0], active_id);
    for (index, id) in workspace.tabs.iter().enumerate() {
        let tab = state.get_tab(id).unwrap();
        let source = &records[index];
        assert_eq!(tab.title, source.title);
        assert_eq!(tab.description, source.description);
        assert_eq!(tab.cwd, source.cwd);
        assert_eq!(tab.agent_type, source.agent_type);
        assert_eq!(tab.agent_session_id, source.agent_session_id);
        assert_eq!(tab.chat_view, source.chat_view);
        assert!(tab.restored_from_snapshot);
        assert_eq!(tab.parent_tab_id, (index == 1).then(|| active_id.clone()));
    }
    assert!(state.closed_store.peek_group(&group_id).is_none());
}

#[test]
fn failed_group_materialization_leaves_no_partial_workspace_or_tabs() {
    let (mut state, group_id, records) = closed_workspace();
    let mut calls = 0;
    let result = state.reopen_closed_group_using(&group_id, |staged, workspace, record| {
        calls += 1;
        let id = staged.materialize_closed_tab(workspace, record)?;
        if calls == 2 {
            return Err("injected second Tab failure".into());
        }
        Ok(id)
    });
    assert!(result.is_err());
    assert_eq!(calls, 2);
    assert!(state.list_workspaces().is_empty());
    assert_eq!(state.all_tabs().count(), 0);
    assert_eq!(state.closed_store.peek_group(&group_id).unwrap().1, records);
    state.reopen_closed_group(&group_id).unwrap();
    assert_eq!(state.list_workspaces().len(), 1);
    assert_eq!(state.all_tabs().count(), 2);
}

#[test]
fn failed_group_history_delete_rolls_back_then_retry_restores_once() {
    let (mut state, group_id, records) = closed_workspace();
    state.closed_store.reject_deletes(true);
    assert!(state.reopen_closed_group(&group_id).is_err());
    assert!(state.list_workspaces().is_empty());
    assert_eq!(state.all_tabs().count(), 0);
    assert_eq!(state.closed_store.peek_group(&group_id).unwrap().1, records);
    state.closed_store.reject_deletes(false);
    state.reopen_closed_group(&group_id).unwrap();
    let ids: Vec<_> = state.all_tabs().map(|tab| tab.id.clone()).collect();
    assert!(state.reopen_closed_group(&group_id).is_err());
    let after_ids: Vec<_> = state.all_tabs().map(|tab| tab.id.clone()).collect();
    assert_eq!(after_ids, ids);
    assert_eq!(state.list_workspaces().len(), 1);
}

#[test]
fn failed_single_history_delete_keeps_original_record_and_live_tabs() {
    let (mut state, _) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    let closing = state.create_tab(&workspace).unwrap();
    state.close_tab(&closing).unwrap();
    let before = state.get_workspace(&workspace).unwrap().tabs.clone();
    let records = state.list_recently_closed(&workspace);
    state.closed_store.reject_deletes(true);
    assert!(state.reopen_closed_tab(&workspace, None).is_err());
    assert_eq!(state.get_workspace(&workspace).unwrap().tabs, before);
    assert_eq!(state.list_recently_closed(&workspace), records);
    state.closed_store.reject_deletes(false);
    state.reopen_closed_tab(&workspace, None).unwrap();
    assert_eq!(
        state.get_workspace(&workspace).unwrap().tabs.len(),
        before.len() + 1
    );
}

#[test]
fn failed_orphan_history_delete_does_not_leave_recreated_workspace() {
    let (mut state, group_id, records) = closed_workspace();
    state.closed_store.reject_deletes(true);
    assert!(state.reopen_orphaned_tab(&records[0].tab_id).is_err());
    assert!(state.list_workspaces().is_empty());
    assert_eq!(state.all_tabs().count(), 0);
    assert_eq!(state.closed_store.peek_group(&group_id).unwrap().1, records);
}

#[test]
fn close_undo_preserves_model_and_effort_overrides_through_sqlite() {
    use crate::multiplexer::types::RestorableLaunchParameters;
    let (mut state, id) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    let launch = RestorableLaunchParameters {
        captured: true,
        model: Some("selected-model".into()),
        requested_model: Some("selected-model".into()),
        requested_reasoning_effort: Some("low".into()),
        resolved_reasoning_effort: Some("low".into()),
        ..Default::default()
    };
    {
        let tab = state.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("provider".into());
        tab.agent_session_id = Some("exact-own-session".into());
        tab.agent_provider_resolved_model = Some("selected-model".into());
        tab.restorable_launch = launch.clone();
    }
    state.close_tab(&id).unwrap();
    let stored = state.list_recently_closed(&workspace);
    assert_eq!(stored[0].restorable_launch, launch);
    assert_eq!(stored[0].model.as_deref(), Some("selected-model"));
    let restored = state.reopen_closed_tab(&workspace, None).unwrap();
    let tab = state.get_tab(&restored).unwrap();
    assert_eq!(tab.restorable_launch, launch);
    assert_eq!(tab.agent_session_id.as_deref(), Some("exact-own-session"));
    assert!(tab.close_restore.as_ref().unwrap().exact_session);
    assert!(tab.session_binding_nonce.is_none());
    assert!(tab.reported_session_binding_nonce.is_none());
}
