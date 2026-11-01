use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use std::{collections::VecDeque, sync::Arc, time::Duration};

struct Store;
#[async_trait]
impl codeterm_account::CredentialStore for Store {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<codeterm_account::Secret>> {
        Ok(Some(codeterm_account::Secret::new("fake-refresh")))
    }
    async fn save(&self, _: &str, _: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        Ok(())
    }
}

struct Transport {
    replies: parking_lot::Mutex<VecDeque<std::result::Result<Value, CallableFailure>>>,
    tokens: parking_lot::Mutex<Vec<String>>,
}
impl Transport {
    fn new(replies: Vec<std::result::Result<Value, CallableFailure>>) -> Self {
        Self {
            replies: parking_lot::Mutex::new(replies.into()),
            tokens: Default::default(),
        }
    }
}
#[async_trait]
impl CallableTransport for Transport {
    async fn call(
        &self,
        _: &str,
        token: &str,
        _: Value,
    ) -> std::result::Result<Value, CallableFailure> {
        self.tokens.lock().push(token.into());
        self.replies
            .lock()
            .pop_front()
            .expect("bounded injected callable port")
    }
}

async fn fixture() -> (ServiceFixture, Arc<codeterm_account::Account>, Calls) {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let account = Arc::new(codeterm_account::Account::new(
        Arc::new(codeterm_account::adapters::fake::Fake::default()),
        Arc::new(Store),
        Duration::from_secs(1),
    ));
    account.refresh().await.unwrap();
    assert!(fixture
        .service
        .ext::<crate::service::account::AccountCell>()
        .0
        .set(account.clone())
        .is_ok());
    let session = account.session().await.unwrap();
    let calls = Calls {
        service: fixture.service.clone(),
        uid: session.identity.uid,
        generation: session.login_generation,
    };
    (fixture, account, calls)
}

#[tokio::test]
async fn unstructured_401_refreshes_same_generation_and_shared_read_succeeds_without_a_click() {
    let (_fixture, account, calls) = fixture().await;
    let old = account.session().await.unwrap();
    let port = Transport::new(vec![
        Err(CallableFailure::Status {
            status: 401,
            retry_after_seconds: None,
            body: None,
        }),
        Ok(json!({"result":{"fresh":true}})),
    ]);
    let value = calls
        .service
        .neighbor_backend_read(
            "friendsList",
            &calls.uid,
            calls.generation,
            calls.invoke_with_transport(&port, "friendsList", json!({})),
        )
        .await
        .unwrap();
    assert_eq!(value, json!({"fresh":true}));
    let refreshed = account.session().await.unwrap();
    assert_eq!(refreshed.login_generation, old.login_generation);
    assert_eq!(refreshed.identity, old.identity);
    assert_ne!(refreshed.id_token.expose(), old.id_token.expose());
    assert_eq!(
        *port.tokens.lock(),
        vec![old.id_token.expose(), refreshed.id_token.expose()]
    );
}

#[tokio::test]
async fn auth_recovery_is_bounded_and_never_replays_a_mutation() {
    for (route, count) in [
        ("friendsList", 2),
        ("friendsSetFriendReceiver", 1),
        ("friendsEnrollVerifierKey", 1),
    ] {
        let (_fixture, account, calls) = fixture().await;
        let port = Transport::new(vec![
            Err(CallableFailure::Status {
                status: 401,
                retry_after_seconds: None,
                body: None
            });
            count
        ]);
        assert_eq!(
            calls.invoke_with_transport(&port, route, json!({})).await,
            Err(if count == 2 {
                Error::Http {
                    status: 401,
                    retry_after_seconds: 0,
                }
            } else {
                Error::Transport
            })
        );
        assert_eq!(port.tokens.lock().len(), count);
        assert!(!account.status().await.session_revoked);
    }
}

#[tokio::test]
async fn structured_session_revocation_is_terminal_and_cannot_be_refreshed() {
    let (_fixture, account, calls) = fixture().await;
    let old = account.session().await.unwrap();
    let port = Transport::new(vec![Err(CallableFailure::Status {
        status: 401,
        retry_after_seconds: None,
        body: Some(
            json!({"error":{"status":"UNAUTHENTICATED","details":{"reason":"session_revoked"}}}),
        ),
    })]);
    assert_eq!(
        calls
            .invoke_with_transport(&port, "friendsList", json!({}))
            .await,
        Err(Error::SessionRevoked)
    );
    assert_eq!(port.tokens.lock().len(), 1);
    assert!(account.status().await.session_revoked);
    assert_eq!(
        account.recover_session(&old).await.unwrap_err(),
        codeterm_account::Error::Cancelled
    );
}

#[tokio::test]
async fn firestore_auth_failure_refreshes_before_the_next_listen_without_identity_change() {
    let (_fixture, account, calls) = fixture().await;
    let old = account.session().await.unwrap();
    calls.firestore_failure("listen", "opaque-target", Some(16), "Unauthenticated");
    let fresh = calls.id_token().await.unwrap();
    assert_ne!(fresh.as_str(), old.id_token.expose());
    assert_eq!(
        account.session().await.unwrap().login_generation,
        old.login_generation
    );
    assert!(codeterm_neighbors::supervisor::retryable(
        Error::Firestore {
            code: 16,
            operation: "listen"
        }
    ));
}

#[tokio::test]
async fn rejected_session_cannot_refresh_after_a_new_login_generation() {
    let (_fixture, account, _calls) = fixture().await;
    let mut old = account.session().await.unwrap();
    old.login_generation = old.login_generation.saturating_add(1);
    assert_eq!(
        account.recover_session(&old).await.unwrap_err(),
        codeterm_account::Error::Cancelled
    );
    assert_eq!(
        account.session().await.unwrap().id_token.expose(),
        "fake-id-0"
    );
}

#[tokio::test]
async fn selected_native_transport_uses_only_its_own_current_account_token() {
    for mismatch in [None, Some("uid"), Some("generation")] {
        let (_fixture, account, mut calls) = fixture().await;
        let expected = account.session().await.unwrap();
        match mismatch {
            Some("uid") => calls.uid = "foreign-account".into(),
            Some("generation") => calls.generation += 1,
            _ => {}
        }
        let selected = super::super::super::transport::bind(
            calls,
            codeterm_account::firebase_endpoints::PRODUCTION,
            || codeterm_account::firebase_endpoints::PRODUCTION,
        )
        .unwrap();
        let token = selected.id_token().await;
        if mismatch.is_some() {
            assert!(matches!(token, Err(Error::Revoked)));
        } else {
            assert_eq!(token.unwrap().as_str(), expected.id_token.expose());
        }
        assert_eq!(
            account.session().await.unwrap().login_generation,
            expected.login_generation
        );
    }
}
