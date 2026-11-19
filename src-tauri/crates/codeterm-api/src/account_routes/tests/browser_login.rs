use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use codeterm_account::{
    Account, CredentialStore, IdentityProvider, LoginFlow, LoginStart, Redemption,
    Result as AccountResult, Secret, Tokens,
};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

struct PendingLogin;

#[async_trait::async_trait]
impl IdentityProvider for PendingLogin {
    fn id(&self) -> &str {
        "pending-login-fixture"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(&self, _: &LoginFlow) -> AccountResult<LoginStart> {
        std::future::pending().await
    }
    async fn redeem(&self, _: Redemption) -> AccountResult<Tokens> {
        unreachable!()
    }
    async fn refresh(&self, _: &Secret) -> AccountResult<Tokens> {
        unreachable!()
    }
    async fn revoke(&self, _: &Secret) -> AccountResult<()> {
        unreachable!()
    }
}

#[async_trait::async_trait]
impl CredentialStore for PendingLogin {
    async fn load(&self, _: &str) -> AccountResult<Option<Secret>> {
        Ok(None)
    }
    async fn save(&self, _: &str, _: &Secret) -> AccountResult<()> {
        unreachable!()
    }
    async fn delete(&self, _: &str) -> AccountResult<()> {
        unreachable!()
    }
}

#[tokio::test]
async fn account_routes_pending_sign_in_does_not_hold_boot_or_readiness() {
    let fixture = codeterm_neighbors_service::test_support::fixture(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    let account = Arc::new(Account::new(
        Arc::new(PendingLogin),
        Arc::new(PendingLogin),
        Duration::from_secs(60),
    ));
    assert!(fixture
        .service
        .ext::<codeterm_neighbors_service::service::account::AccountCell>()
        .0
        .set(account)
        .is_ok());
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let router = crate::routes::create_router(state);
    let request = |method, path: String, body: Body| {
        Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", "Bearer preview-test-token")
            .header("content-type", "application/json")
            .body(body)
            .unwrap()
    };
    let mut events = fixture.service.events.subscribe();
    tokio::time::timeout(Duration::from_secs(2), async {
        let start = router
            .clone()
            .oneshot(request("POST", "/api/account/login".into(), Body::empty()))
            .await
            .unwrap();
        assert_eq!(start.status(), StatusCode::ACCEPTED);
        let attempt: serde_json::Value =
            serde_json::from_slice(&to_bytes(start.into_body(), 4096).await.unwrap()).unwrap();
        let id = attempt["attempt_id"].as_str().unwrap();

        let bootstrap = router
            .clone()
            .oneshot(request(
                "GET",
                "/api/frontend/bootstrap".into(),
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(bootstrap.status(), StatusCode::OK);
        let boot: serde_json::Value =
            serde_json::from_slice(&to_bytes(bootstrap.into_body(), 65536).await.unwrap()).unwrap();
        let ready = router
            .clone()
            .oneshot(request(
                "POST",
                "/api/frontend/ready".into(),
                Body::from(serde_json::json!({"generation": boot["generation"]}).to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(ready.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(ready.into_body(), 4096).await.unwrap().as_ref(),
            b"true"
        );
        let status = router
            .clone()
            .oneshot(request("GET", "/api/account/status".into(), Body::empty()))
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        let pending: serde_json::Value =
            serde_json::from_slice(&to_bytes(status.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(pending["login_attempt_id"], id);
        assert_eq!(pending["pending"], true);

        let wrong = router
            .clone()
            .oneshot(request(
                "POST",
                "/api/account/login/0/cancel".into(),
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::CONFLICT);
        let cancel = router
            .oneshot(request(
                "POST",
                format!("/api/account/login/{id}/cancel"),
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(cancel.status(), StatusCode::ACCEPTED);
        loop {
            let event = events.recv().await.unwrap();
            if let codeterm_events::SystemEvent::Generic { event_type, data } = event.event {
                if event_type == "account-sign-in" && data["settled"] == true {
                    assert_eq!(data["attempt_id"], id);
                    assert_eq!(data["failure"]["error"], "cancelled");
                    break;
                }
            }
        }
    })
    .await
    .expect("boot and readiness must settle while account login is still pending");
}
