use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TabAudience;
use crate::session::store::{SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot};

fn terminal_leaf(tab_id: &str, parent: Option<&str>) -> SplitNodeSnapshot {
    SplitNodeSnapshot::Leaf {
        tab_id: tab_id.into(),
        cwd: Some("D:/project".into()),
        title: Some(format!("title-{tab_id}")),
        description: None,
        title_level: Some("User".into()),
        agent_type: Some("claude".into()),
        agent_type_locked: true,
        claude_session_id: Some(format!("session-{tab_id}")),
        agent_identity: None,
        role: None,
        role_profile: None,
        team_id: None,
        parent_tab_id: parent.map(str::to_string),
        spawned_by_tab_id: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        ownership: None,
        can_spawn: false,
        depth: 0,
        chat_view: Some(false),
        auto_approve: false,
        permission_auto_confirm: false,
        audience: TabAudience::User,
        agent_session_model: None,
        agent_planned_model: None,
        agent_requested_model: None,
        agent_provider_resolved_model: None,
        model_fallback: None,
        model_identity_mismatch: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
        watcher: None,
        pending_delivery: None,
    }
}

fn backup(specs: &[(&str, Option<&str>, &[&str])]) -> SessionSnapshot {
    SessionSnapshot {
        version: "test".into(),
        timestamp: 0,
        active_workspace_id: None,
        teams: vec![],
        workspaces: specs
            .iter()
            .map(|(ws_id, cwd, tab_ids)| WorkspaceSnapshot {
                task_keys: Default::default(),
                id: (*ws_id).into(),
                name: (*ws_id).into(),
                active_tab_id: tab_ids.first().map(|id| (*id).to_string()),
                cwd: cwd.map(str::to_string),
                provisioned_cwd: None,
                protected: false,
                sort_order: 0,
                tabs: tab_ids
                    .iter()
                    .map(|tab_id| TabSnapshot {
                        id: (*tab_id).into(),
                        title: (*tab_id).into(),
                        description: None,
                        sort_order: 0,
                        split_tree: terminal_leaf(tab_id, None),
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[test]
fn merge_recreates_missing_workspaces_and_tabs_with_original_ids() {
    let mut live = MultiplexerState::empty();
    let report = live.merge_session_snapshot(&backup(&[
        ("ws_lost", None, &["t1", "t2"]),
        ("ws_other", None, &["t3"]),
    ]));

    assert_eq!(report.workspaces_added, 2);
    assert_eq!(report.tabs_added, 3);
    assert_eq!(report.tabs_already_present, 0);
    let ws = live
        .get_workspace("ws_lost")
        .expect("workspace restored under its original id");
    assert_eq!(ws.tabs, vec!["t1".to_string(), "t2".to_string()]);
    assert!(!ws.protected, "restored workspace never claims protection");
    let tab = live.get_tab("t1").expect("tab restored under original id");
    assert_eq!(tab.agent_session_id.as_deref(), Some("session-t1"));
    assert!(tab.restored_from_snapshot, "restore machinery takes over");
}

#[test]
fn merge_never_touches_existing_tabs_and_reports_them_as_present() {
    let (mut live, existing_tab) = MultiplexerState::new();
    let live_ws = live.find_workspace_for_tab(&existing_tab).unwrap();
    live.get_tab_mut(&existing_tab).unwrap().title = "user renamed".into();

    let mut snapshot = backup(&[("ws_new", None, &["fresh"])]);
    // The backup also carries the still-live tab inside its old workspace.
    snapshot.workspaces.push(WorkspaceSnapshot {
        task_keys: Default::default(),
        id: live_ws.clone(),
        name: "live".into(),
        active_tab_id: Some(existing_tab.clone()),
        cwd: None,
        provisioned_cwd: None,
        protected: false,
        sort_order: 0,
        tabs: vec![TabSnapshot {
            id: existing_tab.clone(),
            title: "stale title from backup".into(),
            description: None,
            sort_order: 0,
            split_tree: terminal_leaf(&existing_tab, None),
        }],
    });

    let report = live.merge_session_snapshot(&snapshot);
    assert_eq!(report.tabs_added, 1);
    assert_eq!(report.tabs_already_present, 1);
    assert_eq!(
        live.get_tab(&existing_tab).unwrap().title,
        "user renamed",
        "live tab stays untouched"
    );
}

#[test]
fn merge_lands_tabs_in_a_live_workspace_matched_by_cwd() {
    let mut live = MultiplexerState::empty();
    let live_ws = live.create_workspace("recreated".into(), Some("D:/repo".into()));

    let report = live.merge_session_snapshot(&backup(&[("ws_gone", Some("D:/repo"), &["t1"])]));
    assert_eq!(
        report.workspaces_added, 0,
        "matched by path, not duplicated"
    );
    assert_eq!(report.tabs_added, 1);
    let ws = live.get_workspace(&live_ws.id).unwrap();
    assert!(ws.tabs.contains(&"t1".to_string()));
}

#[test]
fn merge_drops_parent_links_pointing_at_tabs_that_no_longer_exist() {
    let mut live = MultiplexerState::empty();
    let mut snapshot = backup(&[("ws", None, &[])]);
    // Role-bearing leaves are the only ones that keep a parent link on restore.
    let mut worker = terminal_leaf("child", Some("vanished-parent"));
    let SplitNodeSnapshot::Leaf { role, .. } = &mut worker else {
        unreachable!()
    };
    *role = Some("Worker".into());
    snapshot.workspaces[0].tabs.push(TabSnapshot {
        id: "child".into(),
        title: "child".into(),
        description: None,
        sort_order: 0,
        split_tree: worker,
    });

    live.merge_session_snapshot(&snapshot);
    assert_eq!(
        live.get_tab("child").unwrap().parent_tab_id,
        None,
        "no dangling hierarchy edge after a partial restore"
    );
}

#[test]
fn merge_does_not_steal_an_agent_session_already_claimed_live() {
    let (mut live, existing_tab) = MultiplexerState::new();
    live.get_tab_mut(&existing_tab).unwrap().agent_session_id = Some("session-t1".into());

    live.merge_session_snapshot(&backup(&[("ws_gone", None, &["t1"])]));
    assert_eq!(
        live.get_tab("t1").unwrap().agent_session_id,
        None,
        "claimed session id is not duplicated onto the restored tab"
    );
}
