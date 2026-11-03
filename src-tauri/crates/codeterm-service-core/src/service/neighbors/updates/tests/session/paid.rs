use super::*;
use crate::service::neighbors::paid_fixture::{reading, Paid};

struct State {
    snapshot: Option<Snapshot>,
    waiting: tokio::sync::mpsc::UnboundedSender<()>,
}
#[async_trait::async_trait]
impl StateListener for State {
    async fn next(&mut self) -> Result<Option<StateEvent>> {
        if let Some(snapshot) = self.snapshot.take() {
            return Ok(Some(StateEvent::Snapshot(snapshot)));
        }
        self.waiting.send(()).unwrap();
        std::future::pending().await
    }
}
struct Signal(tokio::sync::mpsc::UnboundedSender<()>);
impl Drop for Signal {
    fn drop(&mut self) {
        self.0.send(()).unwrap();
    }
}
#[async_trait::async_trait]
impl codeterm_neighbors::registry::RegistryListener for Signal {
    async fn next(&mut self) -> Result<Option<RegistrySignal>> {
        std::future::pending().await
    }
}
struct Ports {
    state: parking_lot::Mutex<Option<State>>,
    opened: tokio::sync::mpsc::UnboundedSender<()>,
    closed: tokio::sync::mpsc::UnboundedSender<()>,
}
#[async_trait::async_trait]
impl SessionPorts for Ports {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>> {
        Ok(Box::new(self.state.lock().take().unwrap()))
    }
    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>> {
        self.opened.send(()).unwrap();
        Ok(Box::new(Signal(self.closed.clone())))
    }
    async fn activate(&self, _: &Session, _: &Value) -> Result<()> {
        Ok(())
    }
    async fn reconcile(
        &self,
        _: &Session,
        _: Value,
        _: Option<Value>,
        _: &mut notifications::Seen,
    ) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn paid_signal_expiry_and_local_lock_close_without_a_state_event_and_upgrade_restarts() {
    for expiry in [true, false] {
        let (fixture, mut session, _, _, _) = fixture(Mode::Controlled).await;
        let paid = fixture.service.ext::<Paid>().clone();
        let (opened, mut opens) = tokio::sync::mpsc::unbounded_channel();
        let (closed, mut closes) = tokio::sync::mpsc::unbounded_channel();
        let (waiting, mut waits) = tokio::sync::mpsc::unbounded_channel();
        session.ports = Arc::new(Ports {
            state: parking_lot::Mutex::new(Some(State {
                snapshot: Some(snapshot(1, Some("fixture-machine"))),
                waiting,
            })),
            opened,
            closed,
        });
        let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
        let control = async {
            opens.recv().await.unwrap();
            waits.recv().await.unwrap();
            if expiry {
                paid.now.store(1_700_086_400, Ordering::SeqCst);
                *paid.provider.0.lock() = Err("offline".into());
            } else {
                paid.now.store(1_700_000_120, Ordering::SeqCst);
                *paid.provider.0.lock() = Ok(reading(
                    &session.expected.identity.uid,
                    "signed-free-downgrade",
                    11,
                    0,
                ));
            }
            paid.billing
                .refresh(
                    Some(&session.expected),
                    codeterm_billing::RefreshEvent::Explicit,
                    std::time::Instant::now() + Duration::from_secs(1),
                )
                .await;
            assert!(paid
                .billing
                .neighbors_locked(&session.expected.identity.uid));
            fixture.service.neighbor_entitlements_changed(true);
            closes.recv().await.unwrap();
            waits.recv().await.unwrap();
            assert!(opens.try_recv().is_err());
            assert_eq!(
                fixture
                    .service
                    .require_neighbor_paid(&session.expected.identity.uid),
                Err(Error::Locked)
            );
            assert_eq!(
                fixture
                    .service
                    .peek_neighbor_projection(
                        &session.expected.identity.uid,
                        session.expected.login_generation,
                        "friendsList",
                        "fixture-machine"
                    )
                    .unwrap()["access"],
                "paid"
            );
            paid.now.fetch_add(60, Ordering::SeqCst);
            let shift = if expiry { 86_400 } else { 0 };
            *paid.provider.0.lock() = Ok(reading(
                &session.expected.identity.uid,
                "valid-paid",
                12,
                shift,
            ));
            paid.billing
                .refresh(
                    Some(&session.expected),
                    codeterm_billing::RefreshEvent::Explicit,
                    std::time::Instant::now() + Duration::from_secs(2),
                )
                .await;
            fixture.service.neighbor_entitlements_changed(true);
            opens.recv().await.unwrap();
            waits.recv().await.unwrap();
            assert!(fixture
                .service
                .require_neighbor_paid(&session.expected.identity.uid)
                .is_ok());
            stop.send(()).unwrap();
        };
        let run = async {
            tokio::select! { result = session.listen() => panic!("Free state watch stopped: {result:?}"), _ = &mut stopped => {} }
        };
        tokio::join!(run, control);
        closes.recv().await.unwrap();
    }
}
