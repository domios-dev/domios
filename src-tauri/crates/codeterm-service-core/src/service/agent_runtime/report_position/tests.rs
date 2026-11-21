use super::*;
use crate::{
    agent::chat_types::ChatMsg,
    multiplexer::state::MultiplexerState,
    service::{agent_runtime::engine::test_support, test_fixture::ServiceFixture},
};

#[test]
fn sender_position_tracks_bound_cache_and_refuses_mismatched_or_empty_runtime() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("acp".into());
    tab.cwd = Some("/workspace".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    let tab = tab.clone();
    let fixture = ServiceFixture::new(mux);
    let runtime = test_support::live_runtime(&tab);
    let session = engine::for_tab(&tab_id).unwrap();

    assert_eq!(sender_position(&fixture.service, &tab_id), None);

    session.cache.lock().reconcile(ChatMsg {
        id: "untimed-ack".into(),
        kind: "assistant".into(),
        content: "acknowledging".into(),
        ..Default::default()
    });
    let captured = sender_position(&fixture.service, &tab_id);
    assert_eq!(
        captured,
        Some(crate::agent::reports::ReportSenderPosition {
            source_session_id: "own-session".into(),
            message_id: "untimed-ack".into(),
        })
    );

    session.cache.lock().reconcile(ChatMsg {
        id: "later-tool".into(),
        kind: "tool_call".into(),
        content: "running".into(),
        ..Default::default()
    });
    assert_eq!(
        sender_position(&fixture.service, &tab_id),
        Some(crate::agent::reports::ReportSenderPosition {
            source_session_id: "own-session".into(),
            message_id: "later-tool".into(),
        })
    );

    {
        let mut mux = fixture.service.mux.write();
        mux.get_tab_mut(&tab_id).unwrap().session_binding_nonce = Some("replaced-nonce".into());
    }
    assert_eq!(sender_position(&fixture.service, &tab_id), None);

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("own-nonce".into());
        tab.agent_session_id = Some("replaced-session".into());
    }
    assert_eq!(sender_position(&fixture.service, &tab_id), None);

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("own-session".into());
        tab.cwd = Some("/other-workspace".into());
    }
    assert_eq!(sender_position(&fixture.service, &tab_id), None);

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/workspace".into());
        tab.agent_type = Some("other-provider".into());
    }
    assert_eq!(sender_position(&fixture.service, &tab_id), None);

    {
        let mut mux = fixture.service.mux.write();
        mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("acp".into());
    }
    drop(runtime);
    assert_eq!(sender_position(&fixture.service, &tab_id), None);
}
