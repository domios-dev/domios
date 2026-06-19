//! Behaviour tests for the sqlite-backed closed history, ported from the
//! pre-sqlite ring implementation plus new grouping coverage (cascade /
//! workspace restore).

use std::collections::HashSet;

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::PaneBacking;

fn ws_id(state: &MultiplexerState) -> String {
    state.list_workspaces()[0].id.clone()
}

#[test]
fn close_tab_records_pane_into_history_with_backing() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (tab_id, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.backing = PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s1".into(),
        };
    }
    state.rename_tab(&wsid, &tab_id, "My Chat").unwrap();
    state.close_tab(&wsid, &tab_id).unwrap();

    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), 1, "closed tab recorded");
    assert_eq!(list[0].pane_id, pane_id);
    assert_eq!(list[0].title, "My Chat");
    assert_eq!(
        list[0].backing,
        PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s1".into(),
        }
    );
}

#[test]
fn close_pane_records_agent_identity() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (_tab_id, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.agent_type = Some("claude".into());
        p.agent_session_id = Some("sess-42".into());
        p.agent_session_model = Some("Opus".into());
        p.cwd = Some("/tmp/work".into());
    }
    state.close_pane(&pane_id).unwrap();

    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].agent_type.as_deref(), Some("claude"));
    assert_eq!(list[0].agent_session_id.as_deref(), Some("sess-42"));
    assert_eq!(list[0].model.as_deref(), Some("Opus"));
    assert_eq!(list[0].cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn reopen_recreates_pane_with_same_backing() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (tab_id, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.backing = PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s7".into(),
        };
    }
    state.rename_tab(&wsid, &tab_id, "Restored Chat").unwrap();
    state.close_tab(&wsid, &tab_id).unwrap();

    let (new_tab, new_pane) = state
        .reopen_closed_pane(&wsid, None, &HashSet::new())
        .unwrap();
    assert_ne!(new_tab, tab_id, "fresh tab id");
    let pane = state.get_pane(&new_pane).expect("reopened pane exists");
    assert_eq!(
        pane.backing,
        PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s7".into(),
        },
        "same backing on reopen"
    );
    assert!(pane.is_chat_backend());
    assert!(pane.restored_from_snapshot, "drives lazy reopen");
    assert_eq!(pane.title, "Restored Chat");
    // Reopened entry is consumed from history.
    assert!(state.list_recently_closed(&wsid).is_empty());
}

#[test]
fn reopen_agent_pane_marks_for_resume() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (_tab_id, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.agent_type = Some("codex".into());
        p.agent_session_id = Some("cx-1".into());
    }
    state.close_pane(&pane_id).unwrap();

    let (_t, new_pane) = state
        .reopen_closed_pane(&wsid, None, &HashSet::new())
        .unwrap();
    let pane = state.get_pane(&new_pane).unwrap();
    assert_eq!(pane.agent_type.as_deref(), Some("codex"));
    assert!(pane.agent_type_locked);
    assert_eq!(pane.agent_session_id.as_deref(), Some("cx-1"));
    assert!(pane.restored_from_snapshot);
    assert_eq!(pane.backing, PaneBacking::Pty);
}

// ── chat-UI default re-applied on reopen (default_new_agent_pane_chat) ──

/// Helper: the chat-capable provider set the service layer passes when the
/// `default_new_agent_pane_chat` preference is ON.
fn capable(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

#[test]
fn reopen_chat_capable_agent_with_pref_on_opens_chat() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (_tab_id, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.agent_type = Some("claude".into());
    }
    state.close_pane(&pane_id).unwrap();

    // pref ON → claude is chat-capable → reopened pane defaults to chat.
    let (_t, new_pane) = state
        .reopen_closed_pane(&wsid, None, &capable(&["claude", "codex"]))
        .unwrap();
    assert!(
        state.get_pane(&new_pane).unwrap().chat_view,
        "chat-capable agent pane reopens in chat when the pref is on"
    );
}

#[test]
fn reopen_chat_capable_agent_with_pref_off_stays_terminal() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (_tab_id, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.agent_type = Some("claude".into());
    }
    state.close_pane(&pane_id).unwrap();

    // pref OFF → empty capable set → terminal.
    let (_t, new_pane) = state
        .reopen_closed_pane(&wsid, None, &HashSet::new())
        .unwrap();
    assert!(
        !state.get_pane(&new_pane).unwrap().chat_view,
        "preference off keeps the reopened pane in terminal"
    );
}

#[test]
fn reopen_unsupported_provider_stays_terminal_even_with_pref_on() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (_tab_id, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.agent_type = Some("aider".into()); // not in the capable set
    }
    state.close_pane(&pane_id).unwrap();

    let (_t, new_pane) = state
        .reopen_closed_pane(&wsid, None, &capable(&["claude", "codex"]))
        .unwrap();
    assert!(
        !state.get_pane(&new_pane).unwrap().chat_view,
        "a provider that does not support chat UI stays terminal"
    );
}

#[test]
fn reopen_cascade_group_applies_chat_default_to_every_pane() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (orch_tab, orch_pane) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &orch_tab, "Orchestrator").unwrap();
    state.get_pane_mut(&orch_pane).unwrap().agent_type = Some("claude".into());
    let (w1_tab, worker1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &w1_tab, "Worker").unwrap();
    let w = state.get_pane_mut(&worker1).unwrap();
    w.parent_pane_id = Some(orch_pane.clone());
    w.agent_type = Some("codex".into());

    state.close_pane(&orch_pane).unwrap();
    let group_id = state.recent_closed_view()[0].group_id.clone();

    let (_ws, _tab, _first) = state
        .reopen_closed_group(&group_id, &capable(&["claude", "codex"]))
        .unwrap();
    let restored: Vec<String> = state
        .list_workspaces()
        .iter()
        .find(|w| w.id == wsid)
        .map(|w| w.pane_ids.clone())
        .unwrap_or_default();
    let agent_panes: Vec<bool> = restored
        .iter()
        .filter_map(|pid| state.get_pane(pid))
        .filter(|p| p.agent_type.is_some())
        .map(|p| p.chat_view)
        .collect();
    assert_eq!(agent_panes.len(), 2, "both agent panes restored");
    assert!(
        agent_panes.iter().all(|&c| c),
        "every chat-capable pane in the group reopens in chat"
    );
}

#[test]
fn session_restore_reapplies_chat_default_for_capable_agents() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (_t1, claude_pane) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&claude_pane).unwrap().agent_type = Some("claude".into());
    let (_t2, other_pane) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&other_pane).unwrap().agent_type = Some("aider".into());

    // Simulates the startup pass after `from_session_snapshot` when the pref is
    // on: only chat-capable agents flip to chat.
    state.apply_default_chat_view_for_capable(&capable(&["claude", "codex"]));
    assert!(state.get_pane(&claude_pane).unwrap().chat_view);
    assert!(!state.get_pane(&other_pane).unwrap().chat_view);

    // Empty set (pref off) is a no-op.
    let (_t3, c2) = state.create_tab(&wsid).unwrap();
    state.get_pane_mut(&c2).unwrap().agent_type = Some("claude".into());
    state.apply_default_chat_view_for_capable(&HashSet::new());
    assert!(!state.get_pane(&c2).unwrap().chat_view);
}

#[test]
fn history_is_bounded_at_cap() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    for _ in 0..(RECENTLY_CLOSED_CAP + 5) {
        let (tab_id, _pane_id) = state.create_tab(&wsid).unwrap();
        state.close_tab(&wsid, &tab_id).unwrap();
    }
    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), RECENTLY_CLOSED_CAP, "history bounded at cap");
}

#[test]
fn history_is_newest_first() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (t1, _p1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t1, "first").unwrap();
    state.close_tab(&wsid, &t1).unwrap();
    let (t2, _p2) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t2, "second").unwrap();
    state.close_tab(&wsid, &t2).unwrap();

    let list = state.list_recently_closed(&wsid);
    assert_eq!(list[0].title, "second", "newest first");
    assert_eq!(list[1].title, "first");
}

#[test]
fn reopen_by_index_selects_older_entry() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (t1, _p1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t1, "older").unwrap();
    state.close_tab(&wsid, &t1).unwrap();
    let (t2, _p2) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t2, "newer").unwrap();
    state.close_tab(&wsid, &t2).unwrap();

    // index 1 = the older "first"-closed entry.
    let (_t, new_pane) = state
        .reopen_closed_pane(&wsid, Some(1), &HashSet::new())
        .unwrap();
    assert_eq!(state.get_pane(&new_pane).unwrap().title, "older");
    // The remaining entry is the newer one.
    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "newer");
}

#[test]
fn reopen_empty_history_errors() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let err = state
        .reopen_closed_pane(&wsid, None, &HashSet::new())
        .unwrap_err();
    assert!(err.contains("No recently closed"));
}

#[test]
fn reopen_out_of_range_index_errors() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (tab_id, _p) = state.create_tab(&wsid).unwrap();
    state.close_tab(&wsid, &tab_id).unwrap();
    let err = state
        .reopen_closed_pane(&wsid, Some(5), &HashSet::new())
        .unwrap_err();
    assert!(err.contains("index 5"));
}

#[test]
fn ring_cap_is_1000() {
    assert_eq!(RECENTLY_CLOSED_CAP, 1000, "history cap must be 1000");
}

#[test]
fn hidden_pane_is_not_recorded() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (tab_id, pane_id) = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_pane_mut(&pane_id) {
        p.hidden = true;
    }
    state.close_tab(&wsid, &tab_id).unwrap();
    assert!(
        state.list_recently_closed(&wsid).is_empty(),
        "hidden worker pane must not pollute the history"
    );
}

// ── cascade grouping ─────────────────────────────────────────────────

#[test]
fn cascade_close_records_one_group_with_hierarchy() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);

    let (orch_tab, orch_pane) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &orch_tab, "Orchestrator").unwrap();
    let (w1_tab, worker1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &w1_tab, "Worker 1").unwrap();
    state.get_pane_mut(&worker1).unwrap().parent_pane_id = Some(orch_pane.clone());
    let (w2_tab, worker2) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &w2_tab, "Worker 2").unwrap();
    state.get_pane_mut(&worker2).unwrap().parent_pane_id = Some(orch_pane.clone());

    state.close_pane(&orch_pane).unwrap();

    // All three land in history under ONE cascade group.
    let list = state.list_recently_closed(&wsid);
    let titles: Vec<&str> = list.iter().map(|e| e.title.as_str()).collect();
    assert!(titles.contains(&"Orchestrator"), "got {:?}", titles);
    assert!(titles.contains(&"Worker 1"), "got {:?}", titles);
    assert!(titles.contains(&"Worker 2"), "got {:?}", titles);

    let view = state.recent_closed_view();
    let group_ids: HashSet<&str> = view.iter().map(|v| v.group_id.as_str()).collect();
    assert_eq!(group_ids.len(), 1, "single cascade group");
    assert!(view.iter().all(|v| v.group_kind == "cascade"));
    // child rows carry the original parent link
    let worker_row = list
        .iter()
        .find(|e| e.title == "Worker 1")
        .expect("worker recorded");
    assert_eq!(
        worker_row.parent_pane_id.as_deref(),
        Some(orch_pane.as_str())
    );
}

#[test]
fn reopen_cascade_group_restores_subtree_and_relinks_parents() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);

    let (orch_tab, orch_pane) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &orch_tab, "Orchestrator").unwrap();
    let (w1_tab, worker1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &w1_tab, "Worker").unwrap();
    state.get_pane_mut(&worker1).unwrap().parent_pane_id = Some(orch_pane.clone());

    state.close_pane(&orch_pane).unwrap();

    let group_id = state.recent_closed_view()[0].group_id.clone();
    let (ws, _tab, first_pane) = state
        .reopen_closed_group(&group_id, &HashSet::new())
        .unwrap();
    assert_eq!(ws, wsid, "cascade reopens into origin workspace");

    // Two panes restored, parent link rebuilt across the new ids.
    let restored: Vec<String> = state
        .list_workspaces()
        .iter()
        .find(|w| w.id == wsid)
        .map(|w| w.pane_ids.clone())
        .unwrap_or_default();
    let child = restored
        .iter()
        .filter_map(|pid| state.get_pane(pid))
        .find(|p| p.title == "Worker")
        .expect("worker restored");
    let parent = restored
        .iter()
        .filter_map(|pid| state.get_pane(pid))
        .find(|p| p.title == "Orchestrator")
        .expect("orchestrator restored");
    assert_eq!(
        child.parent_pane_id.as_deref(),
        Some(parent.id.as_str()),
        "parent_pane_id relinked to the freshly minted orchestrator id"
    );
    assert!(state.get_pane(&first_pane).is_some());
    // group consumed
    assert!(state.recent_closed_view().is_empty());
}

// ── workspace grouping ───────────────────────────────────────────────

#[test]
fn close_workspace_records_a_workspace_group_with_all_tabs() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (t1, _p1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t1, "Tab A").unwrap();
    let (t2, _p2) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t2, "Tab B").unwrap();

    state.close_workspace(&wsid).unwrap();

    let view = state.recent_closed_view();
    assert!(view.iter().all(|v| v.group_kind == "workspace"));
    let group_ids: HashSet<&str> = view.iter().map(|v| v.group_id.as_str()).collect();
    assert_eq!(group_ids.len(), 1, "one workspace group");
    let titles: Vec<&str> = view.iter().map(|v| v.pane.title.as_str()).collect();
    assert!(titles.contains(&"Tab A"), "got {:?}", titles);
    assert!(titles.contains(&"Tab B"), "got {:?}", titles);
    // Workspace is gone, so its entries read as orphaned.
    assert!(view.iter().all(|v| v.orphaned));
}

#[test]
fn reopen_workspace_group_recreates_workspace_with_tabs() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    state.rename_workspace(&wsid, "Project").unwrap();
    let (t1, _p1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t1, "Tab A").unwrap();
    let (t2, _p2) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t2, "Tab B").unwrap();

    state.close_workspace(&wsid).unwrap();
    let group_id = state.recent_closed_view()[0].group_id.clone();

    let (new_ws, _tab, _pane) = state
        .reopen_closed_group(&group_id, &HashSet::new())
        .unwrap();
    let info = state
        .list_workspaces()
        .into_iter()
        .find(|w| w.id == new_ws)
        .expect("workspace recreated");
    assert_eq!(info.name, "Project");
    // initial pane + Tab A + Tab B were all recorded and restored
    let titles: HashSet<String> = info
        .pane_ids
        .iter()
        .filter_map(|pid| state.get_pane(pid))
        .map(|p| p.title.clone())
        .collect();
    assert!(titles.contains("Tab A"), "got {:?}", titles);
    assert!(titles.contains("Tab B"), "got {:?}", titles);
}

#[test]
fn reopen_orphaned_pane_recreates_workspace() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    state.rename_workspace(&wsid, "Gone").unwrap();
    let (t1, pane1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t1, "Lonely").unwrap();

    state.close_workspace(&wsid).unwrap();

    let (new_ws, _tab, new_pane) = state.reopen_orphaned_pane(&pane1, &HashSet::new()).unwrap();
    assert_ne!(new_ws, wsid);
    assert_eq!(state.get_pane(&new_pane).unwrap().title, "Lonely");
}

#[test]
fn reopen_workspace_group_matches_existing_workspace_by_path() {
    let (mut state, _root) = MultiplexerState::new();
    // A workspace rooted at /tmp/proj, with a tab, then fully closed.
    let wsid = state
        .create_workspace("Proj".into(), Some("/tmp/proj".into()))
        .id;
    let (t1, _p1) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &t1, "Tab A").unwrap();
    state.close_workspace(&wsid).unwrap();
    let group_id = state.recent_closed_view()[0].group_id.clone();

    // A DIFFERENT live workspace now points at the SAME folder (trailing slash
    // must still match via normalize_cwd).
    let live_id = state
        .create_workspace("Proj Again".into(), Some("/tmp/proj/".into()))
        .id;
    let count_before = state.list_workspaces().len();

    let (target_ws, _tab, _pane) = state
        .reopen_closed_group(&group_id, &HashSet::new())
        .unwrap();
    assert_eq!(
        target_ws, live_id,
        "restore into the live same-path workspace, not a duplicate"
    );
    assert_eq!(
        state.list_workspaces().len(),
        count_before,
        "no new workspace created when one matches by path"
    );
    let info = state
        .list_workspaces()
        .into_iter()
        .find(|w| w.id == live_id)
        .unwrap();
    let titles: HashSet<String> = info
        .pane_ids
        .iter()
        .filter_map(|pid| state.get_pane(pid))
        .map(|p| p.title.clone())
        .collect();
    assert!(
        titles.contains("Tab A"),
        "restored tab landed here: {:?}",
        titles
    );
}

#[test]
fn single_close_still_works_alongside_groups() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let (tab_id, _p) = state.create_tab(&wsid).unwrap();
    state.rename_tab(&wsid, &tab_id, "Solo").unwrap();
    state.close_tab(&wsid, &tab_id).unwrap();

    let view = state.recent_closed_view();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].group_kind, "single");
    assert_eq!(view[0].index, 0);
    assert!(!view[0].orphaned);
}
