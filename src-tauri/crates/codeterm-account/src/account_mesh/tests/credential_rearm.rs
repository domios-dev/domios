use super::support::*;
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::*;
use crate::{Account, Browser, CredentialStore, Identity, IdentityObserver, IdentityProvider,
    LoginFlow, LoginStart, Redemption, Secret, Session, Tokens};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Provider(AtomicU64);
#[async_trait]
impl IdentityProvider for Provider {
    fn id(&self) -> &str { "credential-rearm" }
    fn needs_browser_callback(&self) -> bool { false }
    async fn start_login(&self, _: &LoginFlow) -> crate::Result<LoginStart> {
        Ok(LoginStart::Immediate(self.refresh(&Secret::new("fixture")).await?))
    }
    async fn redeem(&self, _: Redemption) -> crate::Result<Tokens> {
        Err(crate::Error::Provider)
    }
    async fn refresh(&self, _: &Secret) -> crate::Result<Tokens> {
        let mut session = session("owner");
        session.id_token = Secret::new(format!("credential-{}", self.0.fetch_add(1, Ordering::SeqCst)));
        Ok(Tokens { session, refresh_token: Secret::new("fixture") })
    }
    async fn revoke(&self, _: &Secret) -> crate::Result<()> { Ok(()) }
}

#[derive(Default)]
struct Store(Mutex<Option<Secret>>);
#[async_trait]
impl CredentialStore for Store {
    async fn load(&self, _: &str) -> crate::Result<Option<Secret>> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn save(&self, _: &str, secret: &Secret) -> crate::Result<()> {
        *self.0.lock().unwrap() = Some(secret.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> crate::Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}
struct NoBrowser;
#[async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> crate::Result<()> {
        panic!("immediate fixture must not open a browser")
    }
}

struct Observer {
    mesh: Arc<AccountMesh>,
    sessions: Arc<FakeSessions>,
    rearmed: AtomicU64,
    renewed: tokio::sync::watch::Sender<Option<Result<(), AccountMeshRefusal>>>,
}
#[async_trait]
impl IdentityObserver for Observer {
    async fn changed(&self, identity: Option<&Identity>) {
        self.mesh.identity_changed(identity).await;
    }
    async fn session_refreshed(&self, session: &Session) {
        *self.sessions.0.lock().unwrap() = Some(session.clone());
        if self.mesh.credentials_refreshed(session) {
            self.rearmed.fetch_add(1, Ordering::SeqCst);
            let mesh = self.mesh.clone();
            let renewed = self.renewed.clone();
            tokio::spawn(async move {
                renewed.send_replace(Some(mesh.refresh(EnrollTrigger::Automatic).await));
            });
        }
    }
}

#[tokio::test]
async fn account_mesh_real_same_generation_refresh_rearms_but_revocation_stays_terminal() {
    let harness = Harness::new().await;
    let (renewed_sender, mut automatic) = tokio::sync::watch::channel(None);
    let observer = Arc::new(Observer {
        mesh: harness.mesh.clone(), sessions: harness.sessions.clone(),
        rearmed: AtomicU64::new(0),
        renewed: renewed_sender,
    });
    let provider = Arc::new(Provider::default());
    let account = Arc::new(Account::new(provider.clone(), Arc::new(Store::default()),
        Duration::from_secs(1)).with_identity_observer(Arc::new(
            crate::account_mesh::IdentityObservers(vec![observer.clone()]))));
    account.login(Arc::new(NoBrowser)).await.unwrap();
    let original = account.session().await.unwrap();
    *harness.sessions.0.lock().unwrap() = Some(original.clone());
    harness.mesh.refresh(EnrollTrigger::Automatic).await.unwrap();
    harness.clock.set(START + 3600);
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Unauthenticated);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Lease).await,
        Err(AccountMeshRefusal::Unauthenticated));
    assert_eq!(observer.rearmed.load(Ordering::SeqCst), 0);
    let failed = harness.sessions.0.lock().unwrap().clone().unwrap();
    let mut foreign = failed.clone();
    foreign.login_generation += 1;
    foreign.id_token = Secret::new("foreign-generation");
    assert!(!harness.mesh.credentials_refreshed(&foreign));
    foreign = failed.clone();
    foreign.identity.uid = "another-owner".into();
    assert!(!harness.mesh.credentials_refreshed(&foreign));
    harness.registry.script.lock().unwrap().membership = None;
    let renewed = account.fresh_session().await.unwrap().unwrap();
    assert_eq!(renewed.login_generation, original.login_generation);
    assert_eq!(observer.rearmed.load(Ordering::SeqCst), 1);
    let outcome = tokio::time::timeout(Duration::from_secs(1),
        automatic.wait_for(Option::is_some)).await.unwrap().unwrap();
    assert_eq!(*outcome, Some(Ok(())));
    drop(outcome);
    assert!(account.session_revoked(&renewed).await.unwrap());
    let calls = provider.0.load(Ordering::SeqCst);
    assert!(account.fresh_session().await.unwrap().is_none());
    assert_eq!(provider.0.load(Ordering::SeqCst), calls);
    assert_eq!(harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::SignedOut));
    assert!(!harness.mesh.credentials_refreshed(&renewed));
}
