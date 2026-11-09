use super::*;
use std::future::Future;
use std::task::Poll;

#[derive(Default)]
struct HeldStore {
    token: StoreMutex<Option<Secret>>,
    stage: AtomicUsize,
    entered: Notify,
    release: Notify,
}

impl HeldStore {
    async fn hold(&self, stage: usize) {
        if self.stage.load(Ordering::SeqCst) == stage {
            self.entered.notify_one();
            self.release.notified().await;
        }
    }
}

#[async_trait]
impl CredentialStore for HeldStore {
    async fn load(&self, _: &str) -> Result<Option<Secret>> {
        self.hold(1).await;
        Ok(self.token.lock().clone())
    }
    async fn save(&self, _: &str, token: &Secret) -> Result<()> {
        self.hold(2).await;
        *self.token.lock() = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> Result<()> {
        self.hold(3).await;
        *self.token.lock() = None;
        Ok(())
    }
}

#[tokio::test]
async fn held_credentials_do_not_hold_state_and_late_saves_cannot_undo_logout() {
    for stage in [1, 2] {
        let store = Arc::new(HeldStore::default());
        store.stage.store(stage, Ordering::SeqCst);
        let account = Arc::new(Account::new(
            Arc::new(Provider::default()),
            store.clone(),
            Duration::from_secs(10),
        ));
        let login = tokio::spawn({
            let account = account.clone();
            async move { account.login(Arc::new(NoBrowser)).await }
        });
        store.entered.notified().await;
        assert!(
            tokio::time::timeout(Duration::from_secs(1), account.status())
                .await
                .unwrap()
                .pending
        );
        let logout = account.logout();
        tokio::pin!(logout);
        std::future::poll_fn(|cx| {
            assert!(logout.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert!(
            tokio::time::timeout(Duration::from_secs(1), account.current_session())
                .await
                .unwrap()
                .unwrap()
                .is_none()
        );
        store.stage.store(0, Ordering::SeqCst);
        store.release.notify_one();
        logout.await.unwrap();
        assert_eq!(login.await.unwrap().unwrap_err(), Error::Cancelled);
        assert!(store.token.lock().is_none());
        assert!(account.session().await.is_none());
    }
}

#[tokio::test]
async fn held_credential_delete_keeps_status_and_session_validation_available() {
    let store = Arc::new(HeldStore::default());
    let account = Arc::new(Account::new(
        Arc::new(Provider::default()),
        store.clone(),
        Duration::from_secs(10),
    ));
    account.login(Arc::new(NoBrowser)).await.unwrap();
    store.stage.store(3, Ordering::SeqCst);
    let logout = tokio::spawn({
        let account = account.clone();
        async move { account.logout().await }
    });
    store.entered.notified().await;
    assert!(
        tokio::time::timeout(Duration::from_secs(1), account.status())
            .await
            .unwrap()
            .identity
            .is_none()
    );
    assert!(account.current_session().await.unwrap().is_none());
    store.release.notify_one();
    logout.await.unwrap().unwrap();
    assert!(store.token.lock().is_none());
}

#[derive(Default)]
struct HeldObserver {
    account: StoreMutex<std::sync::Weak<Account>>,
    seen: StoreMutex<Vec<Option<String>>>,
    entered: Notify,
    release: Notify,
}

#[async_trait]
impl IdentityObserver for HeldObserver {
    async fn changed(&self, identity: Option<&Identity>) {
        let account = self.account.lock().upgrade().unwrap();
        let status = account.status().await;
        assert_eq!(
            status.identity.as_ref().map(|i| i.uid.as_str()),
            identity.map(|i| i.uid.as_str())
        );
        self.seen
            .lock()
            .push(identity.map(|identity| identity.uid.clone()));
        if identity.is_some() {
            self.entered.notify_one();
            self.release.notified().await;
        }
    }
}

#[tokio::test]
async fn held_observer_can_reenter_status_and_sign_out_remains_the_last_publication() {
    let observer = Arc::new(HeldObserver::default());
    let account = Arc::new(
        Account::new(
            Arc::new(Provider::default()),
            Arc::new(Store::default()),
            Duration::from_secs(10),
        )
        .with_identity_observer(observer.clone()),
    );
    *observer.account.lock() = Arc::downgrade(&account);
    let login = tokio::spawn({
        let account = account.clone();
        async move { account.login(Arc::new(NoBrowser)).await }
    });
    observer.entered.notified().await;
    let logout = account.logout();
    tokio::pin!(logout);
    std::future::poll_fn(|cx| {
        assert!(logout.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(
        tokio::time::timeout(Duration::from_secs(1), account.status())
            .await
            .unwrap()
            .identity
            .is_none()
    );
    observer.release.notify_one();
    logout.await.unwrap();
    assert_eq!(login.await.unwrap().unwrap_err(), Error::Cancelled);
    assert_eq!(
        *observer.seen.lock(),
        vec![Some("local-dev-user".into()), None]
    );
}

#[tokio::test(start_paused = true)]
async fn observer_completion_has_a_deadline_and_does_not_own_account_state() {
    let observer = Arc::new(HeldObserver::default());
    let account = Arc::new(
        Account::new(
            Arc::new(Provider::default()),
            Arc::new(Store::default()),
            Duration::from_secs(1),
        )
        .with_identity_observer(observer.clone()),
    );
    *observer.account.lock() = Arc::downgrade(&account);
    assert_eq!(
        account.login(Arc::new(NoBrowser)).await.unwrap_err(),
        Error::Timeout
    );
    assert!(!account.status().await.pending);
    assert!(account.logout().await.unwrap().identity.is_none());
    assert_eq!(observer.seen.lock().last(), Some(&None));
}
