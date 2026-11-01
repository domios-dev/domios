use super::*;
use codeterm_account::IdentityObserver;

struct GatedCredentials {
    saving: tokio::sync::mpsc::UnboundedSender<()>,
    release: tokio::sync::Mutex<tokio::sync::oneshot::Receiver<()>>,
    token: tokio::sync::Mutex<Option<codeterm_account::Secret>>,
    fail: bool,
}

#[async_trait]
impl codeterm_account::CredentialStore for GatedCredentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<codeterm_account::Secret>> {
        Ok(self.token.lock().await.clone())
    }
    async fn save(
        &self,
        _: &str,
        token: &codeterm_account::Secret,
    ) -> codeterm_account::Result<()> {
        self.saving.send(()).unwrap();
        (&mut *self.release.lock().await).await.unwrap();
        if self.fail {
            return Err(Error::Credentials);
        }
        *self.token.lock().await = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        *self.token.lock().await = None;
        Ok(())
    }
}

#[tokio::test]
async fn account_login_disconnect_still_publishes_settled_status() {
    for fail in [false, true] {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = Arc::new(fixture.service.clone());
        service.ext::<crate::config::ConfigState>().lock().billing =
            serde_json::json!({"endpoint": "fixture-unavailable"});
        let (saving, mut started) = tokio::sync::mpsc::unbounded_channel();
        let (release, receiver) = tokio::sync::oneshot::channel();
        let credentials = Arc::new(GatedCredentials {
            saving,
            release: tokio::sync::Mutex::new(receiver),
            token: tokio::sync::Mutex::new(None),
            fail,
        });
        let account = Arc::new(
            Account::new(
                Arc::new(codeterm_account::adapters::local::Local),
                credentials.clone(),
                Duration::from_secs(1),
            )
            .with_identity_observer(Arc::new(AccountIdentity(
                Arc::downgrade(&service),
                "local".into(),
            ))),
        );
        assert!(service.ext::<AccountCell>().0.set(account.clone()).is_ok());
        let mut events = service.events.subscribe();
        let caller = service.clone();
        let login = tokio::spawn(async move { caller.account_login(Default::default()).await });
        started.recv().await.unwrap();
        login.abort();
        assert!(login.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
        let mut changed = 0;
        while changed < if fail { 1 } else { 2 } {
            let event = events.recv().await.unwrap();
            if matches!(event.event, codeterm_events::SystemEvent::Generic { event_type, .. } if event_type == "account-changed") {
                changed += 1;
            }
        }
    }).await.expect("identity and settled login status reach reconnected clients");
        let status = account.status().await;
        assert!(!status.pending);
        if fail {
            assert!(status.identity.is_none());
            assert_eq!(
                status.last_sign_in_failure.unwrap().error,
                Error::Credentials
            );
            assert!(credentials.token.lock().await.is_none());
        } else {
            assert_eq!(status.identity.unwrap().uid, "local-dev-user");
            assert!(credentials.token.lock().await.is_some());
        }
    }
}

#[tokio::test]
async fn account_identity_changes_reach_the_frontend_event_stream() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = Arc::new(fixture.service.clone());
    let observer = AccountIdentity(Arc::downgrade(&service), "fixture".into());
    let mut events = service.events.subscribe();
    for identity in [
        Some(codeterm_account::Identity {
            uid: "owner".into(),
            display_name: None,
            email: None,
            provider: "test".into(),
            photo_url: None,
        }),
        None,
    ] {
        observer.changed(identity.as_ref()).await;
        let event = events
            .try_recv()
            .expect("identity changes must reach SSE clients");
        assert!(matches!(
            event.event,
            codeterm_events::SystemEvent::Generic { event_type, data }
                if event_type == "account-changed" && data.is_null()
        ));
    }
}
