use super::*;
use crate::{lock_hold::TimedRwLock, multiplexer::state::MultiplexerState};
use std::sync::Arc;

#[test]
fn navigation_works_without_materializing_restored_tabs() {
    let (mut mux, first) = MultiplexerState::new();
    let workspace = mux.snapshot().workspaces[0].id.clone();
    let second = mux.create_tab(&workspace).unwrap();
    let events = Arc::new(codeterm_events::EventBroadcaster::new());
    let desk = StartupDesk::new(Arc::new(TimedRwLock::new("mux", mux)), events.clone());
    desk.focus_tab(&workspace, &second).unwrap();
    let snapshot = desk.snapshot();
    assert_eq!(snapshot.focused_tab_id.as_deref(), Some(second.as_str()));
    assert_eq!(
        snapshot.workspaces[0].active_tab_id.as_deref(),
        Some(second.as_str())
    );
    assert!(snapshot.tabs.values().all(|tab| tab.pty_id.is_none()));
    let generation = snapshot.generation;
    assert!(desk.focus_tab(&workspace, "missing").is_err());
    desk.focus_tab(&workspace, &first).unwrap();
    assert!(desk.snapshot().generation > generation);
    assert_eq!(events.current_seq(), 2);
    let final_snapshot = desk.snapshot();
    assert_eq!(
        desk.publisher().read_versioned().0,
        final_snapshot.generation
    );
}
