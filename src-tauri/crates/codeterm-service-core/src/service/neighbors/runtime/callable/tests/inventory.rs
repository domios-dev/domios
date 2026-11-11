use super::*;
use codeterm_neighbors::{
    adapter::FirebaseRegistry,
    crypto::{encryption_key_id, sha256_hex},
    keys::{InstallationKeys, KeyRegistry, KeyScope, KeyStore, Slot},
    state::Snapshot,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct InventoryTransport {
    service: ServiceContext,
    uid: String,
    generation: u64,
    enrollment: AtomicUsize,
    selection: AtomicUsize,
    revocation: AtomicUsize,
    acknowledged: tokio::sync::Notify,
    key: parking_lot::Mutex<Option<(String, String)>>,
    snapshot_before_response: bool,
}

fn snapshot(version: u64, key: Option<&(String, String)>) -> Snapshot {
    snapshot_keys(version, &key.cloned().into_iter().collect::<Vec<_>>())
}

fn snapshot_keys(version: u64, keys: &[(String, String)]) -> Snapshot {
    Snapshot::parse(json!({"schema_version":1,"version":version,"updated_at":100,
        "access":"paid","can_invite":false,"friends":[],"incoming":[],"outgoing":[],
        "own_keys":keys.iter().map(|(verifier, encryption)| json!({"verifier_key_id":verifier,
            "enc_key_id":encryption,"installation_id":"fixture-machine","machine_label":"Fixture","created_at":100})).collect::<Vec<_>>(),
        "referral_trial":{"state":"none","trial_end":null,"inviter_uid":null,"friend_request_id":null}})).unwrap()
}

#[async_trait]
impl CallableTransport for InventoryTransport {
    async fn call(
        &self,
        route: &str,
        _: &str,
        data: Value,
    ) -> std::result::Result<Value, CallableFailure> {
        match route {
            "friendsEnrollVerifierKey" => {
                let attempt = self.enrollment.fetch_add(1, Ordering::SeqCst);
                let signing = data["public_key_spki_b64"].as_str().unwrap();
                let encryption = data["encryption_public_key_spki_b64"].as_str().unwrap();
                let key = (
                    format!(
                        "vk_{}",
                        &sha256_hex(format!("verifier-key:{}:{signing}", self.uid).as_bytes())
                            [..32]
                    ),
                    encryption_key_id(encryption),
                );
                *self.key.lock() = Some(key.clone());
                if self.snapshot_before_response && attempt == 0 {
                    assert!(
                        self.service
                            .apply_neighbor_projection(
                                &self.uid,
                                self.generation,
                                snapshot(2, Some(&key))
                            )
                            .await
                    );
                }
                self.acknowledged.notify_one();
                Ok(json!({"result":{"verifier_key_id":key.0}}))
            }
            "friendsRevokeVerifierKey" => {
                self.revocation.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"result":{}}))
            }
            "friendsSetFriendReceiver" => {
                self.selection.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"result":{"selected":true}}))
            }
            _ => panic!("No direct inventory fetch, listener or other callable allowed: {route}"),
        }
    }
}

struct ProjectedCalls {
    calls: Calls,
    transport: Arc<InventoryTransport>,
}

#[async_trait]
impl AuthenticatedCall for ProjectedCalls {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        self.calls
            .local_session()
            .await
            .map_err(|error| key_mutation_error(route, error))?;
        self.calls
            .service
            .neighbor_backend_read(
                route,
                &self.calls.uid,
                self.calls.generation,
                self.calls
                    .invoke_with_transport(self.transport.as_ref(), route, data),
            )
            .await
    }
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        panic!("Key confirmation must not create a Firestore listener");
    }
}

struct InventoryFixture {
    fixture: crate::service::test_fixture::ServiceFixture,
    account: Arc<codeterm_account::Account>,
    port: Arc<InventoryTransport>,
    registry: FirebaseRegistry,
    scope: KeyScope,
    store: Arc<dyn KeyStore>,
}

#[derive(Default)]
struct ImmediateProvider(codeterm_account::adapters::fake::Fake);

#[async_trait]
impl codeterm_account::IdentityProvider for ImmediateProvider {
    fn id(&self) -> &str {
        "fake"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(
        &self,
        _: &codeterm_account::LoginFlow,
    ) -> codeterm_account::Result<codeterm_account::LoginStart> {
        self.0
            .refresh(&codeterm_account::Secret::new("fake-refresh"))
            .await
            .map(codeterm_account::LoginStart::Immediate)
    }
    async fn redeem(
        &self,
        _: codeterm_account::Redemption,
    ) -> codeterm_account::Result<codeterm_account::Tokens> {
        panic!("Immediate sign-in cannot use a callback")
    }
    async fn refresh(
        &self,
        token: &codeterm_account::Secret,
    ) -> codeterm_account::Result<codeterm_account::Tokens> {
        self.0.refresh(token).await
    }
    async fn revoke(&self, token: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        self.0.revoke(token).await
    }
}

async fn fixture(snapshot_before_response: bool) -> InventoryFixture {
    let (fixture, account, calls) =
        super::auth::fixture_with_provider(Arc::new(ImmediateProvider::default())).await;
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    fixture
        .service
        .select_neighbor_projection(&calls.uid, calls.generation)
        .await
        .unwrap();
    assert!(
        fixture
            .service
            .apply_neighbor_projection(&calls.uid, calls.generation, snapshot(1, None))
            .await
    );
    let store = fixture.service.fixture_neighbor_key_store();
    let port = Arc::new(InventoryTransport {
        service: fixture.service.clone(),
        uid: calls.uid.clone(),
        generation: calls.generation,
        enrollment: AtomicUsize::new(0),
        selection: AtomicUsize::new(0),
        revocation: AtomicUsize::new(0),
        acknowledged: Default::default(),
        key: Default::default(),
        snapshot_before_response,
    });
    let registry = FirebaseRegistry {
        uid: calls.uid.clone(),
        installation_id: "fixture-machine".into(),
        machine_label: "Fixture".into(),
        calls: Arc::new(ProjectedCalls {
            calls,
            transport: port.clone(),
        }),
    };
    let scope = KeyScope::installation("fixture-machine", &registry.uid).unwrap();
    InventoryFixture {
        fixture,
        account,
        port,
        registry,
        scope,
        store,
    }
}

#[tokio::test]
async fn enrollment_withholds_selection_and_promotes_the_same_candidate_only_after_snapshot() {
    let f = fixture(false).await;
    let service = &f.fixture.service;
    let action = service.select_neighbor_receiver_for_test(&f.registry, &f.scope);
    tokio::pin!(action);
    let control = async {
        tokio::select! {
            biased;
            result = &mut action => panic!("Stale inventory completed enrollment: {result:?}"),
            () = f.port.acknowledged.notified() => {},
        }
        service.neighbor_reconciliation().idle().await;
        tokio::select! {
            biased;
            result = &mut action => panic!("Stale inventory completed enrollment: {result:?}"),
            () = std::future::ready(()) => {},
        }
        let candidate = f
            .store
            .load(&f.scope, Slot::Candidate)
            .await
            .unwrap()
            .unwrap();
        assert!(f
            .store
            .load(&f.scope, Slot::Current)
            .await
            .unwrap()
            .is_none());
        assert_eq!(f.port.selection.load(Ordering::SeqCst), 0);
        assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.registry.active_key_ids().await,
            Err(Error::KeyInventoryPending)
        );
        assert!(!service.neighbor_published_installation_key("fixture-machine"));
        let key = f.port.key.lock().clone().unwrap();
        assert!(
            service
                .apply_neighbor_projection(&f.port.uid, f.port.generation, snapshot(2, Some(&key)))
                .await
        );
        assert_eq!(action.await.unwrap(), json!({"selected":true}));
        assert_eq!(
            f.store
                .load(&f.scope, Slot::Current)
                .await
                .unwrap()
                .unwrap()
                .as_slice(),
            candidate.as_slice()
        );
        assert!(f
            .store
            .load(&f.scope, Slot::Candidate)
            .await
            .unwrap()
            .is_none());
        assert!(service.neighbor_published_installation_key("fixture-machine"));
        assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 1);
        assert_eq!(f.port.selection.load(Ordering::SeqCst), 1);
        service
            .select_neighbor_receiver_for_test(&f.registry, &f.scope)
            .await
            .unwrap();
        assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 1);
        assert_eq!(f.port.selection.load(Ordering::SeqCst), 2);
    };
    tokio::time::timeout(Duration::from_secs(5), control)
        .await
        .unwrap();
}

#[tokio::test]
async fn snapshot_before_wait_and_idempotent_same_key_need_no_new_version() {
    let f = fixture(true).await;
    f.fixture
        .service
        .select_neighbor_receiver_for_test(&f.registry, &f.scope)
        .await
        .unwrap();
    let lifecycle = InstallationKeys {
        store: f.store.as_ref(),
        registry: &f.registry,
        scope: f.scope.clone(),
        uid: &f.registry.uid,
    };
    let keys = lifecycle.load().await.unwrap().unwrap();
    let result = f
        .registry
        .enroll(&keys.signing_spki(), &keys.encryption_spki())
        .await
        .unwrap();
    assert_eq!(result, keys.verifier_key_id(&f.registry.uid));
    assert_eq!(
        f.fixture
            .service
            .neighbor_projection_version(&f.port.uid, f.port.generation),
        Some(2)
    );
    assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn listener_owned_enrollment_is_pending_without_waiting_or_losing_candidate() {
    let f = fixture(false).await;
    let service = &f.fixture.service;
    let activation = service.fixture_neighbor_registry_activation();
    let started = tokio::time::Instant::now();
    assert_eq!(
        service
            .select_neighbor_receiver_for_test(&f.registry, &f.scope)
            .await,
        Err(Error::KeyInventoryPending)
    );
    assert_eq!(tokio::time::Instant::now(), started);
    let candidate = f
        .store
        .load(&f.scope, Slot::Candidate)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 1);
    assert_eq!(f.port.selection.load(Ordering::SeqCst), 0);
    drop(activation);
    let key = f.port.key.lock().clone().unwrap();
    assert!(
        service
            .apply_neighbor_projection(&f.port.uid, f.port.generation, snapshot(2, Some(&key)))
            .await
    );
    service
        .select_neighbor_receiver_for_test(&f.registry, &f.scope)
        .await
        .unwrap();
    assert_eq!(
        f.store
            .load(&f.scope, Slot::Current)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        candidate.as_slice()
    );
    assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 1);

    let mut renamed = f.registry.clone();
    renamed.machine_label = "Renamed".into();
    service
        .select_neighbor_receiver_for_test(&renamed, &f.scope)
        .await
        .unwrap();
    service
        .select_neighbor_receiver_for_test(&renamed, &f.scope)
        .await
        .unwrap();
    assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 2);
    service
        .select_neighbor_receiver_for_test(&f.registry, &f.scope)
        .await
        .unwrap();
    assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn rotate_returns_without_waiting_for_revocation_while_registry_activation_is_queued() {
    use crate::service::neighbors::{
        runtime::{FixtureKeyPortBinding, FixtureKeyPorts, NeighborRuntimeCommand},
        storage::Metadata,
    };
    let f = fixture(false).await;
    let service = &f.fixture.service;
    f.store
        .save(&f.scope, Slot::Current, Zeroizing::new(vec![7; 64]))
        .await
        .unwrap();
    let lifecycle = InstallationKeys {
        store: f.store.as_ref(),
        registry: &f.registry,
        scope: f.scope.clone(),
        uid: &f.registry.uid,
    };
    let current = lifecycle.load().await.unwrap().unwrap();
    let old = (
        current.verifier_key_id(&f.registry.uid),
        current.encryption_key_id(),
    );
    assert!(
        service
            .apply_neighbor_projection(&f.port.uid, f.port.generation, snapshot(2, Some(&old)))
            .await
    );
    let ports = Arc::new(FixtureKeyPorts {
        registry: Arc::new(f.registry.clone()),
        scope: f.scope.clone(),
        metadata: Metadata::new(f.fixture.temp.path().to_owned(), &f.registry.uid),
    });
    service
        .extensions
        .get_or_init(|| FixtureKeyPortBinding(Arc::downgrade(&ports)));
    let (events, mut observed) = tokio::sync::mpsc::unbounded_channel();
    service.observe_neighbor_owner_for_test(events);
    let rotate = service.neighbor_runtime(NeighborRuntimeCommand::Rotate, None, None);
    tokio::pin!(rotate);
    tokio::select! {
        biased;
        result = &mut rotate => panic!("Rotation completed before key snapshot: {result:?}"),
        () = f.port.acknowledged.notified() => {},
    }
    service.neighbor_reconciliation().idle().await;
    let key = f.port.key.lock().clone().unwrap();
    let activation = service.refresh_neighbor_registry_listeners();
    tokio::pin!(activation);
    tokio::select! {
        biased;
        result = &mut activation => panic!("Activation passed the active Rotate: {result:?}"),
        event = observed.recv() => assert_eq!(event, Some("activation_queued")),
    }
    assert!(
        service
            .apply_neighbor_projection(
                &f.port.uid,
                f.port.generation,
                snapshot_keys(3, &[old, key.clone()])
            )
            .await
    );
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    assert_eq!(rotate.await, Err(Error::Transport.to_string()));
    assert_eq!(activation.await, Err(Error::Revoked.to_string()));
    assert_eq!(tokio::time::Instant::now(), started);
    assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 1);
    assert_eq!(f.port.revocation.load(Ordering::SeqCst), 1);
    assert_eq!(f.port.selection.load(Ordering::SeqCst), 0);
    assert!(f
        .store
        .load(&f.scope, Slot::Candidate)
        .await
        .unwrap()
        .is_some());
    assert!(
        service
            .apply_neighbor_projection(&f.port.uid, f.port.generation, snapshot(4, Some(&key)))
            .await
    );
    let promoted = lifecycle.reconcile().await.unwrap().unwrap();
    assert_eq!(promoted.verifier_key_id(&f.registry.uid), key.0);
    assert!(f
        .store
        .load(&f.scope, Slot::Candidate)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test(start_paused = true)]
async fn deadline_disable_account_switch_and_cancellation_keep_candidate_and_forbid_selection() {
    for mode in ["deadline", "disable", "account", "cancel"] {
        let f = fixture(false).await;
        let service = &f.fixture.service;
        let mut action = Box::pin(service.select_neighbor_receiver_for_test(&f.registry, &f.scope));
        tokio::select! {
            biased;
            result = &mut action => panic!("Stale inventory completed enrollment: {result:?}"),
            () = f.port.acknowledged.notified() => {},
        }
        service.neighbor_reconciliation().idle().await;
        tokio::select! {
            biased;
            result = &mut action => panic!("Stale inventory completed enrollment: {result:?}"),
            () = std::future::ready(()) => {},
        }
        let candidate = f
            .store
            .load(&f.scope, Slot::Candidate)
            .await
            .unwrap()
            .unwrap();
        match mode {
            "deadline" => tokio::time::advance(Duration::from_secs(16)).await,
            "disable" => {
                service
                    .ext::<crate::config::ConfigState>()
                    .lock()
                    .neighbors
                    .enabled = false;
                service.retire_callable_waiters();
            }
            "account" => {
                f.account.logout().await.unwrap();
                f.account.login(Arc::new(NoBrowser)).await.unwrap();
                let session = f.account.session().await.unwrap();
                assert_ne!(session.login_generation, f.port.generation);
                service
                    .select_neighbor_projection(&session.identity.uid, session.login_generation)
                    .await
                    .unwrap();
                service.retire_callable_waiters();
            }
            "cancel" => {}
            _ => unreachable!(),
        }
        if mode != "cancel" {
            assert_eq!(
                action.as_mut().await,
                Err(Error::KeyInventoryPending),
                "{mode}"
            );
        }
        drop(action);
        assert_eq!(
            f.store
                .load(&f.scope, Slot::Candidate)
                .await
                .unwrap()
                .unwrap()
                .as_slice(),
            candidate.as_slice(),
            "{mode}"
        );
        assert!(f
            .store
            .load(&f.scope, Slot::Current)
            .await
            .unwrap()
            .is_none());
        assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 1, "{mode}");
        assert_eq!(f.port.selection.load(Ordering::SeqCst), 0, "{mode}");
        if matches!(mode, "deadline" | "cancel") {
            assert_eq!(
                service
                    .select_neighbor_receiver_for_test(&f.registry, &f.scope)
                    .await,
                Err(Error::KeyInventoryPending)
            );
            assert_eq!(f.port.enrollment.load(Ordering::SeqCst), 1);
        }
    }
}
