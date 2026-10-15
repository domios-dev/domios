use codeterm_service_core::service::tab::RespondPromptOutcome;

#[tokio::test]
async fn respond_prompt_outcome_maps_not_on_screen_to_conflict() {
    let response =
        crate::agent_routes::respond_prompt_response(RespondPromptOutcome::PromptNotOnScreen);
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("response JSON"),
        serde_json::json!({"ok": false, "reason": "prompt_not_on_screen"})
    );

    let invalid = crate::agent_routes::respond_prompt_response(RespondPromptOutcome::InvalidOption);
    assert_eq!(invalid.status(), axum::http::StatusCode::BAD_REQUEST);
}
