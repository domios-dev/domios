use super::*;

#[tokio::test]
async fn general_file_send_keeps_its_authenticated_caller_and_rejects_a_rotated_queued_nonce() {
    let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&general).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("general-session".into());
    tab.session_binding_nonce = Some("current-nonce".into());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
    tab.role_profile = Some(crate::service::general_agent::GENERAL_AGENT_KIND.into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let (sender, mut requests) = mpsc::channel::<Request>(1);
    *service.neighbor_cell().owner.lock() = Some(sender);
    let send = service.neighbor_files(
        json!({"operation":"send","friendship_id":"a".repeat(64),
        "kind":"question","body":"","paths":["opaque-file"]}),
        Some(&general),
        Some("current-nonce"),
    );
    tokio::pin!(send);
    let request = tokio::select! {
        result = &mut send => panic!("General file send failed before its owner queue: {result:?}"),
        request = requests.recv() => request.unwrap(),
    };
    assert!(
        matches!(&request.operation,Operation::Files { caller:Some(id),nonce:Some(nonce),.. }
        if id == &general && nonce == "current-nonce")
    );
    {
        let mut mux = service.mux.write();
        let tab = mux.get_tab_mut(&general).unwrap();
        tab.session_binding_nonce = Some("rotated-nonce".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    let result = request
        .port
        .execute(request.generation, request.operation)
        .await;
    assert!(result
        .as_ref()
        .err()
        .is_some_and(|error| error.contains("authenticated General")));
    request
        .reply
        .send(result)
        .unwrap_or_else(|_| panic!("General abandoned its queued operation"));
    assert!(send.await.unwrap_err().contains("authenticated General"));
}
