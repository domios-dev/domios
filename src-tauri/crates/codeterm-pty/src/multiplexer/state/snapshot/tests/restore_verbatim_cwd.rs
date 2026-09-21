use super::*;

#[test]
fn restored_snapshot_normalizes_verbatim_cwd_before_agent_materialization() {
    let mut state = MultiplexerState::empty();
    let workspace_id = state.create_workspace("Verbatim".into(), None).id;
    let tab_id = state.create_tab(&workspace_id).unwrap();
    let workspace = state
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == workspace_id)
        .unwrap();
    workspace.cwd = Some(r"\\?\C:\workspace\project".into());
    workspace.provisioned_cwd = Some(r"\\?\C:\workspace\project".into());
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.cwd = Some(r"\\?\C:\workspace\project".into());
    tab.provisioned_cwd = Some(r"\\?\C:\workspace\project".into());

    let snapshot = state.to_session_snapshot();
    assert_eq!(
        snapshot.workspaces[0].cwd.as_deref(),
        Some(r"C:\workspace\project")
    );
    assert_eq!(
        snapshot.workspaces[0].provisioned_cwd.as_deref(),
        Some(r"C:\workspace\project")
    );
    let crate::session::store::SplitNodeSnapshot::Leaf {
        cwd,
        agent_identity,
        ..
    } = &snapshot.workspaces[0].tabs[0].split_tree
    else {
        panic!("session tab snapshot must be a leaf");
    };
    assert_eq!(cwd.as_deref(), Some(r"C:\workspace\project"));
    assert_eq!(
        agent_identity
            .as_ref()
            .and_then(|identity| identity.provisioned_cwd.as_deref()),
        Some(r"C:\workspace\project")
    );
    let mut legacy_snapshot = snapshot.clone();
    legacy_snapshot.workspaces[0].cwd = Some(r"\\?\C:\workspace\project".into());
    legacy_snapshot.workspaces[0].provisioned_cwd =
        Some(r"\\?\C:\workspace\project".into());
    let crate::session::store::SplitNodeSnapshot::Leaf {
        cwd,
        agent_identity,
        ..
    } = &mut legacy_snapshot.workspaces[0].tabs[0].split_tree
    else {
        panic!("session tab snapshot must be a leaf");
    };
    *cwd = Some(r"\\?\C:\workspace\project".into());
    agent_identity
        .as_mut()
        .expect("agent identity snapshot")
        .provisioned_cwd = Some(r"\\?\C:\workspace\project".into());

    let restored = MultiplexerState::from_session_snapshot(&legacy_snapshot);

    assert_eq!(
        restored.get_tab(&tab_id).unwrap().cwd.as_deref(),
        Some(r"C:\workspace\project")
    );
    assert_eq!(
        restored.get_tab(&tab_id).unwrap().provisioned_cwd.as_deref(),
        Some(r"C:\workspace\project")
    );
    let restored_workspace = restored
        .get_workspace(&workspace_id)
        .expect("workspace restored");
    assert_eq!(
        restored_workspace.cwd.as_deref(),
        Some(r"C:\workspace\project")
    );
    assert_eq!(
        restored_workspace.provisioned_cwd.as_deref(),
        Some(r"C:\workspace\project")
    );
}
