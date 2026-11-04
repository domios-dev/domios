use super::*;
use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::get,
    Router,
};
use codeterm_account::{Account, Browser, CredentialStore, Identity, IdentityObserver, Secret};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState,
    service::{account::AccountCell, test_fixture::ServiceFixture},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;
use tower::ServiceExt;

struct Credentials;
#[async_trait]
impl CredentialStore for Credentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        Ok(None)
    }
    async fn save(&self, _: &str, _: &Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        Ok(())
    }
}
struct NoBrowser;
#[async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> codeterm_account::Result<()> {
        panic!("local provider is immediate")
    }
}
#[derive(Default)]
struct HeldIdentity {
    entered: Notify,
    release: Notify,
}
#[async_trait]
impl IdentityObserver for HeldIdentity {
    async fn changed(&self, identity: Option<&Identity>) {
        if identity.is_some() {
            self.entered.notify_one();
            self.release.notified().await;
        }
    }
}

#[tokio::test]
async fn held_identity_observer_keeps_account_status_and_real_health_responsive() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    let observer = Arc::new(HeldIdentity::default());
    let account = Arc::new(
        Account::new(
            Arc::new(codeterm_account::adapters::local::Local),
            Arc::new(Credentials),
            Duration::from_secs(10),
        )
        .with_identity_observer(observer.clone()),
    );
    assert!(state
        .svc
        .ext::<AccountCell>()
        .0
        .set(account.clone())
        .is_ok());
    let router = Router::new()
        .route("/api/account/status", get(status))
        .route("/api/health", get(crate::health_routes::health))
        .layer(Extension(CallerIdentity::Local))
        .with_state(state);
    let login = tokio::spawn({
        let account = account.clone();
        async move { account.login(Arc::new(NoBrowser)).await }
    });
    observer.entered.notified().await;
    let responses = tokio::time::timeout(Duration::from_secs(1), async {
        let status = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/account/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let health = router
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        (status, health)
    })
    .await;
    observer.release.notify_one();
    login.await.unwrap().unwrap();
    let (status, health) = responses.expect("both routes must answer while the observer is held");
    assert_eq!(status.status(), StatusCode::OK);
    assert_eq!(health.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_slice(&to_bytes(status.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(status["identity"]["uid"], "local-dev-user");
    let health: serde_json::Value =
        serde_json::from_slice(&to_bytes(health.into_body(), 16384).await.unwrap()).unwrap();
    assert_eq!(health["startup"]["state"], "ready");
}
