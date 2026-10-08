use super::*;

/// Repro: a tab marked skip_snapshot (first-run picker or any ephemeral
/// onboarding tab) must be absent from the serialised session snapshot.
/// Without the fix this test fails — the tab IS included, then restores as
/// a ghost after a daemon restart.
#[test]
fn ghost_repro_skip_snapshot_tab_excluded_from_snapshot() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;
    let tab_id = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_id).unwrap().skip_snapshot = true;

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        0,
        "skip_snapshot Tab must be excluded from the session snapshot — \
         if it weren't, a daemon restart would restore it as a ghost tab"
    );
}

/// Happy path: a regular (non-ephemeral) tab must survive the round-trip
/// and not be accidentally dropped by the ghost-tab fix.
#[test]
fn ghost_repro_normal_tab_survives_snapshot_round_trip() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;
    let tab_id = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_id).unwrap().title = "My session".to_string();
    // skip_snapshot defaults to false

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        1,
        "regular Tab must survive the session snapshot round-trip"
    );

    let restored = MultiplexerState::from_session_snapshot(&snap);
    assert!(
        restored.tabs.contains_key(&tab_id),
        "restored state must include the regular Tab"
    );
}

/// Mixed workspace: only the ephemeral tab is dropped; the real tab survives.
#[test]
fn ghost_repro_mixed_workspace_drops_only_ephemeral() {
    let mut state = MultiplexerState::empty();
    let wsid = state.create_workspace("Test".to_string(), None).id;

    let tab_real = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_real).unwrap().title = "Real session".to_string();

    let tab_picker = state.create_tab(&wsid).unwrap();
    state.get_tab_mut(&tab_picker).unwrap().skip_snapshot = true;

    let snap = state.to_session_snapshot();
    assert_eq!(
        snap.workspaces[0].tabs.len(),
        1,
        "only the real Tab must survive; the ephemeral picker must be excluded"
    );
    let surviving_leaf = &snap.workspaces[0].tabs[0].split_tree;
    match surviving_leaf {
        crate::session::store::SplitNodeSnapshot::Leaf { tab_id, .. } => {
            assert_eq!(tab_id, &tab_real, "surviving tab must be the real Tab");
        }
        _ => panic!("expected a leaf"),
    }
}
