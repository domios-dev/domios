use super::*;
use codeterm_account::account_mesh::callable::CallableSessionValidator;
use codeterm_account::{Account, Browser, Identity, IdentityObserver, Secret};

#[derive(Default)]
struct Credentials(parking_lot::Mutex<Option<Secret>>);
#[async_trait]
impl codeterm_account::CredentialStore for Credentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        Ok(self.0.lock().clone())
    }
    async fn save(&self, _: &str, secret: &Secret) -> codeterm_account::Result<()> {
        *self.0.lock() = Some(secret.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        *self.0.lock() = None;
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
struct Dropped(parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<()>>>);
#[async_trait]
impl IdentityObserver for Dropped {
    async fn changed(&self, _: Option<&Identity>) {}
}
impl Drop for Dropped {
    fn drop(&mut self) {
        if let Some(reply) = self.0.get_mut().take() {
            let _ = reply.send(());
        }
    }
}

#[tokio::test]
async fn initialized_account_mesh_graph_drops_and_sessions_refuse_a_missing_owner() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = Arc::new(fixture.service.clone());
    let owner = Arc::new(OnceCell::new());
    let changes = Arc::new(watch::channel(0).0);
    let sessions = Arc::new(AccountSessions(
        Arc::downgrade(&owner),
        Arc::downgrade(&changes),
    ));
    let mesh = account_mesh_with_sessions(
        &service,
        Arc::new(HeldSignOutStore::default()),
        sessions.clone(),
    );
    let (dropped, finished) = tokio::sync::oneshot::channel();
    let account = Arc::new(
        Account::new(
            Arc::new(codeterm_account::adapters::local::Local),
            Arc::new(Credentials::default()),
            Duration::from_secs(1),
        )
        .with_identity_observer(Arc::new(
            codeterm_account::account_mesh::IdentityObservers(vec![
                mesh.clone(),
                Arc::new(Dropped(parking_lot::Mutex::new(Some(dropped)))),
            ]),
        )),
    );
    assert!(owner.set(account.clone()).is_ok());
    account.login(Arc::new(NoBrowser)).await.unwrap();
    let session = account.session().await.unwrap();
    assert!(sessions.current(&session).await);
    let weak_account = Arc::downgrade(&account);
    let weak_mesh = Arc::downgrade(&mesh);
    drop(account);
    drop(mesh);
    assert!(weak_account.upgrade().is_some());
    drop(owner);
    drop(changes);
    tokio::time::timeout(Duration::from_secs(1), finished)
        .await
        .unwrap()
        .unwrap();
    assert!(weak_account.upgrade().is_none());
    assert!(weak_mesh.upgrade().is_none());
    assert!(matches!(
        sessions.session().await,
        Err(AccountMeshRefusal::Unavailable)
    ));
    assert!(matches!(
        sessions.refresh().await,
        Err(AccountMeshRefusal::Unavailable)
    ));
    assert!(!sessions.current(&session).await);
    tokio::time::timeout(Duration::from_secs(1), sessions.retired(&session))
        .await
        .unwrap();
}
