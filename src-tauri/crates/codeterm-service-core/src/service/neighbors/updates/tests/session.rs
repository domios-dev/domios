use super::super::*;
use codeterm_neighbors::{
    adapter::AuthenticatedCall,
    state::{Snapshot, StateEvent, StateListener},
    supervisor::RetryClock,
};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

struct Credentials;
#[async_trait::async_trait]
impl codeterm_account::CredentialStore for Credentials {
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

struct Clock {
    now: AtomicU64,
    deadlines: tokio::sync::mpsc::UnboundedSender<Duration>,
    advance: tokio::sync::Semaphore,
}
impl Clock {
    fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<Duration>) {
        let (deadlines, receiver) = tokio::sync::mpsc::unbounded_channel();
        (
            Arc::new(Self {
                now: AtomicU64::new(0),
                deadlines,
                advance: tokio::sync::Semaphore::new(0),
            }),
            receiver,
        )
    }
}
#[async_trait::async_trait]
impl RetryClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.now.load(Ordering::SeqCst))
    }
    fn jitter(&self) -> u64 {
        500
    }
    async fn wait(&self, delay: Duration) {
        self.deadlines.send(delay).unwrap();
        self.advance.acquire().await.unwrap().forget();
        self.now
            .fetch_add(delay.as_millis() as u64, Ordering::SeqCst);
    }
}

fn snapshot(version: u64, installation: Option<&str>) -> Snapshot {
    let friends = installation.map(|installation| vec![json!({"friendship_id":"a".repeat(64),"uid":"peer","displayName":"Peer","photoURL":null,"email":null,"since":1,
        "inbound_per_hour":10,"outbound_per_hour":10,"receiving":false,"my_paused":false,"their_paused":false,
        "my_daily_limit":null,"their_daily_limit":null,"my_file_daily_limit_bytes":null,"their_file_daily_limit_bytes":null,
        "my_receiver":{"installation_id":installation,"enc_key_id":"key_active","machine_label":null,"revision":version,"receiving":true},
        "peer_receiver":{"enc_key_id":null,"revision":0,"receiving":false}})]).unwrap_or_default();
    Snapshot::parse(json!({"schema_version":1,"version":version,"updated_at":1,"access":"paid","can_invite":false,
        "friends":friends,"incoming":[],"outgoing":[],"own_keys":[],
        "referral_trial":{"state":"none","trial_end":null,"inviter_uid":null,"friend_request_id":null}})).unwrap()
}

struct Calls {
    service: ServiceContext,
    uid: String,
    generation: u64,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl AuthenticatedCall for Calls {
    async fn invoke(&self, route: &str, _: Value) -> Result<Value> {
        self.service.neighbor_backend_read(route, &self.uid, self.generation, async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"schema_version":1,"access":"locked","can_invite":false,"friends":[],"incoming":[],"outgoing":[]}))
        }).await
    }
    async fn id_token(&self) -> Result<zeroize::Zeroizing<String>> {
        panic!("No real authentication port")
    }
}

struct Stream {
    steps: VecDeque<(u64, StateEvent)>,
    clock: Arc<Clock>,
}
#[async_trait::async_trait]
impl StateListener for Stream {
    async fn next(&mut self) -> Result<Option<StateEvent>> {
        Ok(self.steps.pop_front().map(|(elapsed, event)| {
            self.clock.now.fetch_add(elapsed, Ordering::SeqCst);
            event
        }))
    }
}
struct Signals(Arc<AtomicUsize>);
impl Drop for Signals {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait::async_trait]
impl codeterm_neighbors::registry::RegistryListener for Signals {
    async fn next(&mut self) -> Result<Option<RegistrySignal>> {
        std::future::pending().await
    }
}

enum Mode {
    Initial,
    Duplicate,
    Missing,
    Controlled,
}
struct Ports {
    mode: Mode,
    admissions: AtomicUsize,
    signal_admissions: AtomicUsize,
    signal_drops: Arc<AtomicUsize>,
    inboxes: AtomicUsize,
    starts: AtomicUsize,
    clock: Arc<Clock>,
    controlled: parking_lot::Mutex<Option<VecDeque<(u64, StateEvent)>>>,
}
#[async_trait::async_trait]
impl ports::SessionPorts for Ports {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>> {
        let version = self.admissions.fetch_add(1, Ordering::SeqCst) as u64 + 1;
        let steps = match self.mode {
            Mode::Initial => vec![(0, StateEvent::Snapshot(snapshot(version, None)))].into(),
            Mode::Duplicate => vec![
                (0, StateEvent::Snapshot(snapshot(1, None))),
                (60_000, StateEvent::Snapshot(snapshot(1, None))),
            ]
            .into(),
            Mode::Missing => vec![(0, StateEvent::Missing)].into(),
            Mode::Controlled => self.controlled.lock().take().unwrap(),
        };
        Ok(Box::new(Stream {
            steps,
            clock: self.clock.clone(),
        }))
    }
    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>> {
        self.signal_admissions.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Signals(self.signal_drops.clone())))
    }
    async fn reconcile(
        &self,
        session: &Session,
        list: Value,
        _: Option<Value>,
        _: &mut notifications::Seen,
    ) -> Result<()> {
        let receiving = usize::from(receives_friends(&list, &session.transport.installation_id));
        let old = self.inboxes.swap(receiving, Ordering::SeqCst);
        if receiving > old {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }
        if receiving > 0 {
            session
                .service
                .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
        }
        Ok(())
    }
    fn stop_receiving(&self, session: &Session) {
        self.inboxes.store(0, Ordering::SeqCst);
        session.service.pause_neighbor_reception();
    }
}

async fn fixture(
    mode: Mode,
) -> (
    crate::service::test_fixture::ServiceFixture,
    Session,
    Arc<Ports>,
    Arc<Calls>,
    tokio::sync::mpsc::UnboundedReceiver<Duration>,
) {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    let account = Arc::new(codeterm_account::Account::new(
        Arc::new(codeterm_account::adapters::fake::Fake::default()),
        Arc::new(Credentials),
        Duration::from_secs(1),
    ));
    account.refresh().await.unwrap();
    assert!(fixture
        .service
        .ext::<AccountCell>()
        .0
        .set(account.clone())
        .is_ok());
    let expected = account.session().await.unwrap();
    let (clock, deadlines) = Clock::new();
    let ports = Arc::new(Ports {
        mode,
        admissions: AtomicUsize::new(0),
        signal_admissions: AtomicUsize::new(0),
        signal_drops: Arc::new(AtomicUsize::new(0)),
        inboxes: AtomicUsize::new(0),
        starts: AtomicUsize::new(0),
        clock: clock.clone(),
        controlled: parking_lot::Mutex::new(None),
    });
    let calls = Arc::new(Calls {
        service: fixture.service.clone(),
        uid: expected.identity.uid.clone(),
        generation: expected.login_generation,
        calls: AtomicUsize::new(0),
    });
    let transport = Arc::new(FirebaseTransport {
        calls: calls.clone(),
        uid: expected.identity.uid.clone(),
        project: "unused-project".into(),
        installation_id: "fixture-machine".into(),
    });
    fixture
        .service
        .select_neighbor_projection(&expected.identity.uid, expected.login_generation);
    let session = Session {
        metadata: Metadata::new(fixture.temp.path().to_owned(), &expected.identity.uid),
        service: fixture.service.clone(),
        account,
        expected,
        generation: 0,
        transport,
        ports: ports.clone(),
        clock,
        progress: AtomicU64::new(0),
    };
    (fixture, session, ports, calls, deadlines)
}

#[tokio::test]
async fn production_session_initial_duplicate_and_missing_eof_cycles_grow_to_the_retry_cap() {
    for mode in [Mode::Initial, Mode::Duplicate, Mode::Missing] {
        let (fixture, session, ports, calls, mut deadlines) = fixture(mode).await;
        let control = async {
            let mut total = Duration::ZERO;
            for attempt in 1usize..=200 {
                let expected =
                    Duration::from_millis((500u64 << (attempt - 1).min(10)).min(300_000));
                assert_eq!(deadlines.recv().await, Some(expected));
                total += expected;
                assert_eq!(ports.admissions.load(Ordering::SeqCst), attempt);
                assert_eq!(session.progress(), 0);
                if attempt == 200 {
                    fixture
                        .service
                        .ext::<crate::config::ConfigState>()
                        .lock()
                        .neighbors
                        .enabled = false;
                }
                ports.clock.advance.add_permits(1);
            }
            assert!(total >= Duration::from_secs(180 * 300));
        };
        let (result, _) = tokio::join!(supervise_documents(&session), control);
        assert_eq!(result, Err(Error::Disabled));
        assert_eq!(ports.admissions.load(Ordering::SeqCst), 200);
        assert_eq!(
            calls.calls.load(Ordering::SeqCst),
            usize::from(matches!(ports.mode, Mode::Missing))
        );
    }
}

#[tokio::test]
async fn missing_withdraws_live_authority_stops_inboxes_and_signals_and_uses_one_real_fallback() {
    let (_fixture, session, ports, calls, _) = fixture(Mode::Controlled).await;
    *ports.controlled.lock() = Some(
        vec![
            (
                0,
                StateEvent::Snapshot(snapshot(1, Some("fixture-machine"))),
            ),
            (0, StateEvent::Missing),
        ]
        .into(),
    );
    assert_eq!(session.listen().await, Err(Error::Transport));
    assert_eq!(
        (
            ports.starts.load(Ordering::SeqCst),
            ports.inboxes.load(Ordering::SeqCst)
        ),
        (1, 0)
    );
    assert_eq!(
        (
            ports.signal_admissions.load(Ordering::SeqCst),
            ports.signal_drops.load(Ordering::SeqCst)
        ),
        (1, 1)
    );
    assert!(session
        .service
        .neighbor_friend_receiver_views()
        .as_object()
        .unwrap()
        .is_empty());
    for refresh in [false, true] {
        if refresh {
            session.service.rearm_neighbor_reads();
        }
        assert_eq!(
            calls.invoke("friendsList", json!({})).await.unwrap()["access"],
            "locked"
        );
        for route in ["friendsOwnKeys", "friendsReceiverStatus"] {
            assert!(session
                .service
                .peek_neighbor_projection(
                    &session.transport.uid,
                    session.expected.login_generation,
                    route,
                    "fixture-machine"
                )
                .is_none());
            assert_eq!(calls.invoke(route, json!({})).await, Err(Error::Transport));
        }
    }
    assert_eq!(calls.calls.load(Ordering::SeqCst), 1);
    let mut seen = notifications::Seen::default();
    session
        .project(snapshot(1, Some("fixture-machine")), &mut seen)
        .await
        .unwrap();
    assert!(session
        .service
        .peek_neighbor_projection(
            &session.transport.uid,
            session.expected.login_generation,
            "friendsList",
            "fixture-machine"
        )
        .is_none());
    *ports.controlled.lock() = Some(
        vec![(
            0,
            StateEvent::Snapshot(snapshot(2, Some("fixture-machine"))),
        )]
        .into(),
    );
    assert_eq!(session.listen().await, Err(Error::Transport));
    assert_eq!(
        calls.invoke("friendsList", json!({})).await.unwrap()["access"],
        "paid"
    );
    assert_eq!(ports.starts.load(Ordering::SeqCst), 2);
    assert_eq!(
        (
            ports.signal_admissions.load(Ordering::SeqCst),
            ports.signal_drops.load(Ordering::SeqCst)
        ),
        (2, 2)
    );
    assert_eq!(calls.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sustained_new_projection_progress_can_clear_reconnect_history() {
    let (_fixture, session, ports, _, _) = fixture(Mode::Controlled).await;
    *ports.controlled.lock() = Some(
        vec![
            (0, StateEvent::Snapshot(snapshot(1, None))),
            (31_000, StateEvent::Snapshot(snapshot(2, None))),
        ]
        .into(),
    );
    assert_eq!(session.listen().await, Err(Error::Transport));
    assert_eq!(session.progress(), 1);
}

struct HeldCalls {
    service: ServiceContext,
    uid: String,
    generation: u64,
    started: parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: parking_lot::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl AuthenticatedCall for HeldCalls {
    async fn invoke(&self, route: &str, _: Value) -> Result<Value> {
        self.service.neighbor_backend_read(route, &self.uid, self.generation, async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let started = self.started.lock().take().unwrap();
            let release = self.release.lock().take().unwrap();
            started.send(()).unwrap();
            release.await.unwrap();
            Ok(json!({"schema_version":1,"access":"locked","can_invite":false,"friends":[],"incoming":[],"outgoing":[]}))
        }).await
    }

    async fn id_token(&self) -> Result<zeroize::Zeroizing<String>> {
        panic!("No real authentication port")
    }
}

#[tokio::test]
async fn late_callable_fallback_does_not_publish_over_a_live_projection() {
    let (fixture, mut session, _, _, _) = fixture(Mode::Controlled).await;
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let calls = Arc::new(HeldCalls {
        service: fixture.service.clone(),
        uid: session.transport.uid.clone(),
        generation: session.expected.login_generation,
        started: parking_lot::Mutex::new(Some(started)),
        release: parking_lot::Mutex::new(Some(held)),
        calls: AtomicUsize::new(0),
    });
    session.transport = Arc::new(FirebaseTransport {
        calls: calls.clone(),
        uid: session.transport.uid.clone(),
        project: "unused-project".into(),
        installation_id: "fixture-machine".into(),
    });
    let fallback = session.fallback();
    let project = async {
        running.await.unwrap();
        assert!(fixture.service.apply_neighbor_projection(
            &session.transport.uid, session.expected.login_generation, snapshot(1, None),
        ));
        release.send(()).unwrap();
    };
    tokio::join!(fallback, project);
    assert_eq!(calls.calls.load(Ordering::SeqCst), 1);
    assert_eq!(calls.invoke("friendsList", json!({})).await.unwrap()["access"], "paid");
    assert_eq!(calls.calls.load(Ordering::SeqCst), 1);
    assert!(!fixture.ui.0.lock().iter().any(|(name, _)| name == "neighbors-changed"));
}
