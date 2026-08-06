//! Behaviour tests for the sqlite-backed closed history, ported from the
//! pre-sqlite ring implementation plus new grouping coverage (cascade /
//! workspace restore).

use std::collections::HashSet;

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, TabBacking, TabCloseReason};

fn ws_id(state: &MultiplexerState) -> String {
    state.list_workspaces()[0].id.clone()
}

#[test]
fn close_tab_records_tab_into_history_with_backing() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.backing = TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s1".into(),
        };
    }
    state.rename_tab(&tab_id, "My Chat").unwrap();
    state.close_tab(&tab_id).unwrap();

    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), 1, "closed tab recorded");
    assert_eq!(list[0].tab_id, tab_id);
    assert_eq!(list[0].title, "My Chat");
    assert_eq!(
        list[0].backing,
        TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s1".into(),
        }
    );
}

#[test]
fn close_tab_detaches_children_without_closing_them() {
    let (mut state, orchestrator_id) = MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    let orchestrator_tab = state.find_tab_for_tab(&orchestrator_id).unwrap();
    state
        .set_orchestrator_role(&orchestrator_id)
        .unwrap();
    let worker_id = state.create_tab(&workspace_id).unwrap();
    state
        .set_tab_role(&worker_id, AgentRole::Worker, Some(&orchestrator_id))
        .unwrap();
    state.get_tab_mut(&worker_id).unwrap().report_target_tab_id = Some(orchestrator_id.clone());

    state.close_tab(&orchestrator_tab).unwrap();

    let worker = state.get_tab(&worker_id).expect("worker remains open");
    assert_eq!(worker.parent_tab_id, None);
    assert_eq!(worker.report_target_tab_id, None);
    assert_eq!(worker.depth, 0);
}

#[test]
fn close_tab_records_agent_identity() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.agent_type = Some("claude".into());
        p.agent_session_id = Some("sess-42".into());
        p.role_profile = Some("planner".into());
        p.agent_session_model = Some("Opus".into());
        p.cwd = Some("/tmp/work".into());
    }
    state.close_tab(&tab_id).unwrap();

    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].agent_type.as_deref(), Some("claude"));
    assert_eq!(list[0].agent_session_id.as_deref(), Some("sess-42"));
    assert_eq!(list[0].role_profile.as_deref(), Some("planner"));
    assert_eq!(list[0].model.as_deref(), Some("Opus"));
    assert_eq!(list[0].cwd.as_deref(), Some("/tmp/work"));
}

#[test]
fn reopen_recreates_tab_with_same_backing() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.backing = TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s7".into(),
        };
    }
    state.rename_tab(&tab_id, "Restored Chat").unwrap();
    state.close_tab(&tab_id).unwrap();

    let new_tab = state.reopen_closed_tab(&wsid, None).unwrap();
    assert_ne!(new_tab, tab_id, "fresh tab id");
    let tab = state.get_tab(&new_tab).expect("reopened Tab exists");
    assert_eq!(
        tab.backing,
        TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s7".into(),
        },
        "same backing on reopen"
    );
    assert!(tab.is_chat_backend());
    assert!(tab.restored_from_snapshot, "drives lazy reopen");
    assert_eq!(tab.title, "Restored Chat");
    // Reopened entry is consumed from history.
    assert!(state.list_recently_closed(&wsid).is_empty());
}

#[test]
fn reopen_chatbackend_tab_opens_in_chat() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.backing = TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s1".into(),
        };
    }
    state.rename_tab(&tab_id, "Chat").unwrap();
    state.close_tab(&tab_id).unwrap();

    let new_tab = state.reopen_closed_tab(&wsid, None).unwrap();
    let tab = state.get_tab(&new_tab).unwrap();
    assert!(tab.is_chat_backend(), "backing preserved on reopen");
    assert!(
        tab.chat_view,
        "a chatBackend Tab reopens in chat regardless of the chat-default pref"
    );
}

#[test]
fn reopen_cascade_group_activates_root_not_leaf() {
    // first, but the focused tab on reopen must be the cascade ROOT.
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let orch_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&orch_tab, "Orchestrator").unwrap();
    let w1_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&w1_tab, "Worker").unwrap();
    state.get_tab_mut(&w1_tab).unwrap().parent_tab_id = Some(orch_tab.clone());

    state.close_tab_for_team_close(&orch_tab).unwrap();
    let group_id = state.recent_closed_view()[0].group_id.clone();

    let (_ws, activated) = state.reopen_closed_group(&group_id).unwrap();
    let tab = state.get_tab(&activated).expect("activated Tab exists");
    assert_eq!(
        tab.title, "Orchestrator",
        "the cascade root is activated, not a leaf worker"
    );
    assert!(
        tab.parent_tab_id.is_none(),
        "activated Tab is the subtree root"
    );
}

#[test]
fn reopen_agent_tab_marks_for_resume() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.agent_type = Some("codex".into());
        p.agent_session_id = Some("cx-1".into());
    }
    state.close_tab(&tab_id).unwrap();

    let new_tab = state.reopen_closed_tab(&wsid, None).unwrap();
    let tab = state.get_tab(&new_tab).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("codex"));
    assert!(tab.agent_type_locked);
    assert_eq!(tab.agent_session_id.as_deref(), Some("cx-1"));
    assert!(tab.restored_from_snapshot);
    assert_eq!(tab.backing, TabBacking::Pty);
}

#[test]
fn reopen_agent_tab_preserves_terminal_mode() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.agent_type = Some("claude".into());
        p.chat_view = false;
    }
    state.close_tab(&tab_id).unwrap();

    let new_tab = state.reopen_closed_tab(&wsid, None).unwrap();
    assert!(
        !state.get_tab(&new_tab).unwrap().chat_view,
        "history restore must preserve terminal mode instead of reapplying chat default"
    );
}

#[test]
fn reopen_agent_tab_preserves_chat_mode() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.agent_type = Some("claude".into());
        p.chat_view = true;
    }
    state.close_tab(&tab_id).unwrap();

    let new_tab = state.reopen_closed_tab(&wsid, None).unwrap();
    assert!(
        state.get_tab(&new_tab).unwrap().chat_view,
        "history restore must preserve chat mode"
    );
}

#[test]
fn reopen_unsupported_provider_preserves_chat_mode_too() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.agent_type = Some("aider".into());
        p.chat_view = true;
    }
    state.close_tab(&tab_id).unwrap();

    let new_tab = state.reopen_closed_tab(&wsid, None).unwrap();
    assert!(
        state.get_tab(&new_tab).unwrap().chat_view,
        "history restore preserves the Tab's stored mode, not provider capability defaults"
    );
}

#[test]
fn reopen_cascade_group_preserves_each_tab_mode() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let orch_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&orch_tab, "Orchestrator").unwrap();
    {
        let tab = state.get_tab_mut(&orch_tab).unwrap();
        tab.agent_type = Some("claude".into());
        tab.chat_view = false;
    }
    let w1_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&w1_tab, "Worker").unwrap();
    let w = state.get_tab_mut(&w1_tab).unwrap();
    w.parent_tab_id = Some(orch_tab.clone());
    w.agent_type = Some("codex".into());
    w.chat_view = true;

    state.close_tab_for_team_close(&orch_tab).unwrap();
    let group_id = state.recent_closed_view()[0].group_id.clone();
    let closed_modes: Vec<(String, bool)> = state
        .recent_closed_view()
        .into_iter()
        .filter(|row| row.group_id == group_id)
        .filter_map(|row| {
            row.tab
                .agent_type
                .clone()
                .map(|agent| (agent, row.tab.chat_view))
        })
        .collect();
    assert!(
        closed_modes.contains(&("codex".to_string(), true)),
        "cascade history must store each child Tab mode"
    );

    let (_ws, _first) = state.reopen_closed_group(&group_id).unwrap();
    let restored: Vec<String> = state
        .list_workspaces()
        .iter()
        .find(|w| w.id == wsid)
        .map(|w| w.tab_ids.clone())
        .unwrap_or_default();
    let mut agent_tabs: Vec<(&str, bool)> = restored
        .iter()
        .filter_map(|pid| state.get_tab(pid))
        .filter(|p| p.agent_type.is_some())
        .map(|p| (p.agent_type.as_deref().unwrap_or(""), p.chat_view))
        .collect();
    agent_tabs.sort_by_key(|(provider, _)| *provider);
    assert_eq!(agent_tabs.len(), 2, "both agent Tabs restored");
    assert_eq!(agent_tabs, vec![("claude", false), ("codex", true)]);
}

#[test]
fn history_is_bounded_at_cap() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    for _ in 0..(RECENTLY_CLOSED_CAP + 5) {
        let tab_id = state.create_tab(&wsid).unwrap();
        state.close_tab(&tab_id).unwrap();
    }
    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), RECENTLY_CLOSED_CAP, "history bounded at cap");
}

#[test]
fn history_is_newest_first() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let t1 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t1, "first").unwrap();
    state.close_tab(&t1).unwrap();
    let t2 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t2, "second").unwrap();
    state.close_tab(&t2).unwrap();

    let list = state.list_recently_closed(&wsid);
    assert_eq!(list[0].title, "second", "newest first");
    assert_eq!(list[1].title, "first");
}

#[test]
fn reopen_by_index_selects_older_entry() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let t1 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t1, "older").unwrap();
    state.close_tab(&t1).unwrap();
    let t2 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t2, "newer").unwrap();
    state.close_tab(&t2).unwrap();

    // index 1 = the older "first"-closed entry.
    let new_tab = state.reopen_closed_tab(&wsid, Some(1)).unwrap();
    assert_eq!(state.get_tab(&new_tab).unwrap().title, "older");
    // The remaining entry is the newer one.
    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "newer");
}

#[test]
fn reopen_empty_history_errors() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let err = state.reopen_closed_tab(&wsid, None).unwrap_err();
    assert!(err.contains("No recently closed"));
}

#[test]
fn reopen_out_of_range_index_errors() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    state.close_tab(&tab_id).unwrap();
    let err = state.reopen_closed_tab(&wsid, Some(5)).unwrap_err();
    assert!(err.contains("index 5"));
}

#[test]
fn ring_cap_is_1000() {
    assert_eq!(RECENTLY_CLOSED_CAP, 1000, "history cap must be 1000");
}

#[test]
fn hidden_tab_is_not_recorded() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.hidden = true;
    }
    state.close_tab(&tab_id).unwrap();
    assert!(
        state.list_recently_closed(&wsid).is_empty(),
        "hidden worker Tab must not pollute the history"
    );
}

fn make_hidden_worker(state: &mut MultiplexerState, wsid: &str) -> String {
    let tab_id = state.create_tab(wsid).unwrap();
    let p = state.get_tab_mut(&tab_id).unwrap();
    p.hidden = true;
    p.worker_id = Some("wk-7".into());
    p.agent_type = Some("claude".into());
    p.agent_session_id = Some("sess-9".into());
    p.role_profile = Some("rust-backend-dev".into());
    p.agent_session_model = Some("Opus".into());
    p.cwd = Some("/tmp/lane-worktree".into());
    tab_id
}

#[test]
fn policy_close_of_hidden_worker_records_restorable_entry() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = make_hidden_worker(&mut state, &wsid);
    state
        .close_tab_with_reason(&tab_id, TabCloseReason::PolicyWorker)
        .unwrap();

    let list = state.list_recently_closed(&wsid);
    assert_eq!(list.len(), 1, "policy-driven worker close is recorded");
    let rec = &list[0];
    assert_eq!(rec.tab_id, tab_id);
    assert_eq!(rec.worker_id.as_deref(), Some("wk-7"), "task binding");
    assert_eq!(rec.agent_type.as_deref(), Some("claude"), "provider");
    assert_eq!(rec.agent_session_id.as_deref(), Some("sess-9"), "session");
    assert_eq!(rec.role_profile.as_deref(), Some("rust-backend-dev"));
    assert_eq!(rec.model.as_deref(), Some("Opus"));
    assert_eq!(
        rec.cwd.as_deref(),
        Some("/tmp/lane-worktree"),
        "lane worktree"
    );
}

#[test]
fn policy_close_of_hidden_utility_tab_stays_excluded() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = state.create_tab(&wsid).unwrap();
    if let Some(p) = state.get_tab_mut(&tab_id) {
        p.hidden = true; // no worker_id — an ordinary hidden utility shell
    }
    state
        .close_tab_with_reason(&tab_id, TabCloseReason::PolicyWorker)
        .unwrap();
    assert!(
        state.list_recently_closed(&wsid).is_empty(),
        "a hidden utility tab must not flood the ring even on a policy close"
    );
}

#[test]
fn user_close_of_hidden_worker_stays_excluded() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = make_hidden_worker(&mut state, &wsid);
    state.close_tab(&tab_id).unwrap();
    assert!(
        state.list_recently_closed(&wsid).is_empty(),
        "the hidden-skip is unchanged for a non-policy close"
    );
}

#[test]
fn reopen_policy_closed_worker_restores_binding() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let tab_id = make_hidden_worker(&mut state, &wsid);
    state
        .close_tab_with_reason(&tab_id, TabCloseReason::PolicyWorker)
        .unwrap();

    let new_tab = state.reopen_closed_tab(&wsid, None).unwrap();
    let tab = state.get_tab(&new_tab).expect("worker reopened");
    assert_eq!(tab.worker_id.as_deref(), Some("wk-7"), "task binding resumes");
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert!(tab.agent_type_locked, "provider re-locked for resume");
    assert_eq!(tab.agent_session_id.as_deref(), Some("sess-9"));
    assert_eq!(tab.cwd.as_deref(), Some("/tmp/lane-worktree"), "lane resumes");
    assert!(tab.restored_from_snapshot, "drives lazy reopen/resume");
    assert!(state.list_recently_closed(&wsid).is_empty(), "entry consumed");
}

// ── cascade grouping ─────────────────────────────────────────────────

#[test]
fn cascade_close_records_one_group_with_hierarchy() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);

    let orch_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&orch_tab, "Orchestrator").unwrap();
    let w1_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&w1_tab, "Worker 1").unwrap();
    state.get_tab_mut(&w1_tab).unwrap().parent_tab_id = Some(orch_tab.clone());
    let w2_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&w2_tab, "Worker 2").unwrap();
    state.get_tab_mut(&w2_tab).unwrap().parent_tab_id = Some(orch_tab.clone());

    state.close_tab_for_team_close(&orch_tab).unwrap();

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
    let worker_row = list
        .iter()
        .find(|e| e.title == "Worker 1")
        .expect("worker recorded");
    assert_eq!(worker_row.parent_tab_id.as_deref(), Some(orch_tab.as_str()));
}

#[test]
fn reopen_cascade_group_restores_subtree_and_relinks_parents() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);

    let orch_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&orch_tab, "Orchestrator").unwrap();
    let w1_tab = state.create_tab(&wsid).unwrap();
    state.rename_tab(&w1_tab, "Worker").unwrap();
    state.get_tab_mut(&w1_tab).unwrap().parent_tab_id = Some(orch_tab.clone());

    state.close_tab_for_team_close(&orch_tab).unwrap();

    let group_id = state.recent_closed_view()[0].group_id.clone();
    let (ws, first_tab) = state.reopen_closed_group(&group_id).unwrap();
    assert_eq!(ws, wsid, "cascade reopens into origin workspace");

    // Two tabs restored, parent link rebuilt across the new ids.
    let restored: Vec<String> = state
        .list_workspaces()
        .iter()
        .find(|w| w.id == wsid)
        .map(|w| w.tab_ids.clone())
        .unwrap_or_default();
    let child = restored
        .iter()
        .filter_map(|pid| state.get_tab(pid))
        .find(|p| p.title == "Worker")
        .expect("worker restored");
    let parent = restored
        .iter()
        .filter_map(|pid| state.get_tab(pid))
        .find(|p| p.title == "Orchestrator")
        .expect("orchestrator restored");
    assert_eq!(
        child.parent_tab_id.as_deref(),
        Some(parent.id.as_str()),
        "parent_tab_id relinked to the freshly minted orchestrator id"
    );
    assert!(state.get_tab(&first_tab).is_some());
    assert!(state.recent_closed_view().is_empty());
}

// ── workspace grouping ───────────────────────────────────────────────

#[test]
fn close_workspace_records_a_workspace_group_with_all_tabs() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    let t1 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t1, "Tab A").unwrap();
    let t2 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t2, "Tab B").unwrap();

    state.close_workspace(&wsid).unwrap();

    let view = state.recent_closed_view();
    assert!(view.iter().all(|v| v.group_kind == "workspace"));
    let group_ids: HashSet<&str> = view.iter().map(|v| v.group_id.as_str()).collect();
    assert_eq!(group_ids.len(), 1, "one workspace group");
    let titles: Vec<&str> = view.iter().map(|v| v.tab.title.as_str()).collect();
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
    let t1 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t1, "Tab A").unwrap();
    let t2 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t2, "Tab B").unwrap();

    state.close_workspace(&wsid).unwrap();
    let group_id = state.recent_closed_view()[0].group_id.clone();

    let (new_ws, _tab) = state.reopen_closed_group(&group_id).unwrap();
    let info = state
        .list_workspaces()
        .into_iter()
        .find(|w| w.id == new_ws)
        .expect("workspace recreated");
    assert_eq!(info.name, "Project");
    let titles: HashSet<String> = info
        .tab_ids
        .iter()
        .filter_map(|pid| state.get_tab(pid))
        .map(|p| p.title.clone())
        .collect();
    assert!(titles.contains("Tab A"), "got {:?}", titles);
    assert!(titles.contains("Tab B"), "got {:?}", titles);
}

#[test]
fn reopen_orphaned_tab_recreates_workspace() {
    let (mut state, _root) = MultiplexerState::new();
    let wsid = ws_id(&state);
    state.rename_workspace(&wsid, "Gone").unwrap();
    let t1 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t1, "Lonely").unwrap();

    state.close_workspace(&wsid).unwrap();

    let (new_ws, new_tab) = state.reopen_orphaned_tab(&t1).unwrap();
    assert_ne!(new_ws, wsid);
    assert_eq!(state.get_tab(&new_tab).unwrap().title, "Lonely");
}

#[test]
fn reopen_workspace_group_matches_existing_workspace_by_path() {
    let (mut state, _root) = MultiplexerState::new();
    // A workspace rooted at /tmp/proj, with a tab, then fully closed.
    let wsid = state
        .create_workspace("Proj".into(), Some("/tmp/proj".into()))
        .id;
    let t1 = state.create_tab(&wsid).unwrap();
    state.rename_tab(&t1, "Tab A").unwrap();
    state.close_workspace(&wsid).unwrap();
    let group_id = state.recent_closed_view()[0].group_id.clone();

    // must still match via normalize_cwd).
    let live_id = state
        .create_workspace("Proj Again".into(), Some("/tmp/proj/".into()))
        .id;
    let count_before = state.list_workspaces().len();

    let (target_ws, _tab) = state.reopen_closed_group(&group_id).unwrap();
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
        .tab_ids
        .iter()
        .filter_map(|pid| state.get_tab(pid))
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
    let tab_id = state.create_tab(&wsid).unwrap();
    state.rename_tab(&tab_id, "Solo").unwrap();
    state.close_tab(&tab_id).unwrap();

    let view = state.recent_closed_view();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].group_kind, "single");
    assert_eq!(view[0].index, 0);
    assert!(!view[0].orphaned);
}
