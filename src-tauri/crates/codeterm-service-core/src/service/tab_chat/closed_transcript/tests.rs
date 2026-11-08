// WHAT: resolving a closed tab's transcript from its record.
// WHY: host-kept sessions have no provider store to fall back on once closed.
// ORACLE: every retained row returns in order; an absent one is honestly missing.
use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::tab_chat::chatbackend_history::ChatBackendHistory;
use crate::service::test_fixture::ServiceFixture;

fn row(id: &str, seq: i64, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        seq,
        kind: "assistant".into(),
        content: content.into(),
        ..Default::default()
    }
}

fn record(provider: &str, session: &str) -> ClosedTranscript {
    ClosedTranscript {
        tab_id: "closed-tab".into(),
        provider: provider.into(),
        session_id: session.into(),
        cwd: String::new(),
        chat_backend: false,
        parent_tab_id: None,
        team_id: None,
        title: String::new(),
        role: None,
        model: None,
        closed_at: 0,
        archives: Vec::new(),
    }
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

#[test]
fn a_host_kept_session_reads_every_retained_row_in_order() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let (provider, session) = (unique("closed-provider"), unique("session"));
    ChatBackendHistory::open_at(chatbackend_history::archive_path(&provider, &session))
        .try_upsert(&[row("one", 0, "first"), row("two", 1, "second")])
        .unwrap();
    let all = snapshot(&fixture.service, &record(&provider, &session), 0).unwrap();
    assert_eq!(all.count, 2);
    assert!(!all.truncated);
    assert_eq!(
        all.messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let tail = snapshot(&fixture.service, &record(&provider, &session), 1).unwrap();
    assert_eq!(tail.count, 2);
    assert_eq!(tail.messages.len(), 1);
    assert_eq!(tail.messages[0].id, "two");
}

#[test]
fn a_session_with_no_retained_transcript_is_missing_not_empty() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let error = snapshot(
        &fixture.service,
        &record(&unique("removed-provider"), &unique("session")),
        0,
    )
    .err()
    .unwrap();
    assert_eq!(
        StoredHistoryFailure::from_error(&error),
        Some(StoredHistoryFailure::Missing)
    );
}

#[test]
fn a_record_without_a_session_reads_empty() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let read = snapshot(&fixture.service, &record("provider", ""), 0).unwrap();
    assert_eq!(read.count, 0);
}

#[test]
fn retention_removes_only_session_logs_no_live_tab_resumed() {
    let (mut mux, root) = MultiplexerState::new();
    let (provider, gone, resumed) = (unique("closed-provider"), unique("gone"), unique("resumed"));
    mux.get_tab_mut(&root).unwrap().agent_session_id = Some(resumed.clone());
    let store = crate::multiplexer::state::ClosedTabsStore::in_memory();
    mux.attach_closed_store(store.clone());
    let fixture = ServiceFixture::new(mux);
    let paths = [&gone, &resumed].map(|session| {
        let path = chatbackend_history::archive_path(&provider, session);
        ChatBackendHistory::open_at(path.clone())
            .try_upsert(&[row("one", 0, "body")])
            .unwrap();
        path
    });
    let filler: Vec<ClosedTranscript> = (0..=crate::multiplexer::state::CLOSED_GROUP_CAP)
        .map(|index| ClosedTranscript {
            tab_id: format!("filler-{index}"),
            closed_at: index as u64 + 10,
            ..record(&provider, &format!("filler-session-{index}"))
        })
        .collect();
    store
        .insert_transcripts(&[
            ClosedTranscript {
                tab_id: "gone-tab".into(),
                closed_at: 1,
                ..record(&provider, &gone)
            },
            ClosedTranscript {
                tab_id: "resumed-tab".into(),
                closed_at: 2,
                ..record(&provider, &resumed)
            },
        ])
        .unwrap();
    store.insert_transcripts(&filler).unwrap();
    forget_evicted(&fixture.service);
    assert!(!paths[0].exists());
    assert!(paths[1].exists());
}
