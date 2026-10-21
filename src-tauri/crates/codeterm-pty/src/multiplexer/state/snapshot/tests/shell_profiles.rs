use super::*;

#[test]
fn restore_keeps_selected_shell_and_its_launch_cwd() {
    let mut state = MultiplexerState::empty();
    let workspace = state.create_workspace("Shells".into(), None).id;
    let id = state.create_tab(&workspace).unwrap();
    let profile = crate::pty::shell_profiles::ShellProfile {
        id: "wsl:Ubuntu".into(),
        name: "Ubuntu".into(),
        executable: "wsl.exe".into(),
        args: vec!["--distribution".into(), "Ubuntu".into()],
        wsl_distro: Some("Ubuntu".into()),
    };
    let tab = state.get_tab_mut(&id).unwrap();
    tab.shell_profile = Some(profile.clone());
    tab.cwd = Some(r"\\wsl.localhost\Ubuntu\home\me".into());
    let json = serde_json::to_vec(&state.to_session_snapshot()).unwrap();
    let snapshot = serde_json::from_slice(&json).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    let tab = restored.get_tab(&id).unwrap();
    assert_eq!(tab.shell_profile.as_ref(), Some(&profile));
    assert_eq!(
        tab.shell_profile
            .as_ref()
            .unwrap()
            .launch(tab.cwd.as_deref())
            .unwrap()
            .args,
        ["--distribution", "Ubuntu", "--cd", "/home/me"]
    );
}

#[test]
fn restore_without_selected_shell_preserves_automatic_choice() {
    let mut state = MultiplexerState::empty();
    let workspace = state.create_workspace("Shells".into(), None).id;
    let id = state.create_tab(&workspace).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&state.to_session_snapshot());
    assert!(restored.get_tab(&id).unwrap().shell_profile.is_none());
}
