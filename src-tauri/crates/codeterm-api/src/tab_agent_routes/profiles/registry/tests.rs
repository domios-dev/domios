use crate::auth::CallerIdentity;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Extension, Router,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState,
    service::{tab_credentials::TabCaller, test_fixture::ServiceFixture},
};
use tower::ServiceExt;

#[tokio::test]
async fn acp_management_routes_require_owner_identity_before_core_validation() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let routes = crate::tab_agent_routes::mount(Router::new())
        .with_state(crate::test_support::app_state(&fixture));
    for caller in [
        Some(CallerIdentity::Local),
        Some(CallerIdentity::Tab(TabCaller {
            tab_id: "worker".into(),
            binding_nonce: "fixture".into(),
        })),
        Some(CallerIdentity::Peer {
            instance_id: "peer".into(),
            access_level: codeterm_service_core::mesh::types::AccessLevel::Full,
            relay_protocol_version: 1,
        }),
        Some(CallerIdentity::ServicePush {
            service_id: "push".into(),
        }),
        Some(CallerIdentity::Relayed),
        None,
    ] {
        for (method, path, body) in [
            (
                "GET",
                format!("/api/agents/providers/acp/registry?q={}", "x".repeat(257)),
                "",
            ),
            (
                "POST",
                "/api/agents/providers/acp/registry/install".into(),
                r#"{"id":"invalid!"}"#,
            ),
            (
                "DELETE",
                "/api/agents/providers/acp/profiles/invalid!".into(),
                "",
            ),
        ] {
            let owner = matches!(&caller, Some(CallerIdentity::Local));
            let mut router = routes.clone();
            if let Some(identity) = caller.clone() {
                router = router.layer(Extension(identity));
            }
            let response = router
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if owner {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::FORBIDDEN
                },
                "{method}"
            );
            let text = String::from_utf8(
                axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            if owner {
                assert!(!text.contains("registry unavailable"));
            } else {
                assert!(text.contains("local-owner only"));
            }
        }
    }
}
