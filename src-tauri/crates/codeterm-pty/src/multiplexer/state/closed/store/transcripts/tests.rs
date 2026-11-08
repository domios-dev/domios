// WHAT: which closed tabs keep a transcript index row, and who may read one.
// WHY: an ACP Worker's transcript became unreachable once its tab closed.
// ORACLE: every session-holding agent tab is indexed, shells are not, and the
// read authority stays the parent or Team while retention stays bounded.
use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TabCloseReason;

fn agent_tab(state: &mut MultiplexerState, workspace: &str, session: &str) -> String {
    let id = state.create_tab(workspace).unwrap();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some(session.into());
    tab.cwd = Some("/opaque/cwd".into());
    id
}

fn workspace(state: &MultiplexerState) -> String {
    state.list_workspaces()[0].id.clone()
}

fn record(tab: &str, session: &str, closed_at: u64) -> ClosedTranscript {
    ClosedTranscript {
        tab_id: tab.into(),
        provider: "opaque-provider".into(),
        session_id: session.into(),
        cwd: String::new(),
        chat_backend: false,
        parent_tab_id: Some("parent-tab".into()),
        team_id: Some("team-a".into()),
        title: "Worker".into(),
        role: None,
        model: None,
        closed_at,
        archives: Vec::new(),
    }
}

#[test]
fn a_closed_agent_tab_is_indexed_and_a_shell_is_not() {
    let (mut state, _first) = MultiplexerState::new();
    let workspace = workspace(&state);
    let agent = agent_tab(&mut state, &workspace, "session-a");
    let shell = state.create_tab(&workspace).unwrap();
    state.close_tab(&agent).unwrap();
    state.close_tab(&shell).unwrap();
    let reader = state.closed_transcripts();
    let found = reader.by_tab(&agent).unwrap().unwrap();
    assert_eq!(found.session_id, "session-a");
    assert_eq!(found.provider, "opaque-provider");
    assert!(reader.by_tab(&shell).unwrap().is_none());
}

#[test]
fn hidden_agents_stay_out_of_reopen_history_yet_keep_a_transcript() {
    let (mut state, _first) = MultiplexerState::new();
    let workspace = workspace(&state);
    let hidden = agent_tab(&mut state, &workspace, "session-hidden");
    state.get_tab_mut(&hidden).unwrap().hidden = true;
    state
        .close_tab_with_reason(&hidden, TabCloseReason::AgentRunEnded)
        .unwrap();
    assert!(state
        .list_recently_closed(&workspace)
        .iter()
        .all(|closed| closed.tab_id != hidden));
    assert!(state
        .closed_transcripts()
        .by_tab(&hidden)
        .unwrap()
        .is_some());
}

#[test]
fn a_workspace_close_indexes_each_agent_tab() {
    let (mut state, _first) = MultiplexerState::new();
    let closing = state.create_workspace("Closing".into(), None).id;
    let agent = agent_tab(&mut state, &closing, "session-workspace");
    state.close_workspace(&closing).unwrap();
    assert_eq!(
        state
            .closed_transcripts()
            .by_tab(&agent)
            .unwrap()
            .unwrap()
            .session_id,
        "session-workspace"
    );
}

#[test]
fn a_deferred_close_is_readable_before_and_after_its_batch_is_written() {
    let (mut state, _first) = MultiplexerState::new();
    state.attach_closed_store(ClosedTabsStore::in_memory());
    let workspace = workspace(&state);
    let agent = agent_tab(&mut state, &workspace, "session-deferred");
    state.close_tab(&agent).unwrap();
    assert!(state.closed_transcripts().by_tab(&agent).unwrap().is_some());
    assert!(state
        .closed_store
        .transcript_by_tab(&agent)
        .unwrap()
        .is_none());
    state.take_closed_history_batch().unwrap().persist();
    assert!(state
        .closed_store
        .transcript_by_tab(&agent)
        .unwrap()
        .is_some());
    assert!(state.closed_transcripts().by_tab(&agent).unwrap().is_some());
}

#[test]
fn closed_read_authority_is_the_parent_or_the_team_and_nobody_else() {
    let closed = record("worker-tab", "session", 1);
    assert!(closed.readable_by("parent-tab", None));
    assert!(closed.readable_by("sibling-tab", Some("team-a")));
    assert!(!closed.readable_by("stranger-tab", None));
    assert!(!closed.readable_by("stranger-tab", Some("team-b")));
    assert!(!closed.readable_by("stranger-tab", Some("")));
    assert!(!closed.readable_by("worker-tab", None));
    let solo = ClosedTranscript {
        parent_tab_id: None,
        team_id: None,
        ..record("solo-tab", "session", 1)
    };
    assert!(!solo.readable_by("parent-tab", Some("team-a")));
    assert!(!solo.readable_by("", None));
}

#[test]
fn short_ids_resolve_only_when_unique_and_long_enough() {
    let store = ClosedTabsStore::in_memory();
    store
        .insert_transcripts(&[
            record("abcdef12-one", "session-one", 1),
            record("abcdef13-two", "session-two", 2),
            record("abcdef13-three", "session-three", 3),
        ])
        .unwrap();
    let found = store.transcript_by_tab("abcdef12").unwrap().unwrap();
    assert_eq!(found.tab_id, "abcdef12-one");
    assert!(store.transcript_by_tab("abcdef13").unwrap().is_none());
    assert!(store.transcript_by_tab("abc").unwrap().is_none());
    assert!(store.transcript_by_tab("").unwrap().is_none());
}

#[test]
fn closed_children_list_only_that_parent_newest_first() {
    let store = ClosedTabsStore::in_memory();
    let foreign = ClosedTranscript {
        parent_tab_id: Some("other-parent".into()),
        ..record("foreign", "session-foreign", 9)
    };
    store
        .insert_transcripts(&[
            record("older", "session-older", 1),
            record("newer", "session-newer", 2),
            foreign,
        ])
        .unwrap();
    let ids: Vec<_> = store
        .closed_children("parent-tab", 10)
        .unwrap()
        .into_iter()
        .map(|child| child.tab_id)
        .collect();
    assert_eq!(ids, ["newer", "older"]);
}

#[test]
fn retention_is_bounded_and_reports_only_sessions_nothing_still_names() {
    let store = ClosedTabsStore::in_memory_with_caps(10, 2);
    store
        .insert_transcripts(&[
            record("evicted", "session-gone", 1),
            record("shared-old", "session-shared", 2),
        ])
        .unwrap();
    store
        .insert_transcripts(&[
            record("shared-new", "session-shared", 3),
            record("newest", "session-newest", 4),
        ])
        .unwrap();
    assert!(store.transcript_by_tab("evicted").unwrap().is_none());
    assert!(store.transcript_by_tab("shared-old").unwrap().is_none());
    assert!(store.transcript_by_tab("newest").unwrap().is_some());
    let evicted: Vec<_> = store
        .take_evicted_transcripts()
        .into_iter()
        .map(|record| record.session_id)
        .collect();
    assert_eq!(evicted, ["session-gone"]);
    assert!(store.take_evicted_transcripts().is_empty());
}
