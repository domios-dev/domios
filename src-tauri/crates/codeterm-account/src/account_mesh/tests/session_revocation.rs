use crate::*;
use std::collections::BTreeMap;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Default)]
struct Store(Mutex<BTreeMap<String, Secret>>);
#[async_trait]
impl CredentialStore for Store {
    async fn load(&self, provider: &str) -> Result<Option<Secret>> {
        Ok(self.0.lock().await.get(provider).cloned())
    }
    async fn save(&self, provider: &str, token: &Secret) -> Result<()> {
        self.0.lock().await.insert(provider.into(), token.clone());
        Ok(())
    }
    async fn delete(&self, provider: &str) -> Result<()> {
        self.0.lock().await.remove(provider);
        Ok(())
    }
}

struct NoBrowser;
#[async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> Result<()> {
        panic!("Immediate login does not open a browser")
    }
}

#[derive(Default)]
struct GatedStore {
    store: Store,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    deleted: tokio::sync::Notify,
}

#[async_trait]
impl CredentialStore for GatedStore {
    async fn load(&self, provider: &str) -> Result<Option<Secret>> {
        self.store.load(provider).await
    }
    async fn save(&self, provider: &str, token: &Secret) -> Result<()> {
        self.store.save(provider, token).await
    }
    async fn delete(&self, provider: &str) -> Result<()> {
        self.entered.notify_one();
        self.release.notified().await;
        self.store.delete(provider).await?;
        self.deleted.notify_one();
        Ok(())
    }
}

#[tokio::test]
async fn session_revoked_cleanup_survives_the_detecting_request_being_cancelled() {
    let store = Arc::new(GatedStore::default());
    let account = Arc::new(Account::new(
        Arc::new(adapters::local::Local),
        store.clone(),
        Duration::from_secs(1),
    ));
    account.login(&NoBrowser).await.unwrap();
    let expected = account.session().await.unwrap();
    let caller = account.clone();
    let detecting_request = tokio::spawn(async move { caller.session_revoked(&expected).await });
    store.entered.notified().await;
    detecting_request.abort();
    assert!(detecting_request.await.unwrap_err().is_cancelled());
    store.release.notify_one();
    store.deleted.notified().await;
    assert!(account.session().await.is_none());
    assert!(account.status().await.session_revoked);
    assert!(store.load("local").await.unwrap().is_none());
}

#[tokio::test]
async fn session_revoked_clears_credentials_blocks_mesh_and_preserves_installation_key() {
    let store = Arc::new(Store::default());
    store
        .save("installation-key", &Secret::new("opaque-key"))
        .await
        .unwrap();
    let mesh = super::support::Harness::new().await;
    let account = Arc::new(
        Account::new(
            Arc::new(adapters::local::Local),
            store.clone(),
            Duration::from_secs(1),
        )
        .with_identity_observer(mesh.mesh.clone()),
    );
    account.login(&NoBrowser).await.unwrap();
    let original = account.session().await.unwrap();
    account.refresh().await.unwrap();
    assert!(account.session_revoked(&original).await.unwrap());
    let status = account.status().await;
    assert!(status.identity.is_none());
    assert!(status.session_revoked);
    assert!(store.load("local").await.unwrap().is_none());
    assert_eq!(
        store
            .load("installation-key")
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "opaque-key"
    );
    assert_eq!(
        mesh.state().traffic(),
        crate::account_mesh::Traffic::BlockedSignedOut
    );
    account.login(&NoBrowser).await.unwrap();
    assert!(account.status().await.identity.is_some());
    assert!(!account.status().await.session_revoked);
    assert!(!account.session_revoked(&original).await.unwrap());
    assert!(account.session().await.is_some());
}

#[test]
fn session_revoked_requires_the_authenticated_refusal_reason() {
    use crate::session_revocation::is_session_revoked;
    let body =
        |status, reason| serde_json::json!({"error":{"status":status,"details":{"reason":reason}}});
    assert!(is_session_revoked(&body(
        "UNAUTHENTICATED",
        "session_revoked"
    )));
    for value in [
        body("PERMISSION_DENIED", "session_revoked"),
        body("UNAUTHENTICATED", "other"),
        serde_json::json!({"reason":"session_revoked"}),
    ] {
        assert!(!is_session_revoked(&value));
    }
}
