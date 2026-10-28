use super::*;

#[tokio::test]
async fn owner_visibility_receipts_refuse_missing_or_tab_callers_before_service_access() {
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    for caller in [None, Some(Extension(CallerIdentity::Tab(codeterm_service_core::service::tab_credentials::TabCaller { tab_id:"opaque-tab".into(),binding_nonce:"opaque-nonce".into() })))] {
        let response = crate::account_routes::neighbor_read(State(state.clone()),caller,Json(NeighborRead { friendship_id:"a".repeat(64),through_message_id:"b".repeat(32) })).await.unwrap_err();
        assert_eq!(response.status(),StatusCode::FORBIDDEN);
    }
    let response = crate::account_routes::neighbor_read(State(state),Some(Extension(CallerIdentity::Local)),Json(NeighborRead { friendship_id:"../foreign".into(),through_message_id:"b".repeat(32) })).await.unwrap_err();
    assert_eq!(response.status(),StatusCode::BAD_REQUEST);
}

#[test]
fn owner_visibility_input_rejects_unknown_fields() {
    assert!(serde_json::from_value::<NeighborRead>(serde_json::json!({"friendship_id":"a".repeat(64),"through_message_id":"b".repeat(32),"caller_tab_id":"foreign"})).is_err());
}
