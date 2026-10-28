use super::*;

#[tokio::test]
async fn concierge_cannot_upload_open_applications_or_change_file_limits() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    for command in [
        json!({"operation":"send","friendship_id":"a".repeat(64),"kind":"question","body":"opaque","in_reply_to":null,"paths":["local"]}),
        json!({"operation":"open_folder","friendship_id":"a".repeat(64)}),
        json!({"operation":"usage","friendship_id":"a".repeat(64)}),
        json!({"operation":"set_daily_limit","friendship_id":"a".repeat(64),"file_daily_limit_bytes":null}),
    ] {
        let error = fixture
            .service
            .neighbor_files(command, Some("missing"), Some("nonce"))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            "neighbor_forbidden: This concierge cannot manage Neighbor files"
        );
    }
}

#[tokio::test]
async fn file_operation_rejects_traversal_friendship_before_local_access() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    for friendship in ["../escape", "NUL", "", "/absolute"] {
        let error = fixture
            .service
            .neighbor_files(
                json!({"operation":"list","friendship_id":friendship}),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(error.starts_with("invalid_request:"));
    }
}
