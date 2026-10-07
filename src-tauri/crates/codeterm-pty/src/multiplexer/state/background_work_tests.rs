use super::MultiplexerState;
#[test]
fn restart_guard_close_discards_roster_without_affecting_other_tabs() {
    let (mut mux, first) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let second = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&first)
        .unwrap()
        .provider_signals
        .background
        .history_incomplete = true;
    mux.get_tab_mut(&second)
        .unwrap()
        .provider_signals
        .background
        .history_incomplete = true;
    mux.close_tab(&first).unwrap();
    assert!(mux.get_tab(&first).is_none());
    assert!(!mux
        .get_tab(&second)
        .unwrap()
        .provider_signals
        .background
        .is_empty());
    let fresh = mux.create_tab(&workspace).unwrap();
    assert!(mux
        .get_tab(&fresh)
        .unwrap()
        .provider_signals
        .background
        .is_empty());
}
