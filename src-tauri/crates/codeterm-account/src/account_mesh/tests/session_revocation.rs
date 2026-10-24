use crate::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use std::collections::BTreeMap;

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

#[tokio::test]
async fn session_revoked_clears_credentials_blocks_mesh_and_preserves_installation_key() {
    let store = Arc::new(Store::default());
    store.save("installation-key", &Secret::new("opaque-key")).await.unwrap();
    let mesh = super::support::Harness::new().await;
    let account = Arc::new(Account::new(
        Arc::new(adapters::local::Local), store.clone(), Duration::from_secs(1),
    ).with_identity_observer(mesh.mesh.clone()));
    account.login(&NoBrowser).await.unwrap();
    let original = account.session().await.unwrap();
    account.refresh().await.unwrap();
    assert!(account.session_revoked(&original).await.unwrap());
    let status = account.status().await;
    assert!(status.identity.is_none());
    assert!(status.session_revoked);
    assert!(store.load("local").await.unwrap().is_none());
    assert_eq!(store.load("installation-key").await.unwrap().unwrap().expose(), "opaque-key");
    assert_eq!(mesh.state().traffic(), crate::account_mesh::Traffic::BlockedSignedOut);
    account.login(&NoBrowser).await.unwrap();
    assert!(account.status().await.identity.is_some());
    assert!(!account.status().await.session_revoked);
    assert!(!account.session_revoked(&original).await.unwrap());
    assert!(account.session().await.is_some());
}

#[test]
fn session_revoked_requires_the_authenticated_refusal_reason() {
    use crate::session_revocation::is_session_revoked;
    let body = |status, reason| serde_json::json!({"error":{"status":status,"details":{"reason":reason}}});
    assert!(is_session_revoked(&body("UNAUTHENTICATED", "session_revoked")));
    for value in [body("PERMISSION_DENIED", "session_revoked"), body("UNAUTHENTICATED", "other"), serde_json::json!({"reason":"session_revoked"})] {
        assert!(!is_session_revoked(&value));
    }
}
