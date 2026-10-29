use super::*;
use codeterm_account::account_mesh::callable::{CallableClock, CallableFailure, CallableTransport};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

struct Credentials;
#[async_trait::async_trait]
impl codeterm_account::CredentialStore for Credentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<codeterm_account::Secret>> {
        Ok(Some(codeterm_account::Secret::new("fixture-refresh")))
    }
    async fn save(&self, _: &str, _: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        Ok(())
    }
}

struct Port {
    calls: AtomicUsize,
    now: parking_lot::Mutex<Instant>,
    waiting: tokio::sync::mpsc::UnboundedSender<()>,
    release: tokio::sync::Semaphore,
}
#[async_trait::async_trait]
impl CallableClock for Port {
    fn now(&self) -> Instant {
        *self.now.lock()
    }
    async fn wait(&self, delay: Duration) {
        self.waiting.send(()).unwrap();
        self.release.acquire().await.unwrap().forget();
        *self.now.lock() += delay;
    }
}
#[async_trait::async_trait]
impl CallableTransport for Port {
    fn retry_clock(&self) -> &dyn CallableClock {
        self
    }
    async fn call(
        &self,
        _: &str,
        _: &str,
        _: serde_json::Value,
    ) -> Result<serde_json::Value, CallableFailure> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(CallableFailure::Status {
                status: 429,
                retry_after_seconds: Some(45),
                body: None,
            })
        } else {
            Ok(serde_json::json!({"result":true}))
        }
    }
}

#[tokio::test]
async fn parked_gift_and_neighbor_dispatch_rechecks_the_real_account_generation() {
    for (route, neighbors) in [("redeemGiftKey", false), ("friendsSetFriendReceiver", true)] {
        for retire in [false, true] {
            let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
            let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
            let account = Arc::new(codeterm_account::Account::new(
                Arc::new(codeterm_account::adapters::fake::Fake::default()),
                Arc::new(Credentials),
                Duration::from_secs(1),
            ));
            account.refresh().await.unwrap();
            let mut expected = account.session().await.unwrap();
            let budget_uid = format!("guard-{route}-{retire}");
            assert!(fixture
                .service
                .ext::<AccountCell>()
                .0
                .set(account.clone())
                .is_ok());
            let guard = SessionGuard {
                service: &fixture.service,
                expected: &expected,
                neighbors,
            };
            let (waiting, mut observed) = tokio::sync::mpsc::unbounded_channel();
            let port = Port {
                calls: 0.into(),
                now: parking_lot::Mutex::new(Instant::now()),
                waiting,
                release: tokio::sync::Semaphore::new(0),
            };
            assert!(port
                .call_for_account(
                    &budget_uid,
                    route,
                    expected.id_token.expose(),
                    serde_json::Value::Null,
                    &guard
                )
                .await
                .is_err());
            let queued = port.call_for_account(
                &budget_uid,
                route,
                expected.id_token.expose(),
                serde_json::Value::Null,
                &guard,
            );
            let change = async {
                observed.recv().await.unwrap();
                if retire {
                    account.logout().await.unwrap();
                    fixture.service.retire_callable_waiters();
                }
                port.release.add_permits(1);
            };
            let (result, _) = tokio::join!(queued, change);
            if retire {
                assert_eq!(result, Err(CallableFailure::Retired));
                assert_eq!(port.calls.load(Ordering::SeqCst), 1);
            } else {
                assert!(result.is_ok());
                assert_eq!(port.calls.load(Ordering::SeqCst), 2);
            }
            expected.login_generation += 1;
            let stale = SessionGuard {
                service: &fixture.service,
                expected: &expected,
                neighbors,
            };
            assert!(!stale.current().await);
        }
    }
}
