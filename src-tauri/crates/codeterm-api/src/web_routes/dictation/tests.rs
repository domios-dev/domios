use super::*;

#[test]
fn dictation_requires_explicit_normalized_session_and_frontend_ids() {
    let value = "a2a9f17e-cb7c-4ba1-9a1b-7099ae215153";
    let mut headers = HeaderMap::new();
    assert!(client(&headers, value).is_err());
    headers.insert("x-codeterm-client-id", value.parse().unwrap());
    assert_eq!(client(&headers, value).unwrap(), value);
    assert!(client(&headers, "some-other-session").is_err());
    assert!(client(&headers, &value.to_ascii_uppercase()).is_err());
}

mod origin_guard {
    use std::net::SocketAddr;

    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
    };
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use tower::ServiceExt;

    use crate::auth::{AuthenticatedOrigin, CallerIdentity};

    const SESSION: &str = "a2a9f17e-cb7c-4ba1-9a1b-7099ae215153";
    const TAILNET_PEER: &str = "100.64.0.7:52100";
    const LOOPBACK_PEER: &str = "127.0.0.1:52100";
    const SERVE_HOST: &str = "blackstar.tail0.ts.net";

    struct Call<'a> {
        caller: CallerIdentity,
        origin_kind: AuthenticatedOrigin,
        peer: &'a str,
        origin: Option<&'a str>,
    }

    fn owner<'a>(peer: &'a str, origin: Option<&'a str>) -> Call<'a> {
        Call {
            caller: CallerIdentity::Local,
            origin_kind: AuthenticatedOrigin::Owner,
            peer,
            origin,
        }
    }

    // Requests without a frontend identity stop at the handler with 400, proving the guard admitted them.
    async fn status(path: &str, call: Call<'_>) -> StatusCode {
        let fixture = ServiceFixture::new(MultiplexerState::new().0);
        let app = super::super::router().with_state(crate::test_support::app_state(&fixture));
        let mut request = Request::post(path)
            .header("host", SERVE_HOST)
            .header("content-type", "application/json");
        if let Some(origin) = call.origin {
            request = request.header("origin", origin);
        }
        let body = format!(r#"{{"session":"{SESSION}","tabId":"tab"}}"#);
        let mut request = request.body(Body::from(body)).unwrap();
        request.extensions_mut().insert(call.caller);
        request.extensions_mut().insert(call.origin_kind);
        request
            .extensions_mut()
            .insert(ConnectInfo(call.peer.parse::<SocketAddr>().unwrap()));
        app.oneshot(request).await.unwrap().status()
    }

    fn paths() -> [String; 4] {
        [
            "/api/dictation/start".into(),
            format!("/api/dictation/{SESSION}/frame/1"),
            format!("/api/dictation/{SESSION}/finish/2"),
            format!("/api/dictation/{SESSION}/cancel"),
        ]
    }

    #[tokio::test]
    async fn direct_https_same_origin_owner_and_loopback_reach_dictation() {
        let same_origin = format!("https://{SERVE_HOST}");
        for path in paths() {
            for call in [
                owner(TAILNET_PEER, Some(&same_origin)),
                owner(LOOPBACK_PEER, None),
                owner(LOOPBACK_PEER, Some("http://127.0.0.1:7686")),
                owner(LOOPBACK_PEER, Some("tauri://localhost")),
            ] {
                let status = status(&path, call).await;
                assert!(
                    matches!(
                        status,
                        StatusCode::BAD_REQUEST | StatusCode::UNSUPPORTED_MEDIA_TYPE
                    ),
                    "{path}: {status}"
                );
            }
        }
    }

    #[tokio::test]
    async fn insecure_cross_origin_relayed_and_non_owner_dictation_is_refused() {
        let same_origin = format!("https://{SERVE_HOST}");
        let insecure = format!("http://{SERVE_HOST}");
        for path in paths() {
            for call in [
                owner(TAILNET_PEER, Some(&insecure)),
                owner(TAILNET_PEER, Some("https://attacker.example")),
                owner(TAILNET_PEER, None),
                owner(TAILNET_PEER, Some("http://127.0.0.1:7686")),
                owner(TAILNET_PEER, Some("null")),
                Call {
                    caller: CallerIdentity::Relayed,
                    ..owner(TAILNET_PEER, Some(&same_origin))
                },
                Call {
                    origin_kind: AuthenticatedOrigin::Agent,
                    ..owner(LOOPBACK_PEER, None)
                },
            ] {
                assert_eq!(status(&path, call).await, StatusCode::FORBIDDEN, "{path}");
            }
        }
    }
}
