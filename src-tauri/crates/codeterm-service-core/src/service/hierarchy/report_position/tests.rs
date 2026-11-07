use super::*;
use crate::{
    agent::chat_types::ChatMsg,
    multiplexer::state::MultiplexerState,
    service::{
        agent_runtime::engine::{self, test_support},
        test_fixture::ServiceFixture,
    },
};

#[test]
fn report_position_keeps_the_untimed_source_row_before_final_text() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    let tab = tab.clone();
    let fixture = ServiceFixture::new(mux);
    let _runtime = test_support::live_runtime(&tab);
    let session = engine::for_tab(&tab_id).unwrap();
    session.cache.lock().reconcile(ChatMsg {
        id: "report-tool".into(),
        kind: "tool_call".into(),
        ..Default::default()
    });
    let position = sender_position(&fixture.service, &tab_id);
    session.cache.lock().reconcile(ChatMsg {
        id: "final-text".into(),
        kind: "assistant".into(),
        content: "finished".into(),
        ..Default::default()
    });
    assert_eq!(
        position,
        json!({"source_session_id":"own-session", "message_id":"report-tool"})
    );
    assert_eq!(
        sender_position(&fixture.service, &tab_id)["message_id"],
        json!("final-text")
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .session_binding_nonce = Some("new-nonce".into());
    assert_eq!(sender_position(&fixture.service, &tab_id), Value::Null);
}
