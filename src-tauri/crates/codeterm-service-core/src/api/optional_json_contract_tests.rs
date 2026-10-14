mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::post,
        Router,
    };
    use tower::ServiceExt;

    use crate::api_support::optional_json::OptionalJson;

    async fn optional_body<T>(OptionalJson(body): OptionalJson<T>) -> StatusCode {
        if body.is_some() {
            StatusCode::NO_CONTENT
        } else {
            StatusCode::OK
        }
    }

    async fn required_workspace_body(
        OptionalJson(body): OptionalJson<crate::api::workspace_routes::CreateWorkspaceBody>,
    ) -> Result<StatusCode, (StatusCode, &'static str)> {
        body.ok_or((StatusCode::BAD_REQUEST, "name is required"))?;
        Ok(StatusCode::NO_CONTENT)
    }

    async fn axum_default_optional_body(
        _: Option<axum::Json<crate::api::workspace_routes::CloseWorkspaceBody>>,
    ) -> StatusCode {
        StatusCode::OK
    }

    fn endpoint<T>() -> Router
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        Router::new().route("/", post(optional_body::<T>))
    }

    #[tokio::test]
    async fn optional_body_contract_accepts_empty_and_json_bodies_at_every_optional_site() {
        let sites: Vec<(&str, Router)> = vec![
            (
                "agent restart",
                endpoint::<crate::api::agent_restart_routes::AgentRestartBody>(),
            ),
            ("hook permission", endpoint::<serde_json::Value>()),
            ("hook stop", endpoint::<serde_json::Value>()),
            ("hook session start", endpoint::<serde_json::Value>()),
            (
                "serve enable",
                endpoint::<crate::api::network_routes::ServeEnableBody>(),
            ),
            (
                "keepawake start",
                endpoint::<crate::api::network_routes::KeepAwakeStartBody>(),
            ),
            (
                "resume session",
                endpoint::<crate::api::session_routes::ResumeSessionBody>(),
            ),
            (
                "close workspace",
                endpoint::<crate::api::workspace_routes::CloseWorkspaceBody>(),
            ),
            (
                "reopen closed",
                endpoint::<crate::api::workspace_routes::ReopenClosedBody>(),
            ),
            (
                "create tab",
                endpoint::<crate::api::workspace_routes::CreateTabBody>(),
            ),
        ];

        for (site, router) in sites {
            let no_body = router
                .clone()
                .oneshot(Request::post("/").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                no_body.status(),
                StatusCode::OK,
                "{site} must accept no body"
            );

            let json_body = router
                .oneshot(
                    Request::post("/")
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                json_body.status(),
                StatusCode::NO_CONTENT,
                "{site} must preserve valid JSON handling"
            );
        }
    }

    #[tokio::test]
    async fn optional_body_contract_required_workspace_body_names_the_missing_field() {
        let no_body = Router::new()
            .route("/", post(required_workspace_body))
            .oneshot(Request::post("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(no_body.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(no_body.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(std::str::from_utf8(&body).unwrap().contains("name"));

        let valid_body = Router::new()
            .route("/", post(required_workspace_body))
            .oneshot(
                Request::post("/")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"workspace"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid_body.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn optional_body_contract_axum_option_json_rejects_an_empty_json_request() {
        let response = Router::new()
            .route("/", post(axum_default_optional_body))
            .oneshot(
                Request::post("/")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
