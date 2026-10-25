use super::*;

#[tokio::test]
async fn saved_solution_show_route_returns_document_and_refuses_foreign_workspace() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use codeterm_blueprint_types::{blueprint_document::Document, blueprint_memory::Evidence};
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    use serde_json::json;
    use tower::ServiceExt;

    let (mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let fixture = ServiceFixture::new(mux);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let document = Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"saved","inputs":{"n":{"type":"integer"}},"steps":[{"id":"value","code":{"lang":"python","src":"def main(p): return 8"},"in":{"type":"integer"},"out":{"type":"integer"}}],"result":{"type":{"type":"integer"},"from":"value.out"}})).unwrap();
    let saved = fixture
        .service
        .mem
        .save_blueprint_scoped(
            &format!("blueprints:{workspace}"),
            "team",
            document,
            &["Return fixed data".into()],
            Evidence {
                kept: true,
                ..Default::default()
            },
        )
        .unwrap();
    let router = crate::routes::create_router(state);
    for (workspace_id, authorized, expected) in [
        (workspace.as_str(), true, StatusCode::OK),
        ("foreign-workspace", true, StatusCode::BAD_REQUEST),
        (workspace.as_str(), false, StatusCode::UNAUTHORIZED),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/automation/blueprints/solutions/show")
                    .header(
                        "authorization",
                        if authorized {
                            "Bearer preview-test-token"
                        } else {
                            "Bearer wrong-token"
                        },
                    )
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"workspace_id":workspace_id,"id":saved.id}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await.unwrap())
                    .unwrap();
            assert_eq!(body["document"]["name"], "saved");
            assert_eq!(body["expanded_calls"], json!([]));
        }
    }
}
#[test]
fn human_confirmation_is_target_specific_and_cannot_override_a_tab() {
    let mut headers = HeaderMap::new();
    assert!(confirmed(&headers, "job").is_err());
    headers.insert(
        "x-codeterm-blueprint-confirmation",
        "other".parse().unwrap(),
    );
    assert!(confirmed(&headers, "job").is_err());
    headers.insert("x-codeterm-blueprint-confirmation", "job".parse().unwrap());
    assert!(confirmed(&headers, "job").is_ok());
    headers.insert("x-codeterm-tab-id", "worker".parse().unwrap());
    assert!(confirmed(&headers, "job").is_err());
}
