use super::*;

struct SelectionCalls {
    service: ServiceContext,
    uid: String,
    login_generation: u64,
    key_id: parking_lot::Mutex<String>,
    trace: parking_lot::Mutex<Vec<String>>,
    enrolled: tokio::sync::mpsc::UnboundedSender<()>,
    release: tokio::sync::Semaphore,
    refusal: parking_lot::Mutex<Option<Error>>,
    stale_post_read: std::sync::atomic::AtomicBool,
    committed: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl AuthenticatedCall for SelectionCalls {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        self.trace.lock().push(route.into());
        match route {
            "friendsList" => {
                if self.committed.load(Ordering::SeqCst) && self.stale_post_read.load(Ordering::SeqCst) {
                    return Ok(selection_snapshot(1, 0).list(1));
                }
                Ok(self.service.peek_neighbor_projection(
                    &self.uid, self.login_generation, route, "fixture-machine",
                ).unwrap())
            }
            "friendsOwnKeys" => Ok(json!([{"verifier_key_id":self.key_id.lock().clone()}])),
            "friendsEnrollVerifierKey" => {
                self.enrolled.send(()).unwrap();
                self.release.acquire().await.unwrap().forget();
                Ok(json!({"verifier_key_id":self.key_id.lock().clone()}))
            }
            "friendsSetFriendReceiver" => {
                assert_eq!(data["expected_revision"], 0);
                assert_eq!(data["friendship_id"], "a".repeat(64));
                if let Some(error) = self.refusal.lock().clone() { return Err(error); }
                self.committed.store(true, Ordering::SeqCst);
                Ok(json!({"schema_version":1,"receiver":{
                    "friendship_id":"a".repeat(64),"machine_label":null,
                    "revision":1,"receiving":false,"is_me":true
                }}))
            }
            _ => panic!("unexpected selection fixture call: {route}"),
        }
    }
    async fn id_token(&self) -> Result<zeroize::Zeroizing<String>> {
        panic!("No real authentication")
    }
}

struct ChannelState {
    snapshots: tokio::sync::mpsc::UnboundedReceiver<Snapshot>,
    consumed: tokio::sync::mpsc::UnboundedSender<StateRead>,
}

#[derive(Debug, PartialEq, Eq)]
enum StateRead {
    Requested,
    Snapshot(u64),
}

#[async_trait::async_trait]
impl StateListener for ChannelState {
    async fn next(&mut self) -> Result<Option<StateEvent>> {
        self.consumed.send(StateRead::Requested).unwrap();
        let snapshot = self.snapshots.recv().await.unwrap();
        self.consumed.send(StateRead::Snapshot(snapshot.version)).unwrap();
        Ok(Some(StateEvent::Snapshot(snapshot)))
    }
}

struct DefaultActivation {
    listener: parking_lot::Mutex<Option<ChannelState>>,
}

#[async_trait::async_trait]
impl SessionPorts for DefaultActivation {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>> {
        Ok(Box::new(self.listener.lock().take().unwrap()))
    }
    async fn listen_signals(&self) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>> {
        panic!("fixture never enables inbox reception")
    }
}

fn selection_snapshot(version: u64, revision: u64) -> Snapshot {
    let mut value = snapshot(version, Some("fixture-machine")).list(1);
    value["updated_at"] = json!(1);
    value["own_keys"] = json!([]);
    value["friends"][0]["my_receiver"] = json!({
        "installation_id": if revision == 0 { None } else { Some("fixture-machine") },
        "enc_key_id":null,"machine_label":null,"revision":revision,"receiving":false
    });
    Snapshot::parse(value).unwrap()
}

struct SelectionFixture {
    fixture: crate::service::test_fixture::ServiceFixture,
    session: Session,
    calls: Arc<SelectionCalls>,
    enrollment: tokio::sync::mpsc::UnboundedReceiver<()>,
    send: tokio::sync::mpsc::UnboundedSender<Snapshot>,
    consumption: tokio::sync::mpsc::UnboundedReceiver<StateRead>,
    owner_events: tokio::sync::mpsc::UnboundedReceiver<&'static str>,
    _key_ports: Arc<crate::service::neighbors::runtime::FixtureKeyPorts>,
}

async fn selection_fixture() -> SelectionFixture {
    use codeterm_neighbors::keys::{ConfiguredKeyStore, InstallationKeys, KeyScope, KeyStore, OsKeyStore, Slot};
    let (fixture, mut session, _, _, _) = fixture(Mode::Controlled).await;
    let service = &fixture.service;
    service.ext::<crate::config::ConfigState>().lock().neighbors.key_store =
        Some(codeterm_types::config::neighbors::NeighborKeyStore::File);
    let (enrolled, enrollment) = tokio::sync::mpsc::unbounded_channel();
    let calls = Arc::new(SelectionCalls {
        service: service.clone(), uid: session.transport.uid.clone(),
        login_generation: session.expected.login_generation,
        key_id: Default::default(), trace: Default::default(), enrolled,
        release: tokio::sync::Semaphore::new(0),
        refusal: Default::default(), stale_post_read: Default::default(), committed: Default::default(),
    });
    let registry = Arc::new(codeterm_neighbors::adapter::FirebaseRegistry {
        calls: calls.clone(), uid: session.transport.uid.clone(),
        installation_id: "fixture-machine".into(), machine_label: "Fixture".into(),
    });
    let scope = KeyScope::installation("fixture-machine", &registry.uid).unwrap();
    let store = ConfiguredKeyStore::new(OsKeyStore, Some(service.account_data_root().unwrap()));
    store.save(&scope, Slot::Current, zeroize::Zeroizing::new(vec![7;64])).await.unwrap();
    let keys = InstallationKeys { store: &store, registry: registry.as_ref(), scope: scope.clone(), uid: &registry.uid };
    *calls.key_id.lock() = keys.load().await.unwrap().unwrap().verifier_key_id(&registry.uid);
    let key_ports = Arc::new(crate::service::neighbors::runtime::FixtureKeyPorts {
        registry, scope, metadata: session.metadata.clone(),
    });
    service.extensions.get_or_init(|| crate::service::neighbors::runtime::FixtureKeyPortBinding(Arc::downgrade(&key_ports)));
    assert!(session.service.apply_neighbor_projection(&session.transport.uid, session.expected.login_generation, selection_snapshot(1,0)).await);
    let (send, snapshots) = tokio::sync::mpsc::unbounded_channel();
    let (consumed, consumption) = tokio::sync::mpsc::unbounded_channel();
    session.ports = Arc::new(DefaultActivation { listener: parking_lot::Mutex::new(Some(ChannelState { snapshots, consumed })) });
    let (events, owner_events) = tokio::sync::mpsc::unbounded_channel();
    service.observe_neighbor_owner_for_test(events);
    SelectionFixture { fixture, session, calls, enrollment, send, consumption, owner_events, _key_ports: key_ports }
}

fn selection_command() -> crate::service::neighbors::runtime::NeighborRuntimeCommand {
    serde_json::from_value(json!({"operation":"set_friend_receiver",
        "friendship_id":"a".repeat(64),"receiver_operation":"set",
        "takeover_confirmed":false,"expected_receiver_revision":0})).unwrap()
}

fn assert_selection_once(calls: &SelectionCalls) {
    let trace = calls.trace.lock();
    assert_eq!(trace.iter().filter(|route| *route == "friendsEnrollVerifierKey").count(),1);
    assert_eq!(trace.iter().filter(|route| *route == "friendsSetFriendReceiver").count(),1);
}

async fn owner_event(events: &mut tokio::sync::mpsc::UnboundedReceiver<&'static str>, expected: &'static str) {
    loop {
        let event = events.recv().await.unwrap();
        if event == "confirmation_waiting" && expected != event { continue; }
        assert_eq!(event, expected);
        return;
    }
}

#[tokio::test]
async fn committed_selection_releases_actual_serial_owner_for_default_session_next_snapshot() {
    let SelectionFixture { fixture, session, calls, mut enrollment, send, mut consumption, mut owner_events, _key_ports } = selection_fixture().await;
    let service = &fixture.service;
    let select = service.neighbor_runtime(selection_command(), None, None);
    let drive = async {
        enrollment.recv().await.unwrap();
        send.send(selection_snapshot(2,0)).unwrap();
        assert_eq!(consumption.recv().await, Some(StateRead::Requested));
        assert_eq!(consumption.recv().await, Some(StateRead::Snapshot(2)));
        owner_event(&mut owner_events, "activation_queued").await;
        send.send(selection_snapshot(3,1)).unwrap();
        assert!(consumption.try_recv().is_err());
        calls.release.add_permits(1);
        owner_event(&mut owner_events, "activation_admitted").await;
        assert_eq!(consumption.recv().await, Some(StateRead::Requested));
        assert_eq!(consumption.recv().await, Some(StateRead::Snapshot(3)));
        owner_event(&mut owner_events, "activation_queued").await;
        owner_event(&mut owner_events, "activation_admitted").await;
        assert_eq!(consumption.recv().await, Some(StateRead::Requested));
    };
    let listen = session.listen();
    tokio::pin!(listen);
    let work = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(select, drive) });
    let (result, ()) = tokio::select! {
        result = work => result.expect("owner/Session confirmation cycle"),
        result = &mut listen => panic!("session exited: {result:?}"),
    };
    assert_eq!(result.unwrap()["receiver"]["revision"], 1);
    assert_eq!(service.neighbor_projection_version(&session.transport.uid, session.expected.login_generation), Some(3));
    assert!(service.neighbor_projection_activated(&session.transport.uid, session.expected.login_generation, 3));
    assert_eq!(service.neighbor_friend_receiver_views()["a".repeat(64)]["revision"], 1);
    assert_selection_once(&calls);
}

#[tokio::test]
async fn backend_stale_is_rejection_but_committed_observation_timeout_is_success_without_replay() {
    for stale in [true, false] {
        let mut setup = selection_fixture().await;
        if stale { *setup.calls.refusal.lock() = Some(Error::ReceiverRevisionStale); }
        let select = setup.fixture.service.neighbor_runtime(selection_command(), None, None);
        let release = async {
            setup.enrollment.recv().await.unwrap();
            setup.calls.release.add_permits(1);
            if !stale {
                owner_event(&mut setup.owner_events, "confirmation_waiting").await;
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(15)).await;
            }
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(20), async { tokio::join!(select, release) }).await.unwrap();
        if !stale { tokio::time::resume(); }
        if stale {
            assert_eq!(result.unwrap_err(), Error::ReceiverRevisionStale.to_string());
            assert!(!setup.calls.committed.load(Ordering::SeqCst));
        } else {
            assert_eq!(result.unwrap()["receiver"]["revision"], 1);
            assert!(setup.calls.committed.load(Ordering::SeqCst));
        }
        assert_selection_once(&setup.calls);
        assert_eq!(setup.fixture.service.neighbor_projection_version(&setup.session.transport.uid, setup.session.expected.login_generation), Some(1));
        assert_eq!(setup.fixture.service.neighbor_friend_receiver_views()["a".repeat(64)]["revision"].as_u64().unwrap_or(0), 0);
        assert!(!setup.fixture.service.neighbor_friend_receiving_here(&"a".repeat(64)));
    }
}

#[tokio::test]
async fn committed_selection_with_stale_post_read_does_not_return_backend_stale() {
    let mut setup = selection_fixture().await;
    setup.calls.stale_post_read.store(true, Ordering::SeqCst);
    let select = setup.fixture.service.neighbor_runtime(selection_command(), None, None);
    let project = async {
        setup.enrollment.recv().await.unwrap();
        setup.calls.release.add_permits(1);
        setup.session.project(selection_snapshot(3, 1), &mut notifications::Seen::default()).await.unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(select, project) }).await.unwrap();
    assert_eq!(result.unwrap()["receiver"]["revision"], 1);
    assert_selection_once(&setup.calls);
    assert!(setup.calls.trace.lock().iter().filter(|route| *route == "friendsList").count() >= 3);
}

#[tokio::test]
async fn abandoned_selection_finishes_one_cas_and_releases_default_activation() {
    let mut setup = selection_fixture().await;
    let mut select = Box::pin(setup.fixture.service.neighbor_runtime(selection_command(), None, None));
    tokio::select! {
        event = setup.enrollment.recv() => assert_eq!(event, Some(())),
        _ = &mut select => panic!("selection completed before enrollment release"),
    }
    drop(select);
    let mut seen = notifications::Seen::default();
    let project = setup.session.project(selection_snapshot(2, 0), &mut seen);
    tokio::pin!(project);
    tokio::select! {
        event = setup.owner_events.recv() => assert_eq!(event, Some("activation_queued")),
        _ = &mut project => panic!("activation bypassed serial owner"),
    }
    setup.calls.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), &mut project).await.unwrap().unwrap();
    assert_eq!(setup.owner_events.recv().await, Some("activation_admitted"));
    assert_selection_once(&setup.calls);
    assert!(setup.calls.committed.load(Ordering::SeqCst));
    assert_eq!(setup.fixture.service.neighbor_projection_version(&setup.session.transport.uid, setup.session.expected.login_generation), Some(2));
}

#[tokio::test]
async fn implicit_first_assignment_never_replays_when_local_preparation_is_pending() {
    for operation in ["bind", "change_agent", "set_permission"] {
        let mut setup = selection_fixture().await;
        let command = match operation {
            "bind" => json!({"operation":operation,"friendship_id":"a".repeat(64),"gateway_tab":"opaque-tab"}),
            "change_agent" => json!({"operation":operation,"friendship_id":"a".repeat(64),"provider":"opaque-provider","model":null,"reasoning_effort":null,"service_tier":null}),
            _ => json!({"operation":operation,"friendship_id":"a".repeat(64),"permission":{}}),
        };
        let select = setup.fixture.service.neighbor_runtime(serde_json::from_value(command).unwrap(), None, None);
        let release = async {
            setup.enrollment.recv().await.unwrap();
            setup.calls.release.add_permits(1);
            owner_event(&mut setup.owner_events, "confirmation_waiting").await;
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(15)).await;
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(20), async { tokio::join!(select, release) }).await.unwrap();
        tokio::time::resume();
        let error = result.unwrap_err();
        assert!(error.contains("assignment committed; local preparation pending"));
        assert!(!error.contains("receiver_revision_stale"));
        assert_selection_once(&setup.calls);
        assert!(setup.session.metadata.settings().await.unwrap().bindings.is_empty());
        assert!(setup.session.metadata.settings().await.unwrap().permissions.is_empty());
    }
}

#[tokio::test]
async fn account_or_runtime_retirement_after_commit_preserves_assignment_without_late_effects() {
    for runtime_retirement in [false, true] {
        let mut setup = selection_fixture().await;
        let select = setup.fixture.service.neighbor_runtime(selection_command(), None, None);
        let retire = async {
            setup.enrollment.recv().await.unwrap();
            setup.calls.release.add_permits(1);
            owner_event(&mut setup.owner_events, "confirmation_waiting").await;
            if runtime_retirement {
                setup.fixture.service.neighbor_identity_changed();
            } else {
                setup.session.account.logout().await.unwrap();
                assert!(setup.session.account.session().await.is_none());
                setup.fixture.service.neighbor_projection_unavailable();
            }
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(select, retire) }).await.unwrap();
        assert_eq!(result.unwrap()["receiver"]["revision"], 1);
        assert_selection_once(&setup.calls);
        assert!(setup.fixture.service.neighbor_friend_receiver_views().as_object().unwrap().is_empty());
        assert_eq!(setup.calls.trace.lock().iter().filter(|route| *route == "friendsList").count(), 2);
    }
}

